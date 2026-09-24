use crate::base::{SymbolKind, TypeInfo};
use crate::razor::RazorExtractor;
use crate::tests::helpers::init_parser;
use std::path::PathBuf;

fn local_fact(source: &str, local: &str) -> Option<TypeInfo> {
    let tree = init_parser(source, "razor");
    let mut extractor = RazorExtractor::new(
        "razor".to_string(),
        "Counter.razor".to_string(),
        source.to_string(),
        &PathBuf::from("/tmp/test"),
    );
    let symbols = extractor.extract_symbols(&tree);
    let symbol = symbols
        .iter()
        .find(|s| s.name == local && s.kind == SymbolKind::Variable)
        .unwrap_or_else(|| panic!("missing local {local}"));
    extractor.base.type_info.get(&symbol.id).cloned()
}

fn inferred(source: &str, local: &str) -> Option<String> {
    let fact = local_fact(source, local)?;
    assert!(fact.is_inferred, "{local} fact should be inferred");
    Some(fact.resolved_type)
}

#[test]
fn code_block_method_call_infers_its_return_type() {
    let source = r#"
@code {
    Widget Load() => null;
    void Run() { var widget = Load(); }
}
"#;
    assert_eq!(inferred(source, "widget").as_deref(), Some("Widget"));
}

#[test]
fn code_block_this_call_infers_its_return_type() {
    let source = r#"
@code {
    Widget Load() => null;
    void Run() { var widget = this.Load(); }
}
"#;
    assert_eq!(inferred(source, "widget").as_deref(), Some("Widget"));
}

#[test]
fn code_block_await_unwraps_task_of_t() {
    let source = r#"
@code {
    async Task<Widget> LoadAsync() => null;
    protected override async Task OnInitializedAsync() { var widget = await LoadAsync(); }
}
"#;
    assert_eq!(inferred(source, "widget").as_deref(), Some("Widget"));
}

#[test]
fn markup_code_block_sees_code_block_methods() {
    let source = r#"
@{ var widget = Load(); }
<p>@widget</p>
@code {
    Widget Load() => null;
}
"#;
    assert_eq!(inferred(source, "widget").as_deref(), Some("Widget"));
}

#[test]
fn typeparam_return_records_nothing() {
    let source = r#"
@typeparam TItem
@code {
    TItem Current() => default;
    void Run() { var item = Current(); }
}
"#;
    assert_eq!(inferred(source, "item"), None);
}

#[test]
fn call_to_a_method_outside_the_file_records_nothing() {
    let source = r#"
@code {
    void Run() { var widget = Load(); }
}
"#;
    assert_eq!(inferred(source, "widget"), None);
}

#[test]
fn written_type_wins_over_the_call_type() {
    let source = r#"
@code {
    Widget Load() => null;
    void Run() { IWidget widget = Load(); }
}
"#;
    let fact = local_fact(source, "widget").expect("declared fact");
    assert_eq!(fact.resolved_type, "IWidget");
    assert!(!fact.is_inferred);
}
