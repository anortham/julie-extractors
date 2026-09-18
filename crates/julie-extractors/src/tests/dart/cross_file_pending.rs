//! Phase 4a.dart — Dart emits `StructuredPendingRelationship` for cross-file
//! references (`import 'other.dart'; Other()`).
//!
//! Source under test: `fixtures/extraction/dart/cross_file/source.dart`.

use crate::base::SymbolKind;
use crate::extract_canonical;
use std::path::Path;

#[test]
fn test_dart_emits_structured_pending_for_cross_file_class() {
    let source = include_str!("../../../../../fixtures/extraction/dart/cross_file/source.dart");
    let workspace_root = Path::new("/tmp/test");
    let result = extract_canonical("source.dart", source, workspace_root)
        .expect("canonical Dart extraction must succeed");

    let other = result
        .structured_pending_relationships
        .iter()
        .find(|p| p.target.terminal_name == "Other")
        .unwrap_or_else(|| {
            panic!(
                "expected structured pending for cross-file Other; got {} entries: {:#?}",
                result.structured_pending_relationships.len(),
                result.structured_pending_relationships
            )
        });

    assert!(
        other.pending.line_number > 0,
        "pending.line_number must reflect call site, not 0"
    );
    assert_eq!(
        other.pending.file_path, "source.dart",
        "pending.file_path must match the file under extraction"
    );
    assert!(
        other.caller_scope_symbol_id.is_some(),
        "caller_scope_symbol_id must point at enclosing function"
    );

    assert!(
        result
            .structured_pending_relationships
            .iter()
            .all(|p| p.target.terminal_name != "local_helper"),
        "intra-file local_helper call must not appear as structured pending; got: {:#?}",
        result.structured_pending_relationships
    );
}

#[test]
fn test_dart_negative_local_helper_not_emitted_as_pending() {
    let source = include_str!("../../../../../fixtures/extraction/dart/cross_file/source.dart");
    let workspace_root = Path::new("/tmp/test");
    let result = extract_canonical("source.dart", source, workspace_root)
        .expect("canonical Dart extraction must succeed");

    let local_helper_id = result
        .symbols
        .iter()
        .find(|s| s.name == "local_helper" && s.kind == SymbolKind::Function)
        .map(|s| s.id.clone())
        .expect("local_helper symbol must exist");

    assert!(
        result
            .structured_pending_relationships
            .iter()
            .all(|p| p.pending.callee_name != "local_helper"
                && p.target.terminal_name != "local_helper"),
        "intra-file local_helper call leaked into structured pending"
    );
    assert!(!local_helper_id.is_empty());
}

#[test]
fn test_dart_qualified_chain_call_keeps_receiver_and_namespace() {
    let source = r#"
class Caller {
  void run() {
    Outer.Inner.chain();
    Helper.process();
  }
}
"#;
    let workspace_root = Path::new("/tmp/test");
    let result = extract_canonical("caller.dart", source, workspace_root)
        .expect("canonical Dart extraction must succeed");
    let pending = |terminal: &str| {
        result
            .structured_pending_relationships
            .iter()
            .find(|p| p.target.terminal_name == terminal)
            .unwrap_or_else(|| panic!("{terminal} must be a pending target"))
    };

    let chain = &pending("chain").target;
    assert_eq!(chain.receiver.as_deref(), Some("Inner"));
    assert_eq!(chain.namespace_path, vec!["Outer"]);
    assert_eq!(chain.display_name, "Outer.Inner.chain");

    let two_part = &pending("process").target;
    assert_eq!(two_part.receiver.as_deref(), Some("Helper"));
    assert!(two_part.namespace_path.is_empty());
    assert_eq!(two_part.display_name, "Helper.process");
}
