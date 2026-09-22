use crate::base::{ExtractionResults, IdentifierKind, RelationshipKind, Symbol, SymbolKind};
use crate::extract_canonical;
use std::path::Path;

fn extract(path: &str, source: &str) -> ExtractionResults {
    extract_canonical(path, source, Path::new("/tmp/test")).expect("Vue extraction")
}

fn symbol<'a>(results: &'a ExtractionResults, name: &str) -> &'a Symbol {
    results
        .symbols
        .iter()
        .find(|symbol| symbol.name == name)
        .unwrap_or_else(|| panic!("missing symbol {name} in {:?}", names(results)))
}

fn symbol_of_kind<'a>(results: &'a ExtractionResults, name: &str, kind: SymbolKind) -> &'a Symbol {
    results
        .symbols
        .iter()
        .find(|symbol| symbol.name == name && symbol.kind == kind)
        .unwrap_or_else(|| panic!("missing {name} {kind:?} in {:?}", names(results)))
}

fn names(results: &ExtractionResults) -> Vec<(String, SymbolKind)> {
    results
        .symbols
        .iter()
        .map(|symbol| (symbol.name.clone(), symbol.kind.clone()))
        .collect()
}

fn name_of(results: &ExtractionResults, id: &str) -> String {
    results
        .symbols
        .iter()
        .find(|symbol| symbol.id == id)
        .map(|symbol| symbol.name.clone())
        .unwrap_or_default()
}

fn edges(results: &ExtractionResults) -> Vec<(String, RelationshipKind, String)> {
    results
        .relationships
        .iter()
        .map(|relationship| {
            (
                name_of(results, &relationship.from_symbol_id),
                relationship.kind.clone(),
                name_of(results, &relationship.to_symbol_id),
            )
        })
        .collect()
}

fn pending(results: &ExtractionResults) -> Vec<(String, RelationshipKind, String)> {
    results
        .structured_pending_relationships
        .iter()
        .map(|row| {
            (
                name_of(results, &row.pending.from_symbol_id),
                row.pending.kind.clone(),
                row.target.display_name.clone(),
            )
        })
        .collect()
}

fn has(
    rows: &[(String, RelationshipKind, String)],
    from: &str,
    kind: RelationshipKind,
    to: &str,
) -> bool {
    rows.iter()
        .any(|row| row.0 == from && row.1 == kind && row.2 == to)
}

#[test]
fn nested_templates_stay_inside_one_template_section() {
    let source = r#"<template>
  <div class="list">
    <template v-if="loading">
      <LoadingSpinner />
    </template>
    <ul v-else>
      <li v-for="item in items" @click.stop="select(item)">{{ formatLabel(item) }}</li>
    </ul>
    <BaseButton :disabled="!canSave" @click="save()">Save</BaseButton>
  </div>
</template>
<script setup>
import { formatLabel } from '@/utils/format'
const items = []; const loading = false; const canSave = true
function select(item) {}
function save() {}
</script>
"#;
    let results = extract("Tmpl.vue", source);
    let component = symbol(&results, "Tmpl");
    let sections = component
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get("sections"))
        .and_then(|value| value.as_str());
    assert_eq!(sections, Some("template,script"));
    let pending = pending(&results);
    assert!(
        has(&pending, "Tmpl", RelationshipKind::References, "BaseButton"),
        "{pending:?}"
    );
    assert!(
        has(
            &pending,
            "Tmpl",
            RelationshipKind::References,
            "LoadingSpinner"
        ),
        "{pending:?}"
    );
    let edges = edges(&results);
    assert!(
        has(&edges, "Tmpl", RelationshipKind::Calls, "save"),
        "{edges:?}"
    );
    assert!(
        has(&edges, "Tmpl", RelationshipKind::Calls, "select"),
        "{edges:?}"
    );
}

#[test]
fn one_line_and_multi_line_section_tags_are_recognised() {
    let one_line = extract(
        "OneLine.vue",
        "<template><button @click=\"save\">{{ label }}</button></template>\n<script setup>const label = 'x'; function save() {}</script>\n",
    );
    symbol(&one_line, "label");
    symbol(&one_line, "save");
    assert!(has(
        &edges(&one_line),
        "OneLine",
        RelationshipKind::Calls,
        "save"
    ));
    assert!(has(
        &edges(&one_line),
        "OneLine",
        RelationshipKind::References,
        "label"
    ));

    let multi_line = extract(
        "MultiLine.vue",
        "<template><div /></template>\n<script\n  setup\n  lang=\"ts\"\n>\nconst count: number = 1\n</script>\n",
    );
    symbol(&multi_line, "count");
}

#[test]
fn component_name_comes_only_from_the_options_object_or_the_file_stem() {
    let router_push = extract(
        "UserMenu.vue",
        "<script setup>\nimport { useRouter } from 'vue-router'\nconst router = useRouter()\nfunction openProfile() {\n  router.push({ name: 'profile', params: { id: 1 } })\n}\n</script>\n",
    );
    let component = symbol(&router_push, "UserMenu");
    assert_eq!(component.signature.as_deref(), Some("<UserMenu />"));
    assert!(
        router_push
            .symbols
            .iter()
            .all(|symbol| symbol.name != "profile")
    );

    let declared = extract(
        "declared.vue",
        "<script>\nexport default defineComponent({ name: 'DeclaredName', data() { return { name: 'other' } } })\n</script>\n",
    );
    symbol(&declared, "DeclaredName");
}

#[test]
fn plain_script_blocks_publish_every_module_declaration() {
    let source = r#"<script lang="ts">
import { defineComponent, ref } from 'vue'
/** Shape of a row. */
export interface Row { id: number; name: string }
export type Mode = 'view' | 'edit'
export enum Status { Active, Inactive }
/** Formats a row label. */
export function formatRow(row: Row): string {
  if (row.id > 0) { return row.name }
  return ''
}
export class RowStore { rows: Row[] = []; add(row: Row): void { this.rows.push(row) } }
export const MAX_ROWS = 50
</script>
<script setup lang="ts">
const store = new RowStore()
</script>
"#;
    let results = extract("PlainTs.vue", source);
    for (name, kind) in [
        ("Row", SymbolKind::Interface),
        ("Mode", SymbolKind::Type),
        ("Status", SymbolKind::Enum),
        ("formatRow", SymbolKind::Function),
        ("RowStore", SymbolKind::Class),
        ("add", SymbolKind::Method),
        ("MAX_ROWS", SymbolKind::Variable),
        ("defineComponent", SymbolKind::Import),
        ("ref", SymbolKind::Import),
        ("store", SymbolKind::Variable),
    ] {
        assert!(
            results
                .symbols
                .iter()
                .any(|symbol| symbol.name == name && symbol.kind == kind),
            "missing {name} {kind:?} in {:?}",
            names(&results)
        );
    }
    assert!(
        results
            .symbols
            .iter()
            .all(|symbol| symbol.name != "if" && symbol.name != "for")
    );
    let format_row = symbol_of_kind(&results, "formatRow", SymbolKind::Function);
    assert!(
        format_row
            .doc_comment
            .as_deref()
            .is_some_and(|doc| doc.contains("Formats a row"))
    );
    assert!(format_row.body_span.is_some());
    let add = symbol(&results, "add");
    assert_eq!(
        add.parent_id.as_deref(),
        Some(
            symbol_of_kind(&results, "RowStore", SymbolKind::Class)
                .id
                .as_str()
        )
    );
}

#[test]
fn script_setup_publishes_types_enums_classes_and_namespace_imports() {
    let source = r#"<script setup lang="ts">
import * as api from '@/api'
import { storeToRefs } from 'pinia'
interface Props { pageSize?: number; title: string }
type Filter = 'all' | 'active'
enum Tab { List, Grid }
const { users, loading } = storeToRefs(store)
let timer: number | undefined
class Poller { start() { next() } }
const props = defineProps<Props>()
</script>
"#;
    let results = extract("SetupTs.vue", source);
    for (name, kind) in [
        ("api", SymbolKind::Import),
        ("Props", SymbolKind::Interface),
        ("pageSize", SymbolKind::Property),
        ("title", SymbolKind::Property),
        ("Filter", SymbolKind::Type),
        ("Tab", SymbolKind::Enum),
        ("timer", SymbolKind::Variable),
        ("users", SymbolKind::Variable),
        ("loading", SymbolKind::Variable),
        ("Poller", SymbolKind::Class),
        ("start", SymbolKind::Method),
    ] {
        assert_eq!(symbol(&results, name).kind, kind, "{name}");
    }
    assert_eq!(
        symbol(&results, "start").parent_id.as_deref(),
        Some(symbol(&results, "Poller").id.as_str())
    );
    assert!(
        results
            .symbols
            .iter()
            .all(|symbol| !symbol.name.contains('{'))
    );
    let props = symbol(&results, "props");
    assert_eq!(
        props
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.get("type"))
            .and_then(|value| value.as_str()),
        Some("props")
    );
}

#[test]
fn script_calls_come_from_the_enclosing_function() {
    let source = r#"<script setup>
import { foo } from './other';
function local_helper() { return 42; }
function entry() {
    foo();
    return local_helper();
}
const answer = local_helper();
</script>
"#;
    let results = extract("Cross.vue", source);
    let edges = edges(&results);
    assert!(
        has(&edges, "entry", RelationshipKind::Calls, "local_helper"),
        "{edges:?}"
    );
    assert!(
        has(&edges, "answer", RelationshipKind::Calls, "local_helper"),
        "{edges:?}"
    );
    assert!(
        !has(&edges, "Cross", RelationshipKind::Calls, "foo"),
        "{edges:?}"
    );
    let pending = pending(&results);
    assert!(
        has(&pending, "entry", RelationshipKind::Calls, "foo"),
        "{pending:?}"
    );
}

#[test]
fn template_directives_give_identifiers_and_edges_to_script_bindings() {
    let source = r#"<template>
  <li v-for="item in items" :key="item.id" @click.stop="select(item)">{{ formatLabel(item) }}</li>
  <user-card :user="current" v-on:refresh="reload" />
  <BaseButton :disabled="!canSave" @click="save()">Save</BaseButton>
  <LoadingSpinner v-if="loading" />
</template>
<script setup>
import { formatLabel } from '@/utils/format'
const items = []; const loading = false; const current = null; const canSave = true
function select(item) {}
function reload() {}
function save() {}
</script>
"#;
    let results = extract("Tmpl2.vue", source);
    let component = symbol(&results, "Tmpl2");
    let template_identifiers: Vec<(&str, IdentifierKind)> = results
        .identifiers
        .iter()
        .filter(|identifier| {
            identifier.containing_symbol_id.as_deref() == Some(component.id.as_str())
                && identifier.start_line <= 6
        })
        .map(|identifier| (identifier.name.as_str(), identifier.kind.clone()))
        .collect();
    for expected in [
        ("select", IdentifierKind::Call),
        ("save", IdentifierKind::Call),
        ("formatLabel", IdentifierKind::Call),
        ("reload", IdentifierKind::Call),
        ("items", IdentifierKind::VariableRef),
        ("current", IdentifierKind::VariableRef),
        ("canSave", IdentifierKind::VariableRef),
        ("loading", IdentifierKind::VariableRef),
    ] {
        assert!(
            template_identifiers.contains(&expected),
            "missing {expected:?} in {template_identifiers:?}"
        );
    }
    let edges = edges(&results);
    for (kind, target) in [
        (RelationshipKind::Calls, "select"),
        (RelationshipKind::Calls, "reload"),
        (RelationshipKind::Calls, "save"),
        (RelationshipKind::References, "items"),
        (RelationshipKind::References, "current"),
        (RelationshipKind::References, "canSave"),
        (RelationshipKind::References, "loading"),
    ] {
        assert!(
            has(&edges, "Tmpl2", kind.clone(), target),
            "missing {kind:?} {target} in {edges:?}"
        );
    }
    assert!(has(
        &pending(&results),
        "Tmpl2",
        RelationshipKind::Calls,
        "formatLabel"
    ));
}

#[test]
fn options_api_members_span_their_whole_definition() {
    let source = r#"<script>
export default {
  props: { userId: Number },
  data() { return { first: '', last: '' } },
  computed: { fullName() { return `${this.first} ${this.last}` } },
  methods: {
    async load(id) {
      const user = await fetchUser(id)
      if (user.last) { this.last = user.last }
    },
    save() { this.load(this.userId) },
  },
}
</script>
"#;
    for path in ["Profile.vue", "ProfileTs.vue"] {
        let source = if path == "ProfileTs.vue" {
            source.replace("<script>", "<script lang=\"ts\">")
        } else {
            source.to_string()
        };
        let results = extract(path, &source);
        let load = symbol(&results, "load");
        assert_eq!(load.kind, SymbolKind::Method, "{path}");
        assert!(
            source[load.start_byte as usize..load.end_byte as usize]
                .starts_with("async load(id) {"),
            "{path}"
        );
        assert!(
            load.body_span.is_some() && load.body_hash.is_some(),
            "{path}"
        );
        assert_eq!(
            load.parent_id.as_deref(),
            Some(symbol(&results, "methods").id.as_str()),
            "{path}"
        );
        for member in ["userId", "first", "last", "fullName", "save"] {
            let member = symbol(&results, member);
            assert!(member.parent_id.is_some(), "{path} {}", member.name);
        }
        assert!(
            results.identifiers.iter().any(|identifier| {
                identifier.name == "fetchUser"
                    && identifier.containing_symbol_id.as_deref() == Some(load.id.as_str())
            }),
            "{path}"
        );
        let load_metric = results
            .complexity_metrics
            .iter()
            .find(|metric| metric.symbol_id.as_deref() == Some(load.id.as_str()))
            .expect("load complexity");
        assert_eq!(load_metric.decision_count, 1, "{path}");
    }
}

#[test]
fn tsx_scripts_use_the_tsx_grammar_and_report_script_parse_errors() {
    let tsx = extract(
        "View.vue",
        "<script setup lang=\"tsx\">\ninterface P { a: number }\nconst View = (p: P) => <div>{p.a}</div>\n</script>\n",
    );
    assert_eq!(symbol(&tsx, "P").kind, SymbolKind::Interface);
    assert_eq!(symbol(&tsx, "View").kind, SymbolKind::Function);

    let broken = extract(
        "Broken.vue",
        "<script setup>\nfunction ok() {}\nconst x = (\nfunction later() {}\n</script>\n",
    );
    assert!(!broken.parse_diagnostics.is_empty());
    assert!(
        broken
            .parse_diagnostics
            .iter()
            .all(|diagnostic| diagnostic.start_line >= 2)
    );
}
