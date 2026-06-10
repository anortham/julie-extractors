use tree_sitter::Node;

use super::extractor::BaseExtractor;
use super::types::Symbol;

/// Stable `tag.attribute` carrier for markup attribute value literals.
pub fn tag_attribute_carrier(tag_name: &str, attribute_name: &str) -> String {
    format!(
        "{}.{}",
        tag_name.trim(),
        attribute_name.trim().to_ascii_lowercase()
    )
}

/// Resolve the enclosing HTML/Vue element tag for an attribute or child node.
pub fn enclosing_element_tag_name(content: &str, node: Node<'_>) -> Option<String> {
    let mut current = node.parent();
    while let Some(parent) = current {
        match parent.kind() {
            "script_element" => return Some("script".to_string()),
            "style_element" => return Some("style".to_string()),
            "element" | "self_closing_element" => {
                if let Some(tag) = tag_name_from_element_node(content, parent) {
                    return Some(tag);
                }
            }
            _ => {}
        }
        current = parent.parent();
    }
    None
}

fn tag_name_from_element_node(content: &str, node: Node<'_>) -> Option<String> {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if matches!(child.kind(), "start_tag" | "self_closing_tag") {
            let mut tag_cursor = child.walk();
            for tag_child in child.children(&mut tag_cursor) {
                if tag_child.kind() == "tag_name" {
                    return node_text(content, tag_child).map(str::to_string);
                }
            }
        }
        if child.kind() == "tag_name" {
            return node_text(content, child).map(str::to_string);
        }
    }
    None
}

fn node_text<'a>(content: &'a str, node: Node<'_>) -> Option<&'a str> {
    content.get(node.start_byte()..node.end_byte())
}

/// Tree-sitter node kinds that carry scalar configuration string values.
pub fn is_config_string_value_node(kind: &str) -> bool {
    matches!(
        kind,
        "string" | "double_quote_scalar" | "single_quote_scalar" | "plain_scalar"
    )
}

/// Build a dotted config key path from the parent symbol chain and local key.
pub fn build_config_key_carrier(symbols: &[Symbol], parent_id: Option<&str>, key: &str) -> String {
    let mut segments = Vec::new();
    let mut current = parent_id;
    while let Some(id) = current {
        let Some(symbol) = symbols.iter().find(|symbol| symbol.id == id) else {
            break;
        };
        segments.push(symbol.name.clone());
        current = symbol.parent_id.as_deref();
    }
    segments.reverse();
    segments.push(key.to_string());
    segments.join(".")
}

/// Record a configuration scalar string as a literal with path-aware carrier.
pub fn record_config_string_literal(
    base: &mut BaseExtractor,
    value_node: &Node,
    carrier: &str,
    containing_symbol_id: Option<String>,
) {
    let literal_text = match value_node.kind() {
        "double_quote_scalar" | "single_quote_scalar" => {
            let raw = base.get_node_text(value_node);
            raw.trim().trim_matches('"').trim_matches('\'').to_string()
        }
        _ => base
            .decode_string_literal(value_node)
            .unwrap_or_else(|| base.get_node_text(value_node).trim().to_string()),
    };
    if literal_text.is_empty() {
        return;
    }
    base.record_literal(
        value_node,
        literal_text,
        Some(carrier.to_string()),
        0,
        containing_symbol_id,
    );
}
