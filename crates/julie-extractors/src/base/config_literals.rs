use std::collections::HashMap;

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

/// Positions of a config extractor's symbols by id, so a key path costs one
/// lookup per ancestor instead of a scan of every symbol.
#[derive(Default)]
pub(crate) struct ConfigKeyIndex {
    indexed: usize,
    positions: HashMap<String, usize>,
}

impl ConfigKeyIndex {
    /// The dotted config key path of `key` under the symbol `parent_id`: the
    /// names on the parent chain, then `key`. `symbols` must only grow
    /// between calls; a lookup that misses scans `symbols`.
    pub(crate) fn carrier(
        &mut self,
        symbols: &[Symbol],
        parent_id: Option<&str>,
        key: &str,
    ) -> String {
        if self.indexed > symbols.len() {
            *self = Self::default();
        }
        for (position, symbol) in symbols.iter().enumerate().skip(self.indexed) {
            self.positions.entry(symbol.id.clone()).or_insert(position);
        }
        self.indexed = symbols.len();

        let mut segments = Vec::new();
        let mut current = parent_id;
        while let Some(id) = current {
            let indexed = self
                .positions
                .get(id)
                .and_then(|&position| symbols.get(position))
                .filter(|symbol| symbol.id == id);
            let Some(symbol) = indexed.or_else(|| symbols.iter().find(|symbol| symbol.id == id))
            else {
                break;
            };
            segments.push(symbol.name.as_str());
            current = symbol.parent_id.as_deref();
        }
        segments.reverse();
        segments.push(key);
        segments
            .iter()
            .enumerate()
            .fold(String::new(), |mut carrier, (index, segment)| {
                if index > 0 && !segment.starts_with('[') {
                    carrier.push('.');
                }
                carrier.push_str(segment);
                carrier
            })
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::base::SymbolKind;

    fn symbol(id: &str, name: &str, parent_id: Option<&str>) -> Symbol {
        Symbol {
            id: id.to_string(),
            name: name.to_string(),
            kind: SymbolKind::Module,
            language: "yaml".to_string(),
            file_path: "config.yaml".to_string(),
            start_line: 1,
            start_column: 0,
            end_line: 1,
            end_column: 0,
            start_byte: 0,
            end_byte: 0,
            body_span: None,
            body_hash: None,
            signature: None,
            doc_comment: None,
            visibility: None,
            parent_id: parent_id.map(str::to_string),
            metadata: None,
            annotations: Vec::new(),
            semantic_group: None,
            confidence: None,
            content_type: None,
        }
    }

    #[test]
    fn carrier_joins_the_parent_chain_names_and_the_key() {
        let mut index = ConfigKeyIndex::default();
        let mut symbols = vec![symbol("services", "services", None)];
        assert_eq!(index.carrier(&symbols, None, "version"), "version");
        symbols.push(symbol("item", "[0]", Some("services")));
        symbols.push(symbol("web", "web", Some("item")));
        assert_eq!(
            index.carrier(&symbols, Some("web"), "image"),
            "services[0].web.image"
        );
        assert_eq!(index.carrier(&symbols, Some("missing"), "port"), "port");
    }

    #[test]
    fn carrier_reads_a_replaced_symbol_list_by_scanning() {
        let mut index = ConfigKeyIndex::default();
        let first = vec![symbol("a", "alpha", None), symbol("b", "beta", Some("a"))];
        assert_eq!(index.carrier(&first, Some("b"), "key"), "alpha.beta.key");
        let replaced = vec![symbol("b", "gamma", None), symbol("a", "delta", Some("b"))];
        assert_eq!(
            index.carrier(&replaced, Some("a"), "key"),
            "gamma.delta.key"
        );
        let shorter = vec![symbol("c", "only", None)];
        assert_eq!(index.carrier(&shorter, Some("c"), "key"), "only.key");
    }
}
