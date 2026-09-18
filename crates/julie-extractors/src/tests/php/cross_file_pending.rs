//! Phase 4a.php — PHP emits `StructuredPendingRelationship` for cross-
//! namespace class references (`use App\Other; new Other();`).

use crate::base::SymbolKind;
use crate::extract_canonical;
use std::path::Path;

#[test]
fn test_php_emits_structured_pending_for_cross_namespace_class() {
    let source = include_str!("../../../../../fixtures/extraction/php/cross_file/source.php");
    let workspace_root = Path::new("/tmp/test");
    let result = extract_canonical("source.php", source, workspace_root)
        .expect("canonical PHP extraction must succeed");

    let other = result
        .structured_pending_relationships
        .iter()
        .find(|p| p.target.terminal_name == "Other")
        .unwrap_or_else(|| {
            panic!(
                "expected structured pending for cross-namespace Other; got {} entries: {:#?}",
                result.structured_pending_relationships.len(),
                result.structured_pending_relationships
            )
        });

    assert!(other.pending.line_number > 0);
    assert_eq!(other.pending.file_path, "source.php");
    assert!(other.caller_scope_symbol_id.is_some());

    assert!(
        result
            .structured_pending_relationships
            .iter()
            .all(|p| p.target.terminal_name != "localHelper"),
        "intra-class localHelper must not appear as structured pending"
    );
}

#[test]
fn test_php_negative_local_helper_not_emitted_as_pending() {
    let source = include_str!("../../../../../fixtures/extraction/php/cross_file/source.php");
    let workspace_root = Path::new("/tmp/test");
    let result = extract_canonical("source.php", source, workspace_root)
        .expect("canonical PHP extraction must succeed");

    let id = result
        .symbols
        .iter()
        .find(|s| s.name == "localHelper" && s.kind == SymbolKind::Method)
        .map(|s| s.id.clone())
        .expect("localHelper method symbol must exist");

    assert!(
        result
            .structured_pending_relationships
            .iter()
            .all(|p| p.pending.callee_name != "localHelper"
                && p.target.terminal_name != "localHelper"),
        "intra-class localHelper leaked into pending"
    );
    assert!(!id.is_empty());
}

#[test]
fn test_php_qualified_chain_call_keeps_receiver_and_namespace() {
    let source = r#"<?php
class Caller {
    function run($outer) {
        Outer::Inner::chain();
        $outer->inner->chain2();
        Helper::process();
        $outer->solo();
    }
}
"#;
    let workspace_root = Path::new("/tmp/test");
    let result = extract_canonical("Caller.php", source, workspace_root)
        .expect("canonical PHP extraction must succeed");
    let target = |terminal: &str| {
        result
            .structured_pending_relationships
            .iter()
            .find(|p| p.target.terminal_name == terminal)
            .unwrap_or_else(|| {
                panic!(
                    "missing pending target {terminal}; got: {:#?}",
                    result.structured_pending_relationships
                )
            })
            .target
            .clone()
    };

    let static_chain = target("chain");
    assert_eq!(static_chain.receiver.as_deref(), Some("Inner"));
    assert_eq!(static_chain.namespace_path, vec!["Outer"]);
    assert_eq!(static_chain.display_name, "Outer.Inner.chain");

    let member_chain = target("chain2");
    assert_eq!(member_chain.receiver.as_deref(), Some("inner"));
    assert_eq!(member_chain.namespace_path, vec!["$outer"]);
    assert_eq!(member_chain.display_name, "$outer.inner.chain2");

    let static_two_part = target("process");
    assert_eq!(static_two_part.receiver.as_deref(), Some("Helper"));
    assert!(static_two_part.namespace_path.is_empty());
    assert_eq!(static_two_part.display_name, "Helper.process");

    let member_two_part = target("solo");
    assert_eq!(member_two_part.receiver.as_deref(), Some("outer"));
    assert!(member_two_part.namespace_path.is_empty());
    assert_eq!(member_two_part.display_name, "outer.solo");
}
