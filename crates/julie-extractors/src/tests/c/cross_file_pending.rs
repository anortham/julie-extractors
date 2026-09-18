//! Phase 4a.c — C emits `StructuredPendingRelationship` for cross-
//! translation-unit calls (`extern int other_func(void); other_func();`).

use crate::base::SymbolKind;
use crate::extract_canonical;
use std::path::Path;

#[test]
fn test_c_emits_structured_pending_for_extern_call() {
    let source = include_str!("../../../../../fixtures/extraction/c/cross_file/source.c");
    let workspace_root = Path::new("/tmp/test");
    let result = extract_canonical("source.c", source, workspace_root)
        .expect("canonical C extraction must succeed");

    let other = result
        .structured_pending_relationships
        .iter()
        .find(|p| p.target.terminal_name == "other_func")
        .unwrap_or_else(|| {
            panic!(
                "expected structured pending for extern other_func; got {} entries: {:#?}",
                result.structured_pending_relationships.len(),
                result.structured_pending_relationships
            )
        });

    assert!(other.pending.line_number > 0);
    assert_eq!(other.pending.file_path, "source.c");
    assert!(other.caller_scope_symbol_id.is_some());

    assert!(
        result
            .structured_pending_relationships
            .iter()
            .all(|p| p.target.terminal_name != "local_helper"),
        "intra-file local_helper must not appear as structured pending"
    );
}

#[test]
fn test_c_negative_local_helper_not_emitted_as_pending() {
    let source = include_str!("../../../../../fixtures/extraction/c/cross_file/source.c");
    let workspace_root = Path::new("/tmp/test");
    let result = extract_canonical("source.c", source, workspace_root)
        .expect("canonical C extraction must succeed");

    let id = result
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
        "intra-file local_helper leaked into pending"
    );
    assert!(!id.is_empty());
}

#[test]
fn test_c_qualified_chain_call_keeps_receiver_and_namespace() {
    let source = r#"
struct In { void (*chain)(void); void (*solo)(void); };
struct Out { struct In inner; struct In *pin; };
void run(struct Out outer, struct Out *p) {
    outer.inner.chain();
    p->pin->chain2();
    outer.solo();
}
"#;
    let workspace_root = Path::new("/tmp/test");
    let result = extract_canonical("run.c", source, workspace_root)
        .expect("canonical C extraction must succeed");
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

    let chain = target("chain");
    assert_eq!(chain.receiver.as_deref(), Some("inner"));
    assert_eq!(chain.namespace_path, vec!["outer"]);
    assert_eq!(chain.display_name, "outer.inner.chain");

    let pointer_chain = target("chain2");
    assert_eq!(pointer_chain.receiver.as_deref(), Some("pin"));
    assert_eq!(pointer_chain.namespace_path, vec!["p"]);
    assert_eq!(pointer_chain.display_name, "p.pin.chain2");

    let two_part = target("solo");
    assert_eq!(two_part.receiver.as_deref(), Some("outer"));
    assert!(two_part.namespace_path.is_empty());
    assert_eq!(two_part.display_name, "outer.solo");
}
