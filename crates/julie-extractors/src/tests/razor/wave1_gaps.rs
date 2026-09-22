use crate::base::{ExtractionResults, IdentifierKind, RelationshipKind, Symbol, SymbolKind};
use crate::extract_canonical;
use std::path::Path;

const COUNTER: &str = r#"@page "/counter"
@inherits LayoutComponentBase
@implements IDisposable
@inject ITodoService TodoService
@inject IOptions<AppSettings> Options

<p>Current count: @currentCount</p>
<TodoItem Title="x" />

@code {
    private int currentCount = 0;
    private object current = new();
    private readonly List<string> items = new();
    private const int Max = 10;
    private int a, b;
    [Inject] NavigationManager Nav { get; set; } = default!;
    public Counter() { }
    public record Row(int Id, string Title);
    public enum Status { Active, Archived }
    public interface IValidatable { bool Validate(); }
    public struct Point { public int X; }
    public delegate Task Handler(int id);
    public event Action<string>? OnChanged;
    public string this[int i] => i.ToString();
    private RenderFragment Frag => @<Badge Title="x" />;
    public class FormModel : ModelBase, IValidatable { string Hidden { get; set; } }

    protected override async Task OnInitializedAsync()
    {
        var todos = await TodoService.GetAllAsync(Max);
        Nav.NavigateTo("/x");
        Refresh();
        StateHasChanged();
    }
    private void Refresh() { }
    public void Dispose() { }
}
"#;

fn extract(file: &str, source: &str) -> ExtractionResults {
    extract_canonical(file, source, Path::new("/tmp/test")).expect("Razor extraction")
}

fn symbols_named<'a>(result: &'a ExtractionResults, name: &str) -> Vec<&'a Symbol> {
    result
        .symbols
        .iter()
        .filter(|symbol| symbol.name == name)
        .collect()
}

fn only<'a>(result: &'a ExtractionResults, name: &str) -> &'a Symbol {
    let found = symbols_named(result, name);
    assert_eq!(found.len(), 1, "expected one {name}: {:#?}", found);
    found[0]
}

fn component_id(result: &ExtractionResults) -> String {
    result
        .symbols
        .iter()
        .find(|symbol| symbol.kind == SymbolKind::Class && symbol.parent_id.is_none())
        .map(|symbol| symbol.id.clone())
        .expect("component symbol")
}

#[test]
fn code_block_member_declarations_are_fields_with_every_declarator() {
    let result = extract("Counter.razor", COUNTER);
    for name in ["currentCount", "current", "items", "a", "b"] {
        assert_eq!(only(&result, name).kind, SymbolKind::Field, "{name}");
    }
    assert_eq!(only(&result, "Max").kind, SymbolKind::Constant);
    let signature = |name| only(&result, name).signature.clone().unwrap_or_default();
    assert!(
        signature("items").contains("readonly"),
        "{}",
        signature("items")
    );
    assert!(signature("Max").contains("const"), "{}", signature("Max"));
    assert_eq!(only(&result, "todos").kind, SymbolKind::Variable);
}

#[test]
fn template_expressions_create_no_symbols() {
    let result = extract("Counter.razor", COUNTER);
    assert_eq!(symbols_named(&result, "currentCount").len(), 1);
    assert!(
        result.symbols.iter().all(|symbol| !symbol
            .signature
            .as_deref()
            .unwrap_or("")
            .starts_with("@@")),
        "{:#?}",
        result.symbols
    );
}

#[test]
fn code_members_and_injected_properties_belong_to_the_component() {
    let result = extract("Counter.razor", COUNTER);
    let component = component_id(&result);
    for name in [
        "currentCount",
        "Nav",
        "OnInitializedAsync",
        "Refresh",
        "TodoService",
        "Options",
        "Row",
        "Status",
        "Counter",
    ] {
        let symbol = result
            .symbols
            .iter()
            .find(|symbol| symbol.name == name && symbol.id != component)
            .unwrap_or_else(|| panic!("missing {name}"));
        assert_eq!(
            symbol.parent_id.as_deref(),
            Some(component.as_str()),
            "{name}"
        );
    }
}

#[test]
fn code_block_type_and_member_declarations_emit_symbols() {
    let result = extract("Counter.razor", COUNTER);
    let kind_of = |name: &str| {
        result
            .symbols
            .iter()
            .filter(|symbol| symbol.name == name)
            .map(|symbol| symbol.kind.clone())
            .collect::<Vec<_>>()
    };
    assert!(kind_of("Counter").contains(&SymbolKind::Constructor));
    assert_eq!(kind_of("Row"), vec![SymbolKind::Class]);
    assert_eq!(kind_of("Status"), vec![SymbolKind::Enum]);
    assert_eq!(kind_of("Active"), vec![SymbolKind::EnumMember]);
    assert_eq!(kind_of("IValidatable"), vec![SymbolKind::Interface]);
    assert_eq!(kind_of("Point"), vec![SymbolKind::Struct]);
    assert_eq!(kind_of("Handler"), vec![SymbolKind::Delegate]);
    assert_eq!(kind_of("OnChanged"), vec![SymbolKind::Event]);
    assert_eq!(kind_of("this[int i]"), vec![SymbolKind::Property]);

    let parent_name = |name: &str| {
        let parent_id = only(&result, name).parent_id.clone();
        result
            .symbols
            .iter()
            .find(|symbol| Some(&symbol.id) == parent_id.as_ref())
            .map(|symbol| symbol.name.clone())
    };
    assert_eq!(parent_name("Active").as_deref(), Some("Status"));
    assert_eq!(parent_name("Validate").as_deref(), Some("IValidatable"));
    assert_eq!(parent_name("X").as_deref(), Some("Point"));
    assert_eq!(parent_name("Hidden").as_deref(), Some("FormModel"));
}

#[test]
fn properties_without_access_modifiers_are_extracted() {
    let result = extract("Counter.razor", COUNTER);
    let nav = only(&result, "Nav");
    assert_eq!(nav.kind, SymbolKind::Property);
    assert!(nav.annotations.iter().any(|a| a.annotation == "Inject"));
    assert_eq!(only(&result, "Hidden").kind, SymbolKind::Property);
    assert_eq!(
        result.types.get(&nav.id).map(|t| t.resolved_type.as_str()),
        Some("NavigationManager")
    );
}

#[test]
fn inject_and_model_directives_record_declared_types_and_no_garbage() {
    let result = extract("Counter.razor", COUNTER);
    let resolved = |name: &str| {
        let symbol = only(&result, name);
        result
            .types
            .get(&symbol.id)
            .map(|t| (t.resolved_type.clone(), t.is_inferred))
    };
    assert_eq!(
        resolved("TodoService"),
        Some(("ITodoService".into(), false))
    );
    assert_eq!(resolved("Options"), Some(("IOptions".into(), false)));
    for info in result.types.values() {
        assert!(
            !matches!(
                info.resolved_type.as_str(),
                "unknown" | "var" | "void" | "class"
            ),
            "garbage type fact {info:?}"
        );
    }

    let view = extract(
        "Pages/Orders/Edit.cshtml",
        "@page \"{id:int}\"\n@model ShopApp.Pages.Orders.EditModel\n<p>@Model.Total</p>\n",
    );
    let model = only(&view, "Model");
    assert_eq!(model.kind, SymbolKind::Property);
    assert_eq!(
        view.types.get(&model.id).map(|t| t.resolved_type.as_str()),
        Some("ShopApp.Pages.Orders.EditModel")
    );
}

#[test]
fn component_tags_emit_type_usages_facts_and_pending_uses() {
    let result = extract("Counter.razor", COUNTER);
    let usages: Vec<&str> = result
        .identifiers
        .iter()
        .filter(|identifier| identifier.kind == IdentifierKind::TypeUsage)
        .map(|identifier| identifier.name.as_str())
        .collect();
    assert!(usages.contains(&"TodoItem"), "{usages:?}");
    assert!(usages.contains(&"Badge"), "{usages:?}");

    let tags: Vec<String> = result
        .structural_facts
        .iter()
        .filter(|fact| fact.pattern_id == "blazor.component_reference.v1")
        .filter_map(|fact| {
            fact.metadata
                .as_ref()
                .and_then(|metadata| metadata.get("tag"))
                .and_then(|tag| tag.as_str())
                .map(str::to_string)
        })
        .collect();
    assert_eq!(tags, vec!["TodoItem".to_string(), "Badge".to_string()]);

    let uses: Vec<&str> = result
        .structured_pending_relationships
        .iter()
        .filter(|pending| pending.pending.kind == RelationshipKind::Uses)
        .map(|pending| pending.target.display_name.as_str())
        .collect();
    assert!(
        uses.contains(&"TodoItem") && uses.contains(&"Badge"),
        "{uses:?}"
    );
}

#[test]
fn unresolved_calls_and_base_types_emit_structured_pending_rows() {
    let result = extract("Counter.razor", COUNTER);
    let rows: Vec<(RelationshipKind, String, Option<String>)> = result
        .structured_pending_relationships
        .iter()
        .map(|pending| {
            (
                pending.pending.kind.clone(),
                pending.target.display_name.clone(),
                pending.target.receiver.clone(),
            )
        })
        .collect();
    for expected in [
        (
            RelationshipKind::Calls,
            "TodoService.GetAllAsync",
            Some("TodoService"),
        ),
        (RelationshipKind::Calls, "Nav.NavigateTo", Some("Nav")),
        (RelationshipKind::Calls, "StateHasChanged", None),
        (RelationshipKind::Extends, "LayoutComponentBase", None),
        (RelationshipKind::Implements, "IDisposable", None),
        (RelationshipKind::Extends, "ModelBase", None),
    ] {
        let expected = (
            expected.0,
            expected.1.to_string(),
            expected.2.map(str::to_string),
        );
        assert!(rows.contains(&expected), "missing {expected:?}: {rows:#?}");
    }
    assert!(
        !rows.iter().any(|(_, target, _)| target == "Refresh"),
        "same-file calls resolve to relationships"
    );
    let form_model = only(&result, "FormModel");
    let validatable = only(&result, "IValidatable");
    assert!(
        result
            .relationships
            .iter()
            .any(|rel| rel.kind == RelationshipKind::Implements
                && rel.from_symbol_id == form_model.id
                && rel.to_symbol_id == validatable.id)
    );
    assert!(
        result
            .relationships
            .iter()
            .all(|rel| !rel.to_symbol_id.starts_with("method:")),
        "synthetic targets are not relationships"
    );
}

#[test]
fn parse_errors_before_the_code_block_keep_its_members() {
    let result = extract(
        "C1.razor",
        r#"@namespace App.Shared

@* A comment. *@
<div class="x">@Title</div>

@code {
    [Parameter] public string Title { get; set; } = "";
    private void Close() => Hide();
}
"#,
    );
    let component = component_id(&result);
    for (name, kind) in [
        ("Title", SymbolKind::Property),
        ("Close", SymbolKind::Method),
    ] {
        let symbol = only(&result, name);
        assert_eq!(symbol.kind, kind);
        assert_eq!(symbol.parent_id.as_deref(), Some(component.as_str()));
    }
}

#[test]
fn razor_pages_routes_include_the_page_path() {
    let route = |path: &str, source: &str| {
        extract(path, source)
            .structural_facts
            .iter()
            .find(|fact| fact.pattern_id == "razor.page_directive.v1")
            .and_then(|fact| fact.metadata.as_ref())
            .and_then(|metadata| metadata.get("normalized_route_template"))
            .and_then(|value| value.as_str())
            .map(str::to_string)
    };
    assert_eq!(
        route("Pages/Orders/Edit.cshtml", "@page \"{id:int}\"\n").as_deref(),
        Some("/Orders/Edit/:id")
    );
    assert_eq!(
        route("src/App/Pages/Error.cshtml", "@page\n").as_deref(),
        Some("/Error")
    );
    assert_eq!(
        route("Pages/Orders/Index.cshtml", "@page\n").as_deref(),
        Some("/Orders")
    );
    assert_eq!(
        route("Areas/Admin/Pages/Users/Index.cshtml", "@page\n").as_deref(),
        Some("/Admin/Users")
    );
    assert_eq!(
        route("Pages/Orders/Edit.cshtml", "@page \"/orders/{id}/edit\"\n").as_deref(),
        Some("/orders/:id/edit")
    );
    assert_eq!(
        route("Pages/Counter.razor", "@page \"/counter\"\n").as_deref(),
        Some("/counter")
    );
}

#[test]
fn bunit_test_component_is_a_test_container() {
    let result = extract(
        "CounterTest.razor",
        r#"@inherits BunitContext
@code {
    [Fact]
    public void CounterStartsAtZero() { var cut = Render(@<Counter />); }
}
"#,
    );
    let component = only(&result, "CounterTest");
    assert_eq!(
        component
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.get("test_container"))
            .and_then(|value| value.as_bool()),
        Some(true),
        "{component:#?}"
    );
    assert_eq!(
        only(&result, "CounterStartsAtZero").parent_id.as_deref(),
        Some(component.id.as_str())
    );
}
