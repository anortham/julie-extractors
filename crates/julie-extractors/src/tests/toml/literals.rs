use crate::toml::TomlExtractor;
use std::path::PathBuf;
use tree_sitter::Parser;

fn capture_literals(toml: &str) -> Vec<crate::base::Literal> {
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_toml_ng::LANGUAGE.into())
        .expect("toml grammar");
    let tree = parser.parse(toml, None).expect("parse");
    let mut extractor = TomlExtractor::new(
        "toml".to_string(),
        "test.toml".to_string(),
        toml.to_string(),
        &PathBuf::from("/tmp/test"),
    );
    extractor.extract_symbols(&tree);
    extractor.get_literals()
}

#[test]
fn table_qualified_string_literals_use_dotted_carriers() {
    let toml = r#"
[database]
host = "localhost"
port = "5432"

[database.credentials]
username = "admin"
"#;

    let literals = capture_literals(toml);
    let host = literals
        .iter()
        .find(|literal| literal.literal_text == "localhost")
        .expect("database.host literal");
    assert_eq!(host.carrier.as_deref(), Some("database.host"));

    let username = literals
        .iter()
        .find(|literal| literal.literal_text == "admin")
        .expect("database.credentials.username literal");
    assert_eq!(
        username.carrier.as_deref(),
        Some("database.credentials.username")
    );
}
