use std::collections::BTreeSet;
use std::path::Path;

use crate::base::StructuralFact;

const FIXTURE_SOURCE: &str =
    include_str!("../../../../../fixtures/extraction/lua/basic/source.lua");

fn extract(source: &str) -> crate::ExtractionResults {
    crate::pipeline::extract_canonical(
        "fixtures/extraction/lua/basic/source.lua",
        source,
        Path::new("/repo"),
    )
    .expect("canonical Lua extraction should succeed")
}

fn metadata_str<'a>(fact: &'a StructuralFact, key: &str) -> Option<&'a str> {
    fact.metadata
        .as_ref()
        .and_then(|metadata| metadata.get(key))
        .and_then(|value| value.as_str())
}

fn metadata_u64(fact: &StructuralFact, key: &str) -> Option<u64> {
    fact.metadata
        .as_ref()
        .and_then(|metadata| metadata.get(key))
        .and_then(|value| value.as_u64())
}

#[test]
fn lua_emits_expected_structural_fact_patterns() {
    let results = extract(FIXTURE_SOURCE);
    let pattern_ids = results
        .structural_facts
        .iter()
        .map(|fact| fact.pattern_id.as_str())
        .collect::<BTreeSet<_>>();

    for pattern_id in [
        "lua.require_call.v1",
        "lua.setmetatable_call.v1",
        "lua.coroutine_call.v1",
        "lua.module_return.v1",
        "lua.table_constructor.v1",
    ] {
        assert!(
            pattern_ids.contains(pattern_id),
            "missing structural fact pattern `{pattern_id}`"
        );
    }

    let require = results
        .structural_facts
        .iter()
        .find(|fact| fact.pattern_id == "lua.require_call.v1")
        .expect("expected require call fact");
    assert_eq!(metadata_str(require, "call_name"), Some("require"));
    assert_eq!(metadata_str(require, "required_module"), Some("json"));

    let setmetatable = results
        .structural_facts
        .iter()
        .find(|fact| fact.pattern_id == "lua.setmetatable_call.v1")
        .expect("expected setmetatable call fact");
    assert_eq!(
        metadata_str(setmetatable, "call_name"),
        Some("setmetatable")
    );
    assert_ne!(
        metadata_str(require, "call_name"),
        metadata_str(setmetatable, "call_name"),
        "require call name must not be confused with setmetatable"
    );

    let module_return = results
        .structural_facts
        .iter()
        .find(|fact| fact.pattern_id == "lua.module_return.v1")
        .expect("expected module return fact");
    assert_eq!(
        metadata_str(module_return, "returned_value"),
        Some("Worker")
    );

    let table = results
        .structural_facts
        .iter()
        .find(|fact| fact.pattern_id == "lua.table_constructor.v1")
        .expect("expected table constructor fact");
    assert_eq!(metadata_u64(table, "field_count"), Some(1));
}
