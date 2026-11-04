use crate::yaml::YamlExtractor;
use std::path::PathBuf;
use tree_sitter::Parser;

fn capture_literals(yaml: &str) -> Vec<crate::base::Literal> {
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_yaml::LANGUAGE.into())
        .expect("yaml grammar");
    let tree = parser.parse(yaml, None).expect("parse");
    let mut extractor = YamlExtractor::new(
        "yaml".to_string(),
        "test.yaml".to_string(),
        yaml.to_string(),
        &PathBuf::from("/tmp/test"),
    );
    extractor.extract_symbols(&tree);
    extractor.get_literals()
}

#[test]
fn nested_mapping_literals_use_parent_key_paths() {
    let yaml = r#"
worker:
  name: fixture
  database:
    host: localhost
"#;

    let literals = capture_literals(yaml);
    let name = literals
        .iter()
        .find(|literal| literal.literal_text == "fixture")
        .expect("worker.name literal");
    assert_eq!(name.carrier.as_deref(), Some("worker.name"));

    let host = literals
        .iter()
        .find(|literal| literal.literal_text == "localhost")
        .expect("worker.database.host literal");
    assert_eq!(host.carrier.as_deref(), Some("worker.database.host"));
}

#[test]
fn yaml_config_literals_keep_mapping_scalars_and_exclude_keys_and_comments() {
    let yaml = r#"# "comment value" is not a YAML mapping value
worker:
  name: fixture
  api_url: https://api.example.com/workers
  config_path: ./config/worker.yaml
  retry_count: 3
  enabled: true
  steps:
    - name: compile
    - uses: actions/checkout@v4
"#;
    let literals = capture_literals(yaml);
    let expected = [
        ("fixture", "worker.name", "fixture"),
        (
            "https://api.example.com/workers",
            "worker.api_url",
            "https://api.example.com/workers",
        ),
        (
            "./config/worker.yaml",
            "worker.config_path",
            "./config/worker.yaml",
        ),
        ("3", "worker.retry_count", "3"),
        ("true", "worker.enabled", "true"),
        ("compile", "worker.steps[0].name", "compile"),
        (
            "actions/checkout@v4",
            "worker.steps[1].uses",
            "actions/checkout@v4",
        ),
    ];

    assert_eq!(literals.len(), expected.len());
    for (literal, (text, carrier, token)) in literals.iter().zip(expected) {
        assert_eq!(literal.literal_text, text);
        assert_eq!(literal.carrier.as_deref(), Some(carrier));
        assert_eq!(
            yaml.get(literal.start_byte as usize..literal.end_byte as usize),
            Some(token)
        );
    }
}
