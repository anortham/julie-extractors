use tree_sitter::Node;

use super::extractor::BaseExtractor;

/// Tree-sitter node kinds that carry scalar configuration string values.
pub fn is_config_string_value_node(kind: &str) -> bool {
    matches!(
        kind,
        "string" | "double_quote_scalar" | "single_quote_scalar" | "plain_scalar"
    )
}

/// Record a configuration scalar string as a literal with the property key as carrier.
pub fn record_config_string_literal(
    base: &mut BaseExtractor,
    value_node: &Node,
    key_name: &str,
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
        Some(key_name.to_string()),
        0,
        containing_symbol_id,
    );
}
