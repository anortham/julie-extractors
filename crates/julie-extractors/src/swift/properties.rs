use crate::base::{Symbol, SymbolKind, SymbolOptions};
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};
use std::collections::HashMap;
use tree_sitter::Node;

use super::signatures::return_type_node;
use super::{SwiftExtractor, set_body, type_facts};

/// Extracts Swift properties, variables, and subscripts
impl SwiftExtractor {
    /// One symbol per bound name: `var x, y: Double` and `let (head, tail)`
    /// declare two each. The first name spans the whole declaration so its
    /// accessors and initializer belong to it; later names span their pattern.
    pub(super) fn extract_property(&mut self, node: Node, parent_id: Option<&str>) -> Vec<Symbol> {
        let modifiers = self.extract_modifiers(node);
        let annotations = self.extract_annotations(node);
        let keyword = node
            .children(&mut node.walk())
            .find(|c| c.kind() == "value_binding_pattern")
            .and_then(|pattern| pattern.child_by_field_name("mutability"))
            .map(|n| self.base.get_node_text(&n))
            .unwrap_or_else(|| "var".to_string());
        let prefix: Vec<&str> = modifiers
            .iter()
            .map(String::as_str)
            .filter(|m| super::signatures::explicit_access_level(&[m.to_string()]).is_none())
            .chain([keyword.as_str()])
            .collect();
        let prefix = prefix.join(" ");
        let kind = if type_facts::nearest_callable_ancestor(node) {
            SymbolKind::Variable
        } else {
            SymbolKind::Property
        };
        let doc_comment = self.base.find_doc_comment(&node);

        let children: Vec<Node> = node.children(&mut node.walk()).collect();
        let name_indexes: Vec<usize> = (0..children.len())
            .filter(|&i| node.field_name_for_child(i as u32) == Some("name"))
            .collect();
        let mut symbols = Vec::new();
        for (position, &index) in name_indexes.iter().enumerate() {
            let next_name = name_indexes
                .get(position + 1)
                .copied()
                .unwrap_or(children.len());
            let type_node = children[index + 1..]
                .iter()
                .find(|child| child.kind() == "type_annotation")
                .and_then(|annotation| {
                    annotation
                        .child_by_field_name("name")
                        .or_else(|| annotation.named_child(0))
                });
            let value = (index + 1..next_name)
                .find(|&i| node.field_name_for_child(i as u32) == Some("value"))
                .map(|i| children[i]);
            for (name_node, anchor) in bound_names(children[index]) {
                let name = self.base.get_node_text(&name_node);
                let property_type = type_node.map(|t| self.base.get_node_text(&t));
                let mut signature = format!("{prefix} {name}");
                if let Some(ref property_type) = property_type {
                    signature.push_str(&format!(": {property_type}"));
                }
                let mut metadata = HashMap::from([
                    (
                        "type".to_string(),
                        serde_json::Value::String("property".to_string()),
                    ),
                    (
                        "modifiers".to_string(),
                        serde_json::Value::String(modifiers.join(", ")),
                    ),
                    (
                        "keyword".to_string(),
                        serde_json::Value::String(keyword.clone()),
                    ),
                ]);
                if let Some(property_type) = property_type {
                    metadata.insert(
                        "propertyType".to_string(),
                        serde_json::Value::String(property_type),
                    );
                }
                if let Some(keys) = self.annotation_keys_csv(&annotations) {
                    metadata.insert(
                        "annotationKeys".to_string(),
                        serde_json::Value::String(keys),
                    );
                }
                let first = symbols.is_empty();
                let anchor = if first { node } else { anchor };
                let mut symbol = self.base.create_symbol(
                    &anchor,
                    name,
                    kind.clone(),
                    SymbolOptions {
                        signature: Some(signature),
                        visibility: Some(self.determine_visibility(&modifiers)),
                        parent_id: parent_id.map(|s| s.to_string()),
                        metadata: Some(metadata),
                        doc_comment: if first { doc_comment.clone() } else { None },
                        annotations: annotations.clone(),
                    },
                );
                let body = if first {
                    property_body(node, value)
                } else {
                    None
                };
                set_body(&self.base, &mut symbol, body);
                if !first {
                    symbol.doc_comment = None;
                }
                if let Some(type_node) = type_node {
                    type_facts::record_declared_type(&mut self.base, &symbol.id, type_node);
                } else if let Some(value) = value {
                    type_facts::record_same_file_constructor(
                        &mut self.base,
                        &symbol.id,
                        value,
                        &self.same_file_type_names,
                    );
                }
                symbols.push(symbol);
            }
        }
        symbols
    }

    /// Implementation of extractSubscript method
    pub(super) fn extract_subscript(&mut self, node: Node, parent_id: Option<&str>) -> Symbol {
        let name = "subscript".to_string();
        let parameters = self
            .extract_parameters(node)
            .unwrap_or_else(|| "()".to_string());
        let return_type = self.extract_return_type(node);
        let modifiers = self.extract_modifiers(node);

        let mut signature = "subscript".to_string();

        if !modifiers.is_empty() {
            signature = format!("{} {}", modifiers.join(" "), signature);
        }

        signature.push_str(&parameters);

        if let Some(ref return_type) = return_type {
            signature.push_str(&format!(" -> {}", return_type));
        }

        if let Some(accessor_reqs) = node.children(&mut node.walk()).find(|c| {
            c.kind() == "getter_setter_block" || c.kind() == "protocol_property_requirements"
        }) {
            signature.push_str(&format!(" {}", self.base.get_node_text(&accessor_reqs)));
        }

        let mut metadata = HashMap::from([
            (
                "type".to_string(),
                serde_json::Value::String("subscript".to_string()),
            ),
            (
                "parameters".to_string(),
                serde_json::Value::String(parameters),
            ),
            (
                "modifiers".to_string(),
                serde_json::Value::String(modifiers.join(", ")),
            ),
        ]);
        if let Some(return_type) = return_type {
            metadata.insert(
                "returnType".to_string(),
                serde_json::Value::String(return_type),
            );
        }

        let doc_comment = self.base.find_doc_comment(&node);

        let symbol = self.base.create_symbol(
            &node,
            name,
            SymbolKind::Method,
            SymbolOptions {
                signature: Some(signature),
                visibility: Some(self.determine_visibility(&modifiers)),
                parent_id: parent_id.map(|s| s.to_string()),
                metadata: Some(metadata),
                doc_comment,
                annotations: Vec::new(),
            },
        );
        if let Some(type_node) = return_type_node(node) {
            type_facts::record_declared_type(&mut self.base, &symbol.id, type_node);
        }
        symbol
    }
}

/// Each name a binding pattern declares, with the pattern that holds it.
fn bound_names(pattern: Node) -> Vec<(Node, Node)> {
    let mut names = Vec::new();
    collect_bound_names(pattern, 0, &mut names);
    names
}

fn collect_bound_names<'a>(pattern: Node<'a>, depth: u32, names: &mut Vec<(Node<'a>, Node<'a>)>) {
    if !should_visit_tree_depth(depth) {
        return;
    }
    if let Some(name) = pattern.child_by_field_name("bound_identifier") {
        names.push((name, pattern));
        return;
    }
    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };
    let mut cursor = pattern.walk();
    for child in pattern.named_children(&mut cursor) {
        match child.kind() {
            "simple_identifier" => names.push((child, pattern)),
            "pattern" => collect_bound_names(child, child_depth, names),
            _ => {}
        }
    }
}

/// The code a property runs: its accessors, its observers, or the closure a
/// lazy initializer calls. A stored property without them has no body.
fn property_body<'a>(node: Node<'a>, value: Option<Node<'a>>) -> Option<Node<'a>> {
    node.child_by_field_name("computed_value")
        .or_else(|| {
            node.children(&mut node.walk())
                .find(|child| child.kind() == "willset_didset_block")
        })
        .or_else(|| {
            value.filter(|value| {
                value.kind() == "lambda_literal"
                    || (value.kind() == "call_expression"
                        && value
                            .named_child(0)
                            .is_some_and(|callee| callee.kind() == "lambda_literal"))
            })
        })
}
