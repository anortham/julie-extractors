//! ASP.NET Core MVC and Razor Pages view facts: tag-helper and helper links to
//! pages and actions, partial views, view components, layouts, and `asp-for`
//! model bindings.

use serde_json::Value;
use tree_sitter::Node;

use super::blazor_navigation::{TagAttribute, opening_tag_attributes};
use super::helpers::{base_metadata, fact_for_node, fact_for_span, insert_string, node_text};
use super::static_arg::{StaticArgLang, static_route_arg};
use super::{
    RAZOR_LAYOUT_REFERENCE_PATTERN_ID, RAZOR_MODEL_BINDING_PATTERN_ID, RAZOR_MVC_LINK_PATTERN_ID,
    RAZOR_PARTIAL_REFERENCE_PATTERN_ID, RAZOR_VIEW_COMPONENT_REFERENCE_PATTERN_ID,
};
use crate::base::span::NormalizedSpan;
use crate::base::types::StructuralFact;

const FRAMEWORK: &str = "aspnetcore";
const MODEL_BINDING_ATTRIBUTES: [&str; 2] = ["asp-for", "asp-validation-for"];
const PARTIAL_HELPERS: [&str; 4] = [
    "Partial",
    "PartialAsync",
    "RenderPartial",
    "RenderPartialAsync",
];

/// Facts from an element's tag-helper attributes, `<partial>`, and `<vc:*>`.
pub(super) fn element_facts(node: Node<'_>, file_path: &str, content: &str) -> Vec<StructuralFact> {
    let Some((tag, attributes)) = opening_tag_attributes(node, content) else {
        return Vec::new();
    };
    let tag_span = || {
        let start = node.start_byte() + usize::from(content[node.start_byte()..].starts_with('<'));
        NormalizedSpan::from_content_range(content, start, start + tag.len())
    };
    let mut facts = Vec::new();

    if let Some(metadata) = tag_helper_link(tag, &attributes)
        && let Some(span) = tag_span()
    {
        facts.push(fact_for_span(
            file_path,
            "razor",
            RAZOR_MVC_LINK_PATTERN_ID,
            "mvc_link",
            "tag_name",
            span,
            metadata,
        ));
    }

    for attribute in attributes
        .iter()
        .filter(|attribute| MODEL_BINDING_ATTRIBUTES.contains(&attribute.name))
    {
        let mut metadata = base_metadata("form_binding", FRAMEWORK);
        insert_string(
            &mut metadata,
            "model_expression",
            attribute.value.trim_start_matches('@'),
        );
        insert_string(&mut metadata, "attribute", attribute.name);
        insert_string(&mut metadata, "tag", tag);
        if let Some(fact) = attribute_value_fact(
            file_path,
            content,
            attribute,
            RAZOR_MODEL_BINDING_PATTERN_ID,
            "model_binding",
            metadata,
        ) {
            facts.push(fact);
        }
    }

    if tag.eq_ignore_ascii_case("partial")
        && let Some(name) = attributes.iter().find(|attribute| attribute.name == "name")
        && is_static_value(name)
    {
        let mut metadata = base_metadata("view_composition", FRAMEWORK);
        insert_string(&mut metadata, "partial_name", name.value);
        insert_string(&mut metadata, "source_kind", "tag_helper");
        if let Some(fact) = attribute_value_fact(
            file_path,
            content,
            name,
            RAZOR_PARTIAL_REFERENCE_PATTERN_ID,
            "partial_reference",
            metadata,
        ) {
            facts.push(fact);
        }
    }

    if let Some(kebab_name) = tag.strip_prefix("vc:")
        && !kebab_name.is_empty()
        && let Some(span) = tag_span()
    {
        let mut metadata = base_metadata("view_composition", FRAMEWORK);
        insert_string(&mut metadata, "component_name", &pascal_case(kebab_name));
        insert_string(&mut metadata, "source_kind", "tag_helper");
        insert_string(&mut metadata, "tag", tag);
        facts.push(fact_for_span(
            file_path,
            "razor",
            RAZOR_VIEW_COMPONENT_REFERENCE_PATTERN_ID,
            "view_component_reference",
            "tag_name",
            span,
            metadata,
        ));
    }

    facts
}

/// Facts from `Html.*`, `Url.*`, and `Component.InvokeAsync` calls.
pub(super) fn invocation_fact(
    node: Node<'_>,
    file_path: &str,
    content: &str,
) -> Option<StructuralFact> {
    let function = node.child_by_field_name("function")?;
    if function.kind() != "member_access_expression" {
        return None;
    }
    let receiver = node_text(content, function.child_by_field_name("expression")?)?;
    let name_node = function.child_by_field_name("name")?;
    let method = match name_node.kind() {
        "generic_name" => node_text(content, name_node.named_child(0)?)?,
        _ => node_text(content, name_node)?,
    };
    let arguments = static_arguments(node, content);
    let argument = |index: usize| arguments.get(index).copied().flatten();

    let (pattern_id, capture_name, metadata) = match (receiver, method) {
        ("Html", "ActionLink") => mvc_link(
            "html_helper",
            None,
            [
                ("action", argument(1)?),
                ("controller", argument(2).unwrap_or_default()),
            ],
        )?,
        ("Html", "BeginForm") => mvc_link(
            "html_helper",
            None,
            [
                ("action", argument(0)?),
                ("controller", argument(1).unwrap_or_default()),
            ],
        )?,
        ("Url", "Action") => mvc_link(
            "url_helper",
            None,
            [
                ("action", argument(0)?),
                ("controller", argument(1).unwrap_or_default()),
            ],
        )?,
        ("Url", "Page") => mvc_link(
            "url_helper",
            None,
            [
                ("page", argument(0)?),
                ("page_handler", argument(1).unwrap_or_default()),
            ],
        )?,
        ("Html", helper) if PARTIAL_HELPERS.contains(&helper) => {
            let mut metadata = base_metadata("view_composition", FRAMEWORK);
            insert_string(&mut metadata, "partial_name", argument(0)?);
            insert_string(&mut metadata, "source_kind", "html_helper");
            (
                RAZOR_PARTIAL_REFERENCE_PATTERN_ID,
                "partial_reference",
                metadata,
            )
        }
        ("Component", "InvokeAsync") => {
            let component_name = match name_node.kind() {
                "generic_name" => generic_type_argument(name_node, content)?,
                _ => argument(0)?,
            };
            let mut metadata = base_metadata("view_composition", FRAMEWORK);
            insert_string(&mut metadata, "component_name", component_name);
            insert_string(&mut metadata, "source_kind", "component_invoke");
            (
                RAZOR_VIEW_COMPONENT_REFERENCE_PATTERN_ID,
                "view_component_reference",
                metadata,
            )
        }
        _ => return None,
    };
    Some(fact_for_node(
        file_path,
        "razor",
        pattern_id,
        capture_name,
        node,
        metadata,
    ))
}

/// A layout fact for `Layout = "_Layout"` in a view code block.
pub(super) fn layout_assignment_fact(
    node: Node<'_>,
    file_path: &str,
    content: &str,
) -> Option<StructuralFact> {
    let left = node.child_by_field_name("left")?;
    if left.kind() != "identifier" || node_text(content, left)? != "Layout" {
        return None;
    }
    let layout = static_route_arg(
        node.child_by_field_name("right")?,
        content,
        StaticArgLang::CSharp,
    )?;
    Some(layout_fact(node, file_path, layout, "assignment"))
}

/// A layout fact for a Blazor `@layout MainLayout` directive.
pub(super) fn layout_directive_fact(
    node: Node<'_>,
    file_path: &str,
    content: &str,
) -> Option<StructuralFact> {
    let mut cursor = node.walk();
    let layout = node
        .named_children(&mut cursor)
        .find(|child| !child.kind().starts_with("at_"))
        .and_then(|child| node_text(content, child))?;
    Some(layout_fact(node, file_path, layout.trim(), "directive"))
}

fn layout_fact(node: Node<'_>, file_path: &str, layout: &str, source_kind: &str) -> StructuralFact {
    let mut metadata = base_metadata("view_composition", FRAMEWORK);
    insert_string(&mut metadata, "layout", layout);
    insert_string(&mut metadata, "source_kind", source_kind);
    fact_for_node(
        file_path,
        "razor",
        RAZOR_LAYOUT_REFERENCE_PATTERN_ID,
        "layout_reference",
        node,
        metadata,
    )
}

type FactParts = (
    &'static str,
    &'static str,
    std::collections::HashMap<String, Value>,
);

/// An `mvc_link` fact from named targets. Empty values are left out.
fn mvc_link<const N: usize>(
    source_kind: &str,
    tag: Option<&str>,
    targets: [(&str, &str); N],
) -> Option<FactParts> {
    let mut metadata = base_metadata("server_navigation", FRAMEWORK);
    let mut target_kind = None;
    for (key, value) in targets.into_iter().filter(|(_, value)| !value.is_empty()) {
        insert_string(&mut metadata, key, value);
        target_kind = target_kind.or(match key {
            "page" | "page_handler" => Some("page"),
            "action" | "controller" => Some("action"),
            _ => None,
        });
    }
    insert_string(&mut metadata, "target_kind", target_kind?);
    insert_string(&mut metadata, "source_kind", source_kind);
    if let Some(tag) = tag {
        insert_string(&mut metadata, "tag", tag);
    }
    Some((RAZOR_MVC_LINK_PATTERN_ID, "mvc_link", metadata))
}

/// The link a tag helper element (`asp-page`, `asp-controller`,
/// `asp-action`, ...) targets, with its `asp-route-*` value names.
fn tag_helper_link(
    tag: &str,
    attributes: &[TagAttribute<'_>],
) -> Option<std::collections::HashMap<String, Value>> {
    let static_value = |name: &str| {
        attributes
            .iter()
            .find(|attribute| attribute.name == name && is_static_value(attribute))
            .map_or("", |attribute| attribute.value)
    };
    let (_, _, mut metadata) = mvc_link(
        "tag_helper",
        Some(tag),
        [
            ("page", static_value("asp-page")),
            ("page_handler", static_value("asp-page-handler")),
            ("controller", static_value("asp-controller")),
            ("action", static_value("asp-action")),
            ("area", static_value("asp-area")),
        ],
    )?;
    let route_values: Vec<Value> = attributes
        .iter()
        .filter_map(|attribute| attribute.name.strip_prefix("asp-route-"))
        .map(|name| Value::String(name.to_string()))
        .collect();
    metadata.insert("route_value_names".to_string(), Value::Array(route_values));
    Some(metadata)
}

fn is_static_value(attribute: &TagAttribute<'_>) -> bool {
    !attribute.value.is_empty() && !attribute.value.contains('@')
}

fn attribute_value_fact(
    file_path: &str,
    content: &str,
    attribute: &TagAttribute<'_>,
    pattern_id: &str,
    capture_name: &str,
    metadata: std::collections::HashMap<String, Value>,
) -> Option<StructuralFact> {
    let span = NormalizedSpan::from_content_range(
        content,
        attribute.value_start,
        attribute.value_start + attribute.value.len(),
    )?;
    Some(fact_for_span(
        file_path,
        "razor",
        pattern_id,
        capture_name,
        "attribute_value",
        span,
        metadata,
    ))
}

/// Each positional argument's static string value, or `None` when the
/// argument is not a plain string literal.
fn static_arguments<'a>(node: Node<'_>, content: &'a str) -> Vec<Option<&'a str>> {
    let Some(arguments) = node.child_by_field_name("arguments") else {
        return Vec::new();
    };
    let mut cursor = arguments.walk();
    arguments
        .named_children(&mut cursor)
        .filter(|argument| argument.kind() == "argument")
        .map(|argument| {
            argument
                .named_child(0)
                .and_then(|expression| static_route_arg(expression, content, StaticArgLang::CSharp))
        })
        .collect()
}

fn generic_type_argument<'a>(generic_name: Node<'_>, content: &'a str) -> Option<&'a str> {
    let mut cursor = generic_name.walk();
    let type_arguments = generic_name
        .named_children(&mut cursor)
        .find(|child| child.kind() == "type_argument_list")?;
    node_text(content, type_arguments.named_child(0)?)
}

/// `shopping-cart` as `ShoppingCart`, the class-name form of a `<vc:*>` tag.
fn pascal_case(kebab: &str) -> String {
    kebab
        .split('-')
        .map(|part| {
            let mut characters = part.chars();
            characters
                .next()
                .map(|first| first.to_ascii_uppercase().to_string() + characters.as_str())
                .unwrap_or_default()
        })
        .collect()
}
