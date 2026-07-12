use std::path::Path;

use serde_json::{Number, Value};
use tree_sitter::{Node, Tree};

use super::helpers::{base_metadata, fact_for_node, insert_string, node_text};
use super::{
    BLAZOR_COMPONENT_REFERENCE_PATTERN_ID, RAZOR_CODE_BLOCK_PATTERN_ID,
    RAZOR_PAGE_DIRECTIVE_PATTERN_ID, RAZOR_TEMPLATE_EXPRESSION_PATTERN_ID,
};
use crate::base::http_boundary::{ParamFlavor, normalize_route_template};
use crate::base::types::StructuralFact;
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};

pub(super) fn collect_razor_structural_facts(
    tree: &Tree,
    file_path: &str,
    content: &str,
) -> Vec<StructuralFact> {
    let mut facts = Vec::new();
    let component_context = RazorComponentContext::from_file(tree, file_path, content);
    collect_razor_node(
        tree.root_node(),
        file_path,
        content,
        component_context.as_ref(),
        &mut facts,
        0,
    );
    facts
}

struct RazorComponentContext {
    containing_component: String,
    namespace_context: Vec<String>,
}

impl RazorComponentContext {
    fn from_file(tree: &Tree, file_path: &str, content: &str) -> Option<Self> {
        let path = Path::new(file_path);
        if path.extension().and_then(|extension| extension.to_str()) != Some("razor") {
            return None;
        }
        let containing_component = path.file_stem()?.to_str()?;
        if matches!(containing_component, "_Imports" | "_ViewImports") {
            return None;
        }

        let mut namespace_context = Vec::new();
        collect_local_namespace_context(tree.root_node(), content, &mut namespace_context, 0);

        Some(Self {
            containing_component: containing_component.to_string(),
            namespace_context,
        })
    }
}

fn collect_razor_node(
    node: Node<'_>,
    file_path: &str,
    content: &str,
    component_context: Option<&RazorComponentContext>,
    facts: &mut Vec<StructuralFact>,
    depth: u32,
) {
    if !should_visit_tree_depth(depth) {
        return;
    }

    match node.kind() {
        "razor_page_directive" => {
            if let Some(fact) = razor_page_directive_fact(file_path, content, node) {
                facts.push(fact);
            }
        }
        "razor_block" => {
            if let Some(fact) = razor_code_block_fact(file_path, content, node) {
                facts.push(fact);
            }
        }
        "razor_expression" | "razor_implicit_expression" => {
            if let Some(fact) = razor_template_expression_fact(file_path, content, node) {
                facts.push(fact);
            }
        }
        "element" => {
            if let Some(context) = component_context
                && let Some(fact) =
                    blazor_component_reference_fact(file_path, content, node, context)
            {
                facts.push(fact);
            }
        }
        _ => {}
    }

    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_razor_node(
            child,
            file_path,
            content,
            component_context,
            facts,
            child_depth,
        );
    }
}

fn collect_local_namespace_context(
    node: Node<'_>,
    content: &str,
    context: &mut Vec<String>,
    depth: u32,
) {
    if !should_visit_tree_depth(depth) {
        return;
    }

    let directive = match node.kind() {
        "razor_namespace_directive" => Some("@namespace"),
        "razor_using_directive" => Some("@using"),
        _ => None,
    };
    if let Some(directive) = directive
        && let Some(value) = node_text(content, node)
            .and_then(|text| text.trim().strip_prefix(directive))
            .map(str::trim)
            .filter(|value| !value.is_empty())
        && !context.iter().any(|existing| existing == value)
    {
        context.push(value.to_string());
    }

    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_local_namespace_context(child, content, context, child_depth);
    }
}

fn blazor_component_reference_fact(
    file_path: &str,
    content: &str,
    node: Node<'_>,
    context: &RazorComponentContext,
) -> Option<StructuralFact> {
    let element = node_text(content, node)?;
    let tag = opening_tag_name(element)?;
    if !tag.chars().next().is_some_and(char::is_uppercase) {
        return None;
    }

    let mut metadata = base_metadata("component_reference", "blazor");
    insert_string(&mut metadata, "tag", tag);
    insert_string(
        &mut metadata,
        "containing_component",
        &context.containing_component,
    );
    metadata.insert(
        "namespace_context".to_string(),
        Value::Array(
            context
                .namespace_context
                .iter()
                .cloned()
                .map(Value::String)
                .collect(),
        ),
    );
    metadata.insert(
        "generic_arguments".to_string(),
        Value::Array(component_generic_arguments(node, content)),
    );

    Some(fact_for_node(
        file_path,
        "razor",
        BLAZOR_COMPONENT_REFERENCE_PATTERN_ID,
        "component_reference",
        node,
        metadata,
    ))
}

fn opening_tag_name(element: &str) -> Option<&str> {
    let remainder = element.strip_prefix('<')?;
    let end = remainder
        .find(|character: char| character.is_whitespace() || matches!(character, '/' | '>'))
        .unwrap_or(remainder.len());
    let tag = &remainder[..end];
    (!tag.is_empty()).then_some(tag)
}

fn component_generic_arguments(node: Node<'_>, content: &str) -> Vec<Value> {
    let mut arguments = Vec::new();
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() != "component_attribute" {
            continue;
        }
        let Some(attribute) = node_text(content, child) else {
            continue;
        };
        let Some((name, value)) = component_attribute_parts(attribute) else {
            continue;
        };
        if !name
            .strip_prefix('T')
            .and_then(|rest| rest.chars().next())
            .is_some_and(char::is_uppercase)
        {
            continue;
        }

        let mut fields = serde_json::Map::new();
        fields.insert("name".to_string(), Value::String(name.to_string()));
        fields.insert("value".to_string(), Value::String(value.to_string()));
        arguments.push(Value::Object(fields));
    }
    arguments
}

fn component_attribute_parts(attribute: &str) -> Option<(&str, &str)> {
    let (name, value) = attribute.split_once('=')?;
    let name = name.trim();
    let value = value.trim();
    let value = value
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
        .or_else(|| {
            value
                .strip_prefix('\'')
                .and_then(|value| value.strip_suffix('\''))
        })?;
    Some((name, value))
}

fn razor_page_directive_fact(
    file_path: &str,
    content: &str,
    node: Node<'_>,
) -> Option<StructuralFact> {
    let route = razor_child_text(node, content, "string_literal")?;
    let route = route.trim_matches('"').trim_matches('\'').to_string();
    if route.is_empty() {
        return None;
    }

    let route_parameters = parse_razor_route_parameters(&route);
    let has_route_constraints = route_parameters
        .iter()
        .any(|parameter| parameter.constraint.is_some());

    let mut metadata = base_metadata("component_routing", "razor");
    insert_string(&mut metadata, "directive", "page");
    insert_string(&mut metadata, "route", &route);
    insert_string(&mut metadata, "route_template", &route);
    let normalized = normalize_route_template(&route, ParamFlavor::Braces);
    insert_string(
        &mut metadata,
        "normalized_route_template",
        &normalized.template,
    );
    metadata.insert(
        "route_parameter_count".to_string(),
        Value::Number(Number::from(route_parameters.len())),
    );
    metadata.insert(
        "has_route_constraints".to_string(),
        Value::Bool(has_route_constraints),
    );
    metadata.insert(
        "route_parameters".to_string(),
        Value::Array(
            route_parameters
                .into_iter()
                .map(razor_route_parameter_value)
                .collect(),
        ),
    );

    Some(fact_for_node(
        file_path,
        "razor",
        RAZOR_PAGE_DIRECTIVE_PATTERN_ID,
        "page_directive",
        node,
        metadata,
    ))
}

#[derive(Debug, Clone)]
struct RazorRouteParameter {
    name: String,
    constraint: Option<String>,
    optional: bool,
    catch_all: bool,
}

fn parse_razor_route_parameters(route: &str) -> Vec<RazorRouteParameter> {
    let mut parameters = Vec::new();
    let mut search_start = 0;
    while let Some(open_relative) = route[search_start..].find('{') {
        let open = search_start + open_relative;
        if route.as_bytes().get(open + 1) == Some(&b'{') {
            search_start = open + 2;
            continue;
        }
        let Some(close_relative) = route[open + 1..].find('}') else {
            break;
        };
        let close = open + 1 + close_relative;
        if let Some(parameter) = parse_razor_route_parameter_inner(&route[open + 1..close]) {
            parameters.push(parameter);
        }
        search_start = close + 1;
    }
    parameters
}

fn parse_razor_route_parameter_inner(inner: &str) -> Option<RazorRouteParameter> {
    let mut remainder = inner.trim();
    let catch_all = remainder.starts_with('*');
    if catch_all {
        remainder = remainder.trim_start_matches('*');
    }
    let optional = remainder.ends_with('?');
    if optional {
        remainder = &remainder[..remainder.len() - 1];
    }
    let (name, constraint) = if let Some(colon) = remainder.find(':') {
        (
            remainder[..colon].trim(),
            Some(remainder[colon + 1..].trim().to_string()),
        )
    } else {
        (remainder.trim(), None)
    };
    if name.is_empty() {
        return None;
    }

    Some(RazorRouteParameter {
        name: name.to_string(),
        constraint: constraint.filter(|value| !value.is_empty()),
        optional,
        catch_all,
    })
}

fn razor_route_parameter_value(parameter: RazorRouteParameter) -> Value {
    let mut fields = serde_json::Map::new();
    fields.insert("name".to_string(), Value::String(parameter.name));
    fields.insert("optional".to_string(), Value::Bool(parameter.optional));
    fields.insert("catch_all".to_string(), Value::Bool(parameter.catch_all));
    if let Some(constraint) = parameter.constraint {
        fields.insert("constraint".to_string(), Value::String(constraint));
    }
    Value::Object(fields)
}

fn razor_code_block_fact(file_path: &str, content: &str, node: Node<'_>) -> Option<StructuralFact> {
    let text = node_text(content, node)?;
    let block_type = if text.contains("@code") {
        "code"
    } else if text.contains("@functions") {
        "functions"
    } else {
        return None;
    };

    let mut metadata = base_metadata("component_code", "razor");
    insert_string(&mut metadata, "block_type", block_type);

    Some(fact_for_node(
        file_path,
        "razor",
        RAZOR_CODE_BLOCK_PATTERN_ID,
        "code_block",
        node,
        metadata,
    ))
}

fn razor_template_expression_fact(
    file_path: &str,
    content: &str,
    node: Node<'_>,
) -> Option<StructuralFact> {
    let expression = node_text(content, node)?
        .trim()
        .trim_start_matches('@')
        .trim()
        .to_string();
    if expression.is_empty() {
        return None;
    }

    let mut metadata = base_metadata("component_template", "razor");
    insert_string(&mut metadata, "expression", &expression);
    metadata.insert(
        "implicit".to_string(),
        Value::Bool(node.kind() == "razor_implicit_expression"),
    );

    Some(fact_for_node(
        file_path,
        "razor",
        RAZOR_TEMPLATE_EXPRESSION_PATTERN_ID,
        "template_expression",
        node,
        metadata,
    ))
}
fn razor_child_text<'a>(node: Node<'_>, content: &'a str, child_kind: &str) -> Option<&'a str> {
    razor_child_text_at_depth(node, content, child_kind, 0)
}

fn razor_child_text_at_depth<'a>(
    node: Node<'_>,
    content: &'a str,
    child_kind: &str,
    depth: u32,
) -> Option<&'a str> {
    if !should_visit_tree_depth(depth) {
        return None;
    }
    let child_depth = child_tree_depth(depth)?;
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == child_kind {
            return node_text(content, child);
        }
        if let Some(text) = razor_child_text_at_depth(child, content, child_kind, child_depth) {
            return Some(text);
        }
    }
    None
}
