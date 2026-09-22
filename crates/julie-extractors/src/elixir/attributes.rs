/// Module attribute extraction for Elixir.
///
/// Handles @type, @typep, @opaque, @callback, @spec, @behaviour, @moduledoc, @doc.
/// In tree-sitter-elixir, attributes parse as `unary_operator` with `@` operator.
use super::ElixirExtractor;
use crate::base::{
    BaseExtractor, Symbol, SymbolKind, SymbolOptions, Visibility, find_child_by_type,
    normalize_annotations,
};
use serde_json::Value;
use std::collections::HashMap;
use tree_sitter::Node;

/// Extract a module attribute from a unary_operator node with `@` operator.
pub(super) fn extract_attribute(
    extractor: &mut ElixirExtractor,
    node: &Node,
    parent_id: Option<&str>,
) -> Option<Symbol> {
    // Verify this is an @ operator
    let operator = node.child_by_field_name("operator")?;
    if extractor.base.get_node_text(&operator) != "@" {
        return None;
    }

    let operand = node.child_by_field_name("operand")?;

    match operand.kind() {
        "call" => extract_attribute_call(extractor, node, &operand, parent_id),
        _ => None,
    }
}

fn extract_attribute_call(
    extractor: &mut ElixirExtractor,
    attr_node: &Node,
    call_node: &Node,
    parent_id: Option<&str>,
) -> Option<Symbol> {
    let target = call_node.child_by_field_name("target")?;
    let attr_name = extractor.base.get_node_text(&target);

    match attr_name.as_str() {
        "type" | "typep" | "opaque" => {
            extract_type_attribute(extractor, attr_node, call_node, parent_id, &attr_name)
        }
        "callback" => extract_callback_attribute(extractor, attr_node, call_node, parent_id),
        "spec" => {
            extract_spec_attribute(extractor, call_node);
            None // @spec doesn't create a standalone symbol
        }
        "behaviour" | "behavior" => {
            extract_behaviour_attribute(extractor, attr_node, call_node, parent_id)
        }
        "moduledoc" | "doc" => None,
        _ => None,
    }
}

fn extract_type_attribute(
    extractor: &mut ElixirExtractor,
    attr_node: &Node,
    call_node: &Node,
    parent_id: Option<&str>,
    attr_name: &str,
) -> Option<Symbol> {
    // NOTE: `arguments` is a child type, NOT a named field
    let args = find_child_by_type(call_node, "arguments")?;
    let type_text = extractor.base.get_node_text(&args);

    // Extract the type name (before ::)
    let type_name = type_text.split("::").next()?.trim().to_string();
    if type_name.is_empty() {
        return None;
    }

    let visibility = if attr_name == "typep" {
        Visibility::Private
    } else {
        Visibility::Public
    };

    let signature = format!("@{} {}", attr_name, extractor.base.get_node_text(call_node));
    let annotations = normalize_annotations(&[extractor.base.get_node_text(attr_node)], "elixir");

    let mut symbol = extractor.base.create_symbol(
        attr_node,
        type_name,
        SymbolKind::Type,
        SymbolOptions {
            signature: Some(signature),
            visibility: Some(visibility),
            parent_id: parent_id.map(String::from),
            metadata: None,
            doc_comment: extract_doc_comment_for_node(&extractor.base, attr_node, "typedoc"),
            annotations,
        },
    );
    let definition = args
        .named_child(0)
        .filter(|child| child.kind() == "binary_operator")
        .and_then(|child| child.child_by_field_name("right"));
    super::helpers::set_body_span(&extractor.base, &mut symbol, definition);
    Some(symbol)
}

fn extract_callback_attribute(
    extractor: &mut ElixirExtractor,
    attr_node: &Node,
    call_node: &Node,
    parent_id: Option<&str>,
) -> Option<Symbol> {
    let args = find_child_by_type(call_node, "arguments")?;
    let callback_text = extractor.base.get_node_text(&args);

    // Extract callback name (before `(` or `::`)
    let callback_name = callback_text.split(['(', ':']).next()?.trim().to_string();
    if callback_name.is_empty() {
        return None;
    }

    let signature = format!("@callback {}", extractor.base.get_node_text(call_node));
    let annotations = normalize_annotations(&[extractor.base.get_node_text(attr_node)], "elixir");

    let mut metadata = HashMap::new();
    metadata.insert("callback".to_string(), Value::Bool(true));

    let mut symbol = extractor.base.create_symbol(
        attr_node,
        callback_name,
        SymbolKind::Function,
        SymbolOptions {
            signature: Some(signature),
            visibility: Some(Visibility::Public),
            parent_id: parent_id.map(String::from),
            metadata: Some(metadata),
            doc_comment: extract_doc_comment_for_node(&extractor.base, attr_node, "doc"),
            annotations,
        },
    );
    super::helpers::set_body_span(&extractor.base, &mut symbol, None);
    Some(symbol)
}

fn extract_spec_attribute(extractor: &mut ElixirExtractor, call_node: &Node) {
    let Some(args) = find_child_by_type(call_node, "arguments") else {
        return;
    };
    let spec_text = extractor.base.get_node_text(&args);

    // Extract function name from spec text (before the `(`)
    let fn_name = spec_text
        .split('(')
        .next()
        .map(|s| s.trim().to_string())
        .unwrap_or_default();

    if !fn_name.is_empty() {
        // Store the return type (after ::)
        if let Some(return_type) = spec_text.split("::").last() {
            extractor
                .specs
                .insert(fn_name, return_type.trim().to_string());
        }
    }
}

fn extract_behaviour_attribute(
    extractor: &mut ElixirExtractor,
    attr_node: &Node,
    call_node: &Node,
    parent_id: Option<&str>,
) -> Option<Symbol> {
    let args = find_child_by_type(call_node, "arguments")?;
    let mut cursor = args.walk();
    for child in args.children(&mut cursor) {
        if child.kind() == "alias" {
            let behaviour_name = extractor.base.get_node_text(&child);
            let signature = format!("@behaviour {}", behaviour_name);
            let annotations =
                normalize_annotations(&[extractor.base.get_node_text(attr_node)], "elixir");
            return Some(extractor.base.create_symbol(
                attr_node,
                behaviour_name,
                SymbolKind::Import,
                SymbolOptions {
                    signature: Some(signature),
                    visibility: Some(Visibility::Public),
                    parent_id: parent_id.map(String::from),
                    metadata: None,
                    doc_comment: None,
                    annotations,
                },
            ));
        }
    }
    None
}

/// Attributes that document a declaration of their own. Walking back from a
/// definition stops at these, so a `@type` or `@moduledoc` never lends its
/// neighbouring `@doc` to the wrong declaration.
const DECLARING_ATTRIBUTES: &[&str] = &[
    "moduledoc",
    "type",
    "typep",
    "opaque",
    "callback",
    "macrocallback",
];

/// Documentation for a definition: the string of the nearest preceding
/// `doc_attr` attribute (`@doc`, `@typedoc`), looking past `@spec`, `@impl`
/// and other plain attributes. `@doc false` hides the definition and yields no
/// doc. Without the attribute, a directly preceding `#` comment block is used.
pub(super) fn extract_doc_comment_for_node(
    base: &BaseExtractor,
    node: &Node,
    doc_attr: &str,
) -> Option<String> {
    match preceding_attributes(base, node)
        .into_iter()
        .find(|(name, _)| name == doc_attr)
    {
        Some((_, attribute)) => attribute_string(base, &attribute),
        None => base.find_doc_comment(node),
    }
}

pub(super) fn extract_moduledoc_for_module(base: &BaseExtractor, node: &Node) -> Option<String> {
    for (name, attribute) in module_attributes(base, node) {
        if name != "moduledoc" {
            continue;
        }
        let value = attribute_value(&attribute);
        if value.is_some_and(|value| value.kind() == "boolean") {
            return None;
        }
        if let Some(doc) = attribute_string(base, &attribute) {
            return Some(doc);
        }
    }
    base.find_doc_comment(node)
}

pub(super) fn collect_preceding_annotations(
    base: &BaseExtractor,
    node: &Node,
    allowed_names: &[&str],
) -> Vec<String> {
    let mut annotations: Vec<String> = preceding_attributes(base, node)
        .into_iter()
        .filter(|(name, _)| allowed_names.contains(&name.as_str()))
        .map(|(_, attribute)| base.get_node_text(&attribute))
        .collect();
    annotations.reverse();
    annotations
}

pub(super) fn collect_module_annotations(base: &BaseExtractor, node: &Node) -> Vec<String> {
    module_attributes(base, node)
        .into_iter()
        .filter(|(name, _)| {
            matches!(
                name.as_str(),
                "moduledoc"
                    | "behaviour"
                    | "behavior"
                    | "derive"
                    | "external_resource"
                    | "before_compile"
                    | "after_compile"
            )
        })
        .map(|(_, attribute)| base.get_node_text(&attribute))
        .collect()
}

/// `@` attributes directly above `node`, nearest first. Comments are
/// transparent. The run ends at any other node, after the first `@doc` or
/// `@typedoc`, or before an attribute that declares something itself.
fn preceding_attributes<'a>(base: &BaseExtractor, node: &Node<'a>) -> Vec<(String, Node<'a>)> {
    let mut attributes = Vec::new();
    let mut current = node.prev_named_sibling();
    while let Some(sibling) = current {
        current = sibling.prev_named_sibling();
        if sibling.kind() == "comment" {
            continue;
        }
        let Some(name) = attribute_name(base, &sibling) else {
            break;
        };
        if DECLARING_ATTRIBUTES.contains(&name.as_str()) {
            break;
        }
        let ends_run = matches!(name.as_str(), "doc" | "typedoc");
        attributes.push((name, sibling));
        if ends_run {
            break;
        }
    }
    attributes
}

/// `@` attributes that are direct children of a module's `do` block, so a
/// nested module's attributes never reach its parent.
fn module_attributes<'a>(base: &BaseExtractor, node: &Node<'a>) -> Vec<(String, Node<'a>)> {
    let Some(do_block) = find_child_by_type(node, "do_block") else {
        return Vec::new();
    };
    let mut cursor = do_block.walk();
    do_block
        .named_children(&mut cursor)
        .filter_map(|child| attribute_name(base, &child).map(|name| (name, child)))
        .collect()
}

fn attribute_name(base: &BaseExtractor, node: &Node) -> Option<String> {
    if node.kind() != "unary_operator" {
        return None;
    }
    annotation_name_from_text(&base.get_node_text(node))
}

fn attribute_value<'a>(attribute: &Node<'a>) -> Option<Node<'a>> {
    let operand = attribute.child_by_field_name("operand")?;
    find_child_by_type(&operand, "arguments")?.named_child(0)
}

fn attribute_string(base: &BaseExtractor, attribute: &Node) -> Option<String> {
    super::helpers::string_literal_content(base, &attribute_value(attribute)?)
}

fn annotation_name_from_text(text: &str) -> Option<String> {
    let text = text.trim().strip_prefix('@')?.trim();
    let end = text
        .char_indices()
        .find(|(_, ch)| ch.is_whitespace() || *ch == '(')
        .map(|(index, _)| index)
        .unwrap_or(text.len());
    let name = text[..end].trim();
    if name.is_empty() {
        None
    } else {
        Some(name.to_ascii_lowercase())
    }
}
