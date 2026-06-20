//! GDScript test detection support.
//!
//! GUT (Godot Unit Test) scripts are a top-level `extends GutTest` with `func test_*`
//! methods. The gdscript extractor synthesizes an implicit file-class and emits
//! `base_types = ["GutTest"]` on it. Artifact v1 preserves that metadata
//! evidence but does not copy old Julie's test-container classifier.

use super::{extract_symbols, extract_symbols_for_file};
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
fn extends_guttest_implicit_class_emits_base_types_metadata() {
    // A top-level `extends GutTest` synthesizes an implicit file-class. It must
    // record `["GutTest"]` under `base_types`.
    let code = r#"extends GutTest

func test_player_health():
    assert_eq(1, 1)
"#;
    let symbols = extract_symbols(code);
    let implicit_class = symbols
        .iter()
        .find(|s| s.kind == SymbolKind::Class)
        .unwrap_or_else(|| panic!("expected an implicit file-class, got {symbols:?}"));
    assert_eq!(
        base_types(implicit_class),
        vec!["GutTest".to_string()],
        "implicit class must record its base type under `base_types` for the classifier"
    );
}

#[test]
fn extends_non_test_base_records_its_own_base_type() {
    // The mechanism is general: a `extends Node2D` script records `["Node2D"]`.
    let code = r#"extends Node2D

func _ready():
    pass
"#;
    let symbols = extract_symbols(code);
    let implicit_class = symbols
        .iter()
        .find(|s| s.kind == SymbolKind::Class)
        .unwrap_or_else(|| panic!("expected an implicit file-class, got {symbols:?}"));
    assert_eq!(base_types(implicit_class), vec!["Node2D".to_string()]);
}

#[test]
fn resource_path_extends_emits_filename_implicit_class() {
    let code = r#"extends "res://base_controller.gd"

func run() -> void:
    pass
"#;

    let symbols = extract_symbols_for_file("actors/ResourceExtends.gd", code);
    let implicit_class = symbols
        .iter()
        .find(|s| s.name == "ResourceExtends" && s.kind == SymbolKind::Class)
        .unwrap_or_else(|| {
            panic!("expected filename-derived implicit class for resource extends, got {symbols:?}")
        });

    assert_eq!(
        implicit_class.signature.as_deref(),
        Some("extends res://base_controller.gd")
    );
    assert_eq!(
        base_types(implicit_class),
        vec!["res://base_controller.gd".to_string()]
    );
}
