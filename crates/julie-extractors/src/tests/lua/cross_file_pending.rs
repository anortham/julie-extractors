//! Phase 4a.lua — Lua emits `StructuredPendingRelationship` for cross-
//! module calls (`local other = require("other"); other.fn()`).

use crate::extract_canonical;
use std::path::Path;

#[test]
fn test_lua_emits_structured_pending_for_cross_module_call() {
    let source = include_str!("../../../../../fixtures/extraction/lua/cross_file/source.lua");
    let workspace_root = Path::new("/tmp/test");
    let result = extract_canonical("source.lua", source, workspace_root)
        .expect("canonical Lua extraction must succeed");

    let fn_pending = result
        .structured_pending_relationships
        .iter()
        .find(|p| p.target.terminal_name == "fn")
        .unwrap_or_else(|| {
            panic!(
                "expected structured pending for cross-module fn; got {} entries: {:#?}",
                result.structured_pending_relationships.len(),
                result.structured_pending_relationships
            )
        });

    assert!(fn_pending.pending.line_number > 0);
    assert_eq!(fn_pending.pending.file_path, "source.lua");

    assert!(
        result
            .structured_pending_relationships
            .iter()
            .all(|p| p.target.terminal_name != "local_helper"),
        "intra-file local_helper must not appear as structured pending"
    );
}

#[test]
fn test_lua_negative_local_helper_not_emitted_as_pending() {
    let source = include_str!("../../../../../fixtures/extraction/lua/cross_file/source.lua");
    let workspace_root = Path::new("/tmp/test");
    let result = extract_canonical("source.lua", source, workspace_root)
        .expect("canonical Lua extraction must succeed");

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
fn test_lua_qualified_chain_call_keeps_receiver_and_namespace() {
    let source = r#"
local function run()
  Outer.Inner.chain()
  Outer.Inner:method()
  Helper.process()
end
"#;
    let workspace_root = Path::new("/tmp/test");
    let result = extract_canonical("caller.lua", source, workspace_root)
        .expect("canonical Lua extraction must succeed");
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

    let colon_chain = &pending("method").target;
    assert_eq!(colon_chain.receiver.as_deref(), Some("Inner"));
    assert_eq!(colon_chain.namespace_path, vec!["Outer"]);
    assert_eq!(colon_chain.display_name, "Outer.Inner.method");

    let two_part = &pending("process").target;
    assert_eq!(two_part.receiver.as_deref(), Some("Helper"));
    assert!(two_part.namespace_path.is_empty());
    assert_eq!(two_part.display_name, "Helper.process");
}
