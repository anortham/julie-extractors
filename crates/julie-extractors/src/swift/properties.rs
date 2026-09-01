use crate::base::{Symbol, SymbolKind, SymbolOptions};
use serde_json;
use std::collections::HashMap;
use tree_sitter::Node;

use super::SwiftExtractor;
use super::type_facts;

/// Extracts Swift properties, variables, and subscripts
impl SwiftExtractor {
    /// Implementation of extractProperty method
    pub(super) fn extract_property(
        &mut self,
        node: Node,
        parent_id: Option<&str>,
    ) -> Option<Symbol> {
        let name_node = node.child_by_field_name("name").or_else(|| {
            node.children(&mut node.walk())
                .find(|c| c.kind() == "pattern")
        })?;
        let name = name_node
            .child_by_field_name("bound_identifier")
            .map(|n| self.base.get_node_text(&n))
            .unwrap_or_else(|| self.base.get_node_text(&name_node));

        let modifiers = self.extract_modifiers(node);
        let property_type = self.extract_property_type(node);
        let annotations = self.extract_annotations(node);

        let binding_pattern = node
            .children(&mut node.walk())
            .find(|c| c.kind() == "value_binding_pattern");
        let keyword = if let Some(binding_pattern) = binding_pattern {
            binding_pattern
                .children(&mut binding_pattern.walk())
                .find(|c| c.kind() == "var" || c.kind() == "let")
                .map(|n| self.base.get_node_text(&n))
                .unwrap_or_else(|| "var".to_string())
        } else {
            "var".to_string()
        };

        let non_visibility_modifiers: Vec<_> = modifiers
            .iter()
            .filter(|m| {
                !["public", "private", "internal", "fileprivate", "open"].contains(&m.as_str())
            })
            .cloned()
            .collect();

        let mut signature = if !non_visibility_modifiers.is_empty() {
            format!(
                "{} {} {}",
                non_visibility_modifiers.join(" "),
                keyword,
                name
            )
        } else {
            format!("{} {}", keyword, name)
        };

        if let Some(ref property_type) = property_type {
            signature.push_str(&format!(": {}", property_type));
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
                "propertyType".to_string(),
                serde_json::Value::String(property_type.unwrap_or_else(|| "Any".to_string())),
            ),
            ("keyword".to_string(), serde_json::Value::String(keyword)),
        ]);
        if let Some(keys) = self.annotation_keys_csv(&annotations) {
            metadata.insert(
                "annotationKeys".to_string(),
                serde_json::Value::String(keys),
            );
        }

        let doc_comment = self.base.find_doc_comment(&node);
        let kind = if type_facts::nearest_callable_ancestor(node) {
            SymbolKind::Variable
        } else {
            SymbolKind::Property
        };

        let symbol = self.base.create_symbol(
            &node,
            name,
            kind,
            SymbolOptions {
                signature: Some(signature),
                visibility: Some(self.determine_visibility(&modifiers)),
                parent_id: parent_id.map(|s| s.to_string()),
                metadata: Some(metadata),
                doc_comment,
                annotations,
            },
        );

        if let Some(type_node) = type_facts::property_type_node(node) {
            type_facts::record_declared_type(&mut self.base, &symbol.id, type_node);
        } else if let Some(value) = type_facts::property_value_node(node) {
            type_facts::record_same_file_constructor(
                &mut self.base,
                &symbol.id,
                value,
                &self.same_file_type_names,
            );
        }

        Some(symbol)
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

        let metadata = HashMap::from([
            (
                "type".to_string(),
                serde_json::Value::String("subscript".to_string()),
            ),
            (
                "parameters".to_string(),
                serde_json::Value::String(parameters),
            ),
            (
                "returnType".to_string(),
                serde_json::Value::String(return_type.unwrap_or_else(|| "Any".to_string())),
            ),
            (
                "modifiers".to_string(),
                serde_json::Value::String(modifiers.join(", ")),
            ),
        ]);

        let doc_comment = self.base.find_doc_comment(&node);

        self.base.create_symbol(
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
        )
    }
}
