use crate::php::PhpExtractor;
use std::path::PathBuf;

fn inferred_type(source: &str, local: &str) -> Option<(String, bool, Option<String>)> {
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_php::LANGUAGE_PHP.into())
        .unwrap();
    let tree = parser.parse(source, None).unwrap();
    let mut extractor = PhpExtractor::new(
        "php".to_string(),
        "initializer_types.php".to_string(),
        source.to_string(),
        &PathBuf::from("/tmp/test"),
    );
    let symbols = extractor.extract_symbols(&tree);
    let local = symbols
        .iter()
        .find(|s| s.name == local)
        .unwrap_or_else(|| panic!("missing local {local}"));
    extractor.base.type_info.get(&local.id).map(|fact| {
        let declared = fact
            .metadata
            .as_ref()
            .and_then(|m| m.get("declared"))
            .and_then(|v| v.as_str())
            .map(str::to_string);
        (fact.resolved_type.clone(), fact.is_inferred, declared)
    })
}

fn inferred(name: &str) -> Option<(String, bool, Option<String>)> {
    Some((name.to_string(), true, None))
}

fn inferred_from(name: &str, declared: &str) -> Option<(String, bool, Option<String>)> {
    Some((name.to_string(), true, Some(declared.to_string())))
}

const SERVICE: &str = r#"<?php
class Workspace {}
class Service {
    public function load(): Workspace { return new Workspace(); }
    public function find(): ?Workspace { return null; }
    public static function create(): static { return new static(); }
    public static function make(): self { return new self(); }
    public function untyped() { return 1; }
    public function either(): Workspace|Service { return $this; }
    public function nothing(): void {}
    public function count(): int { return 1; }
"#;

fn in_service(body: &str) -> String {
    format!("{SERVICE}    public function run() {{\n        {body}\n    }}\n}}\n")
}

fn service_local(body: &str) -> Option<(String, bool, Option<String>)> {
    inferred_type(&in_service(body), "x")
}

#[test]
fn same_file_function_call_records_declared_return_type() {
    let source = r#"<?php
class Workspace {}
function load(): Workspace { return new Workspace(); }
function run() {
    $x = load();
}
"#;
    assert_eq!(inferred_type(source, "x"), inferred("Workspace"));
}

#[test]
fn function_call_matches_name_case_insensitively() {
    let source = r#"<?php
function load(): Workspace {}
$x = LOAD();
"#;
    assert_eq!(inferred_type(source, "x"), inferred("Workspace"));
}

#[test]
fn nullable_return_type_records_inner_type() {
    let source = r#"<?php
function find(): ?Workspace {}
$x = find();
"#;
    assert_eq!(
        inferred_type(source, "x"),
        inferred_from("Workspace", "?Workspace")
    );
}

#[test]
fn qualified_return_type_records_name_without_leading_separator() {
    let source = r#"<?php
function load(): \App\Workspace {}
$x = load();
"#;
    assert_eq!(
        inferred_type(source, "x"),
        inferred_from("App\\Workspace", "\\App\\Workspace")
    );
}

#[test]
fn primitive_return_type_records_primitive() {
    assert_eq!(service_local("$x = $this->count();"), inferred("int"));
}

#[test]
fn this_method_call_records_method_return_type() {
    assert_eq!(service_local("$x = $this->load();"), inferred("Workspace"));
    assert_eq!(
        service_local("$x = $this->find();"),
        inferred_from("Workspace", "?Workspace")
    );
    assert_eq!(service_local("$x = $this->LOAD();"), inferred("Workspace"));
}

#[test]
fn self_and_static_calls_resolve_self_and_static_returns_to_the_class() {
    for call in [
        "self::create()",
        "static::create()",
        "self::make()",
        "static::make()",
    ] {
        let expected = if call.ends_with("create()") {
            inferred_from("Service", "static")
        } else {
            inferred_from("Service", "self")
        };
        assert_eq!(service_local(&format!("$x = {call};")), expected, "{call}");
    }
}

#[test]
fn static_call_on_same_file_class_records_return_type() {
    let source = format!(
        "{SERVICE}}}\nfunction run() {{\n    $x = Service::create();\n    $y = service::load();\n}}\n"
    );
    assert_eq!(
        inferred_type(&source, "x"),
        inferred_from("Service", "static")
    );
    assert_eq!(inferred_type(&source, "y"), inferred("Workspace"));
}

#[test]
fn enum_methods_resolve_through_self() {
    let source = r#"<?php
enum Suit {
    case Hearts;
    public static function fallback(): self { return self::Hearts; }
    public function run() {
        $x = self::fallback();
    }
}
"#;
    assert_eq!(inferred_type(source, "x"), inferred_from("Suit", "self"));
}

#[test]
fn functions_in_the_same_namespace_resolve() {
    let source = r#"<?php
namespace App;
function load(): Workspace {}
function run() {
    $x = load();
}
"#;
    assert_eq!(inferred_type(source, "x"), inferred("Workspace"));
}

#[test]
fn new_expression_still_records_constructed_type() {
    assert_eq!(
        service_local("$x = new Workspace();"),
        inferred("Workspace")
    );
}

#[test]
fn methods_without_a_single_named_return_type_record_nothing() {
    for call in ["$this->untyped()", "$this->either()", "$this->nothing()"] {
        assert_eq!(service_local(&format!("$x = {call};")), None, "{call}");
    }
}

#[test]
fn parent_return_type_records_nothing() {
    let source = r#"<?php
class Base {}
class Child extends Base {
    public function up(): parent { return $this; }
    public function run() {
        $x = $this->up();
    }
}
"#;
    assert_eq!(inferred_type(source, "x"), None);
}

#[test]
fn non_this_receiver_records_nothing() {
    assert_eq!(service_local("$x = $other->load();"), None);
}

#[test]
fn method_chain_records_nothing() {
    assert_eq!(service_local("$x = $this->load()->next();"), None);
    assert_eq!(service_local("$x = self::create()->load();"), None);
}

#[test]
fn parent_call_records_nothing() {
    assert_eq!(service_local("$x = parent::load();"), None);
}

#[test]
fn unknown_function_records_nothing() {
    assert_eq!(service_local("$x = make_workspace();"), None);
}

#[test]
fn method_name_called_as_function_records_nothing() {
    let source = format!("{SERVICE}}}\n$x = load();\n");
    assert_eq!(inferred_type(&source, "x"), None);
}

#[test]
fn function_name_called_as_this_method_records_nothing() {
    let source = r#"<?php
function build(): Workspace {}
class Service {
    public function run() {
        $x = $this->build();
    }
}
"#;
    assert_eq!(inferred_type(source, "x"), None);
}

#[test]
fn this_call_resolves_only_against_the_enclosing_class() {
    let source = r#"<?php
class Loader {
    public function load(): Workspace {}
}
class Service {
    public function run() {
        $x = $this->load();
    }
}
"#;
    assert_eq!(inferred_type(source, "x"), None);
}

#[test]
fn static_call_on_class_from_another_file_records_nothing() {
    assert_eq!(service_local("$x = Registry::create();"), None);
    assert_eq!(service_local("$x = \\Other\\Service::create();"), None);
}

#[test]
fn disagreeing_same_named_functions_record_nothing() {
    let source = r#"<?php
if (PHP_OS === 'Linux') {
    function load(): Workspace {}
} else {
    function load(): Project {}
}
$x = load();
"#;
    assert_eq!(inferred_type(source, "x"), None);
}

#[test]
fn same_named_function_without_return_type_records_nothing() {
    let source = r#"<?php
if (PHP_OS === 'Linux') {
    function load() {}
} else {
    function load(): Workspace {}
}
$x = load();
"#;
    assert_eq!(inferred_type(source, "x"), None);
}

#[test]
fn function_in_another_namespace_records_nothing() {
    let source = r#"<?php
namespace A {
    function load(): Workspace {}
}
namespace B {
    $x = load();
}
"#;
    assert_eq!(inferred_type(source, "x"), None);
}

#[test]
fn function_after_a_namespace_switch_records_nothing() {
    let source = r#"<?php
namespace A;
function load(): Workspace {}
namespace B;
$x = load();
"#;
    assert_eq!(inferred_type(source, "x"), None);
}

#[test]
fn this_inside_a_closure_records_nothing() {
    for body in [
        "$f = function () { $x = $this->load(); };",
        "$f = fn () => $x = $this->load();",
        "$f = function () { $x = self::create(); };",
    ] {
        assert_eq!(service_local(body), None, "{body}");
    }
}

#[test]
fn trait_methods_record_nothing() {
    let source = r#"<?php
trait Loads {
    public function load(): Workspace {}
    public function run() {
        $x = $this->load();
    }
}
"#;
    assert_eq!(inferred_type(source, "x"), None);
}

#[test]
fn this_inside_an_anonymous_class_records_nothing() {
    let source = r#"<?php
class Service {
    public function load(): Workspace {}
    public function make() {
        return new class {
            public function load(): Project {}
            public function run() {
                $x = $this->load();
            }
        };
    }
}
"#;
    assert_eq!(inferred_type(source, "x"), None);
}
