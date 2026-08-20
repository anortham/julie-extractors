use crate::base::{Symbol, SymbolKind};
use crate::rust::RustExtractor;
use serde_json::Value;
use std::path::PathBuf;

fn extract(source: &str) -> Vec<Symbol> {
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_rust::LANGUAGE.into())
        .expect("Rust grammar should load");
    let tree = parser.parse(source, None).expect("Rust should parse");
    RustExtractor::new(
        "rust".to_string(),
        "test.rs".to_string(),
        source.to_string(),
        &PathBuf::from("/tmp/test"),
    )
    .extract_symbols(&tree)
}

fn role(symbol: &Symbol, key: &str) -> bool {
    symbol
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get(key))
        .and_then(Value::as_bool)
        == Some(true)
}

fn symbol<'a>(symbols: &'a [Symbol], name: &str, kind: SymbolKind) -> &'a Symbol {
    symbols
        .iter()
        .find(|symbol| symbol.name == name && symbol.kind == kind)
        .unwrap_or_else(|| panic!("expected {kind:?} symbol {name}, got {symbols:?}"))
}

#[test]
fn exact_cfg_test_modules_are_test_containers() {
    let symbols = extract(
        r#"#[cfg(test)]
mod unit_tests {
    fn setup() {}
    fn teardown() {}
}

#[cfg( test )]
pub mod spaced_tests {}
"#,
    );

    assert!(role(
        symbol(&symbols, "unit_tests", SymbolKind::Namespace),
        "test_container"
    ));
    assert!(role(
        symbol(&symbols, "spaced_tests", SymbolKind::Namespace),
        "test_container"
    ));
    assert!(!role(
        symbol(&symbols, "setup", SymbolKind::Function),
        "test_container"
    ));
    assert!(!role(
        symbol(&symbols, "teardown", SymbolKind::Function),
        "test_container"
    ));
}

#[test]
fn non_exact_cfg_attributes_and_ordinary_modules_are_not_containers() {
    let symbols = extract(
        r#"mod ordinary {
    fn setup() {}
    fn teardown() {}
}

#[cfg(feature = "test")]
mod feature_tests {}

#[cfg(testing)]
mod testing_name {}

#[cfg(any(test, feature = "test"))]
mod combined_cfg {}

#[cfg(not(test))]
mod negated_cfg {}

fn setup() {}
fn teardown() {}
"#,
    );

    for name in [
        "ordinary",
        "feature_tests",
        "testing_name",
        "combined_cfg",
        "negated_cfg",
    ] {
        assert!(
            !role(
                symbol(&symbols, name, SymbolKind::Namespace),
                "test_container"
            ),
            "{name} must not be a test container"
        );
    }
    for name in ["setup", "teardown"] {
        assert!(
            !symbols
                .iter()
                .filter(|symbol| symbol.name == name && symbol.kind == SymbolKind::Function)
                .any(|symbol| role(symbol, "test_container")),
            "helper function {name} must not be a test container"
        );
    }
}
