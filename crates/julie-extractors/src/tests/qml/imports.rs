use crate::base::{Symbol, SymbolKind};
use crate::tests::qml::extract_symbols;
use serde_json::Value;

fn imports(code: &str) -> Vec<Symbol> {
    extract_symbols(code)
        .into_iter()
        .filter(|symbol| symbol.kind == SymbolKind::Import)
        .collect()
}

fn metadata_value<'a>(symbol: &'a Symbol, key: &str) -> Option<&'a Value> {
    symbol
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get(key))
}

#[test]
fn qml_imports_publish_normalized_uri_directory_alias_and_javascript_fields() {
    let symbols = imports(
        r#"
import QtQuick 2.15
import QtQuick.Controls 2.15 as Controls
import "components"
import "./js/helpers.js" as Helpers
Item {}
"#,
    );

    assert_eq!(symbols.len(), 4);

    assert_eq!(symbols[0].name, "QtQuick");
    assert_eq!(
        metadata_value(&symbols[0], "source").and_then(Value::as_str),
        Some("QtQuick")
    );
    assert_eq!(
        metadata_value(&symbols[0], "version").and_then(Value::as_str),
        Some("2.15")
    );
    assert_eq!(
        metadata_value(&symbols[0], "source_kind").and_then(Value::as_str),
        Some("uri")
    );
    assert_eq!(
        metadata_value(&symbols[0], "import_kind").and_then(Value::as_str),
        Some("module")
    );
    assert!(metadata_value(&symbols[0], "alias").is_none());
    assert!(metadata_value(&symbols[0], "local_name").is_none());
    assert!(metadata_value(&symbols[0], "imported_name").is_none());
    assert!(metadata_value(&symbols[0], "is_namespace").is_none());

    assert_eq!(symbols[1].name, "QtQuick.Controls");
    assert_eq!(
        metadata_value(&symbols[1], "alias").and_then(Value::as_str),
        Some("Controls")
    );
    assert_eq!(
        metadata_value(&symbols[1], "local_name").and_then(Value::as_str),
        Some("Controls")
    );
    assert_eq!(
        metadata_value(&symbols[1], "imported_name").and_then(Value::as_str),
        Some("QtQuick.Controls")
    );
    assert_eq!(
        metadata_value(&symbols[1], "is_namespace").and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        metadata_value(&symbols[1], "source_kind").and_then(Value::as_str),
        Some("uri")
    );
    assert_eq!(
        metadata_value(&symbols[1], "import_kind").and_then(Value::as_str),
        Some("module")
    );

    assert_eq!(symbols[2].name, "components");
    assert_eq!(
        metadata_value(&symbols[2], "source").and_then(Value::as_str),
        Some("components")
    );
    assert_eq!(
        metadata_value(&symbols[2], "source_kind").and_then(Value::as_str),
        Some("quoted")
    );
    assert_eq!(
        metadata_value(&symbols[2], "import_kind").and_then(Value::as_str),
        Some("directory")
    );
    assert!(metadata_value(&symbols[2], "alias").is_none());
    assert!(metadata_value(&symbols[2], "local_name").is_none());
    assert!(metadata_value(&symbols[2], "imported_name").is_none());
    assert!(metadata_value(&symbols[2], "is_namespace").is_none());

    assert_eq!(symbols[3].name, "./js/helpers.js");
    assert_eq!(
        metadata_value(&symbols[3], "alias").and_then(Value::as_str),
        Some("Helpers")
    );
    assert_eq!(
        metadata_value(&symbols[3], "local_name").and_then(Value::as_str),
        Some("Helpers")
    );
    assert_eq!(
        metadata_value(&symbols[3], "imported_name").and_then(Value::as_str),
        Some("./js/helpers.js")
    );
    assert_eq!(
        metadata_value(&symbols[3], "is_namespace").and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        metadata_value(&symbols[3], "source_kind").and_then(Value::as_str),
        Some("quoted")
    );
    assert_eq!(
        metadata_value(&symbols[3], "import_kind").and_then(Value::as_str),
        Some("javascript")
    );
}

#[test]
fn qml_import_source_kind_distinguishes_unquoted_and_quoted_same_source() {
    let symbols = imports(
        r#"
import Widgets
import "Widgets"
Item {}
"#,
    );

    assert_eq!(symbols.len(), 2);
    assert_eq!(
        metadata_value(&symbols[0], "source").and_then(Value::as_str),
        Some("Widgets")
    );
    assert_eq!(
        metadata_value(&symbols[0], "source_kind").and_then(Value::as_str),
        Some("uri")
    );
    assert_eq!(
        metadata_value(&symbols[1], "source").and_then(Value::as_str),
        Some("Widgets")
    );
    assert_eq!(
        metadata_value(&symbols[1], "source_kind").and_then(Value::as_str),
        Some("quoted")
    );
    assert_eq!(
        metadata_value(&symbols[1], "import_kind").and_then(Value::as_str),
        Some("directory")
    );
}

#[test]
fn qml_import_kind_treats_javascript_extension_case_insensitively() {
    let symbols = imports(
        r#"
import "./js/helpers.JS" as Helpers
Item {}
"#,
    );

    assert_eq!(
        metadata_value(&symbols[0], "source_kind").and_then(Value::as_str),
        Some("quoted")
    );
    assert_eq!(
        metadata_value(&symbols[0], "import_kind").and_then(Value::as_str),
        Some("javascript")
    );
}

#[test]
fn empty_qml_import_source_emits_no_import_symbol() {
    let symbols = imports(
        r#"
import ""
Item {}
"#,
    );

    assert!(symbols.is_empty());
}

#[test]
fn malformed_qml_version_emits_no_import_symbol() {
    let symbols = imports(
        r#"
import QtQuick 2.
Item {}
"#,
    );

    assert!(symbols.is_empty());
}
