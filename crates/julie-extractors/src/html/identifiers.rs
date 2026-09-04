use crate::base::config_literals::{enclosing_element_tag_name, tag_attribute_carrier};
use crate::base::{BaseExtractor, ContainingSymbolIndex, IdentifierKind};
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};
use tree_sitter::Node;

/// Identifier extraction for LSP find_references functionality
pub(super) struct IdentifierExtractor;

impl IdentifierExtractor {
    /// Extract all identifier usages from HTML tree
    pub(super) fn extract_identifiers(
        base: &mut BaseExtractor,
        node: Node,
        containing_symbols: &ContainingSymbolIndex<'_>,
    ) {
        Self::extract_identifiers_at_depth(base, node, containing_symbols, 0);
    }

    fn extract_identifiers_at_depth(
        base: &mut BaseExtractor,
        node: Node,
        containing_symbols: &ContainingSymbolIndex<'_>,
        depth: u32,
    ) {
        if !should_visit_tree_depth(depth) {
            return;
        }

        // Extract identifier from this node if applicable
        Self::extract_identifier_from_node(base, node, containing_symbols);

        // Recursively walk children
        let Some(child_depth) = child_tree_depth(depth) else {
            return;
        };
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            Self::extract_identifiers_at_depth(base, child, containing_symbols, child_depth);
        }
    }

    /// Extract identifier from a single node based on its kind
    fn extract_identifier_from_node(
        base: &mut BaseExtractor,
        node: Node,
        containing_symbols: &ContainingSymbolIndex<'_>,
    ) {
        match node.kind() {
            // HTML attributes: onclick, data-action (as "calls"), id, class (as "member access")
            "attribute" => {
                let mut cursor = node.walk();
                let mut attr_name = None;
                let mut attr_value = None;
                let mut attr_value_node = None;

                for child in node.children(&mut cursor) {
                    match child.kind() {
                        "attribute_name" => {
                            attr_name = Some(base.get_node_text(&child));
                        }
                        "attribute_value" | "quoted_attribute_value" => {
                            let text = base.get_node_text(&child);
                            attr_value =
                                Some(text.trim_matches(|c| c == '"' || c == '\'').to_string());
                            attr_value_node = Some(child);
                        }
                        _ => {}
                    }
                }

                if let (Some(name), Some(value), Some(value_node)) =
                    (&attr_name, &attr_value, attr_value_node)
                    && !value.is_empty()
                {
                    let containing_symbol_id =
                        Self::find_containing_symbol_id(node, containing_symbols);
                    let tag_name = enclosing_element_tag_name(&base.content, node)
                        .map(|tag_name| tag_name.to_ascii_lowercase())
                        .unwrap_or_else(|| "element".to_string());
                    let carrier = tag_attribute_carrier(&tag_name, name);
                    base.record_literal(
                        &value_node,
                        value.clone(),
                        Some(carrier),
                        0,
                        containing_symbol_id,
                    );
                }

                if let (Some(name), Some(value)) = (attr_name, attr_value) {
                    // Event handlers and data-action attributes are "calls"
                    if name.starts_with("on") || name.starts_with("data-action") {
                        let containing_symbol_id =
                            Self::find_containing_symbol_id(node, containing_symbols);

                        base.create_identifier(
                            &node,
                            value,
                            IdentifierKind::Call,
                            containing_symbol_id,
                        );
                    }
                    // id and class attributes are "member access"
                    else if name == "id" || name == "class" {
                        // For class, split by spaces and extract each class name
                        if name == "class" {
                            for class_name in value.split_whitespace() {
                                let containing_symbol_id =
                                    Self::find_containing_symbol_id(node, containing_symbols);

                                base.create_identifier(
                                    &node,
                                    class_name.to_string(),
                                    IdentifierKind::MemberAccess,
                                    containing_symbol_id,
                                );
                            }
                        } else {
                            // id attribute
                            let containing_symbol_id =
                                Self::find_containing_symbol_id(node, containing_symbols);

                            base.create_identifier(
                                &node,
                                value,
                                IdentifierKind::MemberAccess,
                                containing_symbol_id,
                            );
                        }
                    }
                }
            }

            _ => {
                // Skip other node types for now
                // Future: custom element names, template references, etc.
            }
        }
    }

    /// Find the ID of the symbol that contains this node
    fn find_containing_symbol_id(
        node: Node,
        containing_symbols: &ContainingSymbolIndex<'_>,
    ) -> Option<String> {
        containing_symbols.find(node).map(|s| s.id.clone())
    }
}
