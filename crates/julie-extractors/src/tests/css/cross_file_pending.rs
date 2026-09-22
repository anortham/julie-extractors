use crate::base::RelationshipKind;
use crate::extract_canonical;
use std::path::Path;

#[test]
fn css_import_emits_one_structured_import_pending() {
    let source = include_str!("../../../../../fixtures/extraction/css/cross_file/source.css");
    let result = extract_canonical("source.css", source, Path::new("/tmp/test"))
        .expect("canonical CSS extraction must succeed");

    let [pending] = result.structured_pending_relationships.as_slice() else {
        panic!(
            "expected one @import pending row, got {:#?}",
            result.structured_pending_relationships
        );
    };
    assert_eq!(pending.pending.kind, RelationshipKind::Imports);
    assert_eq!(pending.target.display_name, "./other.css");
    assert_eq!(pending.target.import_context.as_deref(), Some("css-import"));
    assert_eq!(result.pending_relationships.len(), 1);
}
