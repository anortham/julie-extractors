//! Script held in attribute values: inline event handlers (`onclick`),
//! Alpine (`@click`, `x-on:*`, `x-data`, `x-init`, `x-effect`), htmx
//! (`hx-on*`), and Stimulus `data-action` descriptors.

use std::collections::HashMap;

use serde_json::Value;
use tree_sitter::{Node, Tree};

use crate::base::relationship_resolution::StructuredPendingRelationship;
use crate::base::{
    BaseExtractor, Identifier, IdentifierKind, Literal, NormalizedSpan, Relationship, Symbol,
    SymbolKind,
};
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};

/// Rows produced by attribute-held script, in host coordinates.
#[derive(Default)]
pub(super) struct HandlerRows {
    pub(super) identifiers: Vec<Identifier>,
    pub(super) literals: Vec<Literal>,
    pub(super) relationships: Vec<Relationship>,
    pub(super) pending: Vec<StructuredPendingRelationship>,
}

pub(super) fn collect_handler_rows(
    base: &BaseExtractor,
    tree: &Tree,
    symbols: &[Symbol],
) -> HandlerRows {
    let containing = base.containing_symbol_index(symbols);
    let functions = crate::embedded::unique_by_name(
        symbols
            .iter()
            .filter(|symbol| symbol.kind == SymbolKind::Function),
    );
    let mut rows = HandlerRows::default();
    let mut attributes = Vec::new();
    collect_attributes(tree.root_node(), &mut attributes, 0);
    for attribute in attributes {
        let Some((name, value)) = attribute_name_and_value(attribute) else {
            continue;
        };
        let name = base.get_node_text(&name).to_ascii_lowercase();
        let Some(caller) =
            owning_element_symbol(attribute, symbols).or_else(|| containing.find(attribute))
        else {
            continue;
        };
        if name == "data-action" {
            stimulus_identifiers(base, value, caller, &mut rows);
        } else if is_script_attribute(&name) {
            script_rows(base, value, caller, &functions, &mut rows);
        }
    }
    rows
}

/// The symbol of the element whose start tag holds the attribute, or of the
/// nearest enclosing element that has one.
fn owning_element_symbol<'s>(attribute: Node<'_>, symbols: &'s [Symbol]) -> Option<&'s Symbol> {
    let mut node = attribute.parent();
    while let Some(current) = node {
        if matches!(
            current.kind(),
            "element" | "script_element" | "style_element"
        ) && let Some(symbol) = symbols.iter().find(|symbol| {
            symbol.start_byte as usize == current.start_byte()
                && symbol.end_byte as usize == current.end_byte()
        }) {
            return Some(symbol);
        }
        node = current.parent();
    }
    None
}

fn collect_attributes<'t>(node: Node<'t>, attributes: &mut Vec<Node<'t>>, depth: u32) {
    if !should_visit_tree_depth(depth) {
        return;
    }
    if node.kind() == "attribute" {
        attributes.push(node);
        return;
    }
    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_attributes(child, attributes, child_depth);
    }
}

fn attribute_name_and_value(attribute: Node<'_>) -> Option<(Node<'_>, Node<'_>)> {
    let mut cursor = attribute.walk();
    let children: Vec<Node<'_>> = attribute.named_children(&mut cursor).collect();
    let name = children
        .iter()
        .find(|child| child.kind() == "attribute_name")?;
    let value = children.iter().find_map(|child| match child.kind() {
        "attribute_value" => Some(*child),
        "quoted_attribute_value" => {
            let mut cursor = child.walk();
            child
                .named_children(&mut cursor)
                .find(|inner| inner.kind() == "attribute_value")
        }
        _ => None,
    })?;
    Some((*name, value))
}

fn is_script_attribute(name: &str) -> bool {
    (name.len() > 2 && name.starts_with("on") && !name.contains(':'))
        || name.starts_with('@')
        || name.starts_with("x-on:")
        || matches!(name, "x-data" | "x-init" | "x-effect")
        || name.starts_with("hx-on")
}

fn script_rows(
    base: &BaseExtractor,
    value: Node<'_>,
    caller: &Symbol,
    functions: &HashMap<&str, Option<&Symbol>>,
    rows: &mut HandlerRows,
) {
    let Some((mut identifiers, literals)) = crate::embedded::extract_expression(
        &base.content,
        value.start_byte(),
        &base.get_node_text(&value),
        &base.file_path,
        true,
    ) else {
        return;
    };
    for identifier in &mut identifiers {
        identifier.containing_symbol_id = Some(caller.id.clone());
    }
    crate::embedded::link_expression_identifiers(
        &base.content,
        caller,
        &identifiers,
        functions,
        None,
        &mut rows.relationships,
        &mut rows.pending,
    );
    rows.identifiers.extend(identifiers);
    rows.literals
        .extend(literals.into_iter().map(|mut literal| {
            literal.containing_symbol_id = Some(caller.id.clone());
            literal
        }));
}

/// `click->hello#greet:prevent` becomes a call identifier `greet` with
/// `controller: hello`.
fn stimulus_identifiers(
    base: &BaseExtractor,
    value: Node<'_>,
    caller: &Symbol,
    rows: &mut HandlerRows,
) {
    let text = base.get_node_text(&value);
    let mut search_from = 0;
    for descriptor in text.split_whitespace() {
        let Some(relative) = text[search_from..].find(descriptor) else {
            continue;
        };
        let descriptor_start = search_from + relative;
        search_from = descriptor_start + descriptor.len();

        let target = descriptor
            .rsplit_once("->")
            .map_or(descriptor, |(_, target)| target);
        let target_start = descriptor_start + descriptor.len() - target.len();
        let (controller, method) = target.split_once('#').unwrap_or(("", target));
        let method = method.split(':').next().unwrap_or(method);
        if !crate::embedded::is_member_path(method) || method.contains('.') {
            continue;
        }
        let method_offset = if controller.is_empty() && !target.contains('#') {
            0
        } else {
            controller.len() + 1
        };
        let start = value.start_byte() + target_start + method_offset;
        let Some(span) =
            NormalizedSpan::from_content_range(&base.content, start, start + method.len())
        else {
            continue;
        };
        let mut metadata = HashMap::new();
        if !controller.is_empty() {
            metadata.insert(
                "controller".to_string(),
                Value::String(controller.to_string()),
            );
        }
        let mut identifier = Identifier {
            id: String::new(),
            name: method.to_string(),
            kind: IdentifierKind::Call,
            language: base.language.clone(),
            file_path: base.file_path.clone(),
            start_line: span.start_line,
            start_column: span.start_column,
            end_line: span.end_line,
            end_column: span.end_column,
            start_byte: span.start_byte,
            end_byte: span.end_byte,
            containing_symbol_id: Some(caller.id.clone()),
            target_symbol_id: None,
            confidence: 1.0,
            receiver_type: None,
            code_context: None,
            metadata: (!metadata.is_empty()).then_some(metadata),
        };
        identifier.refresh_id();
        rows.identifiers.push(identifier);
    }
}
