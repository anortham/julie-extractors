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
