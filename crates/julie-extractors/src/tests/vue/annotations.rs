use crate::base::AnnotationMarker;
use crate::base::SymbolKind;
use crate::vue::VueExtractor;
use std::path::PathBuf;

fn is_vue_component(symbol: &crate::base::Symbol) -> bool {
    symbol.kind == SymbolKind::Class
        && symbol.metadata.as_ref().is_some_and(|metadata| {
            metadata.get("type").and_then(|value| value.as_str()) == Some("vue-sfc")
        })
}

fn create_extractor(file_path: &str, code: &str) -> VueExtractor {
    VueExtractor::new(
        "vue".to_string(),
        file_path.to_string(),
        code.to_string(),
        &PathBuf::from("/tmp/test"),
    )
}

fn annotation<'a>(symbol: &'a crate::base::Symbol, key: &str) -> &'a AnnotationMarker {
    symbol
        .annotations
        .iter()
        .find(|marker| marker.annotation_key == key)
        .unwrap_or_else(|| {
            panic!(
                "symbol `{}` missing annotation `{key}`; has: {:?}",
                symbol.name, symbol.annotations
            )
        })
}

#[test]
fn vue_component_receives_script_setup_macro_annotations() {
    let vue_code = r#"<template>
  <div>{{ title }}</div>
</template>

<script setup lang="ts">
defineOptions({ name: "WorkerPanel" });
const props = defineProps<{ title: string }>();
const emit = defineEmits<{ update: [] }>();

const title = ref("Worker");
</script>"#;

    let mut extractor = create_extractor("worker-panel.vue", vue_code);
    let symbols = extractor.extract_symbols(None);

    let component = symbols
        .iter()
        .find(|symbol| is_vue_component(symbol))
        .expect("component symbol should exist");

    assert_eq!(
        annotation(component, "defineoptions").annotation,
        "defineOptions"
    );
    assert_eq!(
        annotation(component, "defineprops").annotation,
        "defineProps"
    );
    assert_eq!(
        annotation(component, "defineemits").annotation,
        "defineEmits"
    );
}

#[test]
fn vue_define_expose_annotates_referenced_symbols_not_only_macro_call() {
    let vue_code = r#"<script setup lang="ts">
function format(value: string): string {
  return value.trim();
}

const evaluate = (count: number) => count + 1;

defineExpose({ format, evaluate });
</script>"#;

    let mut extractor = create_extractor("expose-targets.vue", vue_code);
    let symbols = extractor.extract_symbols(None);

    let format = symbols
        .iter()
        .find(|symbol| symbol.name == "format" && symbol.kind == SymbolKind::Function)
        .expect("format function should exist");
    assert_eq!(
        annotation(format, "defineexpose").annotation,
        "defineExpose"
    );

    let evaluate = symbols
        .iter()
        .find(|symbol| symbol.name == "evaluate" && symbol.kind == SymbolKind::Function)
        .expect("evaluate function should exist");
    assert_eq!(
        annotation(evaluate, "defineexpose").annotation,
        "defineExpose"
    );

    let macro_symbol = symbols
        .iter()
        .find(|symbol| symbol.name == "defineExpose")
        .expect("defineExpose macro call remains a symbol");
    assert!(
        macro_symbol.annotations.is_empty(),
        "macro call itself should not carry defineExpose as a self-annotation"
    );
}

#[test]
fn vue_component_macro_annotations_skip_css_class_symbols() {
    let vue_code = r#"<template>
  <section class="worker">{{ title }}</section>
</template>

<script setup lang="ts">
defineOptions({ name: "WorkerPanel" });
const props = defineProps<{ title: string }>();
const emit = defineEmits<{ update: [] }>();

const title = ref("Worker");
</script>

<style scoped>
.worker {
  color: #0f766e;
}
</style>"#;

    let mut extractor = create_extractor("styled-worker-panel.vue", vue_code);
    let symbols = extractor.extract_symbols(None);

    let component = symbols
        .iter()
        .find(|symbol| is_vue_component(symbol))
        .expect("Vue SFC component symbol should exist");
    assert_eq!(
        annotation(component, "defineoptions").annotation,
        "defineOptions"
    );
    assert_eq!(
        annotation(component, "defineprops").annotation,
        "defineProps"
    );
    assert_eq!(
        annotation(component, "defineemits").annotation,
        "defineEmits"
    );

    let css_class = symbols
        .iter()
        .find(|symbol| symbol.name == ".worker")
        .expect("CSS class symbol should exist");
    assert!(
        css_class.annotations.is_empty(),
        "style class symbol must not receive script-setup macro annotations"
    );
}

#[test]
fn vue_define_expose_alias_annotates_referenced_symbol_not_public_key() {
    let vue_code = r#"<script setup lang="ts">
function format(value: string): string {
  return value.trim();
}

defineExpose({ publicFormat: format });
</script>"#;

    let mut extractor = create_extractor("expose-alias.vue", vue_code);
    let symbols = extractor.extract_symbols(None);

    let format = symbols
        .iter()
        .find(|symbol| symbol.name == "format" && symbol.kind == SymbolKind::Function)
        .expect("format function should exist");
    assert_eq!(
        annotation(format, "defineexpose").annotation,
        "defineExpose"
    );

    assert!(
        !symbols.iter().any(|symbol| symbol.name == "publicFormat"),
        "publicFormat is an expose alias key, not a source symbol"
    );
}
