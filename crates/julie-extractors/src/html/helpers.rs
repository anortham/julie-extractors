use crate::base::{BaseExtractor, NormalizedSpan};
use std::collections::HashMap;
use tree_sitter::Node;

/// HTML-specific helper utilities
pub(super) struct HTMLHelpers;

impl HTMLHelpers {
    /// Extract tag name from HTML element node
    pub(super) fn extract_tag_name(base: &BaseExtractor, node: Node) -> Option<String> {
        // Look for start_tag or self_closing_tag child and extract tag name
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if matches!(child.kind(), "start_tag" | "self_closing_tag") {
                let mut inner_cursor = child.walk();
                for inner_child in child.children(&mut inner_cursor) {
                    if inner_child.kind() == "tag_name" {
                        return Some(normalize_tag_name(base.get_node_text(&inner_child)));
                    }
                }
            }
        }

        // Fallback: look for any tag_name child
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "tag_name" {
                return Some(normalize_tag_name(base.get_node_text(&child)));
            }
        }

        None
    }

    /// Extract all attributes from an HTML element
    pub(super) fn extract_attributes(base: &BaseExtractor, node: Node) -> HashMap<String, String> {
        let mut attributes = HashMap::new();

        // Find the tag container (start_tag or self_closing_tag)
        let mut cursor = node.walk();
        let tag_container = node
            .children(&mut cursor)
            .find(|c| matches!(c.kind(), "start_tag" | "self_closing_tag"))
            .unwrap_or(node);

        let mut tag_cursor = tag_container.walk();
        for child in tag_container.children(&mut tag_cursor) {
            if child.kind() == "attribute"
                && let (Some(attr_name), attr_value) = extract_attribute_name_value(base, child)
            {
                attributes.insert(attr_name, attr_value.unwrap_or_default());
            }
        }

        attributes
    }

    /// Extract text content from script or style elements
    pub(super) fn extract_text_content(base: &BaseExtractor, node: Node) -> Option<String> {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if matches!(child.kind(), "text" | "raw_text") {
                let text = base.get_node_text(&child).trim().to_string();
                return if text.is_empty() { None } else { Some(text) };
            }
        }
        None
    }

    /// Extract text content from HTML elements
    pub(super) fn extract_element_text_content(base: &BaseExtractor, node: Node) -> Option<String> {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "text" {
                let text = base.get_node_text(&child).trim().to_string();
                return if text.is_empty() { None } else { Some(text) };
            }
        }
        None
    }
}

/// The content between an element's start and end tags. Void and
/// self-closing elements, and elements with no content, have none.
pub(super) fn element_body_span(base: &BaseExtractor, node: Node) -> Option<NormalizedSpan> {
    let mut cursor = node.walk();
    let children: Vec<Node> = node.children(&mut cursor).collect();
    let start_tag = children.iter().find(|child| child.kind() == "start_tag")?;
    let end_tag = children.iter().find(|child| child.kind() == "end_tag")?;
    (start_tag.end_byte() < end_tag.start_byte())
        .then(|| base.span_for_byte_range(start_tag.end_byte(), end_tag.start_byte()))
        .flatten()
}

/// HTML tag and attribute names are ASCII case-insensitive, so `<DIV>` is
/// `div`. A name with any lowercase letter keeps its spelling: SVG uses
/// camelCase names such as `linearGradient` and `viewBox`.
pub(super) fn normalize_tag_name(name: String) -> String {
    if name.bytes().any(|byte| byte.is_ascii_lowercase()) {
        name
    } else {
        name.to_ascii_lowercase()
    }
}

/// The normalized attribute name and its unquoted value; `""` for a bare
/// attribute such as `disabled`.
pub(super) fn extract_attribute_name_value(
    base: &BaseExtractor,
    attr_node: Node,
) -> (Option<String>, Option<String>) {
    let mut name = None;
    let mut value = None;

    let mut cursor = attr_node.walk();
    for child in attr_node.children(&mut cursor) {
        match child.kind() {
            "attribute_name" => {
                name = Some(normalize_tag_name(base.get_node_text(&child)));
            }
            "attribute_value" => value = Some(base.get_node_text(&child)),
            "quoted_attribute_value" => {
                let mut inner = child.walk();
                value = Some(
                    child
                        .named_children(&mut inner)
                        .find(|part| part.kind() == "attribute_value")
                        .map(|part| base.get_node_text(&part))
                        .unwrap_or_default(),
                );
            }
            _ => {}
        }
    }

    (name, value)
}
