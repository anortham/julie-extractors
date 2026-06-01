//! QML test detection support.
//!
//! Qt Quick Test files declare tests as a `TestCase { ... }` root component. There
//! are no annotations, so the qml extractor emits `base_types = ["TestCase"]` on
//! the root component symbol. Artifact v1 preserves that metadata evidence but
//! does not copy old Julie's test-container classifier.

use super::extract_symbols;
use crate::base::SymbolKind;

/// Pull the `base_types` metadata array (strings) off a symbol, if present.
fn base_types(symbol: &crate::base::Symbol) -> Vec<String> {
    symbol
        .metadata
        .as_ref()
        .and_then(|m| m.get("base_types"))
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

#[test]
fn testcase_root_component_emits_base_types_metadata() {
    // A `TestCase { ... }` root is the Qt Quick Test container. The root component
    // is extracted as a Class whose `base_types` records the component type.
    let code = r#"
import QtTest 1.0

TestCase {
    name: "MathTests"

    function test_addition() {
        compare(1 + 1, 2);
    }
}
"#;
    let symbols = extract_symbols(code);
    let root = symbols
        .iter()
        .find(|s| s.kind == SymbolKind::Class)
        .unwrap_or_else(|| panic!("expected a root component Class, got {symbols:?}"));
    assert_eq!(
        base_types(root),
        vec!["TestCase".to_string()],
        "root component must record its base type under `base_types` for the test-role classifier"
    );
}

#[test]
fn non_test_root_component_records_its_own_base_type() {
    // The `base_types` mechanism is general: a plain `Rectangle { }` root records
    // `["Rectangle"]`.
    let code = r#"
import QtQuick 2.0

Rectangle {
    width: 100
    height: 100
}
"#;
    let symbols = extract_symbols(code);
    let root = symbols
        .iter()
        .find(|s| s.kind == SymbolKind::Class)
        .unwrap_or_else(|| panic!("expected a root component Class, got {symbols:?}"));
    assert_eq!(base_types(root), vec!["Rectangle".to_string()]);
}
