use std::path::Path;

use crate::base::{RelationshipKind, SymbolKind};

const BASIC_FIXTURE: &str = include_str!("../../../../../fixtures/extraction/ruby/basic/source.rb");
const CROSS_FILE_FIXTURE: &str =
    include_str!("../../../../../fixtures/extraction/ruby/cross_file/source.rb");

fn extract(path: &str, source: &str) -> crate::ExtractionResults {
    crate::pipeline::extract_canonical(path, source, Path::new("/repo"))
        .expect("canonical Ruby extraction should succeed")
}

#[test]
fn basic_fixture_emits_resolved_same_file_call_relationship() {
    let results = extract("fixtures/extraction/ruby/basic/source.rb", BASIC_FIXTURE);

    let run = results
        .symbols
        .iter()
        .find(|symbol| symbol.name == "run" && symbol.kind == SymbolKind::Method)
        .expect("run method symbol");
    let helper = results
        .symbols
        .iter()
        .find(|symbol| symbol.name == "helper" && symbol.kind == SymbolKind::Method)
        .expect("helper method symbol");

    let call = results
        .relationships
        .iter()
        .find(|relationship| {
            relationship.kind == RelationshipKind::Calls
                && relationship.from_symbol_id == run.id
                && relationship.to_symbol_id == helper.id
        })
        .expect("run -> helper same-file call should resolve to a relationship");

    assert_eq!(call.file_path, "fixtures/extraction/ruby/basic/source.rb");
    assert!(
        results
            .pending_relationships
            .iter()
            .all(|pending| pending.callee_name != "helper"),
        "resolved helper call must not remain in pending_relationships"
    );
}

#[test]
fn cross_file_fixture_preserves_unresolved_module_call_pending() {
    let results = extract(
        "fixtures/extraction/ruby/cross_file/source.rb",
        CROSS_FILE_FIXTURE,
    );

    assert!(
        results
            .structured_pending_relationships
            .iter()
            .any(|pending| pending.target.terminal_name == "do_thing"),
        "cross-file OtherModule.do_thing must remain structured pending"
    );
    assert!(
        results
            .structured_pending_relationships
            .iter()
            .all(|pending| pending.target.terminal_name != "local_helper"),
        "bare local_helper references must not leak into pending relationships"
    );
    assert!(
        results
            .relationships
            .iter()
            .all(|relationship| relationship.kind != RelationshipKind::Calls),
        "cross-file fixture has no resolved call edges"
    );
}
