use crate::base::{Symbol, SymbolKind, SymbolOptions, Visibility};
use std::collections::HashMap;
use tree_sitter::Node;

use super::signatures::return_type_node;
use super::{SwiftExtractor, clear_body, type_facts};

/// Extracts protocol-specific members and requirements
impl SwiftExtractor {
    /// Implementation of extractProtocolFunction method
    pub(super) fn extract_protocol_function(
        &mut self,
        node: Node,
        parent_id: Option<&str>,
    ) -> Option<Symbol> {
        let name_node = node
            .children(&mut node.walk())
            .find(|c| c.kind() == "simple_identifier");
        let name = name_node.map(|n| self.base.get_node_text(&n))?;

        let parameters = self.extract_parameters(node);
        let return_type = self.extract_return_type(node);

        let params_str = parameters.unwrap_or_else(|| "()".to_string());

        let mut signature = format!("func {}", name);
        signature.push_str(&params_str);
        if let Some(effects) = self.extract_effects(node) {
            signature.push_str(&format!(" {effects}"));
        }
        if let Some(ref return_type) = return_type {
            signature.push_str(&format!(" -> {return_type}"));
        }

        let mut metadata = HashMap::from([
            (
                "type".to_string(),
                serde_json::Value::String("protocol-requirement".to_string()),
            ),
            (
                "parameters".to_string(),
                serde_json::Value::String(params_str),
            ),
        ]);
        if let Some(return_type) = return_type {
            metadata.insert(
                "returnType".to_string(),
                serde_json::Value::String(return_type),
            );
        }

        let mut symbol = self.base.create_symbol(
            &node,
            name,
            SymbolKind::Method,
            SymbolOptions {
                signature: Some(signature),
                visibility: Some(Visibility::Internal),
                parent_id: parent_id.map(|s| s.to_string()),
                metadata: Some(metadata),
                doc_comment: None,
                annotations: Vec::new(),
            },
        );
        clear_body(&mut symbol);
        if let Some(type_node) = return_type_node(node) {
            type_facts::record_declared_type(&mut self.base, &symbol.id, type_node);
        }
        Some(symbol)
    }

    /// Implementation of extractProtocolProperty method
    pub(super) fn extract_protocol_property(
        &mut self,
        node: Node,
        parent_id: Option<&str>,
    ) -> Option<Symbol> {
        let pattern_node = node
            .children(&mut node.walk())
            .find(|c| c.kind() == "pattern")?;
        let name = pattern_node
            .children(&mut pattern_node.walk())
            .find(|c| c.kind() == "simple_identifier")
            .map(|n| self.base.get_node_text(&n))?;

        // Check for static modifier
        let modifiers_node = node
            .children(&mut node.walk())
            .find(|c| c.kind() == "modifiers");
        let is_static = modifiers_node
            .map(|modifiers_node| {
                modifiers_node
                    .children(&mut modifiers_node.walk())
                    .any(|c| {
                        c.kind() == "property_modifier" && self.base.get_node_text(&c) == "static"
                    })
            })
            .unwrap_or(false);

        let property_type = self.extract_property_type(node);

        // Extract getter/setter requirements
        let protocol_requirements = node
            .children(&mut node.walk())
            .find(|c| c.kind() == "protocol_property_requirements");
        let accessors = protocol_requirements
            .map(|req| format!(" {}", self.base.get_node_text(&req)))
            .unwrap_or_else(String::new);

        let mut signature = if is_static {
            format!("static var {}", name)
        } else {
            format!("var {}", name)
        };

        if let Some(ref property_type) = property_type {
            signature.push_str(&format!(": {}", property_type));
        }

        if !accessors.is_empty() {
            signature.push_str(&accessors);
        }

        let mut metadata = HashMap::from([
            (
                "type".to_string(),
                serde_json::Value::String("protocol-requirement".to_string()),
            ),
            (
                "accessors".to_string(),
                serde_json::Value::String(accessors),
            ),
            (
                "isStatic".to_string(),
                serde_json::Value::String(is_static.to_string()),
            ),
        ]);

        if let Some(property_type) = property_type {
            metadata.insert(
                "propertyType".to_string(),
                serde_json::Value::String(property_type),
            );
        }

        let mut symbol = self.base.create_symbol(
            &node,
            name,
            SymbolKind::Property,
            SymbolOptions {
                signature: Some(signature),
                visibility: Some(Visibility::Internal),
                parent_id: parent_id.map(|s| s.to_string()),
                metadata: Some(metadata),
                doc_comment: None,
                annotations: Vec::new(),
            },
        );
        clear_body(&mut symbol);
        if let Some(type_node) = type_facts::property_type_node(node) {
            type_facts::record_declared_type(&mut self.base, &symbol.id, type_node);
        }
        Some(symbol)
    }

    /// Implementation of extractAssociatedType method
    pub(super) fn extract_associated_type(
        &mut self,
        node: Node,
        parent_id: Option<&str>,
    ) -> Option<Symbol> {
        let name = node
            .children(&mut node.walk())
            .find(|c| c.kind() == "type_identifier" || c.kind() == "simple_identifier")
            .map(|n| self.base.get_node_text(&n))?;

        let mut signature = format!("associatedtype {}", name);

        // Check for type constraints
        if let Some(inheritance) = self.extract_inheritance(node) {
            signature.push_str(&format!(": {}", inheritance));
        }

        let metadata = HashMap::from([(
            "type".to_string(),
            serde_json::Value::String("associatedtype".to_string()),
        )]);

        let mut symbol = self.base.create_symbol(
            &node,
            name,
            SymbolKind::Type,
            SymbolOptions {
                signature: Some(signature),
                visibility: Some(Visibility::Internal),
                parent_id: parent_id.map(|s| s.to_string()),
                metadata: Some(metadata),
                doc_comment: None,
                annotations: Vec::new(),
            },
        );
        clear_body(&mut symbol);
        Some(symbol)
    }
}
