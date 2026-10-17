use crate::rust::RustExtractor;
use std::path::PathBuf;
use tree_sitter::Parser;

#[test]
fn test_rust_function_return_types() {
    let code = r#"
fn get_name() -> String {
    "Alice".to_string()
}

fn calculate_age() -> i32 {
    42
}

fn process_data() -> Result<Vec<u8>, std::io::Error> {
    Ok(vec![1, 2, 3])
}
"#;

    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_rust::LANGUAGE.into())
        .expect("Failed to load Rust grammar");

    let tree = parser.parse(code, None).expect("Failed to parse code");
    let workspace_root = PathBuf::from("/test");

    let mut extractor = RustExtractor::new(
        "rust".to_string(),
        "test.rs".to_string(),
        code.to_string(),
        &workspace_root,
    );

    let symbols = extractor.extract_symbols(&tree);
    let return_type = |name: &str| {
        let symbol = symbols.iter().find(|s| s.name == name).unwrap();
        let fact = extractor.base.type_info.get(&symbol.id).unwrap();
        let declared = fact
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.get("declared"))
            .and_then(|declared| declared.as_str())
            .map(str::to_string);
        (fact.resolved_type.clone(), fact.is_inferred, declared)
    };

    assert_eq!(return_type("get_name"), ("String".to_string(), false, None));
    assert_eq!(
        return_type("calculate_age"),
        ("i32".to_string(), false, None)
    );
    assert_eq!(
        return_type("process_data"),
        (
            "Result".to_string(),
            false,
            Some("Result<Vec<u8>, std::io::Error>".to_string())
        )
    );
}

#[test]
fn test_rust_return_type_inference_stops_before_where_clause() {
    let code = r#"
fn make_iter<T>(items: Vec<T>) -> impl Iterator<Item = Result<T, std::io::Error>>
where
    T: Clone,
{
    items.into_iter().map(Ok)
}
"#;

    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_rust::LANGUAGE.into())
        .expect("Failed to load Rust grammar");

    let tree = parser.parse(code, None).expect("Failed to parse code");
    let workspace_root = PathBuf::from("/test");

    let mut extractor = RustExtractor::new(
        "rust".to_string(),
        "test.rs".to_string(),
        code.to_string(),
        &workspace_root,
    );

    let symbols = extractor.extract_symbols(&tree);
    let make_iter = symbols
        .iter()
        .find(|s| s.name == "make_iter")
        .expect("Should find make_iter");
    let fact = extractor.base.type_info.get(&make_iter.id).unwrap();

    assert_eq!(fact.resolved_type, "Iterator");
    assert_eq!(
        fact.metadata
            .as_ref()
            .and_then(|metadata| metadata.get("declared"))
            .and_then(|declared| declared.as_str()),
        Some("impl Iterator<Item = Result<T, std::io::Error>>")
    );
}

#[test]
fn test_rust_variable_types() {
    let code = r#"
fn main() {
    let count: i32 = 42;
    let name: String = "Alice".to_string();
    let items: Vec<u8> = vec![1, 2, 3];
}
"#;

    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_rust::LANGUAGE.into())
        .expect("Failed to load Rust grammar");

    let tree = parser.parse(code, None).expect("Failed to parse code");
    let workspace_root = PathBuf::from("/test");

    let mut extractor = RustExtractor::new(
        "rust".to_string(),
        "test.rs".to_string(),
        code.to_string(),
        &workspace_root,
    );

    let symbols = extractor.extract_symbols(&tree);
    assert!(extractor.infer_types(&symbols).is_empty());

    for (name, expected) in [("count", "i32"), ("name", "String"), ("items", "Vec")] {
        let symbol = symbols
            .iter()
            .find(|s| s.name == name)
            .unwrap_or_else(|| panic!("missing symbol {name}"));
        assert_eq!(
            extractor
                .base
                .type_info
                .get(&symbol.id)
                .map(|fact| fact.resolved_type.as_str()),
            Some(expected),
            "type fact of `{name}`"
        );
    }
}

#[test]
fn test_rust_struct_field_types() {
    let code = r#"
struct User {
    id: u64,
    name: String,
    age: Option<u32>,
}
"#;

    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_rust::LANGUAGE.into())
        .expect("Failed to load Rust grammar");

    let tree = parser.parse(code, None).expect("Failed to parse code");
    let workspace_root = PathBuf::from("/test");

    let mut extractor = RustExtractor::new(
        "rust".to_string(),
        "test.rs".to_string(),
        code.to_string(),
        &workspace_root,
    );

    let symbols = extractor.extract_symbols(&tree);
    assert!(extractor.infer_types(&symbols).is_empty());

    for (name, expected) in [("id", "u64"), ("name", "String"), ("age", "Option")] {
        let field = symbols
            .iter()
            .find(|s| s.name == name)
            .unwrap_or_else(|| panic!("missing field {name}"));
        assert_eq!(
            extractor
                .base
                .type_info
                .get(&field.id)
                .map(|fact| fact.resolved_type.as_str()),
            Some(expected),
            "type fact of `{name}`"
        );
    }
}

#[test]
fn rust_derive_attribute_markers_persist_on_structs() {
    let code = r#"
#[derive(Debug, Clone)]
struct User {
    id: u64,
}
"#;

    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_rust::LANGUAGE.into())
        .expect("Failed to load Rust grammar");

    let tree = parser.parse(code, None).expect("Failed to parse code");
    let workspace_root = PathBuf::from("/test");

    let mut extractor = RustExtractor::new(
        "rust".to_string(),
        "test.rs".to_string(),
        code.to_string(),
        &workspace_root,
    );

    let symbols = extractor.extract_symbols(&tree);
    let user = symbols
        .iter()
        .find(|s| s.name == "User")
        .expect("Should extract User struct");

    let annotations: Vec<_> = user
        .annotations
        .iter()
        .map(|marker| {
            (
                marker.annotation.as_str(),
                marker.annotation_key.as_str(),
                marker.raw_text.as_deref(),
                marker.carrier.as_deref(),
            )
        })
        .collect();

    assert_eq!(
        annotations,
        vec![
            ("Debug", "debug", Some("Debug"), Some("derive")),
            ("Clone", "clone", Some("Clone"), Some("derive")),
        ]
    );
    assert!(
        user.signature
            .as_deref()
            .unwrap_or_default()
            .contains("#[derive(Debug, Clone)] struct User")
    );
}
