use crate::base::{
    ExtractionResults, Identifier, IdentifierKind, Relationship, RelationshipKind, Symbol,
    SymbolKind,
};
use crate::extract_canonical;
use std::path::Path;

fn extract(source: &str) -> ExtractionResults {
    extract_canonical("styles/functions.css", source, Path::new("/tmp/test"))
        .expect("canonical CSS extraction must succeed")
}

fn symbol<'a>(result: &'a ExtractionResults, name: &str) -> &'a Symbol {
    result
        .symbols
        .iter()
        .find(|symbol| symbol.name == name)
        .unwrap_or_else(|| panic!("symbol {name:?} missing from {:#?}", result.symbols))
}

fn calls(result: &ExtractionResults) -> Vec<&Identifier> {
    result
        .identifiers
        .iter()
        .filter(|identifier| identifier.kind == IdentifierKind::Call)
        .collect()
}

fn call_names(result: &ExtractionResults) -> Vec<&str> {
    calls(result)
        .into_iter()
        .map(|identifier| identifier.name.as_str())
        .collect()
}

fn call<'a>(result: &'a ExtractionResults, name: &str) -> &'a Identifier {
    calls(result)
        .into_iter()
        .find(|identifier| identifier.name == name)
        .unwrap_or_else(|| panic!("call {name:?} missing from {:#?}", result.identifiers))
}

fn source_text<'a>(source: &'a str, identifier: &Identifier) -> &'a str {
    &source[identifier.start_byte as usize..identifier.end_byte as usize]
}

fn arg_count(identifier: &Identifier) -> Option<u64> {
    identifier.metadata.as_ref()?.get("arg_count")?.as_u64()
}

fn call_edges(result: &ExtractionResults) -> Vec<(&str, &str)> {
    let name_of = |id: &str| {
        result
            .symbols
            .iter()
            .find(|symbol| symbol.id == id)
            .map_or("?", |symbol| symbol.name.as_str())
    };
    result
        .relationships
        .iter()
        .filter(|relationship| relationship.kind == RelationshipKind::Calls)
        .map(|relationship: &Relationship| {
            (
                name_of(&relationship.from_symbol_id),
                name_of(&relationship.to_symbol_id),
            )
        })
        .collect()
}

#[test]
fn custom_function_declarations_are_functions_named_by_their_dashed_name() {
    let source = "@function --anchor_css() { result: 41; }\n@function --名_css() { result: 42; }\n@function --escaped\\ name_css() { result: 43; }\n@function --foo\\31 () { result: 44; }\n@function --\u{2000}_css() { result: 45; }\n@function --scale(--value, --factor: 2) { result: calc(var(--value) * var(--factor)); }\n";
    let result = extract(source);

    for (name, signature_start) in [
        ("--anchor_css", "@function --anchor_css()"),
        ("--名_css", "@function --名_css()"),
        ("--escaped name_css", "@function --escaped\\ name_css()"),
        ("--foo1", "@function --foo\\31 ()"),
        ("--\u{2000}_css", "@function --\u{2000}_css()"),
        ("--scale", "@function --scale(--value, --factor: 2)"),
    ] {
        let function = symbol(&result, name);
        assert_eq!(function.kind, SymbolKind::Function, "{name}");
        let signature = function.signature.as_deref().unwrap_or_default();
        assert!(
            signature.starts_with(signature_start),
            "{name} signature {signature:?}"
        );
        let metadata = function.metadata.as_ref().expect("at-rule metadata");
        assert_eq!(metadata["atRuleType"], "function", "{name}");
        assert_eq!(metadata["type"], "at-rule", "{name}");
    }

    let en_quad = symbol(&result, "--\u{2000}_css");
    let metadata = en_quad.metadata.as_ref().unwrap();
    assert_eq!(metadata["prelude"], "--\u{2000}_css()");
    assert_eq!(metadata["ruleName"], "@function --\u{2000}_css()");
    assert!(
        !result
            .symbols
            .iter()
            .any(|symbol| symbol.name.starts_with("@function")),
        "{:#?}",
        result.symbols
    );
}

#[test]
fn custom_function_calls_carry_the_declared_name_and_link_to_the_declaration() {
    let source = "@function --anchor_css() { result: 41; }\n@function --caller_css() { result: --anchor_css(); }\n@function --名_css() { result: 42; }\n@function --escaped\\ name_css() { result: 43; }\n@function --caller_unicode_css() { result: --名_css(); }\n@function --caller_escaped_css() { result: --escaped\\ name_css(); }\n";
    let result = extract(source);

    assert_eq!(
        call_names(&result),
        ["--anchor_css", "--名_css", "--escaped name_css"]
    );
    for (name, raw, caller) in [
        ("--anchor_css", "--anchor_css", "--caller_css"),
        ("--名_css", "--名_css", "--caller_unicode_css"),
        (
            "--escaped name_css",
            "--escaped\\ name_css",
            "--caller_escaped_css",
        ),
    ] {
        let identifier = call(&result, name);
        assert_eq!(source_text(source, identifier), raw);
        assert_eq!(arg_count(identifier), Some(0), "{name}");
        assert_eq!(
            identifier.containing_symbol_id.as_deref(),
            Some(symbol(&result, caller).id.as_str()),
            "{name}"
        );
    }

    assert_eq!(
        call_edges(&result),
        [
            ("--caller_css", "--anchor_css"),
            ("--caller_unicode_css", "--名_css"),
            ("--caller_escaped_css", "--escaped name_css"),
        ]
    );
    let first_edge = result
        .relationships
        .iter()
        .find(|relationship| relationship.kind == RelationshipKind::Calls)
        .unwrap();
    let span = first_edge.span.expect("call edge span");
    let anchor_call = call(&result, "--anchor_css");
    assert_eq!(
        (span.start_byte, span.end_byte),
        (anchor_call.start_byte, anchor_call.end_byte)
    );
    let metadata = first_edge.metadata.as_ref().unwrap();
    assert_eq!(metadata["referenceName"], "--anchor_css");
    assert_eq!(metadata["referenceType"], "custom-function");
}

#[test]
fn a_call_in_a_result_descriptor_with_a_unicode_name_is_recorded() {
    let source = "@function --foo() { result: 1; }\n@function --foo名() { result: 2; }\n@function --caller_css() { result: --foo名(); }\n";
    let result = extract(source);

    assert_eq!(call_names(&result), ["--foo名"]);
    let identifier = call(&result, "--foo名");
    assert_eq!(source_text(source, identifier), "--foo名");
    assert_eq!(identifier.start_line, 3);
    assert_eq!(arg_count(identifier), Some(0));
    assert_eq!(call_edges(&result), [("--caller_css", "--foo名")]);
}

#[test]
fn escaped_and_en_quad_calls_match_their_declarations() {
    let source = "@function --foo1() { result: 1; }\n@function --\u{2000}_css() { result: 2; }\n@function --caller() { result: --foo\\31 (1, 2) + --\u{2000}_css(); }\n";
    let result = extract(source);

    assert_eq!(call_names(&result), ["--foo1", "--\u{2000}_css"]);
    assert_eq!(source_text(source, call(&result, "--foo1")), "--foo\\31 ");
    assert_eq!(arg_count(call(&result, "--foo1")), Some(2));
    assert_eq!(
        call_edges(&result),
        [("--caller", "--foo1"), ("--caller", "--\u{2000}_css")]
    );
}

#[test]
fn calls_in_property_values_count_comma_separated_arguments() {
    let source = ".card { width: --size(3px, 4px); margin: --pad(); padding: --gap({1px, 2px}, calc(1px + 2px), \"a,b\"); color: --tint( /* none */ ); }\n@function --size(--a, --b) { result: calc(var(--a) + var(--b)); }\n";
    let result = extract(source);

    let counts: Vec<(&str, Option<u64>)> = calls(&result)
        .into_iter()
        .filter(|identifier| identifier.name.starts_with("--"))
        .map(|identifier| (identifier.name.as_str(), arg_count(identifier)))
        .collect();
    assert_eq!(
        counts,
        [
            ("--size", Some(2)),
            ("--pad", Some(0)),
            ("--gap", Some(3)),
            ("--tint", Some(0)),
        ]
    );
    assert_eq!(call_edges(&result), [(".card", "--size")]);
    for builtin in ["calc", "var"] {
        assert!(
            calls(&result)
                .iter()
                .filter(|identifier| identifier.name == builtin)
                .all(|identifier| arg_count(identifier).is_none()),
            "{builtin}"
        );
    }
}

#[test]
fn nested_custom_function_calls_are_each_recorded() {
    let source =
        "@function --inner(--x) { result: var(--x); }\n.a { width: --outer(--inner(1px), 2px); }\n";
    let result = extract(source);

    let names: Vec<(&str, Option<u64>)> = calls(&result)
        .into_iter()
        .filter(|identifier| identifier.name.starts_with("--"))
        .map(|identifier| (identifier.name.as_str(), arg_count(identifier)))
        .collect();
    assert_eq!(names, [("--outer", Some(2)), ("--inner", Some(1))]);
    assert_eq!(call_edges(&result), [(".a", "--inner")]);
}

#[test]
fn dashed_names_outside_calls_are_not_calls() {
    let source = "/* --commented(1) */\n:root { --brand: red; content: \"--quoted(1)\"; background: url(\"--in-url(1).png\"); }\n.b { color: var(--brand); width: --brand (1px); }\n@media (--narrow) { .c { color: red; } }\n";
    let result = extract(source);

    assert!(
        calls(&result)
            .iter()
            .all(|identifier| !identifier.name.starts_with("--")),
        "{:#?}",
        calls(&result)
    );
    assert!(call_edges(&result).is_empty());
    assert!(
        result
            .identifiers
            .iter()
            .any(|identifier| identifier.name == "--brand"
                && identifier.kind == IdentifierKind::VariableRef)
    );
}

#[test]
fn a_call_with_no_same_file_declaration_has_no_edge() {
    let source = ".a { width: --elsewhere(1px); }\n";
    let result = extract(source);

    assert_eq!(call_names(&result), ["--elsewhere"]);
    assert_eq!(arg_count(call(&result, "--elsewhere")), Some(1));
    assert!(call_edges(&result).is_empty());
}

#[test]
fn other_at_rules_keep_their_names() {
    let source = "@layer components { .a { color: red; } }\n@property --brand { syntax: \"<color>\"; inherits: true; initial-value: red; }\n@supports (display: grid) { .b { display: grid; } }\n";
    let result = extract(source);

    for name in ["@layer components", "@property --brand", ".a", ".b"] {
        symbol(&result, name);
    }
    assert!(
        !result
            .symbols
            .iter()
            .any(|symbol| symbol.kind == SymbolKind::Function),
        "{:#?}",
        result.symbols
    );
}

#[test]
fn a_style_block_call_keeps_file_coordinates() {
    let source = "<html><head><style>\n@function --pad() { result: 1px; }\n.x { padding: --pad(); }\n</style></head></html>\n";
    let result = extract_canonical("page.html", source, Path::new("/tmp/test"))
        .expect("canonical HTML extraction must succeed");

    let identifier = call(&result, "--pad");
    assert_eq!(source_text(source, identifier), "--pad");
    assert_eq!(identifier.start_line, 3);
    assert_eq!(symbol(&result, "--pad").kind, SymbolKind::Function);
    assert_eq!(call_edges(&result), [(".x", "--pad")]);
}
