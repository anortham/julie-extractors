use std::collections::BTreeSet;
use std::path::Path;

use crate::base::{StructuralFact, SymbolKind};
use crate::tests::helpers::{facts_with_pattern, metadata_str};

fn extract(source: &str) -> crate::ExtractionResults {
    crate::pipeline::extract_canonical("source.vue", source, Path::new("/repo"))
        .expect("canonical Vue extraction should succeed")
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
fn vue_template_route_references_survive_nested_template_blocks() {
    let source = r#"<template>
  <LayoutShell>
    <template #actions>
      <RouterLink to="/inside-slot">Inside</RouterLink>
    </template>

    <RouterLink to="/after-slot">After</RouterLink>
  </LayoutShell>
</template>
"#;

    let results = extract(source);
    let route_refs = facts_with_pattern(&results, "vue.route_reference.v1");

    assert_eq!(
        route_refs
            .iter()
            .filter_map(|fact| metadata_str(fact, "target_path"))
            .collect::<BTreeSet<_>>(),
        BTreeSet::from(["/after-slot", "/inside-slot"])
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

#[test]
fn vue_route_definitions_use_owning_object_and_child_context() {
    let source = r#"<script setup lang="ts">
import AdminView from '../views/AdminView.vue'
import BillingView from '../views/BillingView.vue'
import SettingsView from '../views/SettingsView.vue'

const routes = [
  {
    meta: {
      requiresAuth: true,
    },
    path: '/admin',
    component: AdminView,
    children: [
      {
        path: 'settings',
        component: SettingsView,
      },
      {
        path: '/billing',
        component: BillingView,
      },
    ],
  },
]
</script>

<template>
  <RouterView />
</template>
"#;

    let results = extract(source);
    let definitions = facts_with_pattern(&results, "vue.route_definition.v1");
    assert_eq!(
        definitions
            .iter()
            .filter_map(|fact| metadata_str(fact, "target_path"))
            .collect::<BTreeSet<_>>(),
        BTreeSet::from(["/admin", "/billing", "settings"])
    );

    let admin = definitions
        .iter()
        .find(|fact| metadata_str(fact, "target_path") == Some("/admin"))
        .expect("expected parent admin route");
    assert_eq!(metadata_str(admin, "component_name"), Some("AdminView"));

    let settings = definitions
        .iter()
        .find(|fact| metadata_str(fact, "target_path") == Some("settings"))
        .expect("expected child settings route");
    assert_eq!(
        metadata_str(settings, "component_name"),
        Some("SettingsView")
    );
    assert_eq!(metadata_str(settings, "parent_route_path"), Some("/admin"));
    assert_eq!(
        metadata_str(settings, "effective_route_template"),
        Some("/admin/settings")
    );

    let billing = definitions
        .iter()
        .find(|fact| metadata_str(fact, "target_path") == Some("/billing"))
        .expect("expected absolute child billing route");
    assert_eq!(metadata_str(billing, "component_name"), Some("BillingView"));
    assert_eq!(metadata_str(billing, "parent_route_path"), Some("/admin"));
    assert_eq!(
        metadata_str(billing, "effective_route_template"),
        Some("/billing")
    );
}

#[test]
fn vue_route_definitions_ignore_nested_redirect_and_meta_paths() {
    let source = r#"<script setup lang="ts">
import LoginView from '../views/LoginView.vue'

const routes = [
  {
    path: '/login',
    component: LoginView,
    redirect: { state: { path: '/redirect-target' } },
    meta: {
      path: '/meta-target',
      nested: [{ path: '/meta-nested-target' }],
    },
  },
]
</script>

<template>
  <RouterView />
</template>
"#;

    let results = extract(source);
    let definitions = facts_with_pattern(&results, "vue.route_definition.v1");
    assert_eq!(definitions.len(), 1);
    assert_eq!(
        definitions
            .iter()
            .filter_map(|fact| metadata_str(fact, "target_path"))
            .collect::<BTreeSet<_>>(),
        BTreeSet::from(["/login"])
    );
}

#[test]
fn vue_route_definitions_require_router_context() {
    let source = r#"<script setup lang="ts">
const unrelatedWidget = {
  path: '/not-a-route',
  component: Widget,
}
</script>

<template>
  <div />
</template>
"#;

    let results = extract(source);
    assert!(
        facts_with_pattern(&results, "vue.route_definition.v1").is_empty(),
        "plain objects with path properties are not Vue Router route definitions"
    );
}

#[test]
fn vue_script_comments_and_strings_do_not_emit_route_definitions() {
    let source = r#"<script setup lang="ts">
import { createRouter, createWebHistory } from 'vue-router'
import RealView from '../views/RealView.vue'

// const commentedRoutes = [{ path: '/commented', component: CommentedView }]
const docs = "const stringRoutes = [{ path: '/string-route', component: StringView }]";
const routes = [
  // { path: '/commented', component: CommentedView },
  {
    note: "path: '/string-route'",
  },
  {
    path: '/real',
    component: RealView,
  },
]

const router = createRouter({
  history: createWebHistory(),
  routes,
})
</script>

<template>
  <RouterView />
</template>
"#;

    let results = extract(source);
    let definitions = facts_with_pattern(&results, "vue.route_definition.v1");
    assert_eq!(
        definitions.len(),
        1,
        "only executable Vue Router route objects should emit"
    );
    assert_eq!(metadata_str(definitions[0], "target_path"), Some("/real"));
    let routes_start = source.find("const routes").unwrap() as u32;
    assert!(definitions[0].start_byte > routes_start);
}

#[test]
fn vue_route_definitions_follow_create_router_routes_identifier() {
    let source = r#"<script setup lang="ts">
import { createRouter, createWebHistory } from 'vue-router'
import DashboardView from '../views/DashboardView.vue'

const appRoutes = [
  {
    path: '/dashboard',
    name: 'dashboard',
    component: DashboardView,
  },
]

const router = createRouter({
  history: createWebHistory(),
  routes: appRoutes,
})
</script>

<template>
  <RouterView />
</template>
"#;

    let results = extract(source);
    let definitions = facts_with_pattern(&results, "vue.route_definition.v1");
    assert_eq!(definitions.len(), 1);
    assert_eq!(
        metadata_str(definitions[0], "target_path"),
        Some("/dashboard")
    );
    assert_eq!(
        metadata_str(definitions[0], "component_path"),
        Some("../views/DashboardView.vue")
    );
}

#[test]
fn vue_emits_style_css_facts_and_slot_shorthand_directive() {
    let source = r#"<template>
  <panel>
    <template #header>Title</template>
    <slot />
  </panel>
</template>

<script setup>
const ready = true;
</script>

<style scoped>
.active {
  color: #0f766e;
}
@media (min-width: 40rem) {
  .active { display: block; }
}
</style>
"#;
    let results = extract(source);

    let slot = facts_with_pattern(&results, "vue.template_directive.v1")
        .into_iter()
        .find(|fact| metadata_str(fact, "directive") == Some("v-slot"))
        .expect("expected v-slot shorthand fact");
    assert_eq!(metadata_str(slot, "attribute_name"), Some("#header"));
    assert_eq!(metadata_str(slot, "argument"), Some("header"));
    assert_eq!(metadata_bool(slot, "shorthand"), Some(true));

    let selectors = facts_with_pattern(&results, "css.selector_rule.v1");
    assert!(
        selectors
            .iter()
            .any(|fact| metadata_str(fact, "selector") == Some(".active")),
        "expected CSS selector facts from Vue <style>: {selectors:#?}"
    );
    assert!(
        selectors.iter().all(|fact| fact.language == "vue"),
        "embedded CSS facts should keep host language vue"
    );

    let media = facts_with_pattern(&results, "css.media_query.v1");
    assert!(
        media
            .iter()
            .any(|fact| metadata_str(fact, "query") == Some("(min-width: 40rem)")),
        "expected CSS media fact from Vue <style>: {media:#?}"
    );
}
