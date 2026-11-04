use crate::base::Literal;
use crate::typescript::TypeScriptExtractor;
use std::path::PathBuf;

fn capture_tsx(code: &str) -> Vec<Literal> {
    let mut parser = super::init_tsx_parser();
    let tree = parser.parse(code, None).expect("parse TSX");
    assert!(!tree.root_node().has_error());
    let mut extractor = TypeScriptExtractor::new(
        "tsx".to_string(),
        "test.tsx".to_string(),
        code.to_string(),
        &PathBuf::from("/test/workspace"),
    );
    let symbols = extractor.extract_symbols(&tree);
    extractor.extract_identifiers(&tree, &symbols);
    extractor.get_literals()
}

#[test]
fn call_argument_literals_exclude_confusables_and_keep_utf8_span() {
    let code = r#"
type Payload = { value: number };
function View(value: Payload) {
    // request("comment")
    const standalone = "loose";
    return <button data-label="native-tsx" onClick={() => request(42, false, value, { payload: "nested" }, "café")}>{value.value}</button>;
}
"#;
    let literals = capture_tsx(code);
    assert_eq!(literals.len(), 1, "{literals:?}");
    let literal = &literals[0];
    let source_token = "\"café\"";
    let source_start = code.find(source_token).unwrap() as u32;
    assert_eq!(literal.literal_text, "café");
    assert_eq!(literal.language, "tsx");
    assert_eq!(literal.carrier.as_deref(), Some("request"));
    assert_eq!(literal.arg_position, 4);
    assert_eq!(literal.start_byte, source_start);
    assert_eq!(literal.end_byte, source_start + source_token.len() as u32);
}
