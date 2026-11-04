use crate::base::IdentifierKind;
use crate::typescript::TypeScriptExtractor;
use std::path::PathBuf;

#[test]
fn tsx_shadowed_access_keeps_exact_byte_occurrences() {
    let code = r#"
type Payload = { value: number };
const payload: Payload = { value: 1 };
function View(payload: number) {
    payload += 1;
    return <span>{payload}</span>;
}
consume(payload);
"#;
    let mut parser = super::init_tsx_parser();
    let tree = parser.parse(code, None).unwrap();
    assert!(!tree.root_node().has_error());
    let workspace_root = PathBuf::from("/tmp/test");
    let mut extractor = TypeScriptExtractor::new(
        "tsx".to_string(),
        "View.tsx".to_string(),
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
        .expect("TSX call identifier");
    let type_usage = identifiers
        .iter()
        .find(|identifier| {
            identifier.name == "Payload" && identifier.kind == IdentifierKind::TypeUsage
        })
        .expect("TSX type usage");

    assert_eq!(
        access_at(code.find("payload += 1").unwrap()),
        Some("read_write".to_string())
    );
    assert_eq!(
        access_at(code.find("{payload}").unwrap() + 1),
        Some("read".to_string())
    );
    assert_eq!(
        access_at(code.find("consume(payload)").unwrap() + "consume(".len()),
        Some("read".to_string())
    );
    assert_eq!(call.name, "consume");
    assert!(
        call.metadata
            .as_ref()
            .is_none_or(|metadata| !metadata.contains_key("access"))
    );
    assert!(
        type_usage
            .metadata
            .as_ref()
            .is_none_or(|metadata| !metadata.contains_key("access"))
    );
}
