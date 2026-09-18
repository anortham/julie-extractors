//! Phase 4a.csharp — C# emits `StructuredPendingRelationship` for cross-
//! namespace class references (`using OtherNs; new OtherClass();`).
//!
//! Source under test: `fixtures/extraction/csharp/cross_file/Source.cs`.

use crate::base::SymbolKind;
use crate::extract_canonical;
use std::path::Path;

#[test]
fn test_csharp_emits_structured_pending_for_cross_namespace_class() {
    let source = include_str!("../../../../../fixtures/extraction/csharp/cross_file/Source.cs");
    let workspace_root = Path::new("/tmp/test");
    let result = extract_canonical("Source.cs", source, workspace_root)
        .expect("canonical C# extraction must succeed");

    let other = result
        .structured_pending_relationships
        .iter()
        .find(|p| p.target.terminal_name == "OtherClass")
        .unwrap_or_else(|| {
            panic!(
                "expected structured pending for cross-namespace OtherClass; got {} entries: {:#?}",
                result.structured_pending_relationships.len(),
                result.structured_pending_relationships
            )
        });

    assert!(
        other.pending.line_number > 0,
        "pending.line_number must reflect call site, not 0"
    );
    assert_eq!(
        other.pending.file_path, "Source.cs",
        "pending.file_path must match the file under extraction"
    );
    assert!(
        other.caller_scope_symbol_id.is_some(),
        "caller_scope_symbol_id must point at enclosing method/class"
    );

    assert!(
        result
            .structured_pending_relationships
            .iter()
            .all(|p| p.target.terminal_name != "LocalHelper"),
        "intra-class LocalHelper call must not appear as structured pending; got: {:#?}",
        result.structured_pending_relationships
    );
}

#[test]
fn test_csharp_negative_local_helper_not_emitted_as_pending() {
    let source = include_str!("../../../../../fixtures/extraction/csharp/cross_file/Source.cs");
    let workspace_root = Path::new("/tmp/test");
    let result = extract_canonical("Source.cs", source, workspace_root)
        .expect("canonical C# extraction must succeed");

    let local_helper_id = result
        .symbols
        .iter()
        .find(|s| s.name == "LocalHelper" && s.kind == SymbolKind::Method)
        .map(|s| s.id.clone())
        .expect("LocalHelper method symbol must exist");

    assert!(
        result
            .structured_pending_relationships
            .iter()
            .all(|p| p.pending.callee_name != "LocalHelper"
                && p.target.terminal_name != "LocalHelper"),
        "intra-class LocalHelper call leaked into structured pending"
    );
    assert!(!local_helper_id.is_empty());
}

#[test]
fn test_csharp_qualified_call_chain_keeps_receiver_and_namespace() {
    let source = r#"
class Caller
{
    void Run()
    {
        EmailSettingQueryRepository.EmailSettingPlaceholderGenerators.AnnualAttestationNotification("u", "n");
        Helper.Process();
    }
}
"#;
    let workspace_root = Path::new("/tmp/test");
    let result = extract_canonical("Caller.cs", source, workspace_root)
        .expect("canonical C# extraction must succeed");

    let chain: Vec<_> = result
        .structured_pending_relationships
        .iter()
        .filter(|p| p.target.terminal_name == "AnnualAttestationNotification")
        .collect();
    assert_eq!(
        chain.len(),
        1,
        "expected exactly one pending target for the chained call; got: {:#?}",
        result.structured_pending_relationships
    );
    let chain = &chain[0].target;
    assert_eq!(
        chain.receiver.as_deref(),
        Some("EmailSettingPlaceholderGenerators")
    );
    assert_eq!(chain.namespace_path, vec!["EmailSettingQueryRepository"]);
    assert_eq!(
        chain.display_name,
        "EmailSettingQueryRepository.EmailSettingPlaceholderGenerators.AnnualAttestationNotification"
    );

    let two_part = result
        .structured_pending_relationships
        .iter()
        .find(|p| p.target.terminal_name == "Process")
        .expect("Helper.Process must stay a pending target");
    assert_eq!(two_part.target.receiver.as_deref(), Some("Helper"));
    assert!(two_part.target.namespace_path.is_empty());
    assert_eq!(two_part.target.display_name, "Helper.Process");
}
