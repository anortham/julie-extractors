use std::collections::BTreeSet;
use std::path::Path;

use crate::base::StructuralFact;
use crate::tests::helpers::{facts_with_pattern, metadata_str};

fn extract(file_path: &str, source: &str) -> crate::ExtractionResults {
    crate::pipeline::extract_canonical(file_path, source, Path::new("/repo"))
        .expect("canonical Erlang extraction should succeed")
}


fn only_fact<'a>(results: &'a crate::ExtractionResults, pattern_id: &str) -> &'a StructuralFact {
    let facts = facts_with_pattern(results, pattern_id);
    assert_eq!(
        facts.len(),
        1,
        "expected exactly one {pattern_id} fact, got {}",
        facts.len()
    );
    facts[0]
}


fn metadata_u64(fact: &StructuralFact, key: &str) -> Option<u64> {
    fact.metadata
        .as_ref()
        .and_then(|metadata| metadata.get(key))
        .and_then(|value| value.as_u64())
}

fn pattern_ids(results: &crate::ExtractionResults) -> BTreeSet<&str> {
    results
        .structural_facts
        .iter()
        .map(|fact| fact.pattern_id.as_str())
        .collect()
}

const SERVER: &str = r#"-module(ledger_server).
-behaviour(gen_server).

-include("ledger_records.hrl").
-include_lib("stdlib/include/assert.hrl").

-export([start/0, stop/1]).
-export_type([handle/0]).

-callback init(Args :: term()) -> {ok, term()}.
-callback terminate(term(), term()) -> ok.

-type handle() :: reference().

start() ->
    ok.

stop(_Handle) ->
    ok.
"#;

#[test]
fn a_module_attribute_carries_its_declared_module_name() {
    let results = extract("ledger_server.erl", SERVER);
    let module = only_fact(&results, "erlang.module_attribute.v1");

    assert_eq!(module.capture_name, "module_attribute");
    assert_eq!(module.node_kind, "module_attribute");
    assert_eq!(metadata_str(module, "module"), Some("ledger_server"));
    assert_eq!(metadata_str(module, "query_family"), Some("module"));
}

#[test]
fn a_behaviour_declaration_records_the_target_and_the_spelling_as_written() {
    let results = extract("ledger_server.erl", SERVER);
    let behaviour = only_fact(&results, "erlang.behaviour_declaration.v1");

    assert_eq!(behaviour.capture_name, "behaviour_declaration");
    assert_eq!(behaviour.node_kind, "behaviour_attribute");
    assert_eq!(metadata_str(behaviour, "behaviour"), Some("gen_server"));
    assert_eq!(metadata_str(behaviour, "attribute"), Some("behaviour"));
}

#[test]
fn the_american_behavior_spelling_is_recorded_verbatim() {
    let results = extract(
        "supervisor_shim.erl",
        "-module(supervisor_shim).\n-behavior(supervisor).\n",
    );
    let behaviour = only_fact(&results, "erlang.behaviour_declaration.v1");

    assert_eq!(metadata_str(behaviour, "behaviour"), Some("supervisor"));
    assert_eq!(metadata_str(behaviour, "attribute"), Some("behavior"));
}

#[test]
fn export_attributes_separate_function_and_type_lists() {
    let results = extract("ledger_server.erl", SERVER);
    let exports = facts_with_pattern(&results, "erlang.export_attribute.v1");

    assert_eq!(exports.len(), 2);

    let functions = exports
        .iter()
        .find(|fact| metadata_str(fact, "export_kind") == Some("function"))
        .expect("function export attribute");
    assert_eq!(functions.node_kind, "export_attribute");
    assert_eq!(metadata_u64(functions, "exported_count"), Some(2));

    let types = exports
        .iter()
        .find(|fact| metadata_str(fact, "export_kind") == Some("type"))
        .expect("type export attribute");
    assert_eq!(types.node_kind, "export_type_attribute");
    assert_eq!(metadata_u64(types, "exported_count"), Some(1));
}

#[test]
fn an_empty_export_list_still_emits_a_zero_count_fact() {
    let results = extract("silent.erl", "-module(silent).\n-export([]).\n");
    let export = only_fact(&results, "erlang.export_attribute.v1");

    assert_eq!(metadata_str(export, "export_kind"), Some("function"));
    assert_eq!(metadata_u64(export, "exported_count"), Some(0));
}

#[test]
fn callback_declarations_carry_name_and_arity() {
    let results = extract("ledger_server.erl", SERVER);
    let callbacks = facts_with_pattern(&results, "erlang.callback_declaration.v1");

    let named: Vec<(Option<&str>, Option<u64>)> = callbacks
        .iter()
        .map(|fact| {
            (
                metadata_str(fact, "callback_name"),
                metadata_u64(fact, "arity"),
            )
        })
        .collect();

    assert_eq!(
        named,
        vec![(Some("init"), Some(1)), (Some("terminate"), Some(2))]
    );
    assert_eq!(callbacks[0].capture_name, "callback_declaration");
    assert_eq!(callbacks[0].node_kind, "callback");
}

#[test]
fn include_directives_separate_include_from_include_lib() {
    let results = extract("ledger_server.erl", SERVER);
    let includes = facts_with_pattern(&results, "erlang.include_directive.v1");

    assert_eq!(includes.len(), 2);

    let plain = includes
        .iter()
        .find(|fact| metadata_str(fact, "include_kind") == Some("include"))
        .expect("plain include");
    assert_eq!(plain.node_kind, "pp_include");
    assert_eq!(metadata_str(plain, "path"), Some("ledger_records.hrl"));
    assert_eq!(metadata_str(plain, "application"), None);

    let lib = includes
        .iter()
        .find(|fact| metadata_str(fact, "include_kind") == Some("include_lib"))
        .expect("include_lib");
    assert_eq!(lib.node_kind, "pp_include_lib");
    assert_eq!(metadata_str(lib, "path"), Some("stdlib/include/assert.hrl"));
    assert_eq!(metadata_str(lib, "application"), Some("stdlib"));
}

/// `-include_lib` without a path separator names no application, so the
/// optional key stays absent rather than repeating the bare file name.
#[test]
fn an_include_lib_without_a_path_prefix_records_no_application() {
    let results = extract(
        "bare.erl",
        "-module(bare).\n-include_lib(\"assert.hrl\").\n",
    );
    let include = only_fact(&results, "erlang.include_directive.v1");

    assert_eq!(metadata_str(include, "include_kind"), Some("include_lib"));
    assert_eq!(metadata_str(include, "path"), Some("assert.hrl"));
    assert_eq!(metadata_str(include, "application"), None);
}

/// A macro-spelled include path carries no literal to record, so the directive
/// emits nothing rather than a fact with an invented path.
#[test]
fn a_macro_spelled_include_path_emits_no_fact() {
    let results = extract(
        "macro_include.erl",
        "-module(macro_include).\n-define(HDR, \"x.hrl\").\n-include(?HDR).\n",
    );

    assert!(facts_with_pattern(&results, "erlang.include_directive.v1").is_empty());
}

#[test]
fn a_plain_module_emits_only_the_shapes_it_declares() {
    let results = extract("plain.erl", "-module(plain).\n\nrun() ->\n    ok.\n");

    assert_eq!(
        pattern_ids(&results),
        BTreeSet::from(["erlang.module_attribute.v1"])
    );
}

#[test]
fn structural_facts_are_ordered_by_source_position() {
    let results = extract("ledger_server.erl", SERVER);
    let lines: Vec<u32> = results
        .structural_facts
        .iter()
        .map(|fact| fact.start_line)
        .collect();
    let mut sorted = lines.clone();
    sorted.sort_unstable();

    assert_eq!(lines, sorted);
}
