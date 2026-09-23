//! Type extraction for classes, properties, and exports
//!
//! Handles extraction of class declarations, property definitions,
//! and export statements.

use crate::base::{Symbol, SymbolKind, SymbolOptions, Visibility};
use serde_json::json;
use std::collections::HashMap;
use tree_sitter::Node;

/// The name a class declaration or class expression binds: its own name,
/// else the variable or member it is assigned to, else `default` for an
/// anonymous default export. Other anonymous class expressions bind nothing.
pub(crate) fn class_binding_name(base: &crate::base::BaseExtractor, class: Node) -> Option<String> {
    if let Some(name) = class.child_by_field_name("name") {
        return Some(base.get_node_text(&name));
    }
    let mut value = class;
    while let Some(parent) = value
        .parent()
        .filter(|p| p.kind() == "parenthesized_expression")
    {
        value = parent;
    }
    let parent = value.parent()?;
    match parent.kind() {
        "variable_declarator" => parent
            .child_by_field_name("name")
            .filter(|name| name.kind() == "identifier")
            .map(|name| base.get_node_text(&name)),
        "assignment_expression" => {
            let left = parent.child_by_field_name("left")?;
            match left.kind() {
                "identifier" => Some(base.get_node_text(&left)),
                "member_expression" => left
                    .child_by_field_name("property")
                    .map(|property| base.get_node_text(&property))
                    .filter(|name| name != "exports"),
                _ => None,
            }
        }
        "export_statement" => Some("default".to_string()),
        _ => None,
    }
}

impl super::JavaScriptExtractor {
    /// Extract class declarations - direct Implementation of extractClass
    pub(super) fn extract_class(
        &mut self,
        node: Node,
        parent_id: Option<String>,
    ) -> Option<Symbol> {
        let name = class_binding_name(&self.base, node)?;

        // Extract extends clause (reference logic)
        let heritage = node.child_by_field_name("heritage").or_else(|| {
            node.children(&mut node.walk())
                .find(|c| c.kind() == "class_heritage")
        });

        let extends_clause = heritage.and_then(|h| {
            h.children(&mut h.walk())
                .find(|c| c.kind() == "extends_clause")
        });

        let signature = self.build_class_signature(&node);

        let mut metadata = HashMap::new();
        metadata.insert(
            "extends".to_string(),
            json!(extends_clause.map(|ec| self.base.get_node_text(&ec))),
        );
        metadata.insert("isGenerator".to_string(), json!(false)); // JavaScript classes are not generators
        metadata.insert(
            "hasPrivateFields".to_string(),
            json!(self.has_private_fields(&node)),
        );

        // Extract JSDoc comment
        let doc_comment = self.base.find_doc_comment(&node);

        // Decorators sit on the class node itself, or on the wrapping
        // export_statement when the class is exported.
        let annotations = self.extract_decorator_annotations(node);

        Some(self.base.create_symbol(
            &node,
            name,
            SymbolKind::Class,
            SymbolOptions {
                signature: Some(signature),
                visibility: Some(Visibility::Public),
                parent_id,
                metadata: Some(metadata),
                doc_comment,
                annotations,
            },
        ))
    }

    /// Extract property definitions - implementation's extractProperty
    pub(super) fn extract_property(
        &mut self,
        node: Node,
        parent_id: Option<String>,
    ) -> Option<Symbol> {
        let name_node = node
            .child_by_field_name("key")
            .or_else(|| node.child_by_field_name("name"))
            .or_else(|| node.child_by_field_name("property"));

        let name = name_node.map(|n| self.base.get_node_text(&n))?;
        let value_node = node.child_by_field_name("value");
        let signature = self.build_property_signature(&node, &name);

        // If the value is a function, treat it as a method (reference logic)
        if let Some(value) = &value_node
            && (value.kind() == "arrow_function"
                || value.kind() == "function_expression"
                || value.kind() == "generator_function")
        {
            let method_signature = self.build_method_signature(value, &name);

            let is_static = node
                .children(&mut node.walk())
                .any(|c| c.kind() == "static");
            let mut metadata = HashMap::new();
            metadata.insert("isAsync".to_string(), json!(self.is_async(value)));
            metadata.insert("isGenerator".to_string(), json!(self.is_generator(value)));
            metadata.insert("isStatic".to_string(), json!(is_static));
            metadata.insert(
                "parameters".to_string(),
                json!(self.extract_parameters(value)),
            );

            // Extract JSDoc comment
            let doc_comment = self.base.find_doc_comment(&node);

            return Some(self.base.create_symbol(
                &node,
                name,
                SymbolKind::Method,
                SymbolOptions {
                    signature: Some(method_signature),
                    visibility: Some(self.extract_visibility(&node)),
                    parent_id,
                    metadata: Some(metadata),
                    doc_comment,
                    annotations: self.extract_decorator_annotations(node),
                },
            ));
        }

        // Determine if this is a class field or regular property (reference logic)
        let symbol_kind = match node.kind() {
            "public_field_definition" | "field_definition" | "property_definition" => {
                SymbolKind::Field
            }
            _ => SymbolKind::Property,
        };

        let is_static = node
            .children(&mut node.walk())
            .any(|c| c.kind() == "static");
        let mut metadata = HashMap::new();
        metadata.insert(
            "value".to_string(),
            json!(value_node.map(|v| self.base.get_node_text(&v))),
        );
        metadata.insert(
            "isComputed".to_string(),
            json!(self.is_computed_property(&node)),
        );
        metadata.insert("isPrivate".to_string(), json!(name.starts_with('#')));
        metadata.insert("isStatic".to_string(), json!(is_static));

        // Extract JSDoc comment
        let doc_comment = self.base.find_doc_comment(&node);

        let annotations = if node.kind() == "pair" {
            Vec::new()
        } else {
            self.extract_decorator_annotations(node)
        };
        let symbol = self.base.create_symbol(
            &node,
            name,
            symbol_kind,
            SymbolOptions {
                signature: Some(signature),
                visibility: Some(self.extract_visibility(&node)),
                parent_id,
                metadata: Some(metadata),
                doc_comment,
                annotations,
            },
        );
        if let Some(value) = value_node {
            super::type_facts::record_new_expression_fact(
                &mut self.base,
                &symbol.id,
                value,
                &super::type_facts::TYPE_NAME_RULES,
            );
        }
        Some(symbol)
    }
}
