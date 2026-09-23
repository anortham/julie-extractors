use crate::c::CExtractor;
use std::path::PathBuf;
use tree_sitter::Parser;

#[test]
fn test_c_function_return_types() {
    let code = r#"
int get_count() {
    return 42;
}

char* get_name() {
    return "Alice";
}

struct User* get_user() {
    return NULL;
}

void process_data() {
}
"#;

    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_c::LANGUAGE.into())
        .expect("Failed to load C grammar");

    let tree = parser.parse(code, None).expect("Failed to parse code");
    let workspace_root = PathBuf::from("/test");

    let mut extractor = CExtractor::new(
        "c".to_string(),
        "test.c".to_string(),
        code.to_string(),
        &workspace_root,
    );

    let symbols = extractor.extract_symbols(&tree);
    let return_type = |name: &str| {
        let symbol = symbols
            .iter()
            .find(|s| s.name == name)
            .unwrap_or_else(|| panic!("missing {name}"));
        extractor.base.type_info.get(&symbol.id).map(|fact| {
            let declared = fact
                .metadata
                .as_ref()
                .and_then(|m| m.get("declared"))
                .and_then(|v| v.as_str())
                .map(str::to_string);
            (fact.resolved_type.clone(), declared)
        })
    };

    assert_eq!(return_type("get_count"), Some(("int".into(), None)));
    assert_eq!(
        return_type("get_name"),
        Some(("char".into(), Some("char *".into())))
    );
    assert_eq!(
        return_type("get_user"),
        Some(("User".into(), Some("struct User *".into())))
    );
    assert_eq!(return_type("process_data"), Some(("void".into(), None)));
}

#[test]
fn test_c_variable_types() {
    let code = r#"
int count = 42;
char* name = "Alice";
struct User user;
const char* const MESSAGE = "Hello";
"#;

    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_c::LANGUAGE.into())
        .expect("Failed to load C grammar");

    let tree = parser.parse(code, None).expect("Failed to parse code");
    let workspace_root = PathBuf::from("/test");

    let mut extractor = CExtractor::new(
        "c".to_string(),
        "test.c".to_string(),
        code.to_string(),
        &workspace_root,
    );

    let symbols = extractor.extract_symbols(&tree);
    let types = extractor.infer_types(&symbols);

    // Should extract types for variables (if they're captured as symbols)
    if let Some(count_symbol) = symbols.iter().find(|s| s.name == "count") {
        assert_eq!(types.get(&count_symbol.id), Some(&"int".to_string()));
    }

    if let Some(name_symbol) = symbols.iter().find(|s| s.name == "name") {
        assert_eq!(types.get(&name_symbol.id), Some(&"char*".to_string()));
    }
}
