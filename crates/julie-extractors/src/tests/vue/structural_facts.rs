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

fn metadata_i64(fact: &StructuralFact, key: &str) -> Option<i64> {
    fact.metadata
        .as_ref()
        .and_then(|metadata| metadata.get(key))
        .and_then(|value| value.as_i64())
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

#[test]
fn vue_emits_route_reference_facts() {
    let source = r#"<template>
  <nav>
    <RouterLink to="/todos">Todos</RouterLink>
    <router-link to="/admin">Admin</router-link>
    <RouterLink :to="'/projects'">Projects</RouterLink>
    <RouterLink :to="computedRoute">Computed</RouterLink>
    <button @click="$router.push('/settings')">Settings</button>
    <input v-if="$router.push('/false-positive')" v-model="/model" :class="'/class'" />
  </nav>
</template>
"#;

    let results = extract(source);
    let route_refs = facts_with_pattern(&results, "vue.route_reference.v1");

    assert_eq!(
        route_refs
            .iter()
            .filter_map(|fact| metadata_str(fact, "target_path"))
            .collect::<BTreeSet<_>>(),
        BTreeSet::from(["/admin", "/projects", "/settings", "/todos"])
    );
    assert!(
        route_refs
            .iter()
            .all(|fact| metadata_str(fact, "query_family") == Some("frontend_navigation"))
    );
    assert!(
        route_refs
            .iter()
            .all(|fact| metadata_str(fact, "framework") == Some("vue"))
    );
    assert!(
        route_refs
            .iter()
            .all(|fact| metadata_str(fact, "verb") == Some("GET"))
    );
    assert!(
        route_refs
            .iter()
            .all(|fact| metadata_i64(fact, "pattern_version") == Some(1))
    );

    let todos = route_refs
        .iter()
        .find(|fact| metadata_str(fact, "target_path") == Some("/todos"))
        .expect("expected plain RouterLink to route fact");
    assert_eq!(metadata_str(todos, "source_kind"), Some("router_link"));
    assert_eq!(metadata_str(todos, "attribute_name"), Some("to"));
    assert_eq!(metadata_str(todos, "expression"), None);

    let admin = route_refs
        .iter()
        .find(|fact| metadata_str(fact, "target_path") == Some("/admin"))
        .expect("expected plain router-link to route fact");
    assert_eq!(metadata_str(admin, "source_kind"), Some("router_link"));
    assert_eq!(metadata_str(admin, "attribute_name"), Some("to"));

    let projects = route_refs
        .iter()
        .find(|fact| metadata_str(fact, "target_path") == Some("/projects"))
        .expect("expected bound literal RouterLink route fact");
    assert_eq!(metadata_str(projects, "source_kind"), Some("router_link"));
    assert_eq!(metadata_str(projects, "attribute_name"), Some(":to"));
    assert_eq!(metadata_str(projects, "expression"), Some("'/projects'"));

    let settings = route_refs
        .iter()
        .find(|fact| metadata_str(fact, "target_path") == Some("/settings"))
        .expect("expected router push route fact");
    assert_eq!(
        metadata_str(settings, "source_kind"),
        Some("router_navigation_expression")
    );
    assert_eq!(metadata_str(settings, "attribute_name"), Some("@click"));
    assert_eq!(
        metadata_str(settings, "expression"),
        Some("$router.push('/settings')")
    );

    assert!(
        route_refs
            .iter()
            .all(|fact| metadata_str(fact, "target_path") != Some("/false-positive"))
    );
    assert!(
        route_refs
            .iter()
            .all(|fact| metadata_str(fact, "target_path") != Some("/model"))
    );
    assert!(
        route_refs
            .iter()
            .all(|fact| metadata_str(fact, "target_path") != Some("/class"))
    );
}

#[test]
fn vue_emits_static_route_definition_facts() {
    let source = r#"<template>
  <nav>
    <RouterLink to="/calendar">Calendar</RouterLink>
    <router-link to="/settings">Settings</router-link>
    <RouterLink :to="{ name: 'dynamic' }">Dynamic</RouterLink>
  </nav>
</template>

<script setup lang="ts">
import CalendarView from '../views/CalendarView.vue'
import SettingsView from '../views/SettingsView.vue'

const routes = [
  {
    path: '/calendar',
    name: 'calendar',
    component: CalendarView,
  },
  { path: '/settings', component: SettingsView },
  { path: dynamicPath, component: CalendarView },
]
</script>
"#;

    let results = extract(source);
    let definitions = facts_with_pattern(&results, "vue.route_definition.v1");
    assert_eq!(definitions.len(), 2);
    assert_eq!(
        definitions
            .iter()
            .filter_map(|fact| metadata_str(fact, "target_path"))
            .collect::<BTreeSet<_>>(),
        BTreeSet::from(["/calendar", "/settings"])
    );
    let calendar_definition = definitions
        .iter()
        .find(|fact| metadata_str(fact, "target_path") == Some("/calendar"))
        .expect("expected calendar route definition");
    assert_eq!(
        metadata_str(calendar_definition, "query_family"),
        Some("frontend_navigation")
    );
    assert_eq!(metadata_str(calendar_definition, "framework"), Some("vue"));
    assert_eq!(
        metadata_str(calendar_definition, "source_kind"),
        Some("vue_router_route")
    );
    assert_eq!(
        metadata_str(calendar_definition, "route_source"),
        Some("string_literal")
    );
    assert_eq!(
        metadata_str(calendar_definition, "route_name"),
        Some("calendar")
    );
    assert_eq!(
        metadata_str(calendar_definition, "component_name"),
        Some("CalendarView")
    );
    assert_eq!(
        metadata_str(calendar_definition, "component_path"),
        Some("../views/CalendarView.vue")
    );
}
