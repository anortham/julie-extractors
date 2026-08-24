//! QML test detection support.
//!
//! Qt Quick Test files declare tests as a `TestCase { ... }` root component. There
//! are no annotations, so the qml extractor emits `base_types = ["TestCase"]` on
//! the root component symbol. Artifact v1 preserves that metadata evidence but
//! does not copy old Julie's test-container classifier.

use super::{extract_symbols, extract_symbols_with_path};
use crate::base::{Symbol, SymbolKind};

fn meta_bool(symbol: &Symbol, key: &str) -> bool {
    symbol
        .metadata
        .as_ref()
        .and_then(|m| m.get(key))
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
}

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
    assert!(
        meta_bool(root, "test_container"),
        "a TestCase root is the Qt Quick Test container"
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
    assert!(
        !meta_bool(root, "test_container"),
        "a Rectangle root is not a test container"
    );
}

#[test]
fn qml_init_test_case_is_lifecycle_and_test_fn_is_case() {
    let code = r#"
import QtTest 1.0

TestCase {
    name: "CalculatorTests"

    function initTestCase() {
    }

    function cleanupTestCase() {
    }

    function test_addition() {
        compare(1 + 1, 2);
    }

    function verify_addition() {
        compare(2, 2);
    }
}
"#;
    let symbols = extract_symbols_with_path(code, "test_source.qml");
    let init = symbols
        .iter()
        .find(|s| s.name == "initTestCase")
        .unwrap_or_else(|| panic!("expected initTestCase, got {symbols:?}"));
    assert!(meta_bool(init, "is_test"));
    assert!(meta_bool(init, "test_lifecycle"));

    let cleanup = symbols
        .iter()
        .find(|s| s.name == "cleanupTestCase")
        .unwrap_or_else(|| panic!("expected cleanupTestCase, got {symbols:?}"));
    assert!(meta_bool(cleanup, "is_test"));
    assert!(meta_bool(cleanup, "test_lifecycle"));

    let case = symbols
        .iter()
        .find(|s| s.name == "test_addition")
        .unwrap_or_else(|| panic!("expected test_addition, got {symbols:?}"));
    assert!(meta_bool(case, "is_test"));
    assert!(!meta_bool(case, "test_lifecycle"));

    let helper = symbols
        .iter()
        .find(|s| s.name == "verify_addition")
        .unwrap_or_else(|| panic!("expected verify_addition, got {symbols:?}"));
    assert!(!meta_bool(helper, "is_test"));
}

#[test]
fn qml_data_helpers_are_not_tests_and_benchmarks_are_tests() {
    let code = r#"
import QtTest 1.0

TestCase {
    name: "CalculatorTests"

    function test_addition() {
    }

    function test_addition_data() {
    }

    function init_data() {
    }

    function benchmark_addition() {
    }

    function benchmark_once_addition() {
    }
}
"#;
    let symbols = extract_symbols_with_path(code, "autotests/tst_calculator.qml");

    assert!(meta_bool(
        symbols.iter().find(|s| s.name == "test_addition").unwrap(),
        "is_test"
    ));
    assert!(!meta_bool(
        symbols
            .iter()
            .find(|s| s.name == "test_addition_data")
            .unwrap(),
        "is_test"
    ));
    assert!(
        symbols
            .iter()
            .find(|s| s.name == "test_addition_data")
            .unwrap()
            .metadata
            .as_ref()
            .is_none()
    );
    assert!(!meta_bool(
        symbols.iter().find(|s| s.name == "init_data").unwrap(),
        "is_test"
    ));
    assert!(
        symbols
            .iter()
            .find(|s| s.name == "init_data")
            .unwrap()
            .metadata
            .as_ref()
            .is_none()
    );
    assert!(meta_bool(
        symbols
            .iter()
            .find(|s| s.name == "benchmark_addition")
            .unwrap(),
        "is_test"
    ));
    assert!(meta_bool(
        symbols
            .iter()
            .find(|s| s.name == "benchmark_once_addition")
            .unwrap(),
        "is_test"
    ));
}

#[test]
fn qml_test_names_without_a_testcase_root_are_not_tests() {
    let code = r#"
import QtQuick 2.15

Item {
    function test_application_helper() {
    }

    function benchmark_application_helper() {
    }
}
"#;
    let symbols = extract_symbols_with_path(code, "autotests/tst_application.qml");

    for name in ["test_application_helper", "benchmark_application_helper"] {
        assert!(
            !meta_bool(symbols.iter().find(|s| s.name == name).unwrap(), "is_test"),
            "{name} must not be a Qt Quick Test without a TestCase root"
        );
        assert!(
            symbols
                .iter()
                .find(|s| s.name == name)
                .unwrap()
                .metadata
                .as_ref()
                .is_none()
        );
    }
}
