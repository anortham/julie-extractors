// Tests for factory::convert_types_map

use crate::factory::convert_types_map;
use std::collections::HashMap;

#[test]
fn test_convert_types_map_basic() {
    let mut types = HashMap::new();
    types.insert("sym_a".to_string(), "String".to_string());
    types.insert("sym_b".to_string(), "i32".to_string());

    let result = convert_types_map(types, "rust");

    assert_eq!(result.len(), 2);

    let info_a = result.get("sym_a").expect("should have sym_a");
    assert_eq!(info_a.symbol_id, "sym_a");
    assert_eq!(info_a.resolved_type, "String");
    assert_eq!(info_a.language, "rust");
    assert!(info_a.is_inferred);
    assert!(info_a.generic_params.is_none());
    assert!(info_a.constraints.is_none());
    assert!(info_a.metadata.is_none());

    let info_b = result.get("sym_b").expect("should have sym_b");
    assert_eq!(info_b.symbol_id, "sym_b");
    assert_eq!(info_b.resolved_type, "i32");
    assert_eq!(info_b.language, "rust");
    assert!(info_b.is_inferred);
}

#[test]
fn test_convert_types_map_drops_unbindable_values() {
    let junk = [
        "self.data or {}",
        "unique symbol",
        "Result<Vec<String>,",
        "HashMap<",
        "String, Error>",
        "TSource>",
        "\tint",
        "line\nbreak",
    ];
    let mut types = HashMap::new();
    for (index, value) in junk.iter().enumerate() {
        types.insert(format!("sym_{index}"), value.to_string());
    }

    let result = convert_types_map(types, "python");

    assert!(
        result.is_empty(),
        "junk values must be dropped, kept: {:?}",
        result
            .values()
            .map(|t| &t.resolved_type)
            .collect::<Vec<_>>()
    );
}

#[test]
fn test_convert_types_map_keeps_bindable_values() {
    let clean = [
        "Foo[]",
        "mod.Foo",
        "Dictionary<string>",
        "IEnumerable<TSource>",
        "&str",
        "Option<Vec<String>>",
    ];
    let mut types = HashMap::new();
    for (index, value) in clean.iter().enumerate() {
        types.insert(format!("sym_{index}"), value.to_string());
    }

    let result = convert_types_map(types, "rust");

    assert_eq!(result.len(), clean.len());
    for value in clean {
        assert!(
            result.values().any(|t| t.resolved_type == value),
            "`{value}` must be kept exactly as-is"
        );
    }
}

#[test]
fn test_convert_types_map_empty() {
    let types: HashMap<String, String> = HashMap::new();
    let result = convert_types_map(types, "python");
    assert!(result.is_empty(), "Empty input should produce empty output");
}

#[test]
fn test_convert_types_map_language_propagation() {
    let mut types = HashMap::new();
    types.insert("id_1".to_string(), "int".to_string());
    types.insert("id_2".to_string(), "float".to_string());
    types.insert("id_3".to_string(), "str".to_string());

    let result = convert_types_map(types, "python");

    for info in result.values() {
        assert_eq!(
            info.language, "python",
            "All entries should have language='python', got '{}'",
            info.language
        );
    }
}
