//! Phase 4a.ruby — Ruby emits `StructuredPendingRelationship` for cross-
//! file calls (`require 'other'; OtherModule.do_thing`).

use crate::base::SymbolKind;
use crate::extract_canonical;
use std::path::Path;

#[test]
fn test_ruby_emits_structured_pending_for_cross_file_call() {
    let source = include_str!("../../../../../fixtures/extraction/ruby/cross_file/source.rb");
    let workspace_root = Path::new("/tmp/test");
    let result = extract_canonical("source.rb", source, workspace_root)
        .expect("canonical Ruby extraction must succeed");

    let do_thing = result
        .structured_pending_relationships
        .iter()
        .find(|p| p.target.terminal_name == "do_thing")
        .unwrap_or_else(|| {
            panic!(
                "expected structured pending for cross-file do_thing; got {} entries: {:#?}",
                result.structured_pending_relationships.len(),
                result.structured_pending_relationships
            )
        });

    assert!(do_thing.pending.line_number > 0);
    assert_eq!(do_thing.pending.file_path, "source.rb");
    assert!(do_thing.caller_scope_symbol_id.is_some());

    assert!(
        result
            .structured_pending_relationships
            .iter()
            .all(|p| p.target.terminal_name != "local_helper"),
        "intra-class local_helper must not appear as structured pending"
    );
}

#[test]
fn test_ruby_negative_local_helper_not_emitted_as_pending() {
    let source = include_str!("../../../../../fixtures/extraction/ruby/cross_file/source.rb");
    let workspace_root = Path::new("/tmp/test");
    let result = extract_canonical("source.rb", source, workspace_root)
        .expect("canonical Ruby extraction must succeed");

    let id = result
        .symbols
        .iter()
        .find(|s| s.name == "local_helper" && s.kind == SymbolKind::Method)
        .map(|s| s.id.clone())
        .expect("local_helper method symbol must exist");

    assert!(
        result
            .structured_pending_relationships
            .iter()
            .all(|p| p.pending.callee_name != "local_helper"
                && p.target.terminal_name != "local_helper"),
        "intra-class local_helper leaked into pending"
    );
    assert!(!id.is_empty());
}

#[test]
fn test_ruby_qualified_chain_call_keeps_receiver_and_namespace() {
    let source = r#"
class Caller
  def run(outer)
    Outer::Inner.chain
    outer.inner.chain2
    Helper.process
    self.run
    client.self.run
    Outer::Inner.run!
    Outer::Inner.ready?
    build().dispatch
    self.call
  end
end
"#;
    let workspace_root = Path::new("/tmp/test");
    let result = extract_canonical("caller.rb", source, workspace_root)
        .expect("canonical Ruby extraction must succeed");
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

    let constant_chain = target("chain");
    assert_eq!(constant_chain.receiver.as_deref(), Some("Inner"));
    assert_eq!(constant_chain.namespace_path, vec!["Outer"]);
    assert_eq!(constant_chain.display_name, "Outer.Inner.chain");

    let dotted_chain = target("chain2");
    assert_eq!(dotted_chain.receiver.as_deref(), Some("inner"));
    assert_eq!(dotted_chain.namespace_path, vec!["outer"]);
    assert_eq!(dotted_chain.display_name, "outer.inner.chain2");

    let two_part = target("process");
    assert_eq!(two_part.receiver.as_deref(), Some("Helper"));
    assert!(two_part.namespace_path.is_empty());
    assert_eq!(two_part.display_name, "Helper.process");

    let expression_chain = target("run");
    assert_eq!(expression_chain.receiver.as_deref(), Some("self"));
    assert_eq!(expression_chain.namespace_path, vec!["client"]);
    assert_eq!(expression_chain.display_name, "client.self.run");

    let bang = target("run!");
    assert_eq!(bang.receiver.as_deref(), Some("Inner"));
    assert_eq!(bang.namespace_path, vec!["Outer"]);
    assert_eq!(bang.display_name, "Outer.Inner.run!");

    let question = target("ready?");
    assert_eq!(question.receiver.as_deref(), Some("Inner"));
    assert_eq!(question.namespace_path, vec!["Outer"]);
    assert_eq!(question.display_name, "Outer.Inner.ready?");

    let expression_call = target("dispatch");
    assert_eq!(expression_call.receiver.as_deref(), Some("build"));
    assert!(expression_call.namespace_path.is_empty());
    assert_eq!(expression_call.display_name, "build.dispatch");
    assert_eq!(result.relationships.len(), 1);
}
