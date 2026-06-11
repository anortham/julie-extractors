use std::collections::HashSet;
use std::path::PathBuf;

use crate::base::SymbolKind;
use crate::factory::extract_symbols_and_relationships;

const BASIC_FIXTURE: &str = include_str!("../../../../../fixtures/extraction/qml/basic/source.qml");

#[test]
fn canonical_qml_extraction_emits_property_and_function_types() {
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_qmljs::LANGUAGE.into())
        .expect("load QML grammar");
    let tree = parser
        .parse(BASIC_FIXTURE, None)
        .expect("parse QML basic fixture");

    let workspace_root = PathBuf::from("/repo");
    let results = extract_symbols_and_relationships(
        &tree,
        "fixtures/extraction/qml/basic/source.qml",
        BASIC_FIXTURE,
        "qml",
        &workspace_root,
    )
    .expect("canonical QML extraction should succeed");

    assert!(
        !results.types.is_empty(),
        "QML canonical extraction must emit TypeInfo rows"
    );

    let symbol_ids: HashSet<&str> = results
        .symbols
        .iter()
        .map(|symbol| symbol.id.as_str())
        .collect();
    for type_key in results.types.keys() {
        assert!(
            symbol_ids.contains(type_key.as_str()),
            "type key '{type_key}' must reference a real symbol id"
        );
    }

    let title = results
        .symbols
        .iter()
        .find(|symbol| symbol.kind == SymbolKind::Property && symbol.name == "title")
        .expect("title property symbol");
    let worker_id = results
        .symbols
        .iter()
        .find(|symbol| symbol.kind == SymbolKind::Property && symbol.name == "workerId")
        .expect("workerId property symbol");
    let build_index = results
        .symbols
        .iter()
        .find(|symbol| symbol.kind == SymbolKind::Function && symbol.name == "buildIndex")
        .expect("buildIndex function symbol");

    assert_eq!(
        results
            .types
            .get(&title.id)
            .map(|type_info| type_info.resolved_type.as_str()),
        Some("string")
    );
    assert_eq!(
        results
            .types
            .get(&worker_id.id)
            .map(|type_info| type_info.resolved_type.as_str()),
        Some("int")
    );
    assert_eq!(
        results
            .types
            .get(&build_index.id)
            .map(|type_info| type_info.resolved_type.as_str()),
        Some("void")
    );

    for type_info in results.types.values() {
        assert_eq!(type_info.language, "qml");
        assert!(type_info.is_inferred);
    }
}
