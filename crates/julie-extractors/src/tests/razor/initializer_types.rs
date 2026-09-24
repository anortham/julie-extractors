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

#[test]
fn code_block_local_named_like_the_method_records_nothing() {
    let source = r#"
@code {
    int Load() => 1;
    void Shadowed() { Func<string> Load = () => ""; var shadowed = Load(); }
    void Control() { var control = Load(); }
}
"#;
    assert_eq!(inferred(source, "shadowed"), None);
    assert_eq!(inferred(source, "control").as_deref(), Some("int"));
}

#[test]
fn nested_class_property_named_like_the_method_records_nothing() {
    let source = r#"
@code {
    int Load() => 1;
    class Inner { Func<string> Load { get; } = null; void M() { var shadowed = Load(); } }
    class Plain { void M() { var control = Load(); } }
}
"#;
    assert_eq!(inferred(source, "shadowed"), None);
    assert_eq!(inferred(source, "control").as_deref(), Some("int"));
}

#[test]
fn markup_bindings_named_like_the_method_record_nothing() {
    let shadowed = [
        r#"@{ Func<string> Load = () => ""; var widget = Load(); }"#,
        r#"@foreach (var Load in loaders) { var widget = Load(); }"#,
        r#"@inject Func<string> Load
@{ var widget = Load(); }"#,
    ];
    for markup in shadowed {
        let source = format!("{markup}\n@code {{ int Load() => 1; }}\n");
        assert_eq!(inferred(&source, "widget"), None, "{markup}");
    }
    let control = "@{ var widget = Load(); }\n@code { int Load() => 1; }\n";
    assert_eq!(inferred(control, "widget").as_deref(), Some("int"));
}

#[test]
fn code_block_call_with_arguments_records_nothing() {
    let source = r#"
@code {
    Widget Load(int id) => null;
    void Run() { var widget = Load(1); var self = this.Load(1); }
}
"#;
    assert_eq!(inferred(source, "widget"), None);
    assert_eq!(inferred(source, "self"), None);
}
