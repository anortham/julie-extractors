use crate::base::IdentifierKind;
use crate::javascript::JavaScriptExtractor;
use std::path::PathBuf;

#[test]
fn jsx_shadowed_access_keeps_exact_host_occurrences() {
    let code = r#"
const value = 1;
function View(value) {
    value += 1;
    return <span>{value}</span>;
}
consume(value);
"#;
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_javascript::LANGUAGE.into())
        .unwrap();
    let tree = parser.parse(code, None).unwrap();
    assert!(!tree.root_node().has_error());
    let workspace_root = PathBuf::from("/tmp/test");
    let mut extractor = JavaScriptExtractor::new(
        "jsx".to_string(),
        "View.jsx".to_string(),
        code.to_string(),
        &workspace_root,
    );
    let symbols = extractor.extract_symbols(&tree);
    let identifiers = extractor.extract_identifiers(&tree, &symbols);
    let access_at = |start: usize| -> Option<String> {
        identifiers
            .iter()
            .find(|identifier| {
                identifier.start_byte == start as u32
                    && identifier.kind == IdentifierKind::VariableRef
            })?
            .metadata
            .as_ref()?
            .get("access")?
            .as_str()
            .map(str::to_owned)
    };
    let call = identifiers
        .iter()
        .find(|identifier| identifier.kind == IdentifierKind::Call)
        .expect("JSX call identifier");

    assert_eq!(
        access_at(code.find("value += 1").unwrap()),
        Some("read_write".to_string())
    );
    assert_eq!(
        access_at(code.find("{value}").unwrap() + 1),
        Some("read".to_string())
    );
    assert_eq!(
        access_at(code.find("consume(value)").unwrap() + "consume(".len()),
        Some("read".to_string())
    );
    assert_eq!(call.name, "consume");
    assert!(
        call.metadata
            .as_ref()
            .is_none_or(|metadata| !metadata.contains_key("access"))
    );
}
