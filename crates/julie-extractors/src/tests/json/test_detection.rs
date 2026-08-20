use crate::base::Symbol;
use crate::json::JsonExtractor;
use std::path::PathBuf;
use tree_sitter::Parser;

fn extract_symbols(code: &str) -> Vec<Symbol> {
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_json::LANGUAGE.into())
        .expect("Error loading JSON grammar");
    let tree = parser.parse(code, None).expect("Failed to parse JSON");
    let mut extractor = JsonExtractor::new(
        "json".to_string(),
        "test.json".to_string(),
        code.to_string(),
        &PathBuf::from("/tmp/test"),
    );
    extractor.extract_symbols(&tree)
}

fn role(symbols: &[Symbol], name: &str, key: &str) -> bool {
    symbols
        .iter()
        .find(|symbol| symbol.name == name)
        .is_some_and(|symbol| has_role(symbol, key))
}

fn has_role(symbol: &Symbol, key: &str) -> bool {
    symbol
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get(key))
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false)
}

#[test]
fn json_schema_test_suite_groups_and_cases_emit_roles() {
    let symbols = extract_symbols(
        r#"[
  {
    "description": "string schema",
    "schema": {"type": "string"},
    "tests": [
      {"description": "valid string", "data": "ok", "valid": true},
      {"description": "invalid number", "data": 3, "valid": false}
    ]
  }
]"#,
    );

    assert!(role(&symbols, "description", "test_container"));
    assert_eq!(
        symbols
            .iter()
            .filter(|symbol| symbol.name == "description")
            .filter(|symbol| {
                symbol
                    .metadata
                    .as_ref()
                    .and_then(|metadata| metadata.get("is_test"))
                    .and_then(serde_json::Value::as_bool)
                    == Some(true)
            })
            .count(),
        2
    );
}

#[test]
fn boolean_schema_groups_match_the_suite_shape() {
    let symbols = extract_symbols(
        r#"[
  {
    "description": "boolean schema",
    "schema": true,
    "tests": [{"description": "valid data", "data": {}, "valid": true}]
  }
]"#,
    );

    assert!(role(&symbols, "description", "test_container"));
    assert!(symbols.iter().any(|symbol| has_role(symbol, "is_test")));
}

#[test]
fn lookalike_shapes_outside_top_level_groups_do_not_emit_roles() {
    let symbols = extract_symbols(
        r#"{
  "description": "ordinary",
  "schema": {"type": "string"},
  "tests": [{"description": "ordinary", "data": true, "valid": true}],
  "nested": [{"description": "ordinary", "data": true, "valid": true}]
}"#,
    );

    assert!(symbols.iter().all(|symbol| {
        symbol
            .metadata
            .as_ref()
            .is_none_or(|metadata| !metadata.contains_key("test_container"))
            && symbol
                .metadata
                .as_ref()
                .is_none_or(|metadata| !metadata.contains_key("is_test"))
    }));
}

#[test]
fn incomplete_groups_and_cases_do_not_emit_roles() {
    let symbols = extract_symbols(
        r#"[
  {"description": "no schema", "tests": []},
  {"description": "bad test", "schema": {}, "tests": [
    {"description": "missing valid", "data": 1},
    {"description": "bad valid", "data": 1, "valid": "true"}
  ]}
]"#,
    );

    assert_eq!(
        symbols
            .iter()
            .filter(|symbol| has_role(symbol, "test_container"))
            .count(),
        1
    );
    assert_eq!(
        symbols
            .iter()
            .filter(|symbol| has_role(symbol, "is_test"))
            .count(),
        0
    );
}
