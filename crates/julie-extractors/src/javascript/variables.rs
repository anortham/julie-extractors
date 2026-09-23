//! Variable and destructuring extraction for JavaScript
//!
//! Handles extraction of variable declarations, including destructuring
//! patterns for objects and arrays.

use crate::base::{Symbol, SymbolKind, SymbolOptions};
use serde_json::json;
use std::collections::HashMap;
use tree_sitter::Node;

impl super::JavaScriptExtractor {
    /// Extract variable declarations - direct Implementation of extractVariable
    pub(super) fn extract_variable(
        &mut self,
        node: Node,
        parent_id: Option<String>,
    ) -> Option<Symbol> {
        let name_node = node.child_by_field_name("name");
        let name = name_node.map(|n| self.base.get_node_text(&n))?;

        let value_node = node.child_by_field_name("value");
        let signature = self.build_variable_signature(&node, &name);

        // For variable_declarators, check the parent variable_declaration for JSDoc
        // Variable declarators receive comments on their parent declaration node
        let doc_node = if node.kind() == "variable_declarator" {
            node.parent().unwrap_or(node)
        } else {
            node
        };

        // Check if this is a CommonJS require statement (reference logic)
        if let Some(value) = &value_node {
            let required_member = (value.kind() == "member_expression")
                .then(|| value.child_by_field_name("object"))
                .flatten()
                .filter(|object| self.is_require_call(object))
                .zip(value.child_by_field_name("property"));
            let require_node = if self.is_require_call(value) {
                Some(*value)
            } else {
                required_member.map(|(object, _)| object)
            };
            if let Some(require_node) = require_node {
                let mut metadata = HashMap::new();
                if let Some(source) = self.extract_require_source(&require_node) {
                    metadata.insert("source".to_string(), json!(source));
                }
                if let Some((_, property)) = required_member {
                    metadata.insert(
                        "importedName".to_string(),
                        json!(self.base.get_node_text(&property)),
                    );
                }
                metadata.insert("isCommonJS".to_string(), json!(true));

                // Extract JSDoc comment
                let doc_comment = self.base.find_doc_comment(&doc_node);

                return Some(self.base.create_symbol(
                    &node,
                    name,
                    SymbolKind::Import,
                    SymbolOptions {
                        signature: Some(signature),
                        visibility: Some(self.extract_visibility(&node)),
                        parent_id,
                        metadata: Some(metadata),
                        doc_comment,
                        annotations: Vec::new(),
                    },
                ));
            }

            if matches!(
                value.kind(),
                "arrow_function" | "function_expression" | "generator_function" | "class"
            ) {
                return None;
            }
        }

        let mut metadata = HashMap::new();
        metadata.insert(
            "declarationType".to_string(),
            json!(self.get_declaration_type(&node)),
        );
        metadata.insert(
            "initializer".to_string(),
            json!(value_node.map(|v| self.base.get_node_text(&v))),
        );
        metadata.insert(
            "isConst".to_string(),
            json!(self.is_const_declaration(&node)),
        );
        metadata.insert("isLet".to_string(), json!(self.is_let_declaration(&node)));

        // Extract JSDoc comment
        let doc_comment = self.base.find_doc_comment(&doc_node);

        let symbol = self.base.create_symbol(
            &node,
            name,
            SymbolKind::Variable,
            SymbolOptions {
                signature: Some(signature),
                visibility: Some(self.extract_visibility(&node)),
                parent_id,
                metadata: Some(metadata),
                doc_comment,
                annotations: Vec::new(),
            },
        );
        if let Some(value) = &value_node {
            super::type_facts::record_new_expression_fact(
                &mut self.base,
                &symbol.id,
                *value,
                &super::type_facts::TYPE_NAME_RULES,
            );
        }
        Some(symbol)
    }

    /// One symbol per binding in a destructuring declarator. Bindings taken by
    /// key from `require("...")` are CommonJS imports; every other binding,
    /// including renamed, defaulted, nested, and rest bindings, is a variable.
    pub(super) fn extract_destructuring_variables(
        &mut self,
        node: Node,
        parent_id: Option<String>,
    ) -> Vec<Symbol> {
        let Some(pattern) = node.child_by_field_name("name") else {
            return Vec::new();
        };
        let value_node = node.child_by_field_name("value");
        let require_source = value_node
            .filter(|value| self.is_require_call(value))
            .and_then(|value| self.extract_require_source(&value));
        let declaration_type = self.get_declaration_type(&node);
        let value_text = value_node
            .map(|value| self.base.get_node_text(&value))
            .unwrap_or_default();
        let doc_node = node.parent().unwrap_or(node);
        let doc_comment = self.base.find_doc_comment(&doc_node);

        let mut bindings = Vec::new();
        collect_pattern_bindings(pattern, None, &mut bindings);

        bindings
            .into_iter()
            .map(|binding| {
                let name = self.base.get_node_text(&binding.name);
                let imported_name = binding
                    .key
                    .map(|key| self.base.get_node_text(&key))
                    .unwrap_or_else(|| name.clone());
                let mut metadata = HashMap::new();
                let kind = match (&require_source, binding.top_level_object) {
                    (Some(source), true) => {
                        metadata.insert("source".to_string(), json!(source));
                        metadata.insert("importedName".to_string(), json!(imported_name));
                        metadata.insert("isCommonJS".to_string(), json!(true));
                        SymbolKind::Import
                    }
                    _ => {
                        metadata.insert("declarationType".to_string(), json!(declaration_type));
                        metadata.insert("isDestructured".to_string(), json!(true));
                        metadata.insert(
                            "destructuringType".to_string(),
                            json!(if pattern.kind() == "array_pattern" {
                                "array"
                            } else {
                                "object"
                            }),
                        );
                        if binding.is_rest {
                            metadata.insert("isRestParameter".to_string(), json!(true));
                        }
                        SymbolKind::Variable
                    }
                };
                let signature = format!(
                    "{} {} = {}",
                    declaration_type,
                    self.base.get_node_text(&pattern),
                    value_text
                );
                self.base.create_symbol(
                    &binding.name,
                    name,
                    kind,
                    SymbolOptions {
                        signature: Some(signature),
                        visibility: Some(self.extract_visibility(&node)),
                        parent_id: parent_id.clone(),
                        metadata: Some(metadata),
                        doc_comment: doc_comment.clone(),
                        annotations: Vec::new(),
                    },
                )
            })
            .collect()
    }
}

pub(crate) struct PatternBinding<'tree> {
    pub(crate) name: Node<'tree>,
    key: Option<Node<'tree>>,
    top_level_object: bool,
    is_rest: bool,
}

/// Collect the identifiers a destructuring pattern binds, in source order.
/// `key` is the property a top-level object binding reads (`audit` in
/// `{ audit: log }`).
pub(crate) fn collect_pattern_bindings<'tree>(
    pattern: Node<'tree>,
    parent_kind: Option<&str>,
    bindings: &mut Vec<PatternBinding<'tree>>,
) {
    collect_pattern_bindings_at(pattern, parent_kind, bindings, 0);
}

fn collect_pattern_bindings_at<'tree>(
    pattern: Node<'tree>,
    parent_kind: Option<&str>,
    bindings: &mut Vec<PatternBinding<'tree>>,
    depth: u32,
) {
    let Some(child_depth) = crate::tree_traversal::child_tree_depth(depth) else {
        return;
    };
    let top_level_object = parent_kind.is_none() && pattern.kind() == "object_pattern";
    let mut cursor = pattern.walk();
    for child in pattern.named_children(&mut cursor) {
        match child.kind() {
            "identifier" | "shorthand_property_identifier_pattern" => {
                bindings.push(PatternBinding {
                    name: child,
                    key: None,
                    top_level_object,
                    is_rest: false,
                });
            }
            "pair_pattern" => {
                let Some(value) = child.child_by_field_name("value") else {
                    continue;
                };
                let value = match value.kind() {
                    "assignment_pattern" => value.child_by_field_name("left").unwrap_or(value),
                    _ => value,
                };
                if value.kind() == "identifier" {
                    bindings.push(PatternBinding {
                        name: value,
                        key: child.child_by_field_name("key"),
                        top_level_object,
                        is_rest: false,
                    });
                } else {
                    collect_pattern_bindings_at(value, Some(pattern.kind()), bindings, child_depth);
                }
            }
            "object_assignment_pattern" | "assignment_pattern" => {
                if let Some(left) = child.child_by_field_name("left") {
                    if matches!(
                        left.kind(),
                        "identifier" | "shorthand_property_identifier_pattern"
                    ) {
                        bindings.push(PatternBinding {
                            name: left,
                            key: None,
                            top_level_object,
                            is_rest: false,
                        });
                    } else {
                        collect_pattern_bindings_at(
                            left,
                            Some(pattern.kind()),
                            bindings,
                            child_depth,
                        );
                    }
                }
            }
            "rest_pattern" => {
                let mut rest_cursor = child.walk();
                if let Some(target) = child.named_children(&mut rest_cursor).next() {
                    if target.kind() == "identifier" {
                        bindings.push(PatternBinding {
                            name: target,
                            key: None,
                            top_level_object: false,
                            is_rest: true,
                        });
                    } else {
                        collect_pattern_bindings_at(
                            target,
                            Some(pattern.kind()),
                            bindings,
                            child_depth,
                        );
                    }
                }
            }
            "object_pattern" | "array_pattern" => {
                collect_pattern_bindings_at(child, Some(pattern.kind()), bindings, child_depth);
            }
            _ => {}
        }
    }
}
