use crate::json::JsonExtractor;
use std::path::PathBuf;
use tree_sitter::Parser;

fn capture_literals(json: &str) -> Vec<crate::base::Literal> {
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_json::LANGUAGE.into())
        .expect("json grammar");
    let tree = parser.parse(json, None).expect("parse");
    let mut extractor = JsonExtractor::new(
        "json".to_string(),
        "test.json".to_string(),
        json.to_string(),
        &PathBuf::from("/tmp/test"),
    );
    extractor.extract_symbols(&tree);
    extractor.get_literals()
}

#[test]
fn nested_string_literals_use_path_aware_carriers() {
    let json = r#"{
  "worker": {
    "name": "fixture",
    "api_url": "https://api.example.com/workers"
  }
}"#;

    let literals = capture_literals(json);
    let name = literals
        .iter()
        .find(|literal| literal.literal_text == "fixture")
        .expect("worker.name literal");
    assert_eq!(name.carrier.as_deref(), Some("worker.name"));

    let api_url = literals
        .iter()
        .find(|literal| literal.literal_text.contains("api.example.com"))
        .expect("worker.api_url literal");
    assert_eq!(api_url.carrier.as_deref(), Some("worker.api_url"));
}

#[test]
fn top_level_string_literal_carrier_is_bare_key() {
    let json = r#"{"name": "fixture"}"#;
    let literals = capture_literals(json);
    let literal = literals
        .iter()
        .find(|literal| literal.literal_text == "fixture")
        .expect("name literal");
    assert_eq!(literal.carrier.as_deref(), Some("name"));
}
