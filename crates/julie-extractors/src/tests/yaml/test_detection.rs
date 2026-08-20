use crate::base::Symbol;
use crate::yaml::YamlExtractor;
use std::path::PathBuf;
use tree_sitter::Parser;

fn extract_symbols(source: &str) -> Vec<Symbol> {
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_yaml::LANGUAGE.into())
        .expect("YAML grammar should load");
    let tree = parser.parse(source, None).expect("YAML should parse");
    let mut extractor = YamlExtractor::new(
        "yaml".to_string(),
        "test.yaml".to_string(),
        source.to_string(),
        &PathBuf::from("/tmp/test"),
    );
    extractor.extract_symbols(&tree)
}

fn symbol_at_line<'a>(symbols: &'a [Symbol], name: &str, line: u32) -> &'a Symbol {
    symbols
        .iter()
        .find(|symbol| symbol.name == name && symbol.start_line == line)
        .expect("expected YAML symbol")
}

fn role(symbol: &Symbol, key: &str) -> bool {
    symbol
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get(key))
        .and_then(|value| value.as_bool())
        == Some(true)
}

fn has_test_role(symbol: &Symbol) -> bool {
    ["test_container", "is_test", "test_lifecycle"]
        .into_iter()
        .any(|key| role(symbol, key))
}

#[test]
fn container_structure_test_v2_roles_require_exact_structure() {
    let source = r#"schemaVersion: 2.0.0
commandTests:
  - name: run command
    command: [echo, hello]
    setup: setup.sh
    teardown: teardown.sh
  - name: "quoted command"
    command: [echo, quoted]
  - name: 123
    command: [echo, numeric]
    metadata:
      name: nested name
      setup: nested.sh
setup: outside.sh
teardown: outside.sh
"#;
    let symbols = extract_symbols(source);

    assert!(role(
        symbol_at_line(&symbols, "commandTests", 2),
        "test_container"
    ));
    assert!(role(symbol_at_line(&symbols, "name", 3), "is_test"));
    assert!(!role(symbol_at_line(&symbols, "name", 3), "test_case"));
    assert!(role(symbol_at_line(&symbols, "setup", 5), "test_lifecycle"));
    assert!(role(symbol_at_line(&symbols, "setup", 5), "is_test"));
    assert!(role(
        symbol_at_line(&symbols, "teardown", 6),
        "test_lifecycle"
    ));
    assert!(role(symbol_at_line(&symbols, "teardown", 6), "is_test"));
    assert!(role(symbol_at_line(&symbols, "name", 7), "is_test"));
    assert!(!role(symbol_at_line(&symbols, "name", 7), "test_case"));
    assert!(!role(symbol_at_line(&symbols, "name", 9), "is_test"));
    assert!(!role(symbol_at_line(&symbols, "name", 12), "is_test"));
    assert!(!role(
        symbol_at_line(&symbols, "setup", 14),
        "test_lifecycle"
    ));
    assert!(!role(
        symbol_at_line(&symbols, "teardown", 15),
        "test_lifecycle"
    ));
}

#[test]
fn container_structure_test_roles_reject_missing_marker_and_wrong_scope() {
    for source in [
        r#"commandTests:
  - name: no marker
    setup: setup.sh
    teardown: teardown.sh
"#,
        r#"schemaVersion: 1.0.0
commandTests:
  - name: wrong schema
    setup: setup.sh
"#,
        r#"schemaVersion: 2.0.0
wrapper:
  commandTests:
    - name: nested command tests
      setup: setup.sh
"#,
        r#"schemaVersion: 2.0.0
commandTests: {}
"#,
    ] {
        let symbols = extract_symbols(source);
        assert!(!symbols.iter().any(has_test_role));
    }
}
