use crate::base::RelationshipKind;

#[test]
fn test_html_relationships_walk_past_doctype() {
    let code = r#"<!doctype html>
<html>
  <body>
    <a href="/workers">Workers</a>
  </body>
</html>"#;

    let result = crate::extract_canonical("page.html", code, std::path::Path::new("/tmp/test"))
        .expect("canonical HTML extraction must succeed");
    let anchor = result
        .symbols
        .iter()
        .find(|symbol| symbol.name == "a")
        .expect("anchor symbol should be extracted");
    assert!(
        result
            .structured_pending_relationships
            .iter()
            .any(|pending| {
                pending.pending.kind == RelationshipKind::References
                    && pending.target.display_name == "/workers"
                    && pending.pending.from_symbol_id == anchor.id
            }),
        "href reference should be extracted even when the document starts with a doctype"
    );
}
