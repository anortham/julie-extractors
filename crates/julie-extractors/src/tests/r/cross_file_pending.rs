//! Phase 4a.r — R emits `StructuredPendingRelationship` for cross-package
//! calls (`library(other); other::do_thing()`).

use crate::base::RelationshipKind;
use crate::extract_canonical;
use std::path::Path;

#[test]
fn test_r_emits_structured_pending_for_cross_package_call() {
    let source = include_str!("../../../../../fixtures/extraction/r/cross_file/source.R");
    let workspace_root = Path::new("/tmp/test");
    let result = extract_canonical("source.R", source, workspace_root)
        .expect("canonical R extraction must succeed");

    let do_thing = result
        .structured_pending_relationships
        .iter()
        .find(|p| p.target.terminal_name == "do_thing")
        .unwrap_or_else(|| {
            panic!(
                "expected structured pending for cross-package do_thing; got {} entries: {:#?}",
                result.structured_pending_relationships.len(),
                result.structured_pending_relationships
            )
        });

    assert!(do_thing.pending.line_number > 0);
    assert_eq!(do_thing.pending.file_path, "source.R");

    assert!(
        result
            .structured_pending_relationships
            .iter()
            .all(|p| p.target.terminal_name != "local_helper"),
        "intra-file local_helper must not appear as structured pending"
    );
}

#[test]
fn test_r_negative_local_helper_not_emitted_as_pending() {
    let source = include_str!("../../../../../fixtures/extraction/r/cross_file/source.R");
    let workspace_root = Path::new("/tmp/test");
    let result = extract_canonical("source.R", source, workspace_root)
        .expect("canonical R extraction must succeed");

    assert!(
        result
            .structured_pending_relationships
            .iter()
            .all(|p| p.pending.callee_name != "local_helper"
                && p.target.terminal_name != "local_helper"),
        "intra-file local_helper leaked into pending"
    );
}

#[test]
fn test_r_qualified_chain_call_keeps_receiver_and_namespace() {
    let source = r#"
run <- function(outer, x) {
  Outer$Inner$chain()
  outer@inner$chain2()
  Helper$process()
  print.default(x)
}
"#;
    let workspace_root = Path::new("/tmp/test");
    let result = extract_canonical("run.R", source, workspace_root)
        .expect("canonical R extraction must succeed");
    let call_target = |terminal: &str| {
        result
            .structured_pending_relationships
            .iter()
            .find(|p| {
                p.target.terminal_name == terminal && p.pending.kind == RelationshipKind::Calls
            })
            .unwrap_or_else(|| {
                panic!(
                    "missing pending call target {terminal}; got: {:#?}",
                    result.structured_pending_relationships
                )
            })
            .target
            .clone()
    };

    let dollar_chain = call_target("chain");
    assert_eq!(dollar_chain.receiver.as_deref(), Some("Inner"));
    assert_eq!(dollar_chain.namespace_path, vec!["Outer"]);
    assert_eq!(dollar_chain.display_name, "Outer.Inner.chain");

    let slot_chain = call_target("chain2");
    assert_eq!(slot_chain.receiver.as_deref(), Some("inner"));
    assert_eq!(slot_chain.namespace_path, vec!["outer"]);
    assert_eq!(slot_chain.display_name, "outer.inner.chain2");

    let two_part = call_target("process");
    assert_eq!(two_part.receiver.as_deref(), Some("Helper"));
    assert!(two_part.namespace_path.is_empty());
    assert_eq!(two_part.display_name, "Helper.process");

    let dotted_name = call_target("print.default");
    assert_eq!(dotted_name.receiver, None);
    assert!(dotted_name.namespace_path.is_empty());
    assert_eq!(dotted_name.display_name, "print.default");
}
