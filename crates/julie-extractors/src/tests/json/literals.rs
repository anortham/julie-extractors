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

#[test]
fn json_config_literals_keep_string_values_and_exclude_keys_and_comments() {
    let json = r#"{
  // "comment value" is not a JSON string value
  "worker": {
    "name": "fixture",
    "api_url": "https://api.example.com/workers",
    "config_path": "./config/worker.json",
    "steps": [
      { "name": "compile" },
      { "uses": "actions/checkout@v4" }
    ]
  }
}"#;
    let literals = capture_literals(json);
    let expected = [
        ("fixture", "worker.name", "\"fixture\""),
        (
            "https://api.example.com/workers",
            "worker.api_url",
            "\"https://api.example.com/workers\"",
        ),
        (
            "./config/worker.json",
            "worker.config_path",
            "\"./config/worker.json\"",
        ),
        ("compile", "worker.steps[0].name", "\"compile\""),
        (
            "actions/checkout@v4",
            "worker.steps[1].uses",
            "\"actions/checkout@v4\"",
        ),
    ];

    assert_eq!(literals.len(), expected.len());
    for (literal, (text, carrier, token)) in literals.iter().zip(expected) {
        assert_eq!(literal.literal_text, text);
        assert_eq!(literal.carrier.as_deref(), Some(carrier));
        assert_eq!(
            json.get(literal.start_byte as usize..literal.end_byte as usize),
            Some(token)
        );
    }
}
