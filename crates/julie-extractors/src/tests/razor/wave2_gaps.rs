use crate::base::{
    ExtractionResults, IdentifierKind, LiteralKind, RelationshipKind, StructuralFact, Symbol,
    SymbolKind, Visibility,
};
use crate::extract_canonical;
use crate::language_policy::classify_literals_by_carrier;
use crate::tests::helpers::{facts_with_pattern, metadata_str};
use std::path::Path;

const ORDER_PAGE: &str =
    include_str!("../../../../../fixtures/extraction/razor/wave2_component/OrderPage.razor");
const MVC_VIEW: &str =
    include_str!("../../../../../fixtures/extraction/razor/mvc_views/Views/Home/Index.cshtml");

fn extract(file: &str, source: &str) -> ExtractionResults {
    extract_canonical(file, source, Path::new("/repo")).expect("Razor extraction")
}

fn order_page() -> ExtractionResults {
    extract("Components/OrderPage.razor", ORDER_PAGE)
}

fn mvc_view() -> ExtractionResults {
    extract("Views/Home/Index.cshtml", MVC_VIEW)
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
    assert_eq!(found.len(), 1, "expected one {name}: {found:#?}");
    found[0]
}

fn file_class(result: &ExtractionResults) -> &Symbol {
    result
        .symbols
        .iter()
        .find(|symbol| symbol.kind == SymbolKind::Class && symbol.parent_id.is_none())
        .expect("file class symbol")
}

fn symbol_name<'a>(result: &'a ExtractionResults, id: &str) -> &'a str {
    result
        .symbols
        .iter()
        .find(|symbol| symbol.id == id)
        .map_or("", |symbol| symbol.name.as_str())
}

fn relationship_pairs(result: &ExtractionResults, kind: RelationshipKind) -> Vec<(&str, &str)> {
    result
        .relationships
        .iter()
        .filter(|relationship| relationship.kind == kind)
        .map(|relationship| {
            (
                symbol_name(result, &relationship.from_symbol_id),
                symbol_name(result, &relationship.to_symbol_id),
            )
        })
        .collect()
}

fn pending_targets(result: &ExtractionResults, kind: RelationshipKind) -> Vec<&str> {
    result
        .pending_relationships
        .iter()
        .filter(|pending| pending.kind == kind)
        .map(|pending| pending.callee_name.as_str())
        .collect()
}

fn identifier_kinds<'a>(result: &'a ExtractionResults, name: &str) -> Vec<&'a IdentifierKind> {
    result
        .identifiers
        .iter()
        .filter(|identifier| identifier.name == name)
        .map(|identifier| &identifier.kind)
        .collect()
}

fn metadata<'a>(symbol: &'a Symbol, key: &str) -> Option<&'a serde_json::Value> {
    symbol
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get(key))
}

fn fact_with<'a>(facts: &[&'a StructuralFact], key: &str, value: &str) -> &'a StructuralFact {
    facts
        .iter()
        .copied()
        .find(|fact| metadata_str(fact, key) == Some(value))
        .unwrap_or_else(|| panic!("no fact with {key}={value}: {facts:#?}"))
}

#[test]
fn each_directive_emits_one_row() {
    let result = order_page();
    for name in ["@page", "@namespace", "@inherits", "@implements"] {
        assert_eq!(symbols_named(&result, name).len(), 1, "{name}");
    }
    for token_name in ["namespace", "inherits", "implements", "layout", "typeparam"] {
        assert!(
            symbols_named(&result, token_name).is_empty(),
            "{token_name}"
        );
    }
    assert!(symbols_named(&result, "@attribute").is_empty());
    assert_eq!(only(&result, "@implements").kind, SymbolKind::Import);
}

#[test]
fn using_alias_directive_names_the_alias() {
    let result = order_page();
    let alias = only(&result, "Json");
    assert_eq!(alias.kind, SymbolKind::Import);
    assert_eq!(
        alias.signature.as_deref(),
        Some("@using Json = System.Text.Json.JsonSerializer")
    );
    assert_eq!(
        metadata(alias, "directiveValue").and_then(|value| value.as_str()),
        Some("System.Text.Json.JsonSerializer")
    );
}

#[test]
fn component_carries_inherits_implements_and_layout_edges() {
    let result = order_page();
    assert!(
        pending_targets(&result, RelationshipKind::Extends)
            .contains(&"OwningComponentBase<IOrderService>")
    );
    assert!(pending_targets(&result, RelationshipKind::Implements).contains(&"IAsyncDisposable"));
    assert!(pending_targets(&result, RelationshipKind::Uses).contains(&"AdminLayout"));
    for type_name in [
        "OwningComponentBase",
        "IOrderService",
        "IAsyncDisposable",
        "AdminLayout",
    ] {
        assert!(
            identifier_kinds(&result, type_name).contains(&&IdentifierKind::TypeUsage),
            "{type_name}"
        );
    }
}

#[test]
fn component_symbol_models_its_directives() {
    let result = order_page();
    let component = file_class(&result);
    assert_eq!(component.name, "OrderPage");
    assert_eq!(
        component.signature.as_deref(),
        Some("component Shop.Components.Orders.OrderPage<TItem>")
    );
    let text = |key| metadata(component, key).and_then(|value| value.as_str());
    assert_eq!(text("layout"), Some("AdminLayout"));
    assert_eq!(text("renderMode"), Some("InteractiveServer"));
    assert_eq!(
        metadata(component, "typeParameters"),
        Some(&serde_json::json!(["TItem"]))
    );
    assert_eq!(
        metadata(component, "typeParameterConstraints"),
        Some(&serde_json::json!({"TItem": "where TItem : class"}))
    );
}

#[test]
fn attribute_directives_become_component_annotations() {
    let result = order_page();
    let annotations: Vec<(&str, Option<&str>)> = file_class(&result)
        .annotations
        .iter()
        .map(|marker| (marker.annotation.as_str(), marker.raw_text.as_deref()))
        .collect();
    assert!(
        annotations.contains(&("Authorize", Some("Authorize(Roles = \"Admin\")"))),
        "{annotations:?}"
    );
    assert!(
        annotations
            .iter()
            .any(|(name, _)| *name == "StreamRendering"),
        "{annotations:?}"
    );
    let id = only(&result, "Id");
    assert!(
        id.annotations
            .iter()
            .any(|marker| marker.annotation == "Parameter")
    );
}

#[test]
fn in_file_base_class_gets_extends_edge_and_signature() {
    let result = order_page();
    assert!(
        relationship_pairs(&result, RelationshipKind::Extends).contains(&("ItemRow", "BaseRow"))
    );
    assert_eq!(
        only(&result, "ItemRow").signature.as_deref(),
        Some("public class ItemRow : BaseRow")
    );
    assert!(identifier_kinds(&result, "BaseRow").contains(&&IdentifierKind::TypeUsage));
}

#[test]
fn generic_method_calls_are_named_by_the_method() {
    let result = order_page();
    for name in ["GetFromJsonAsync", "Render"] {
        let kinds = identifier_kinds(&result, name);
        assert!(kinds.contains(&&IdentifierKind::Call), "{name}: {kinds:?}");
        assert!(
            !kinds.contains(&&IdentifierKind::TypeUsage),
            "{name}: {kinds:?}"
        );
    }
    assert!(identifier_kinds(&result, "OrderRow").contains(&&IdentifierKind::TypeUsage));
    let call_type_arguments: Vec<(&str, Vec<&str>)> = result
        .type_argument_usages
        .iter()
        .filter_map(|usage| {
            let call = result
                .identifiers
                .iter()
                .find(|identifier| identifier.id == usage.identifier_id)
                .filter(|identifier| identifier.kind == IdentifierKind::Call)?;
            Some((
                call.name.as_str(),
                usage
                    .arguments
                    .iter()
                    .map(|argument| argument.type_name.as_str())
                    .collect(),
            ))
        })
        .collect();
    assert!(
        call_type_arguments.contains(&("GetFromJsonAsync", vec!["List"])),
        "{call_type_arguments:?}"
    );
    assert!(
        call_type_arguments.contains(&("Render", vec!["OrderRow"])),
        "{call_type_arguments:?}"
    );
}

#[test]
fn calls_resolve_through_the_enclosing_class_hierarchy() {
    let result = order_page();
    let calls = relationship_pairs(&result, RelationshipKind::Calls);
    let base_render = symbols_named(&result, "Render")
        .into_iter()
        .find(|symbol| symbol_name(&result, symbol.parent_id.as_deref().unwrap_or("")) == "BaseRow")
        .expect("BaseRow.Render");
    let item_render = symbols_named(&result, "Render")
        .into_iter()
        .find(|symbol| symbol_name(&result, symbol.parent_id.as_deref().unwrap_or("")) == "ItemRow")
        .expect("ItemRow.Render");
    let targets_of = |from: &str| -> Vec<&str> {
        result
            .relationships
            .iter()
            .filter(|relationship| {
                relationship.kind == RelationshipKind::Calls
                    && symbol_name(&result, &relationship.from_symbol_id) == from
            })
            .map(|relationship| relationship.to_symbol_id.as_str())
            .collect()
    };
    assert!(
        targets_of("Redraw").contains(&item_render.id.as_str()),
        "{calls:?}"
    );
    assert!(
        result.relationships.iter().any(|relationship| {
            relationship.kind == RelationshipKind::Calls
                && relationship.from_symbol_id == item_render.id
                && relationship.to_symbol_id == base_render.id
        }),
        "base.Render() must call BaseRow.Render: {calls:?}"
    );
    assert!(
        pending_targets(&result, RelationshipKind::Calls).contains(&"Render"),
        "the component's Render<OrderRow>() must not bind to a nested class method"
    );
}

#[test]
fn modifierless_members_and_locals_are_private() {
    let result = order_page();
    for name in ["total", "_order", "cut", "twice"] {
        assert_eq!(
            only(&result, name).visibility,
            Some(Visibility::Private),
            "{name}"
        );
    }
    assert_eq!(only(&result, "Id").visibility, Some(Visibility::Public));
}

#[test]
fn bodies_cover_the_component_lambdas_and_callables_only() {
    let result = order_page();
    let component = file_class(&result);
    let body = component.body_span.expect("component body");
    assert_eq!(
        (body.start_byte, body.end_byte as usize),
        (0, ORDER_PAGE.len())
    );
    for name in [
        "@page",
        "@inherits",
        "Json",
        "Http",
        "total",
        "_order",
        "cut",
    ] {
        assert!(only(&result, name).body_span.is_none(), "{name}");
    }
    for name in ["format", "twice", "Refresh"] {
        assert!(only(&result, name).body_span.is_some(), "{name}");
    }
}

#[test]
fn json_http_calls_carry_url_literals() {
    let mut result = order_page();
    classify_literals_by_carrier(&mut result.literals);
    let urls: Vec<(&str, &str)> = result
        .literals
        .iter()
        .filter(|literal| literal.kind == LiteralKind::Url)
        .map(|literal| {
            (
                literal.literal_text.as_str(),
                literal.carrier.as_deref().unwrap_or(""),
            )
        })
        .collect();
    assert!(
        urls.contains(&("api/orders", "GetFromJsonAsync")),
        "{urls:?}"
    );
    assert!(
        urls.contains(&("api/orders/audit", "PostAsJsonAsync")),
        "{urls:?}"
    );
}

#[test]
fn relative_http_client_urls_emit_requests() {
    let result = order_page();
    let facts = facts_with_pattern(&result, "http.client_request.v1");
    let request = fact_with(&facts, "target_path", "api/orders");
    assert_eq!(metadata_str(request, "url_kind"), Some("relative"));
    assert_eq!(metadata_str(request, "verb"), Some("GET"));
    fact_with(&facts, "target_path", "api/orders/audit");
}

#[test]
fn template_control_flow_counts_toward_file_complexity() {
    let result = order_page();
    let file = result
        .complexity_metrics
        .iter()
        .find(|metric| metric.scope == "file")
        .expect("file complexity");
    assert_eq!(file.decision_count, 3);
    assert_eq!(file.loop_count, 1);
    assert!(file.max_nesting_depth >= 3, "{file:?}");
}

#[test]
fn event_handler_method_groups_call_their_handlers() {
    let result = order_page();
    let calls = relationship_pairs(&result, RelationshipKind::Calls);
    assert!(calls.contains(&("OrderPage", "Refresh")), "{calls:?}");
    assert!(calls.contains(&("OrderPage", "DeleteAsync")), "{calls:?}");
    let missing = extract(
        "Components/Toolbar.razor",
        "<button @onclick=\"SaveAll\">Save</button>\n",
    );
    assert!(pending_targets(&missing, RelationshipKind::Calls).contains(&"SaveAll"));
}

#[test]
fn render_fragment_parameters_are_not_components() {
    let result = order_page();
    let tags: Vec<&str> = facts_with_pattern(&result, "blazor.component_reference.v1")
        .into_iter()
        .filter_map(|fact| metadata_str(fact, "tag"))
        .collect();
    for tag in [
        "RadzenDataGrid",
        "RadzenDataGridColumn",
        "NavLink",
        "OrderRow",
    ] {
        assert!(tags.contains(&tag), "{tag}: {tags:?}");
    }
    for fragment in ["Columns", "Template"] {
        assert!(!tags.contains(&fragment), "{fragment}: {tags:?}");
        assert!(identifier_kinds(&result, fragment).is_empty(), "{fragment}");
        assert!(!pending_targets(&result, RelationshipKind::Uses).contains(&fragment));
    }
    let nested = extract(
        "Components/Shell.razor",
        "<CascadingValue Value=\"theme\"><Router /></CascadingValue>\n",
    );
    let nested_tags: Vec<&str> = facts_with_pattern(&nested, "blazor.component_reference.v1")
        .into_iter()
        .filter_map(|fact| metadata_str(fact, "tag"))
        .collect();
    assert!(nested_tags.contains(&"Router"), "{nested_tags:?}");
}

#[test]
fn navigation_targets_resolve_relative_interpolated_and_templated_routes() {
    let result = order_page();
    let facts = facts_with_pattern(&result, "razor.route_reference.v1");

    let nav_link = fact_with(&facts, "raw_target", "orders");
    assert_eq!(metadata_str(nav_link, "target_path"), Some("/orders"));
    assert_eq!(metadata_str(nav_link, "source_kind"), Some("href"));
    assert_eq!(
        nav_link
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.get("base_relative")),
        Some(&serde_json::Value::Bool(true))
    );

    let back = fact_with(&facts, "raw_target", "orders/list");
    assert_eq!(metadata_str(back, "target_path"), Some("/orders/list"));
    assert_eq!(metadata_str(back, "source_kind"), Some("navigate_to"));

    let remove = fact_with(&facts, "target_path", "/orders/{id}/delete");
    assert_eq!(
        metadata_str(remove, "route_source"),
        Some("interpolated_string")
    );
    assert!(metadata_str(remove, "raw_target").is_none());

    let open = fact_with(&facts, "target_path", "/orders/{_order.Id}");
    assert_eq!(
        metadata_str(open, "route_source"),
        Some("template_expression")
    );
    assert_eq!(metadata_str(open, "source_kind"), Some("href"));
}

#[test]
fn asset_hrefs_and_external_targets_are_not_routes() {
    let result = extract(
        "Components/Head.razor",
        "<link href=\"css/app.css\" rel=\"stylesheet\" />\n<a href=\"mailto:a@b.c\">Mail</a>\n<a href=\"https://example.com\">Out</a>\n<a href=\"@url\">Dynamic</a>\n",
    );
    assert!(facts_with_pattern(&result, "razor.route_reference.v1").is_empty());
}

#[test]
fn razor_pages_routes_come_from_the_file_path() {
    let edit = extract(
        "src/Shop/Pages/Products/Edit.cshtml",
        "@page \"{id:int}\"\n@model EditModel\n",
    );
    let edit_facts = facts_with_pattern(&edit, "razor.page_directive.v1");
    assert_eq!(
        metadata_str(edit_facts[0], "route"),
        Some("/Products/Edit/{id:int}")
    );

    let index = extract("src/Shop/Pages/Index.cshtml", "@page\n@model IndexModel\n");
    let index_facts = facts_with_pattern(&index, "razor.page_directive.v1");
    assert_eq!(index_facts.len(), 1);
    assert_eq!(metadata_str(index_facts[0], "route"), Some("/"));
    assert!(identifier_kinds(&index, "IndexModel").contains(&&IdentifierKind::TypeUsage));
}

#[test]
fn cshtml_view_symbol_contains_template_code_and_calls() {
    let result = mvc_view();
    let view = file_class(&result);
    assert_eq!(view.name, "Index");
    assert_eq!(view.signature.as_deref(), Some("view Index"));
    assert_eq!(
        metadata(view, "type").and_then(|value| value.as_str()),
        Some("razor-view")
    );
    assert_eq!(
        only(&result, "Model").parent_id.as_deref(),
        Some(view.id.as_str())
    );
    assert_eq!(
        only(&result, "Price").parent_id.as_deref(),
        Some(view.id.as_str())
    );
    assert!(relationship_pairs(&result, RelationshipKind::Calls).contains(&("Index", "Price")));
    assert!(
        result
            .identifiers
            .iter()
            .filter(|identifier| identifier.name == "product")
            .all(|identifier| identifier.containing_symbol_id.as_deref() == Some(view.id.as_str()))
    );
}

#[test]
fn view_import_files_have_no_file_class() {
    let result = extract("Views/_ViewImports.cshtml", "@using Shop.Models\n");
    assert!(
        result
            .symbols
            .iter()
            .all(|symbol| symbol.kind != SymbolKind::Class)
    );
}

#[test]
fn mvc_tag_helpers_and_helpers_emit_link_facts() {
    let result = mvc_view();
    let links = facts_with_pattern(&result, "razor.mvc_link.v1");
    assert_eq!(links.len(), 5, "{links:#?}");

    let create = fact_with(&links, "action", "Create");
    assert_eq!(metadata_str(create, "controller"), Some("Products"));
    assert_eq!(metadata_str(create, "area"), Some("Admin"));
    assert_eq!(metadata_str(create, "target_kind"), Some("action"));
    assert_eq!(metadata_str(create, "source_kind"), Some("tag_helper"));

    let details = fact_with(&links, "page", "/Orders/Details");
    assert_eq!(metadata_str(details, "target_kind"), Some("page"));
    assert_eq!(
        details
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.get("route_value_names")),
        Some(&serde_json::json!(["id"]))
    );

    fact_with(&links, "page_handler", "Save");
    let action_link = fact_with(&links, "action", "Edit");
    assert_eq!(
        metadata_str(action_link, "source_kind"),
        Some("html_helper")
    );
    let url_action = fact_with(&links, "action", "Index");
    assert_eq!(metadata_str(url_action, "controller"), Some("Home"));
    assert_eq!(metadata_str(url_action, "source_kind"), Some("url_helper"));
}

#[test]
fn partials_view_components_layouts_and_bindings_emit_facts() {
    let result = mvc_view();
    let partials = facts_with_pattern(&result, "razor.partial_reference.v1");
    assert_eq!(
        metadata_str(
            fact_with(&partials, "partial_name", "_OrderLines"),
            "source_kind"
        ),
        Some("tag_helper")
    );
    assert_eq!(
        metadata_str(
            fact_with(&partials, "partial_name", "_Summary"),
            "source_kind"
        ),
        Some("html_helper")
    );

    let components = facts_with_pattern(&result, "razor.view_component_reference.v1");
    fact_with(&components, "component_name", "Cart");
    assert_eq!(
        metadata_str(
            fact_with(&components, "component_name", "ShoppingCart"),
            "tag"
        ),
        Some("vc:shopping-cart")
    );

    let layouts = facts_with_pattern(&result, "razor.layout_reference.v1");
    assert_eq!(metadata_str(layouts[0], "layout"), Some("_Layout"));
    assert_eq!(metadata_str(layouts[0], "source_kind"), Some("assignment"));

    let bindings = facts_with_pattern(&result, "razor.model_binding.v1");
    assert_eq!(bindings.len(), 2, "{bindings:#?}");
    assert!(
        bindings
            .iter()
            .all(|fact| metadata_str(fact, "model_expression") == Some("Input.Email"))
    );
    fact_with(&bindings, "attribute", "asp-validation-for");

    let component = order_page();
    let blazor_layout = facts_with_pattern(&component, "razor.layout_reference.v1")
        .into_iter()
        .map(|fact| {
            (
                metadata_str(fact, "layout"),
                metadata_str(fact, "source_kind"),
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        blazor_layout,
        vec![(Some("AdminLayout"), Some("directive"))]
    );
}
