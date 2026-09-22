//! Razor emits structured pending rows for references that resolve in other
//! files: calls on injected services, the `@inherits`/`@implements` bases, and
//! component tags. Same-file calls stay resolved relationships.

use crate::base::RelationshipKind;
use crate::extract_canonical;
use std::path::Path;

#[test]
fn razor_emits_structured_pending_for_cross_file_references() {
    let source = include_str!("../../../../../fixtures/extraction/razor/cross_file/source.razor");
    let result = extract_canonical("source.razor", source, Path::new("/tmp/test"))
        .expect("canonical Razor extraction must succeed");

    let rows: Vec<(RelationshipKind, &str, Option<&str>)> = result
        .structured_pending_relationships
        .iter()
        .map(|pending| {
            (
                pending.pending.kind.clone(),
                pending.target.display_name.as_str(),
                pending.target.receiver.as_deref(),
            )
        })
        .collect();

    assert!(
        rows.contains(&(RelationshipKind::Calls, "Items.LoadAsync", Some("Items"))),
        "{rows:?}"
    );
    assert!(
        rows.contains(&(RelationshipKind::Extends, "LayoutComponentBase", None)),
        "{rows:?}"
    );
    assert!(
        rows.contains(&(RelationshipKind::Implements, "IDisposable", None)),
        "{rows:?}"
    );
    assert!(
        rows.contains(&(RelationshipKind::Uses, "ItemCard", None)),
        "{rows:?}"
    );
    assert!(
        rows.iter().all(|(_, target, _)| *target != "LocalHelper"),
        "same-file LocalHelper must resolve, not pend: {rows:?}"
    );
}
