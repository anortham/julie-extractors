use crate::base::Symbol;
use crate::toml::TomlExtractor;
use std::path::Path;
use tree_sitter::Parser;

fn symbols(source: &str) -> Vec<Symbol> {
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_toml_ng::LANGUAGE.into())
        .expect("Error loading TOML grammar");
    let tree = parser.parse(source, None).expect("Failed to parse TOML");
    let mut extractor = TomlExtractor::new(
        "toml".to_string(),
        "test.toml".to_string(),
        source.to_string(),
        Path::new("/tmp/test"),
    );
    extractor.extract_symbols(&tree)
}

fn has_role(symbol: &Symbol, role: &str) -> bool {
    symbol
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get(role))
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false)
}

fn symbol<'a>(symbols: &'a [Symbol], name: &str) -> &'a Symbol {
    symbols
        .iter()
        .find(|symbol| symbol.name == name)
        .unwrap_or_else(|| panic!("expected {name} in {symbols:?}"))
}

fn assert_no_test_roles(symbols: &[Symbol]) {
    assert!(symbols.iter().all(|symbol| {
        !has_role(symbol, "is_test")
            && !has_role(symbol, "test_container")
            && !has_role(symbol, "test_lifecycle")
    }));
}

#[test]
fn trycmd_dotted_contract_marks_bin_name() {
    let symbols = symbols(
        r#"
bin.name = "demo"
args = ["--help"]
status = 0
stdout = "demo\n"
stderr = ""
"#,
    );

    assert!(has_role(symbol(&symbols, "bin.name"), "is_test"));
    assert!(!has_role(symbol(&symbols, "status"), "is_test"));
    assert!(!has_role(symbol(&symbols, "stdout"), "is_test"));
    assert!(!has_role(symbol(&symbols, "stderr"), "is_test"));
}

#[test]
fn trycmd_table_contract_marks_bin_table() {
    let symbols = symbols(
        r#"
[bin]
name = "demo"
status = 0
stdout = "demo\n"
stderr = ""
"#,
    );

    assert!(has_role(symbol(&symbols, "bin"), "is_test"));
}

#[test]
fn trycmd_requires_all_expected_streams() {
    let symbols = symbols(
        r#"
bin.name = "demo"
status = 0
stdout = "demo\n"
"#,
    );

    assert_no_test_roles(&symbols);
}

#[test]
fn nextest_named_tables_emit_roles_with_version_marker() {
    let symbols = symbols(
        r#"
nextest-version = 0.9

[test-groups.unit]
max-threads = 1

[scripts.setup.database]
command = "prepare-db"
"#,
    );

    assert!(has_role(
        symbol(&symbols, "test-groups.unit"),
        "test_container"
    ));
    let setup = symbol(&symbols, "scripts.setup.database");
    assert!(has_role(setup, "is_test"));
    assert!(has_role(setup, "test_lifecycle"));
}

#[test]
fn nextest_named_tables_emit_roles_with_experimental_marker() {
    let symbols = symbols(
        r#"
experimental = true

[test-groups.unit]
max-threads = 1
"#,
    );

    assert!(has_role(
        symbol(&symbols, "test-groups.unit"),
        "test_container"
    ));
}

#[test]
fn nextest_roles_require_marker_and_named_paths() {
    let without_marker = symbols(
        r#"
[test-groups.unit]
max-threads = 1

[scripts.setup.database]
command = "prepare-db"
"#,
    );
    assert_no_test_roles(&without_marker);

    let generic_paths = symbols(
        r#"
nextest-version = 0.9

[test-groups]
unit = { max-threads = 1 }

[scripts.setup]
database = { command = "prepare-db" }
"#,
    );
    assert_no_test_roles(&generic_paths);
}
