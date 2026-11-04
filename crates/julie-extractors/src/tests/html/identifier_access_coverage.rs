use crate::base::{ExtractionResults, Identifier, IdentifierKind};
use crate::extract_canonical;
use std::path::Path;

fn extract(source: &str) -> ExtractionResults {
    extract_canonical("pages/access.html", source, Path::new("/repo")).expect("HTML extraction")
}

fn occurrence(source: &str, needle: &str, index: usize) -> usize {
    source
        .match_indices(needle)
        .nth(index)
        .map(|(start, _)| start)
        .unwrap_or_else(|| panic!("missing occurrence {index} of {needle:?}"))
}

fn identifier_at(results: &ExtractionResults, start: usize, kind: IdentifierKind) -> &Identifier {
    results
        .identifiers
        .iter()
        .find(|identifier| identifier.start_byte as usize == start && identifier.kind == kind)
        .unwrap_or_else(|| panic!("missing {kind:?} identifier at byte {start}"))
}

fn owner<'a>(results: &'a ExtractionResults, identifier: &Identifier) -> Option<&'a str> {
    let id = identifier.containing_symbol_id.as_deref()?;
    results
        .symbols
        .iter()
        .find(|symbol| symbol.id == id)
        .map(|symbol| symbol.name.as_str())
}

fn access(identifier: &Identifier) -> Option<&str> {
    identifier
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get("access"))
        .and_then(serde_json::Value::as_str)
}

fn coordinate(source: &str, byte: usize) -> (u32, u32) {
    let prefix = &source[..byte];
    let line = prefix.bytes().filter(|byte| *byte == b'\n').count() as u32 + 1;
    let column = prefix.rsplit('\n').next().unwrap().len() as u32;
    (line, column)
}

fn assert_host_span(source: &str, identifier: &Identifier, start: usize, expected: &str) {
    let end = start + expected.len();
    assert_eq!(identifier.start_byte as usize, start);
    assert_eq!(identifier.end_byte as usize, end);
    assert_eq!(
        (identifier.start_line, identifier.start_column),
        coordinate(source, start)
    );
    assert_eq!(
        (identifier.end_line, identifier.end_column),
        coordinate(source, end)
    );
    assert_eq!(&source[start..end], expected);
}

#[test]
fn ordinary_and_babel_member_access_keep_access_kinds_owners_and_host_spans() {
    let source = concat!(
        "<div>λ</div><script>\r\n",
        "const receiver = {};\r\n",
        "function update(receiver) {\r\n",
        "  receiver.member;\r\n",
        "  receiver.member = 1;\r\n",
        "  receiver.member += 2;\r\n",
        "  persist(receiver.member);\r\n",
        "  function nested() { return receiver.member; }\r\n",
        "  function shadow(receiver) { receiver.member; }\r\n",
        "}\r\n",
        "update(receiver);\r\n",
        "</script>\r\n",
        "<script type=\"text/babel\">\r\n",
        "const target = {};\r\n",
        "function render(target) {\r\n",
        "  target.member;\r\n",
        "  target.member = 3;\r\n",
        "  target.member += 4;\r\n",
        "  consume(target.member);\r\n",
        "  return <Widget value={target.member} />;\r\n",
        "}\r\n",
        "</script>\r\n",
    );
    let results = extract(source);

    assert!(results.parse_diagnostics.is_empty());

    for (expression_index, expected_access, expected_owner) in [
        (0, "read", "update"),
        (1, "write", "update"),
        (2, "read_write", "update"),
        (3, "read", "update"),
        (4, "read", "nested"),
        (5, "read", "shadow"),
    ] {
        let expression_start = occurrence(source, "receiver.member", expression_index);
        let receiver = identifier_at(&results, expression_start, IdentifierKind::VariableRef);
        let member_start = expression_start + "receiver.".len();
        let member = identifier_at(&results, member_start, IdentifierKind::MemberAccess);

        assert_eq!(access(receiver), Some("read"));
        assert_eq!(access(member), Some(expected_access));
        assert_eq!(owner(&results, member), Some(expected_owner));
        assert_eq!(member.language, "javascript");
        assert_host_span(source, receiver, expression_start, "receiver");
        assert_host_span(source, member, member_start, "member");
    }

    for (expression_index, expected_access) in [
        (0, "read"),
        (1, "write"),
        (2, "read_write"),
        (3, "read"),
        (4, "read"),
    ] {
        let expression_start = occurrence(source, "target.member", expression_index);
        let receiver = identifier_at(&results, expression_start, IdentifierKind::VariableRef);
        let member_start = expression_start + "target.".len();
        let member = identifier_at(&results, member_start, IdentifierKind::MemberAccess);

        assert_eq!(access(receiver), Some("read"));
        assert_eq!(access(member), Some(expected_access));
        assert_eq!(owner(&results, member), Some("render"));
        assert_eq!(member.language, "jsx");
        assert_host_span(source, receiver, expression_start, "target");
        assert_host_span(source, member, member_start, "member");
    }

    for (needle, expected_owner) in [("persist", "update"), ("consume", "render")] {
        let start = occurrence(source, needle, 0);
        let call = identifier_at(&results, start, IdentifierKind::Call);
        assert_eq!(access(call), None);
        assert_eq!(owner(&results, call), Some(expected_owner));
        assert_host_span(source, call, start, needle);
    }
}
