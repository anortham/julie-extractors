use std::collections::BTreeSet;
use std::path::Path;

use crate::base::{StructuralFact, SymbolKind};

fn extract(source: &str) -> crate::ExtractionResults {
    crate::pipeline::extract_canonical("source.vue", source, Path::new("/repo"))
        .expect("canonical Vue extraction should succeed")
}

fn facts_with_pattern<'a>(
    results: &'a crate::ExtractionResults,
    pattern_id: &str,
) -> Vec<&'a StructuralFact> {
    results
        .structural_facts
        .iter()
        .filter(|fact| fact.pattern_id == pattern_id)
        .collect()
}

fn metadata_str<'a>(fact: &'a StructuralFact, key: &str) -> Option<&'a str> {
    fact.metadata
        .as_ref()
        .and_then(|metadata| metadata.get(key))
        .and_then(|value| value.as_str())
}

fn metadata_bool(fact: &StructuralFact, key: &str) -> Option<bool> {
    fact.metadata
        .as_ref()
        .and_then(|metadata| metadata.get(key))
        .and_then(|value| value.as_bool())
}

fn metadata_array<'a>(fact: &'a StructuralFact, key: &str) -> Vec<&'a str> {
    fact.metadata
        .as_ref()
        .and_then(|metadata| metadata.get(key))
        .and_then(|value| value.as_array())
        .into_iter()
        .flatten()
        .filter_map(|value| value.as_str())
        .collect()
}

#[test]
fn vue_emits_sfc_section_and_template_directive_facts() {
    let source = r#"<template>
  <section v-if="ready">
    <input v-model.trim="title" :class="{ active: ready }" @click.prevent="submit" />
  </section>
</template>

<script setup lang="ts">
defineOptions({ name: "WorkerPanel" });
const ready = true;
const title = "Worker";
function submit() {}
</script>

<style scoped>
.active {
  color: #0f766e;
}
</style>
"#;

    let results = extract(source);
    let component = results
        .symbols
        .iter()
        .find(|symbol| {
            symbol.kind == SymbolKind::Class
                && symbol.metadata.as_ref().is_some_and(|metadata| {
                    metadata.get("type").and_then(|value| value.as_str()) == Some("vue-sfc")
                })
        })
        .expect("expected Vue SFC component symbol");

    let sections = facts_with_pattern(&results, "vue.sfc_section.v1");
    assert_eq!(
        sections
            .iter()
            .filter_map(|fact| metadata_str(fact, "section_type"))
            .collect::<BTreeSet<_>>(),
        BTreeSet::from(["script", "style", "template"])
    );
    assert!(
        sections
            .iter()
            .all(|fact| metadata_str(fact, "query_family") == Some("component_structure"))
    );
    assert!(
        sections
            .iter()
            .all(|fact| fact.containing_symbol_id.as_deref() == Some(component.id.as_str()))
    );
    let script = sections
        .iter()
        .find(|fact| metadata_str(fact, "section_type") == Some("script"))
        .expect("expected script section fact");
    assert_eq!(metadata_str(script, "lang"), Some("ts"));
    assert_eq!(metadata_bool(script, "setup"), Some(true));

    let style = sections
        .iter()
        .find(|fact| metadata_str(fact, "section_type") == Some("style"))
        .expect("expected style section fact");
    assert_eq!(metadata_bool(style, "scoped"), Some(true));

    let directives = facts_with_pattern(&results, "vue.template_directive.v1");
    assert_eq!(
        directives
            .iter()
            .filter_map(|fact| metadata_str(fact, "directive"))
            .collect::<BTreeSet<_>>(),
        BTreeSet::from(["v-bind", "v-if", "v-model", "v-on"])
    );

    let click = directives
        .iter()
        .find(|fact| metadata_str(fact, "directive") == Some("v-on"))
        .expect("expected v-on directive");
    assert_eq!(
        metadata_str(click, "attribute_name"),
        Some("@click.prevent")
    );
    assert_eq!(metadata_str(click, "argument"), Some("click"));
    assert_eq!(metadata_bool(click, "shorthand"), Some(true));
    assert_eq!(metadata_array(click, "modifiers"), vec!["prevent"]);
    assert_eq!(metadata_str(click, "expression"), Some("submit"));

    let model = directives
        .iter()
        .find(|fact| metadata_str(fact, "directive") == Some("v-model"))
        .expect("expected v-model directive");
    assert_eq!(metadata_array(model, "modifiers"), vec!["trim"]);
    assert_eq!(metadata_str(model, "expression"), Some("title"));
}
