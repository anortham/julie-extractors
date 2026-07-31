use std::collections::HashMap;

use serde_json::Value;
use tree_sitter::Node;

use crate::base::{BaseExtractor, Symbol, SymbolKind, SymbolOptions, Visibility};

/// Attributes that promote an element to a symbol, in priority order.
const NAME_ATTRIBUTES: [&str; 2] = ["name", "id"];

/// The start tag of an element: `STag` for `<a>…</a>`, `EmptyElemTag` for `<a/>`.
pub(super) fn tag_node<'tree>(element: Node<'tree>) -> Option<Node<'tree>> {
    let mut cursor = element.walk();
    element
        .children(&mut cursor)
        .find(|child| matches!(child.kind(), "STag" | "EmptyElemTag"))
}

pub(super) fn tag_name(base: &BaseExtractor, tag: Node<'_>) -> Option<String> {
    let mut cursor = tag.walk();
    tag.children(&mut cursor)
        .find(|child| child.kind() == "Name")
        .map(|name| base.get_node_text(&name))
}

/// Attribute name paired with its `AttValue` node, in source order.
pub(super) fn attributes<'tree>(
    base: &BaseExtractor,
    tag: Node<'tree>,
) -> Vec<(String, Node<'tree>)> {
    let mut cursor = tag.walk();
    let mut attributes = Vec::new();

    for child in tag.children(&mut cursor) {
        if child.kind() != "Attribute" {
            continue;
        }

        let mut attribute_cursor = child.walk();
        let mut name = None;
        let mut value = None;
        for part in child.children(&mut attribute_cursor) {
            match part.kind() {
                "Name" if name.is_none() => name = Some(base.get_node_text(&part)),
                "AttValue" => value = Some(part),
                _ => {}
            }
        }

        if let (Some(name), Some(value)) = (name, value) {
            attributes.push((name, value));
        }
    }

    attributes
}

pub(super) fn attribute_value(base: &BaseExtractor, value: Node<'_>) -> String {
    let text = base.get_node_text(&value);
    let unquoted = text.strip_prefix(['"', '\'']).unwrap_or(&text);
    let unquoted = unquoted.strip_suffix(['"', '\'']).unwrap_or(unquoted);
    unquoted.to_string()
}

/// `xsi:type` and `type` name the same attribute for matching purposes. The prefix is
/// dropped only to recognise the attribute; recorded values keep their prefix, because
/// v1 does no namespace resolution.
pub(super) fn local_name(name: &str) -> &str {
    name.rsplit(':').next().unwrap_or(name)
}

pub(super) fn extract_element(
    base: &mut BaseExtractor,
    element: Node<'_>,
    parent_id: Option<&str>,
) -> Option<Symbol> {
    let tag = tag_node(element)?;
    extract_from_tag(base, element, tag, parent_id, has_child_element(element))
}

/// A start tag stranded in an ERROR region — an unclosed element — still names a
/// component. Recovering it keeps one missing end tag from collapsing the whole
/// document to zero symbols.
pub(super) fn extract_orphan_tag(
    base: &mut BaseExtractor,
    tag: Node<'_>,
    parent_id: Option<&str>,
) -> Option<Symbol> {
    extract_from_tag(base, tag, tag, parent_id, false)
}

fn extract_from_tag(
    base: &mut BaseExtractor,
    span_node: Node<'_>,
    tag: Node<'_>,
    parent_id: Option<&str>,
    has_child_elements: bool,
) -> Option<Symbol> {
    let tag_name = tag_name(base, tag)?;
    let attributes = attributes(base, tag);
    let (name_attribute, name) = promoted_name(base, &attributes)?;
    let signature = collapse_whitespace(&base.get_node_text(&tag));

    let kind = if has_child_elements {
        SymbolKind::Module
    } else {
        SymbolKind::Variable
    };

    let mut metadata = HashMap::new();
    metadata.insert("tag".to_string(), Value::String(tag_name));
    metadata.insert("name_attribute".to_string(), Value::String(name_attribute));

    Some(base.create_symbol(
        &span_node,
        name,
        kind,
        SymbolOptions {
            signature: Some(signature),
            visibility: Some(Visibility::Public),
            parent_id: parent_id.map(str::to_string),
            metadata: Some(metadata),
            doc_comment: None,
            annotations: Vec::new(),
        },
    ))
}

pub(super) fn is_orphan_tag(node: Node<'_>) -> bool {
    matches!(node.kind(), "STag" | "EmptyElemTag")
        && node.parent().map(|parent| parent.kind()) != Some("element")
}

fn promoted_name(
    base: &BaseExtractor,
    attributes: &[(String, Node<'_>)],
) -> Option<(String, String)> {
    for candidate in NAME_ATTRIBUTES {
        for (name, value) in attributes {
            if local_name(name) != candidate {
                continue;
            }
            let value = attribute_value(base, *value);
            if value.trim().is_empty() {
                continue;
            }
            return Some((candidate.to_string(), value));
        }
    }

    None
}

fn has_child_element(element: Node<'_>) -> bool {
    let mut cursor = element.walk();
    element.children(&mut cursor).any(|child| {
        child.kind() == "content" && {
            let mut content_cursor = child.walk();
            child
                .children(&mut content_cursor)
                .any(|grandchild| grandchild.kind() == "element")
        }
    })
}

fn collapse_whitespace(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}
