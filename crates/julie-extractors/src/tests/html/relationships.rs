use crate::base::{ExtractionResults, RelationshipKind};
use crate::extract_canonical;
use std::path::Path;

fn extract(source: &str) -> ExtractionResults {
    extract_canonical("page.html", source, Path::new("/repo")).expect("HTML extraction")
}

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

#[test]
fn custom_element_extends_has_matching_legacy_and_structured_pending_rows() {
    let source = "<script>class Widget extends HTMLElement {}\nclass Plain {}\n</script>";
    let result = extract(source);

    assert!(result.parse_diagnostics.is_empty());
    let widget = result
        .symbols
        .iter()
        .find(|symbol| symbol.name == "Widget")
        .expect("custom element class");
    let plain = result
        .symbols
        .iter()
        .find(|symbol| symbol.name == "Plain")
        .expect("ordinary class");
    assert_eq!(
        result
            .pending_relationships
            .iter()
            .filter(|pending| pending.kind == RelationshipKind::Extends)
            .count(),
        1
    );
    assert_eq!(
        result
            .structured_pending_relationships
            .iter()
            .filter(|pending| pending.pending.kind == RelationshipKind::Extends)
            .count(),
        1
    );

    let structured = result
        .structured_pending_relationships
        .iter()
        .find(|pending| pending.pending.kind == RelationshipKind::Extends)
        .expect("unresolved superclass");
    assert_eq!(structured.pending.callee_name, "HTMLElement");
    assert_eq!(structured.pending.from_symbol_id, widget.id);
    assert_ne!(structured.pending.from_symbol_id, plain.id);
    let span = structured.span.expect("exact superclass span");
    assert_eq!(
        &source[span.start_byte as usize..span.end_byte as usize],
        "extends HTMLElement"
    );
    assert_eq!(result.pending_relationships[0], structured.pending);
}

#[test]
fn ordinary_classes_without_superclasses_emit_no_extends_rows() {
    let source = "<script>class Plain {}\nfunction render() { return Plain; }</script>";
    let result = extract(source);

    assert!(result.parse_diagnostics.is_empty());
    assert!(
        result
            .pending_relationships
            .iter()
            .all(|pending| pending.kind != RelationshipKind::Extends)
    );
    assert!(
        result
            .structured_pending_relationships
            .iter()
            .all(|pending| pending.pending.kind != RelationshipKind::Extends)
    );
}
