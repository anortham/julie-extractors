use crate::base::Symbol;
use crate::vue::VueExtractor;

fn symbols(source: &str) -> Vec<Symbol> {
    let mut extractor = VueExtractor::new(
        "vue".to_string(),
        "Component.vue".to_string(),
        source.to_string(),
        std::path::Path::new("."),
    );
    extractor.extract_symbols(None)
}

fn role(symbol: &Symbol, key: &str) -> bool {
    symbol
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get(key))
        .and_then(serde_json::Value::as_bool)
        == Some(true)
}

fn symbol<'a>(symbols: &'a [Symbol], name: &str) -> &'a Symbol {
    symbols
        .iter()
        .find(|symbol| symbol.name == name)
        .unwrap_or_else(|| panic!("missing symbol `{name}` in {symbols:#?}"))
}

#[test]
fn script_setup_routes_js_test_calls_with_host_spans_and_parents() {
    let source = r#"<template><output>ready</output></template>
<script setup lang="ts">
describe("vue setup suite", () => {
  beforeEach(() => {});
  test("renders setup", () => {});
});
function testNamedButOrdinary(): void {}
const ordinary = { test(_name: string, callback: () => void) { callback(); } };
ordinary.test("ordinary member call", () => {});
</script>"#;

    let symbols = symbols(source);
    let container = symbol(&symbols, "vue setup suite");
    let lifecycle = symbol(&symbols, "beforeEach");
    let test_case = symbol(&symbols, "renders setup");
    let ordinary_declaration = symbol(&symbols, "testNamedButOrdinary");

    assert!(role(container, "test_container"));
    assert!(!role(container, "is_test"));
    assert!(role(lifecycle, "is_test"));
    assert!(role(lifecycle, "test_lifecycle"));
    assert!(role(test_case, "is_test"));
    assert!(!role(test_case, "test_lifecycle"));
    assert_eq!(lifecycle.parent_id.as_deref(), Some(container.id.as_str()));
    assert_eq!(test_case.parent_id.as_deref(), Some(container.id.as_str()));

    assert_eq!(container.start_line, 3);
    assert_eq!(lifecycle.start_line, 4);
    assert_eq!(test_case.start_line, 5);
    assert_eq!(
        &source[container.start_byte as usize..container.end_byte as usize],
        "describe(\"vue setup suite\", () => {\n  beforeEach(() => {});\n  test(\"renders setup\", () => {});\n})"
    );

    assert!(!role(ordinary_declaration, "is_test"));
    assert!(
        !symbols
            .iter()
            .any(|symbol| symbol.name == "ordinary member call")
    );
}

#[test]
fn regular_script_routes_js_test_calls_without_member_call_false_positives() {
    let source = r#"<template><output>ready</output></template>
<script>
suite("vue options suite", () => {
  afterAll(() => {});
  it("renders options", () => {});
});
const ordinary = { it(_name, callback) { callback(); } };
ordinary.it("ordinary member call", () => {});
</script>"#;

    let symbols = symbols(source);
    let container = symbol(&symbols, "vue options suite");
    let lifecycle = symbol(&symbols, "afterAll");
    let test_case = symbol(&symbols, "renders options");

    assert!(role(container, "test_container"));
    assert!(role(lifecycle, "is_test"));
    assert!(role(lifecycle, "test_lifecycle"));
    assert!(role(test_case, "is_test"));
    assert_eq!(lifecycle.parent_id.as_deref(), Some(container.id.as_str()));
    assert_eq!(test_case.parent_id.as_deref(), Some(container.id.as_str()));
    assert_eq!(container.start_line, 3);
    assert_eq!(lifecycle.start_line, 4);
    assert_eq!(test_case.start_line, 5);
    assert!(
        !symbols
            .iter()
            .any(|symbol| symbol.name == "ordinary member call")
    );
}
