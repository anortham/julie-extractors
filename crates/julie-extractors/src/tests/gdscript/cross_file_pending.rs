//! Phase 4a.gdscript — GDScript emits `StructuredPendingRelationship` for
//! cross-script references (`extends "res://other.gd"; other_method()`).
//!
//! Source under test: `fixtures/extraction/gdscript/cross_file/source.gd`.

use crate::base::SymbolKind;
use crate::extract_canonical;
use std::path::Path;

#[test]
fn test_gdscript_emits_structured_pending_for_cross_script_call() {
    let source = include_str!("../../../../../fixtures/extraction/gdscript/cross_file/source.gd");
    let workspace_root = Path::new("/tmp/test");
    let result = extract_canonical("source.gd", source, workspace_root)
        .expect("canonical GDScript extraction must succeed");

    let other = result
        .structured_pending_relationships
        .iter()
        .find(|p| p.target.terminal_name == "other_method")
        .unwrap_or_else(|| {
            panic!(
                "expected structured pending for cross-script other_method; got {} entries: {:#?}",
                result.structured_pending_relationships.len(),
                result.structured_pending_relationships
            )
        });

    assert!(
        other.pending.line_number > 0,
        "pending.line_number must reflect call site, not 0"
    );
    assert_eq!(
        other.pending.file_path, "source.gd",
        "pending.file_path must match the file under extraction"
    );
    assert!(
        other.caller_scope_symbol_id.is_some(),
        "caller_scope_symbol_id must point at enclosing func"
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
fn test_gdscript_negative_local_helper_not_emitted_as_pending() {
    let source = include_str!("../../../../../fixtures/extraction/gdscript/cross_file/source.gd");
    let workspace_root = Path::new("/tmp/test");
    let result = extract_canonical("source.gd", source, workspace_root)
        .expect("canonical GDScript extraction must succeed");

    let local_helper_id = result
        .symbols
        .iter()
        .find(|s| {
            s.name == "local_helper"
                && (s.kind == SymbolKind::Function || s.kind == SymbolKind::Method)
        })
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
fn test_gdscript_qualified_chain_call_keeps_receiver_and_namespace() {
    let source = r#"
extends Node

func run():
    Outer.Inner.chain()
    Helper.process()
"#;
    let workspace_root = Path::new("/tmp/test");
    let result = extract_canonical("caller.gd", source, workspace_root)
        .expect("canonical GDScript extraction must succeed");
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
}
