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
fn compound_cfg_test_modules_are_test_containers() {
    let symbols = extract(
        r#"#[cfg(all(test, feature = "slow"))]
mod all_tests {}

#[cfg(any(test, miri))]
mod any_tests {}

#[cfg(all(unix, any(test, windows)))]
mod nested_tests {}
"#,
    );

    for name in ["all_tests", "any_tests", "nested_tests"] {
        assert!(
            role(
                symbol(&symbols, name, SymbolKind::Namespace),
                "test_container"
            ),
            "{name} must be a test container"
        );
    }
}

#[test]
fn a_cfg_test_module_publishes_its_attribute_and_the_container_role() {
    let symbols = extract(
        r#"#[cfg(test)]
mod unit_tests {}
"#,
    );

    let module = symbol(&symbols, "unit_tests", SymbolKind::Namespace);
    assert_eq!(
        module
            .annotations
            .iter()
            .map(|marker| marker.annotation_key.as_str())
            .collect::<Vec<_>>(),
        vec!["cfg"]
    );
    assert_eq!(
        module
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.get("test_role"))
            .and_then(Value::as_str),
        Some("test_container")
    );
}

#[test]
fn rust_test_attributes_publish_their_role_on_functions() {
    let symbols = extract(
        r#"#[tokio::test]
async fn async_case() {}

#[test_case(1, 2)]
#[test_case(3, 4)]
fn table_case(a: i32, b: i32) {}

#[rstest]
#[case(1)]
fn rstest_case(#[case] a: i32) {}

#[rstest]
#[case::six_times_seven(6, 7)]
fn rstest_named_case(#[case] a: i32, #[case] b: i32) {}

#[rstest]
fn plain_rstest() {}

#[fixture]
fn connection() -> u8 { 0 }

#[tokio::main]
async fn production_entry_point() {}
"#,
    );

    let role_of = |name: &str| {
        symbol(&symbols, name, SymbolKind::Function)
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.get("test_role"))
            .and_then(Value::as_str)
            .map(str::to_string)
    };

    assert_eq!(role_of("async_case").as_deref(), Some("test_case"));
    assert_eq!(role_of("plain_rstest").as_deref(), Some("test_case"));
    assert_eq!(role_of("table_case").as_deref(), Some("parameterized_test"));
    assert_eq!(
        role_of("rstest_case").as_deref(),
        Some("parameterized_test")
    );
    assert_eq!(
        role_of("rstest_named_case").as_deref(),
        Some("parameterized_test")
    );
    assert_eq!(role_of("connection").as_deref(), Some("fixture_setup"));
    assert_eq!(role_of("production_entry_point"), None);
}

#[test]
fn non_test_cfg_attributes_and_ordinary_modules_are_not_containers() {
    let symbols = extract(
        r#"mod ordinary {
    fn setup() {}
    fn teardown() {}
}

#[cfg(feature = "test")]
mod feature_tests {}

#[cfg(testing)]
mod testing_name {}

#[cfg(not(test))]
mod negated_cfg {}

#[cfg(all(unix, not(test)))]
mod nested_negated_cfg {}

fn setup() {}
fn teardown() {}
"#,
    );

    for name in [
        "ordinary",
        "feature_tests",
        "testing_name",
        "negated_cfg",
        "nested_negated_cfg",
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
