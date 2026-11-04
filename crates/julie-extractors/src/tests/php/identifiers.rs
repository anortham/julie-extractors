//! Identifier Extraction Tests for PHP
//!
//! Tests for extracting identifiers (function calls, member access, chained access,
//! type usages) from PHP code. Validates that identifier extraction correctly:
//! - Finds function calls
//! - Finds member access patterns
//! - Handles chained member access
//! - Tracks containing symbols
//! - Avoids duplicate identifiers at same location
//! - Extracts type_usage identifiers for type annotations

use crate::base::IdentifierKind;
use crate::php::PhpExtractor;
use std::path::PathBuf;
use tree_sitter::Parser;

fn init_test_parser() -> Parser {
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_php::LANGUAGE_PHP.into())
        .expect("Error loading PHP grammar");
    parser
}

#[test]
fn test_extract_function_calls() {
    let php_code = r#"<?php

class Calculator {
    public function add(int $a, int $b): int {
        return $a + $b;
    }

    public function calculate(): int {
        $result = $this->add(5, 3);      // Function call to add
        echo print_r($result, true);     // Function call to print_r
        return $result;
    }
}
"#;

    let mut parser = init_test_parser();
    let tree = parser.parse(php_code, None).unwrap();

    let workspace_root = PathBuf::from("/tmp/test");
    let mut extractor = PhpExtractor::new(
        "php".to_string(),
        "test.php".to_string(),
        php_code.to_string(),
        &workspace_root,
    );

    // Extract symbols first
    let symbols = extractor.extract_symbols(&tree);

    // NOW extract identifiers (this will FAIL until we implement it)
    let identifiers = extractor.extract_identifiers(&tree, &symbols);

    // Verify we found the function calls
    let add_call = identifiers.iter().find(|id| id.name == "add");
    assert!(
        add_call.is_some(),
        "Should extract 'add' function call identifier"
    );
    let add_call = add_call.unwrap();
    assert_eq!(add_call.kind, IdentifierKind::Call);

    let print_r_call = identifiers.iter().find(|id| id.name == "print_r");
    assert!(
        print_r_call.is_some(),
        "Should extract 'print_r' function call identifier"
    );
    let print_r_call = print_r_call.unwrap();
    assert_eq!(print_r_call.kind, IdentifierKind::Call);

    // Verify containing symbol is set correctly (should be inside calculate method)
    assert!(
        add_call.containing_symbol_id.is_some(),
        "Function call should have containing symbol"
    );

    // Find the calculate method symbol
    let calculate_method = symbols.iter().find(|s| s.name == "calculate").unwrap();

    // Verify the add call is contained within calculate method
    assert_eq!(
        add_call.containing_symbol_id.as_ref(),
        Some(&calculate_method.id),
        "add call should be contained within calculate method"
    );
}

#[test]
fn test_extract_member_access() {
    let php_code = r#"<?php

class User {
    public string $name;
    public string $email;

    public function printInfo(): void {
        echo $this->name;         // Member access: $this->name
        $email = $this->email;    // Member access: $this->email
    }
}
"#;

    let mut parser = init_test_parser();
    let tree = parser.parse(php_code, None).unwrap();

    let workspace_root = PathBuf::from("/tmp/test");
    let mut extractor = PhpExtractor::new(
        "php".to_string(),
        "test.php".to_string(),
        php_code.to_string(),
        &workspace_root,
    );

    let symbols = extractor.extract_symbols(&tree);
    let identifiers = extractor.extract_identifiers(&tree, &symbols);

    // Verify we found member access identifiers
    let name_access = identifiers
        .iter()
        .filter(|id| id.name == "name" && id.kind == IdentifierKind::MemberAccess)
        .count();
    assert!(
        name_access > 0,
        "Should extract 'name' member access identifier"
    );

    let email_access = identifiers
        .iter()
        .filter(|id| id.name == "email" && id.kind == IdentifierKind::MemberAccess)
        .count();
    assert!(
        email_access > 0,
        "Should extract 'email' member access identifier"
    );
}

#[test]
fn test_file_scoped_containing_symbol() {
    // This test ensures we ONLY match symbols from the SAME FILE
    // Critical bug fix from Rust implementation (line 1311-1318 in rust.rs)
    let php_code = r#"<?php

class Service {
    public function process(): void {
        $this->helper();              // Call to helper in same file
    }

    private function helper(): void {
        // Helper method
    }
}
"#;

    let mut parser = init_test_parser();
    let tree = parser.parse(php_code, None).unwrap();

    let workspace_root = PathBuf::from("/tmp/test");
    let mut extractor = PhpExtractor::new(
        "php".to_string(),
        "test.php".to_string(),
        php_code.to_string(),
        &workspace_root,
    );

    let symbols = extractor.extract_symbols(&tree);
    let identifiers = extractor.extract_identifiers(&tree, &symbols);

    // Find the helper call
    let helper_call = identifiers.iter().find(|id| id.name == "helper");
    assert!(helper_call.is_some());
    let helper_call = helper_call.unwrap();

    // Verify it has a containing symbol (the process method)
    assert!(
        helper_call.containing_symbol_id.is_some(),
        "helper call should have containing symbol from same file"
    );

    // Verify the containing symbol is the process method
    let process_method = symbols.iter().find(|s| s.name == "process").unwrap();
    assert_eq!(
        helper_call.containing_symbol_id.as_ref(),
        Some(&process_method.id),
        "helper call should be contained within process method"
    );
}

#[test]
fn test_chained_member_access() {
    let php_code = r#"<?php

class DataService {
    public function execute(): void {
        $result = $user->account->balance;      // Chained member access
        $name = $customer->profile->name;       // Chained member access
    }
}
"#;

    let mut parser = init_test_parser();
    let tree = parser.parse(php_code, None).unwrap();

    let workspace_root = PathBuf::from("/tmp/test");
    let mut extractor = PhpExtractor::new(
        "php".to_string(),
        "test.php".to_string(),
        php_code.to_string(),
        &workspace_root,
    );

    let symbols = extractor.extract_symbols(&tree);
    let identifiers = extractor.extract_identifiers(&tree, &symbols);

    // Should extract the rightmost identifiers in chains
    let balance_access = identifiers
        .iter()
        .find(|id| id.name == "balance" && id.kind == IdentifierKind::MemberAccess);
    assert!(
        balance_access.is_some(),
        "Should extract 'balance' from chained member access"
    );

    let name_access = identifiers
        .iter()
        .find(|id| id.name == "name" && id.kind == IdentifierKind::MemberAccess);
    assert!(
        name_access.is_some(),
        "Should extract 'name' from chained member access"
    );
}

#[test]
fn test_no_duplicate_identifiers() {
    let php_code = r#"<?php

class Test {
    public function run(): void {
        $this->process();
        $this->process();  // Same call twice
    }

    private function process(): void {
    }
}
"#;

    let mut parser = init_test_parser();
    let tree = parser.parse(php_code, None).unwrap();

    let workspace_root = PathBuf::from("/tmp/test");
    let mut extractor = PhpExtractor::new(
        "php".to_string(),
        "test.php".to_string(),
        php_code.to_string(),
        &workspace_root,
    );

    let symbols = extractor.extract_symbols(&tree);
    let identifiers = extractor.extract_identifiers(&tree, &symbols);

    // Should extract BOTH calls (they're at different locations)
    let process_calls: Vec<_> = identifiers
        .iter()
        .filter(|id| id.name == "process" && id.kind == IdentifierKind::Call)
        .collect();

    assert_eq!(
        process_calls.len(),
        2,
        "Should extract both process calls at different locations"
    );

    // Verify they have different line numbers
    assert_ne!(
        process_calls[0].start_line, process_calls[1].start_line,
        "Duplicate calls should have different line numbers"
    );
}

#[test]
fn test_php_type_usage_identifiers() {
    // PHP type annotations should produce type_usage identifiers for class types.
    // This is what drives centrality scores for types like Request, Response, App.
    let php_code = r#"<?php

class UserService {
    public Request $request;

    public function handle(Request $req, int $count): Response {
        $x = $obj instanceof Router;
        return new Response();
    }
}

function standalone(App $app): void {
    $app->run();
}
"#;

    let mut parser = init_test_parser();
    let tree = parser.parse(php_code, None).unwrap();

    let workspace_root = PathBuf::from("/tmp/test");
    let mut extractor = PhpExtractor::new(
        "php".to_string(),
        "test.php".to_string(),
        php_code.to_string(),
        &workspace_root,
    );

    let symbols = extractor.extract_symbols(&tree);
    let identifiers = extractor.extract_identifiers(&tree, &symbols);

    // Collect all type_usage identifiers
    let type_usages: Vec<_> = identifiers
        .iter()
        .filter(|id| id.kind == IdentifierKind::TypeUsage)
        .collect();

    let type_names: Vec<&str> = type_usages.iter().map(|id| id.name.as_str()).collect();

    // Property type: public Request $request
    assert!(
        type_names.contains(&"Request"),
        "Should extract 'Request' type_usage from property type and parameter type. Got: {:?}",
        type_names
    );

    // Return type: ): Response
    assert!(
        type_names.contains(&"Response"),
        "Should extract 'Response' type_usage from return type. Got: {:?}",
        type_names
    );

    // instanceof: $obj instanceof Router
    assert!(
        type_names.contains(&"Router"),
        "Should extract 'Router' type_usage from instanceof. Got: {:?}",
        type_names
    );

    // Standalone function parameter: App $app
    assert!(
        type_names.contains(&"App"),
        "Should extract 'App' type_usage from function parameter. Got: {:?}",
        type_names
    );

    // Request appears twice (property type + parameter type)
    let request_count = type_usages.iter().filter(|id| id.name == "Request").count();
    assert!(
        request_count >= 2,
        "Request should appear at least twice (property + parameter). Got: {}",
        request_count
    );

    // Builtin types should NOT appear as type_usage: int, void
    assert!(
        !type_names.contains(&"int"),
        "Builtin 'int' should not be a type_usage identifier"
    );
    assert!(
        !type_names.contains(&"void"),
        "Builtin 'void' should not be a type_usage identifier"
    );

    // Verify containing symbols are set correctly
    let app_usage = type_usages.iter().find(|id| id.name == "App").unwrap();
    let standalone_fn = symbols.iter().find(|s| s.name == "standalone").unwrap();
    assert_eq!(
        app_usage.containing_symbol_id.as_ref(),
        Some(&standalone_fn.id),
        "App type_usage should be contained within standalone function"
    );
}

#[test]
fn test_php_variable_ref_emission() {
    let php_code = r#"<?php

class Sample {
    public int $bar = 0;

    public function evaluate(int $seed, int $unusedParam): int {
        $count = 0;
        $count += 1;
        $x = 5;
        $x = 7;
        $total = $seed;
        $g = GraphTraversal::reach();
        $f = configure(mode: 5, source: $seed);
        // ghostToken appears only in this comment and must never be extracted
        echo VISIBILITY_UNKNOWN;
        return $total > 0 ? $total : $fallback;
    }
}
"#;

    let mut parser = init_test_parser();
    let tree = parser.parse(php_code, None).unwrap();
    let workspace_root = PathBuf::from("/tmp/test");
    let mut extractor = PhpExtractor::new(
        "php".to_string(),
        "test.php".to_string(),
        php_code.to_string(),
        &workspace_root,
    );
    let symbols = extractor.extract_symbols(&tree);
    let identifiers = extractor.extract_identifiers(&tree, &symbols);

    let var_refs: Vec<&str> = identifiers
        .iter()
        .filter(|id| id.kind == IdentifierKind::VariableRef)
        .map(|id| id.name.as_str())
        .collect();
    let access = |name: &str, start: usize| {
        identifiers
            .iter()
            .find(|id| {
                id.name == name
                    && id.kind == IdentifierKind::VariableRef
                    && id.start_byte == start as u32
            })
            .and_then(|id| id.metadata.as_ref())
            .and_then(|metadata| metadata.get("access"))
            .and_then(serde_json::Value::as_str)
    };

    for expected in [
        "count",
        "seed",
        "total",
        "GraphTraversal",
        "VISIBILITY_UNKNOWN",
        "fallback",
    ] {
        assert!(
            var_refs.contains(&expected),
            "expected variable_ref for {expected}; got {var_refs:?}"
        );
    }

    assert!(
        identifiers
            .iter()
            .any(|id| id.name == "reach" && id.kind == IdentifierKind::Call),
        "GraphTraversal::reach() must still yield a call named reach"
    );

    for forbidden in [
        "unusedParam",
        "mode",
        "ghostToken",
        "Sample",
        "evaluate",
        "bar",
        "this",
        "configure",
        "int",
    ] {
        assert!(
            !var_refs.contains(&forbidden),
            "{forbidden} must NOT be a variable_ref; got {var_refs:?}"
        );
    }
    assert_eq!(access("x", php_code.find("$x = 7").unwrap()), Some("write"));
    assert!(
        !identifiers.iter().any(|id| id.name == "ghostToken"),
        "comment-only ghostToken must not be extracted at all"
    );

    let mut keys: Vec<(String, String, u32, u32)> = identifiers
        .iter()
        .map(|id| {
            (
                id.name.clone(),
                id.kind.to_string(),
                id.start_byte,
                id.end_byte,
            )
        })
        .collect();
    let before = keys.len();
    keys.sort();
    keys.dedup();
    assert_eq!(before, keys.len(), "duplicate identifier rows detected");

    // containing_symbol_id is populated on a variable_ref.
    let evaluate = symbols
        .iter()
        .find(|s| s.name == "evaluate")
        .expect("evaluate method extracted");
    let graph_ref = identifiers
        .iter()
        .find(|id| id.name == "GraphTraversal" && id.kind == IdentifierKind::VariableRef)
        .expect("GraphTraversal variable_ref");
    assert_eq!(
        graph_ref.containing_symbol_id.as_deref(),
        Some(evaluate.id.as_str()),
        "receiver variable_ref should be contained in evaluate"
    );
}

#[test]
fn test_php_variable_access_facts() {
    let code = r#"<?php
$global_plain = 0;
$global_plain = 1;
class Holder { public $value; public $count; }
function consume($value) {}
function typed(Holder $typed) { consume($typed); }
function writes($initial, $box, $argument, $records) {
    $initialized = $initial;
    $plain = 0;
    $plain = $initial;
    $compound = 0;
    $compound += $initial;
    $box->value = $initial;
    $box->count++;
    $box->count--;
    $items = [0];
    $index = 0;
    $items[$index]++;
    $items[$index]--;
    $counter = 0;
    $counter++;
    $counter--;
    consume($argument);
    foreach ($records as &$captured) {
        consume($captured);
    }
}
"#;
    let mut parser = init_test_parser();
    let tree = parser.parse(code, None).unwrap();
    assert!(!tree.root_node().has_error());
    let workspace_root = PathBuf::from("/tmp/test");
    let mut extractor = PhpExtractor::new(
        "php".to_string(),
        "test.php".to_string(),
        code.to_string(),
        &workspace_root,
    );
    let symbols = extractor.extract_symbols(&tree);
    let identifiers = extractor.extract_identifiers(&tree, &symbols);
    let access = |name: &str, kind: IdentifierKind, start: usize| {
        identifiers
            .iter()
            .find(|identifier| {
                identifier.name == name
                    && identifier.kind == kind
                    && identifier.start_byte == start as u32
            })
            .and_then(|identifier| identifier.metadata.as_ref())
            .and_then(|metadata| metadata.get("access"))
            .and_then(serde_json::Value::as_str)
    };
    let has_variable_ref_at = |start: usize| {
        identifiers.iter().any(|identifier| {
            identifier.kind == IdentifierKind::VariableRef && identifier.start_byte == start as u32
        })
    };

    let initialized_target = code.find("$initialized = $initial").unwrap();
    let plain_declaration = code.find("$plain = 0").unwrap();
    let plain = code.find("$plain = $initial").unwrap();
    let global_declaration = code.find("$global_plain = 0").unwrap();
    let global_write = code.find("$global_plain = 1").unwrap();
    let compound = code.find("$compound += $initial").unwrap();
    let member = code.find("$box->value").unwrap() + "$box->".len();
    let argument = code.find("consume($argument)").unwrap() + "consume(".len();
    let binding = code.find("$captured)").unwrap();
    let member_increment = code.find("$box->count++").unwrap();
    let member_decrement = code.find("$box->count--").unwrap();
    let indexed_increment = code.find("$items[$index]++").unwrap();
    let indexed_decrement = code.find("$items[$index]--").unwrap();
    let scalar_increment = code.find("$counter++").unwrap();
    let scalar_decrement = code.find("$counter--").unwrap();
    let items_declaration = code.find("$items = [0]").unwrap();
    let index_declaration = code.find("$index = 0").unwrap();
    let counter_declaration = code.find("$counter = 0").unwrap();
    assert!(!has_variable_ref_at(initialized_target));
    assert!(!has_variable_ref_at(plain_declaration));
    assert!(!has_variable_ref_at(global_declaration));
    assert!(!has_variable_ref_at(items_declaration));
    assert!(!has_variable_ref_at(index_declaration));
    assert!(!has_variable_ref_at(counter_declaration));
    assert_eq!(
        access("global_plain", IdentifierKind::VariableRef, global_write),
        Some("write")
    );
    assert_eq!(
        access(
            "initial",
            IdentifierKind::VariableRef,
            code.find("$initialized = $initial").unwrap() + "$initialized = ".len()
        ),
        Some("read")
    );
    assert_eq!(
        access("plain", IdentifierKind::VariableRef, plain),
        Some("write")
    );
    assert_eq!(
        access("compound", IdentifierKind::VariableRef, compound),
        Some("read_write")
    );
    assert_eq!(
        access("value", IdentifierKind::MemberAccess, member),
        Some("write")
    );
    assert_eq!(
        identifiers
            .iter()
            .filter(|identifier| identifier.start_byte == member as u32)
            .count(),
        1
    );
    assert_eq!(
        access("argument", IdentifierKind::VariableRef, argument),
        Some("read")
    );
    for start in [member_increment, member_decrement] {
        assert_eq!(
            access(
                "count",
                IdentifierKind::MemberAccess,
                start + "$box->".len()
            ),
            Some("write")
        );
        assert_eq!(
            access("box", IdentifierKind::VariableRef, start),
            Some("read")
        );
        assert_eq!(
            identifiers
                .iter()
                .filter(|identifier| { identifier.start_byte == (start + "$box->".len()) as u32 })
                .count(),
            1
        );
    }
    for start in [indexed_increment, indexed_decrement] {
        assert_eq!(
            access("items", IdentifierKind::VariableRef, start),
            Some("read")
        );
        assert_eq!(
            access(
                "index",
                IdentifierKind::VariableRef,
                start + "$items[".len()
            ),
            Some("read")
        );
    }
    for start in [scalar_increment, scalar_decrement] {
        assert_eq!(
            access("counter", IdentifierKind::VariableRef, start),
            Some("write")
        );
    }
    let call_start = code.find("consume($argument)").unwrap();
    let call = identifiers
        .iter()
        .find(|identifier| identifier.start_byte == call_start as u32)
        .expect("consume call identifier");
    assert_eq!(call.name, "consume");
    assert_eq!(call.kind, IdentifierKind::Call);
    assert_eq!(
        call.metadata
            .as_ref()
            .and_then(|metadata| metadata.get("access")),
        None
    );
    assert!(
        identifiers
            .iter()
            .any(|identifier| identifier.kind == IdentifierKind::Call)
    );
    assert!(
        identifiers
            .iter()
            .filter(|identifier| identifier.kind == IdentifierKind::Call)
            .all(|identifier| identifier
                .metadata
                .as_ref()
                .is_none_or(|metadata| !metadata.contains_key("access")))
    );
    assert!(
        identifiers
            .iter()
            .any(|identifier| identifier.kind == IdentifierKind::TypeUsage)
    );
    assert!(
        identifiers
            .iter()
            .filter(|identifier| identifier.kind == IdentifierKind::TypeUsage)
            .all(|identifier| identifier
                .metadata
                .as_ref()
                .is_none_or(|metadata| !metadata.contains_key("access")))
    );
    if symbols.iter().any(|symbol| {
        symbol.name == "captured"
            && symbol.start_byte <= binding as u32
            && symbol.end_byte >= (binding + "$captured".len()) as u32
    }) {
        assert!(
            identifiers
                .iter()
                .all(|identifier| identifier.start_byte != binding as u32)
        );
    } else {
        assert_eq!(
            access("captured", IdentifierKind::VariableRef, binding),
            Some("write")
        );
    }
}

#[test]
fn shadowed_parameter_access_stays_at_its_byte_occurrences() {
    let code = r#"<?php
$value = 1;
function update($value) {
    $value += 1;
    return $value;
}
consume($value);
"#;
    let mut parser = init_test_parser();
    let tree = parser.parse(code, None).unwrap();
    assert!(!tree.root_node().has_error());
    let workspace_root = PathBuf::from("/tmp/test");
    let mut extractor = PhpExtractor::new(
        "php".to_string(),
        "test.php".to_string(),
        code.to_string(),
        &workspace_root,
    );
    let symbols = extractor.extract_symbols(&tree);
    let identifiers = extractor.extract_identifiers(&tree, &symbols);
    let access_at = |start: usize| -> Option<String> {
        identifiers
            .iter()
            .find(|identifier| {
                identifier.start_byte == start as u32
                    && identifier.kind == IdentifierKind::VariableRef
            })?
            .metadata
            .as_ref()?
            .get("access")?
            .as_str()
            .map(str::to_owned)
    };

    assert_eq!(
        access_at(code.find("$value += 1").unwrap()),
        Some("read_write".to_string())
    );
    assert_eq!(
        access_at(code.find("return $value").unwrap() + "return ".len()),
        Some("read".to_string())
    );
    assert_eq!(
        access_at(code.find("consume($value)").unwrap() + "consume(".len()),
        Some("read".to_string())
    );
}
