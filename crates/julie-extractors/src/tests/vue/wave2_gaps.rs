use crate::base::{ExtractionResults, RelationshipKind, StructuralFact, Symbol, SymbolKind};
use crate::extract_canonical;
use std::path::Path;

fn extract(path: &str, source: &str) -> ExtractionResults {
    extract_canonical(path, source, Path::new("/tmp/test")).expect("Vue extraction")
}

fn find<'a>(results: &'a ExtractionResults, name: &str, kind: SymbolKind) -> &'a Symbol {
    results
        .symbols
        .iter()
        .find(|symbol| symbol.name == name && symbol.kind == kind)
        .unwrap_or_else(|| {
            let names: Vec<(&str, &SymbolKind)> = results
                .symbols
                .iter()
                .map(|symbol| (symbol.name.as_str(), &symbol.kind))
                .collect();
            panic!("missing {name} {kind:?} in {names:?}")
        })
}

fn name_of(results: &ExtractionResults, id: Option<&String>) -> Option<String> {
    let id = id?;
    results
        .symbols
        .iter()
        .find(|symbol| &symbol.id == id)
        .map(|symbol| symbol.name.clone())
}

fn facts<'a>(results: &'a ExtractionResults, pattern: &str) -> Vec<&'a StructuralFact> {
    results
        .structural_facts
        .iter()
        .filter(|fact| fact.pattern_id == pattern)
        .collect()
}

fn meta_str<'a>(fact: &'a StructuralFact, key: &str) -> Option<&'a str> {
    fact.metadata.as_ref()?.get(key)?.as_str()
}

#[test]
fn options_api_extends_and_mixins_become_pending_extends() {
    let results = extract(
        "src/components/EditForm.vue",
        "<script>\nimport BaseForm from './BaseForm.vue'\nimport { formMixin } from './mixins/form'\nexport default {\n  extends: BaseForm,\n  mixins: [formMixin, loggingMixin],\n  props: ['title'],\n}\n</script>\n",
    );
    let component = find(&results, "EditForm", SymbolKind::Class);
    let rows: Vec<(&str, Option<&str>)> = results
        .structured_pending_relationships
        .iter()
        .filter(|pending| pending.pending.kind == RelationshipKind::Extends)
        .map(|pending| {
            assert_eq!(pending.pending.from_symbol_id, component.id);
            (
                pending.target.display_name.as_str(),
                pending.target.import_context.as_deref(),
            )
        })
        .collect();
    assert_eq!(
        rows,
        [
            ("BaseForm", Some("BaseForm")),
            ("formMixin", Some("formMixin")),
            ("loggingMixin", None),
        ]
    );
}

#[test]
fn template_attribute_symbols_do_not_duplicate_script_bindings() {
    let results = extract(
        "src/SearchBox.vue",
        "<template>\n  <input v-model=\"query\" ref=\"box\" />\n  <input v-model=\"(data as Dto).name\" />\n  <input v-model=\"draft\" />\n  <p>{{ query }}</p>\n</template>\n<script setup>\nimport { ref } from 'vue'\nconst query = ref('')\nconst box = ref(null)\n</script>\n",
    );
    let names: Vec<(&str, &SymbolKind)> = results
        .symbols
        .iter()
        .filter(|symbol| {
            symbol
                .metadata
                .as_ref()
                .and_then(|metadata| metadata.get("type"))
                .and_then(|value| value.as_str())
                .is_some_and(|kind| kind.starts_with("template-"))
        })
        .map(|symbol| (symbol.name.as_str(), &symbol.kind))
        .collect();
    assert_eq!(names, [("draft", &SymbolKind::Property)]);
}

#[test]
fn type_facts_hold_declared_types_not_kind_labels() {
    let results = extract(
        "src/Types.vue",
        "<template><input v-model=\"draft\" /></template>\n<script setup lang=\"ts\">\nimport { ref, computed } from 'vue'\nconst page = ref<number>(0)\nconst total = computed<string>(() => 'x')\nconst user = await $fetch('/api/me')\n</script>\n<script lang=\"ts\">\nexport default {\n  props: { size: { type: Number }, label: String, tags: [Array, String] },\n}\n</script>\n<style>\n.footer { color: red; }\n</style>\n",
    );
    let type_of = |name: &str, kind: SymbolKind| {
        let symbol = find(&results, name, kind);
        results
            .types
            .get(&symbol.id)
            .map(|info| info.resolved_type.clone())
    };
    assert_eq!(
        type_of("page", SymbolKind::Variable).as_deref(),
        Some("Ref<number>")
    );
    assert_eq!(
        type_of("total", SymbolKind::Variable).as_deref(),
        Some("ComputedRef<string>")
    );
    assert_eq!(
        type_of("size", SymbolKind::Property).as_deref(),
        Some("number")
    );
    assert_eq!(
        type_of("label", SymbolKind::Property).as_deref(),
        Some("string")
    );
    assert_eq!(
        type_of("tags", SymbolKind::Property).as_deref(),
        Some("Array | string")
    );
    for label in ["vue-sfc", "template-v-model", "css-rule", "variable", "ref"] {
        assert!(
            !results
                .types
                .values()
                .any(|info| info.resolved_type == label),
            "type rows must not hold the label {label}: {:#?}",
            results.types
        );
    }
}

#[test]
fn script_setup_macros_declare_props_emits_and_models() {
    let results = extract(
        "src/Dialog.vue",
        "<template><p>{{ modelValue }} {{ name }}</p></template>\n<script setup lang=\"ts\">\ninterface Props { name: string; count?: number }\nconst props = withDefaults(defineProps<Props>(), { count: 0 })\nconst emit = defineEmits<{ (e: 'select', id: number): void; (e: 'close'): void }>()\nconst visible = defineModel('visible', { type: Boolean })\ndefineModel()\n</script>\n<script lang=\"ts\">\nexport const other = 1\n</script>\n",
    );
    for name in ["name", "count"] {
        let member = find(&results, name, SymbolKind::Property);
        assert_eq!(
            member
                .metadata
                .as_ref()
                .and_then(|metadata| metadata.get("vueOption"))
                .and_then(|value| value.as_str()),
            Some("props"),
            "{name}"
        );
    }
    let emit = find(&results, "emit", SymbolKind::Variable);
    for name in ["select", "close"] {
        assert_eq!(
            find(&results, name, SymbolKind::Event).parent_id.as_ref(),
            Some(&emit.id)
        );
    }
    let visible = find(&results, "visible", SymbolKind::Variable);
    let visible_prop = results
        .symbols
        .iter()
        .find(|symbol| {
            symbol.name == "visible"
                && symbol.kind == SymbolKind::Property
                && symbol.parent_id.as_ref() == Some(&visible.id)
        })
        .expect("defineModel('visible') declares prop visible");
    assert_eq!(visible_prop.parent_id.as_ref(), Some(&visible.id));
    find(&results, "update:visible", SymbolKind::Event);
    find(&results, "modelValue", SymbolKind::Property);
    find(&results, "update:modelValue", SymbolKind::Event);
    assert!(
        results.relationships.iter().any(|relationship| {
            relationship.kind == RelationshipKind::References
                && name_of(&results, Some(&relationship.to_symbol_id)).as_deref() == Some("name")
        }),
        "{{ name }} must reference the name prop"
    );
}

#[test]
fn script_setup_object_and_array_macros_declare_members() {
    let results = extract(
        "src/Field.vue",
        "<script setup>\nconst props = defineProps({\n  modelValue: { type: String, required: true },\n  size: { type: Number, default: 1 },\n})\nconst emit = defineEmits(['update:modelValue', 'close'])\n</script>\n",
    );
    let props = find(&results, "props", SymbolKind::Variable);
    let model = find(&results, "modelValue", SymbolKind::Property);
    assert_eq!(model.parent_id.as_ref(), Some(&props.id));
    assert_eq!(
        results
            .types
            .get(&model.id)
            .map(|info| info.resolved_type.as_str()),
        Some("string")
    );
    find(&results, "size", SymbolKind::Property);
    find(&results, "update:modelValue", SymbolKind::Event);
    find(&results, "close", SymbolKind::Event);
}

#[test]
fn template_component_tags_carry_the_import_binding() {
    let results = extract(
        "src/UserGrid.vue",
        "<template>\n  <user-card v-for=\"p in people\" :key=\"p\" />\n  <UserCard name=\"solo\" />\n  <el-button />\n</template>\n<script setup>\nimport UserCard from './UserCard.vue'\nimport { ElButton } from 'element-plus'\nconst people = []\n</script>\n",
    );
    let rows: Vec<(&str, Option<&str>, u32)> = results
        .structured_pending_relationships
        .iter()
        .filter(|pending| pending.pending.kind == RelationshipKind::References)
        .map(|pending| {
            (
                pending.target.display_name.as_str(),
                pending.target.import_context.as_deref(),
                pending.pending.line_number,
            )
        })
        .collect();
    assert_eq!(
        rows,
        [
            ("UserCard", Some("UserCard"), 2),
            ("UserCard", Some("UserCard"), 3),
            ("ElButton", None, 4),
        ]
    );
}

#[test]
fn script_navigation_and_fetch_calls_become_facts() {
    let results = extract(
        "src/Nav.vue",
        "<script setup lang=\"ts\">\nimport { useRouter } from 'vue-router'\nconst router = useRouter()\nconst { data: posts } = await useFetch('/api/posts')\nconst user = await $fetch('/api/me', { method: 'POST' })\nfunction goHome() { router.push('/home') }\nasync function logout() { await navigateTo('/login') }\n</script>\n",
    );
    let route = facts(&results, "vue.route_reference.v1");
    assert_eq!(route.len(), 1);
    assert_eq!(meta_str(route[0], "target_path"), Some("/home"));
    assert_eq!(
        meta_str(route[0], "source_kind"),
        Some("router_navigation_call")
    );
    assert_eq!(
        name_of(&results, route[0].containing_symbol_id.as_ref()).as_deref(),
        Some("goHome")
    );
    let nuxt = facts(&results, "nuxt.route_reference.v1");
    assert_eq!(nuxt.len(), 1);
    assert_eq!(meta_str(nuxt[0], "target_path"), Some("/login"));
    assert_eq!(meta_str(nuxt[0], "source_kind"), Some("navigate_to"));
    let requests: Vec<(Option<&str>, Option<&str>, Option<&str>)> =
        facts(&results, "http.client_request.v1")
            .into_iter()
            .map(|fact| {
                (
                    meta_str(fact, "client"),
                    meta_str(fact, "target_path"),
                    meta_str(fact, "verb"),
                )
            })
            .collect();
    assert_eq!(
        requests,
        [
            (Some("nuxt"), Some("/api/posts"), Some("GET")),
            (Some("ofetch"), Some("/api/me"), Some("POST")),
        ]
    );
}

#[test]
fn script_comment_markers_become_marker_facts() {
    let results = extract(
        "src/Marker.vue",
        "<script setup>\n// FIXME: remove debug color\nconst color = \"red\"\n</script>\n",
    );
    let markers = facts(&results, "code.marker.v1");
    assert_eq!(markers.len(), 1);
    assert_eq!(meta_str(markers[0], "marker"), Some("FIXME"));
    assert_eq!(markers[0].language, "vue");
    assert_eq!(markers[0].start_line, 2);
}

#[test]
fn component_and_template_symbols_use_zero_based_columns_and_real_docs() {
    let source = "<template>\n  <input v-model=\"draft\" />\n</template>\n<script setup>\nimport { ref } from 'vue'\n</script>\n";
    let results = extract("src/Docs.vue", source);
    let component = find(&results, "Docs", SymbolKind::Class);
    assert_eq!((component.start_line, component.start_column), (1, 0));
    assert_eq!(component.doc_comment, None);
    let body = component.body_span.expect("component body span");
    assert_eq!(
        &source[body.start_byte as usize..body.end_byte as usize],
        source.trim_end()
    );
    let draft = find(&results, "draft", SymbolKind::Property);
    assert_eq!(
        &source[draft.start_byte as usize..draft.end_byte as usize],
        "draft"
    );
    assert_eq!((draft.start_line, draft.start_column), (2, 18));
    assert_eq!(draft.body_span, None);

    let documented = extract(
        "src/Documented.vue",
        "<!-- Shows the order list. -->\n<template><p /></template>\n",
    );
    assert_eq!(
        find(&documented, "Documented", SymbolKind::Class)
            .doc_comment
            .as_deref(),
        Some("<!-- Shows the order list. -->")
    );
}

#[test]
fn preprocessor_style_blocks_emit_no_css_rows() {
    let results = extract(
        "src/Card.vue",
        "<style lang=\"scss\" scoped>\n$radius: 4px;\n@use '@/styles/mixins' as m;\n.card {\n  border-radius: $radius;\n  &__title { font-weight: 600; }\n}\n</style>\n<style>\n.footer { color: red; }\n</style>\n",
    );
    let css: Vec<&str> = results
        .symbols
        .iter()
        .filter(|symbol| symbol.kind != SymbolKind::Class)
        .map(|symbol| symbol.name.as_str())
        .collect();
    assert_eq!(css, [".footer"]);
    let selectors: Vec<Option<&str>> = facts(&results, "css.selector_rule.v1")
        .into_iter()
        .map(|fact| meta_str(fact, "selector"))
        .collect();
    assert_eq!(selectors, [Some(".footer")]);
}

#[test]
fn options_api_walks_only_the_component_options_object() {
    let results = extract(
        "src/Upload.vue",
        "<template><p>{{ count }}</p></template>\n<script lang=\"ts\">\nimport { defineComponent, ref } from 'vue'\nimport axios from 'axios'\nexport default defineComponent({\n  props: ['title', 'items'],\n  data: () => ({ open: false }),\n  setup(props, { emit }) {\n    const count = ref(props.start)\n    function reset(): void { count.value = 0 }\n    return { count, reset }\n  },\n  created() { this.init() },\n  mounted() {},\n  watch: { userId(newId) { this.load(newId) } },\n  methods: {\n    upload(payload) { return axios.request({ url: '/upload', data: payload, props: { retry: 1 } }) },\n  },\n})\n</script>\n",
    );
    for (name, kind) in [
        ("title", SymbolKind::Property),
        ("items", SymbolKind::Property),
        ("open", SymbolKind::Property),
        ("setup", SymbolKind::Method),
        ("count", SymbolKind::Variable),
        ("reset", SymbolKind::Function),
        ("created", SymbolKind::Method),
        ("mounted", SymbolKind::Method),
        ("userId", SymbolKind::Method),
        ("upload", SymbolKind::Method),
    ] {
        find(&results, name, kind);
    }
    assert!(
        !results.symbols.iter().any(|symbol| symbol.name == "retry"),
        "request config objects are not component options"
    );
    let count = find(&results, "count", SymbolKind::Variable);
    assert!(
        results.relationships.iter().any(|relationship| {
            relationship.kind == RelationshipKind::References
                && relationship.to_symbol_id == count.id
        }),
        "{{ count }} must reference the setup binding"
    );
}
