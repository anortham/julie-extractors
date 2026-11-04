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

fn annotation_keys(symbol: &crate::base::Symbol) -> Vec<&str> {
    let mut keys: Vec<_> = symbol
        .annotations
        .iter()
        .map(|annotation| annotation.annotation_key.as_str())
        .collect();
    keys.sort_unstable();
    keys
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

#[test]
fn vue_embedded_annotations_follow_extraction_levels_and_keep_host_offsets() {
    let source = r#"<template>
  <div>{{ title }}</div>
</template>

<script setup lang="ts">
defineOptions({ name: "WorkerPanel" });
const props = defineProps<{ title: string }>();
const emit = defineEmits<{ update: [] }>();
function format(value: string): string {
  return value.trim();
}
defineExpose({ format });
</script>"#;
    let format_source = "function format(value: string): string {\n  return value.trim();\n}";
    let format_start = source.find(format_source).unwrap();

    for (level, has_annotations) in [
        (crate::ExtractionLevel::Symbols, false),
        (crate::ExtractionLevel::Facts, false),
        (crate::ExtractionLevel::References, true),
        (crate::ExtractionLevel::Full, true),
    ] {
        let results = crate::pipeline::extract_canonical_at(
            "src/Worker.vue",
            source,
            std::path::Path::new("/tmp/test"),
            level,
        )
        .expect("canonical Vue extraction should succeed");
        assert!(
            results.parse_diagnostics.is_empty(),
            "{:#?}",
            results.parse_diagnostics
        );

        let component = results
            .symbols
            .iter()
            .find(|symbol| is_vue_component(symbol))
            .expect("Vue component symbol should exist");
        let format = results
            .symbols
            .iter()
            .find(|symbol| symbol.name == "format" && symbol.kind == SymbolKind::Function)
            .expect("format function should exist");
        let component_keys = if has_annotations {
            vec!["defineemits", "defineoptions", "defineprops"]
        } else {
            Vec::new()
        };
        let format_keys = if has_annotations {
            vec!["defineexpose"]
        } else {
            Vec::new()
        };

        assert_eq!(annotation_keys(component), component_keys, "{level:?}");
        assert_eq!(annotation_keys(format), format_keys, "{level:?}");
        assert_eq!(format.start_byte as usize, format_start);
        assert_eq!(
            source.get(format.start_byte as usize..format.end_byte as usize),
            Some(format_source)
        );
    }
}
