use crate::base::{Symbol, SymbolKind, SymbolOptions, Visibility};
use std::collections::HashMap;
use tree_sitter::Node;

use super::{SwiftExtractor, clear_body};

/// Extracts Swift enum cases and members
impl SwiftExtractor {
    /// Implementation of extractEnumCases method
    pub(super) fn extract_enum_cases(
        &mut self,
        node: Node,
        symbols: &mut Vec<Symbol>,
        parent_id: Option<&str>,
    ) {
        let declaration_annotations = self.extract_annotations(node);
        for child in node.children(&mut node.walk()) {
            if child.kind() == "enum_case_element" {
                let name_node = child
                    .children(&mut child.walk())
                    .find(|c| c.kind() == "pattern" || c.kind() == "type_identifier");
                if let Some(name_node) = name_node {
                    let name = self.base.get_node_text(&name_node);
                    let associated_values = child
                        .children(&mut child.walk())
                        .find(|c| c.kind() == "enum_case_parameters");

                    let mut signature = name.clone();
                    if let Some(associated_values) = associated_values {
                        signature.push_str(&self.base.get_node_text(&associated_values));
                    }

                    let mut metadata = HashMap::from([(
                        "type".to_string(),
                        serde_json::Value::String("enum-case".to_string()),
                    )]);
                    if let Some(keys) = self.annotation_keys_csv(&declaration_annotations) {
                        metadata.insert(
                            "annotationKeys".to_string(),
                            serde_json::Value::String(keys),
                        );
                    }

                    let symbol = self.base.create_symbol(
                        &child,
                        name,
                        SymbolKind::EnumMember,
                        SymbolOptions {
                            signature: Some(signature),
                            visibility: Some(Visibility::Internal),
                            parent_id: parent_id.map(|s| s.to_string()),
                            metadata: Some(metadata),
                            doc_comment: None,
                            annotations: declaration_annotations.clone(),
                        },
                    );
                    symbols.push(symbol);
                }
            }
        }
    }

    /// One enum member per name in a `case a = 1, b, c(Int)` entry. The first
    /// member spans the entry so it keeps the entry's doc comment; later
    /// members span their own name.
    pub(super) fn extract_enum_case(&mut self, node: Node, parent_id: Option<&str>) -> Vec<Symbol> {
        let annotations = self.extract_annotations(node);
        let doc_comment = self.base.find_doc_comment(&node);
        let children: Vec<Node> = node.children(&mut node.walk()).collect();
        let field = |index: usize| node.field_name_for_child(index as u32);
        let name_indexes: Vec<usize> = (0..children.len())
            .filter(|&index| field(index) == Some("name"))
            .collect();

        let mut symbols = Vec::new();
        for (position, &index) in name_indexes.iter().enumerate() {
            let next_name = name_indexes
                .get(position + 1)
                .copied()
                .unwrap_or(children.len());
            let name = self.base.get_node_text(&children[index]);
            let mut signature = name.clone();
            for (part, child) in children.iter().enumerate().take(next_name).skip(index + 1) {
                match field(part) {
                    Some("data_contents") => signature.push_str(&self.base.get_node_text(child)),
                    Some("raw_value") => {
                        signature.push_str(&format!(" = {}", self.base.get_node_text(child)))
                    }
                    _ => {}
                }
            }

            let mut metadata = HashMap::from([(
                "type".to_string(),
                serde_json::Value::String("enum-case".to_string()),
            )]);
            if let Some(keys) = self.annotation_keys_csv(&annotations) {
                metadata.insert(
                    "annotationKeys".to_string(),
                    serde_json::Value::String(keys),
                );
            }
            let first = symbols.is_empty();
            let anchor = if first { node } else { children[index] };
            let mut symbol = self.base.create_symbol(
                &anchor,
                name,
                SymbolKind::EnumMember,
                SymbolOptions {
                    signature: Some(signature),
                    visibility: Some(Visibility::Internal),
                    parent_id: parent_id.map(|s| s.to_string()),
                    metadata: Some(metadata),
                    doc_comment: if first { doc_comment.clone() } else { None },
                    annotations: annotations.clone(),
                },
            );
            clear_body(&mut symbol);
            if !first {
                symbol.doc_comment = None;
            }
            symbols.push(symbol);
        }
        symbols
    }
}
