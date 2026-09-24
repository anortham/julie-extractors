/// Extraction for C# variable declarations in Razor files
use crate::base::{Symbol, SymbolKind, SymbolOptions, Visibility};
use std::collections::HashMap;
use tree_sitter::Node;

impl super::RazorExtractor {
    /// Extract variable declaration
    pub(super) fn extract_variable_declaration(
        &mut self,
        node: Node,
        parent_id: Option<&str>,
    ) -> Option<Symbol> {
        // Extract variable name and type from variable declaration
        let mut variable_type = None;

        // Find the type (if present)
        if let Some(type_node) = self.find_child_by_types(
            node,
            &[
                "predefined_type",
                "identifier",
                "generic_name",
                "qualified_name",
                "nullable_type",
                "array_type",
                "var",
            ],
        ) {
            let type_text = self.base.get_node_text(&type_node);
            if type_text != "var" {
                // Don't use "var" as the actual type
                variable_type = Some(type_text);
            }
        }

        // Find variable declarators
        let mut declarators = Vec::new();
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "variable_declarator"
                && let Some(identifier) = self.find_child_by_type(child, "identifier")
            {
                let name = self.base.get_node_text(&identifier);

                let mut decl_cursor = child.walk();
                let decl_children: Vec<_> = child.children(&mut decl_cursor).collect();
                let initializer_node = decl_children
                    .iter()
                    .position(|c| c.kind() == "=")
                    .and_then(|pos| decl_children.get(pos + 1).copied());

                declarators.push((name, initializer_node));
            }
        }

        // For now, handle the first declarator (most common case)
        if let Some((name, initializer_node)) = declarators.into_iter().next() {
            let variable_name = name;
            let initializer = initializer_node.map(|init| self.base.get_node_text(&init));

            let mut signature_parts = Vec::new();
            if let Some(ref var_type) = variable_type {
                signature_parts.push(var_type.clone());
            } else {
                signature_parts.push("var".to_string());
            }
            signature_parts.push(variable_name.clone());
            if let Some(init) = &initializer {
                signature_parts.push(format!("= {}", init));
            }

            let symbol = self.base.create_symbol(
                &node,
                variable_name,
                SymbolKind::Variable,
                SymbolOptions {
                    signature: Some(signature_parts.join(" ")),
                    visibility: Some(Visibility::Private),
                    parent_id: parent_id.map(|s| s.to_string()),
                    metadata: Some({
                        let mut metadata = HashMap::new();
                        metadata.insert(
                            "type".to_string(),
                            serde_json::Value::String("variable-declaration".to_string()),
                        );
                        if let Some(var_type) = variable_type {
                            metadata.insert(
                                "variableType".to_string(),
                                serde_json::Value::String(var_type),
                            );
                        }
                        if let Some(init) = initializer {
                            metadata
                                .insert("initializer".to_string(), serde_json::Value::String(init));
                        }
                        metadata
                    }),
                    doc_comment: None,
                    annotations: Vec::new(),
                },
            );
            let is_var = node
                .child_by_field_name("type")
                .is_some_and(|ty| ty.kind() == "implicit_type");
            if is_var && let Some(init) = initializer_node {
                super::type_facts::record_new_expression_type(&mut self.base, &symbol.id, init);
                if let Some(declared) = self.return_types.initializer_type(&self.base, init) {
                    super::type_facts::record_inferred_type(&mut self.base, &symbol.id, &declared);
                }
            }
            Some(symbol)
        } else {
            None
        }
    }
}
