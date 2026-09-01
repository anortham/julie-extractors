/// Stub implementations for declaration-like C# symbol extraction (fields, local functions, variables)
use crate::base::{Symbol, SymbolKind, SymbolOptions};
use crate::test_detection::apply_callable_test_metadata;
use std::collections::HashMap;
use tree_sitter::Node;

impl super::RazorExtractor {
    /// Extract field declaration
    pub(super) fn extract_field(&mut self, node: Node, parent_id: Option<&str>) -> Option<Symbol> {
        // Extract field name and type
        let mut field_name: Option<String> = None;
        let mut field_type = None;

        // Find variable declarator in field declaration
        if let Some(var_decl) = self.find_child_by_type(node, "variable_declaration") {
            if var_decl
                .child_by_field_name("type")
                .is_some_and(|ty| ty.kind() == "implicit_type")
            {
                return None;
            }

            // Extract type
            if let Some(type_node) = self.find_child_by_types(
                var_decl,
                &[
                    "predefined_type",
                    "identifier",
                    "generic_name",
                    "qualified_name",
                    "nullable_type",
                    "array_type",
                ],
            ) {
                field_type = Some(self.base.get_node_text(&type_node));
            }

            // Find variable declarator(s)
            if let Some(var_declarator) = self.find_child_by_type(var_decl, "variable_declarator")
                && let Some(identifier) = self.find_child_by_type(var_declarator, "identifier")
            {
                field_name = Some(self.base.get_node_text(&identifier));
            }
        }

        let field_name = field_name?;

        let modifiers = self.extract_modifiers(node);
        let attributes = self.extract_attributes(node);

        let mut signature_parts = Vec::new();
        if !attributes.is_empty() {
            signature_parts.push(attributes.join(" "));
        }
        if !modifiers.is_empty() {
            signature_parts.push(modifiers.join(" "));
        }
        if let Some(ref f_type) = field_type {
            signature_parts.push(f_type.clone());
        }
        signature_parts.push(field_name.clone());

        let type_node = self
            .find_child_by_type(node, "variable_declaration")
            .and_then(|decl| decl.child_by_field_name("type"));
        let symbol = self.base.create_symbol(
            &node,
            field_name,
            SymbolKind::Field,
            SymbolOptions {
                signature: Some(signature_parts.join(" ")),
                visibility: Some(self.determine_visibility(&modifiers)),
                parent_id: parent_id.map(|s| s.to_string()),
                metadata: Some({
                    let mut metadata = HashMap::new();
                    metadata.insert(
                        "type".to_string(),
                        serde_json::Value::String("field".to_string()),
                    );
                    metadata.insert(
                        "modifiers".to_string(),
                        serde_json::Value::String(modifiers.join(", ")),
                    );
                    if let Some(f_type) = field_type {
                        metadata.insert("fieldType".to_string(), serde_json::Value::String(f_type));
                    }
                    metadata.insert(
                        "attributes".to_string(),
                        serde_json::Value::String(attributes.join(", ")),
                    );
                    metadata
                }),
                doc_comment: None,
                annotations: Vec::new(),
            },
        );
        if let Some(type_node) = type_node {
            super::type_facts::record_declared_type(&mut self.base, &symbol.id, type_node);
        }
        Some(symbol)
    }

    /// Extract local function statement
    pub(super) fn extract_local_function(
        &mut self,
        node: Node,
        parent_id: Option<&str>,
    ) -> Option<Symbol> {
        // Extract function name using same logic as extract_method
        let mut name: Option<String> = None;

        let mut cursor = node.walk();
        let children: Vec<_> = node.children(&mut cursor).collect();

        if let Some(param_list_idx) = children.iter().position(|c| c.kind() == "parameter_list") {
            // Look backwards from parameter list to find the method name identifier
            for i in (0..param_list_idx).rev() {
                if children[i].kind() == "identifier" {
                    name = Some(self.base.get_node_text(&children[i]));
                    break;
                }
            }
        } else {
            // Fallback: find the last identifier (which should be method name in most cases)
            for child in children.iter().rev() {
                if child.kind() == "identifier" {
                    name = Some(self.base.get_node_text(child));
                    break;
                }
            }
        }

        let name = name?;

        let modifiers = self.extract_modifiers(node);
        let parameters = self.extract_method_parameters(node);
        let return_type = self.extract_return_type(node);
        let attributes = self.extract_attributes(node);

        let mut signature_parts = Vec::new();
        if !attributes.is_empty() {
            signature_parts.push(attributes.join(" "));
        }
        if !modifiers.is_empty() {
            signature_parts.push(modifiers.join(" "));
        }
        if let Some(ref ret_type) = return_type {
            signature_parts.push(ret_type.clone());
        } else {
            signature_parts.push("void".to_string()); // Default return type for local functions
        }
        signature_parts.push(format!(
            "{}{}",
            name,
            parameters.clone().unwrap_or_else(|| "()".to_string())
        ));

        // Test detection uses normalized annotation keys supplied by later extraction tasks.
        let mut metadata = HashMap::new();
        metadata.insert(
            "type".to_string(),
            serde_json::Value::String("local-function".to_string()),
        );
        metadata.insert(
            "modifiers".to_string(),
            serde_json::Value::String(modifiers.join(", ")),
        );
        if let Some(params) = &parameters {
            metadata.insert(
                "parameters".to_string(),
                serde_json::Value::String(params.clone()),
            );
        }
        if let Some(ret_type) = return_type {
            metadata.insert(
                "returnType".to_string(),
                serde_json::Value::String(ret_type),
            );
        }
        metadata.insert(
            "attributes".to_string(),
            serde_json::Value::String(attributes.join(", ")),
        );
        apply_callable_test_metadata(
            "razor",
            &name,
            &self.base.file_path,
            &SymbolKind::Method,
            &[],
            None,
            &mut metadata,
        );

        let symbol = self.base.create_symbol(
            &node,
            name,
            SymbolKind::Method,
            SymbolOptions {
                signature: Some(signature_parts.join(" ")),
                visibility: Some(self.determine_visibility(&modifiers)),
                parent_id: parent_id.map(|s| s.to_string()),
                metadata: Some(metadata),
                doc_comment: None,
                annotations: Vec::new(),
            },
        );
        if let Some(returns) = node.child_by_field_name("type") {
            super::type_facts::record_return_type(&mut self.base, &symbol.id, returns);
        }
        Some(symbol)
    }

    /// Extract local variable declaration
    pub(super) fn extract_local_variable(
        &mut self,
        node: Node,
        parent_id: Option<&str>,
    ) -> Option<Symbol> {
        let var_decl = self.find_child_by_type(node, "variable_declaration")?;
        let var_declarator = self.find_child_by_type(var_decl, "variable_declarator")?;
        let identifier = self.find_child_by_type(var_declarator, "identifier")?;
        let variable_name = self.base.get_node_text(&identifier);

        let mut cursor = var_declarator.walk();
        let children: Vec<_> = var_declarator.children(&mut cursor).collect();
        let equals_pos = children.iter().position(|c| c.kind() == "=");
        let initializer_node = equals_pos.and_then(|pos| children.get(pos + 1).copied());
        let initializer = initializer_node.map(|init| self.base.get_node_text(&init));

        let type_node = var_decl.child_by_field_name("type").or_else(|| {
            self.find_child_by_types(
                var_decl,
                &[
                    "predefined_type",
                    "identifier",
                    "generic_name",
                    "qualified_name",
                    "nullable_type",
                    "array_type",
                    "implicit_type",
                ],
            )
        });
        let variable_type = type_node.and_then(|ty| {
            let text = self.base.get_node_text(&ty);
            if ty.kind() == "implicit_type" || text == "var" {
                None
            } else {
                Some(text)
            }
        });
        let is_var = type_node.is_some_and(|ty| {
            ty.kind() == "implicit_type" || self.base.get_node_text(&ty) == "var"
        });
        let modifiers = self.extract_modifiers(node);
        let attributes = self.extract_attributes(node);
        let mut signature_parts = Vec::new();
        if !attributes.is_empty() {
            signature_parts.push(attributes.join(" "));
        }
        if !modifiers.is_empty() {
            signature_parts.push(modifiers.join(" "));
        }
        if let Some(var_type) = &variable_type {
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
                visibility: Some(self.determine_visibility(&modifiers)),
                parent_id: parent_id.map(|s| s.to_string()),
                metadata: Some({
                    let mut metadata = HashMap::new();
                    metadata.insert(
                        "type".to_string(),
                        serde_json::Value::String("local-variable".to_string()),
                    );
                    if let Some(var_type) = variable_type {
                        metadata.insert(
                            "variableType".to_string(),
                            serde_json::Value::String(var_type),
                        );
                    }
                    if let Some(init) = initializer {
                        metadata.insert("initializer".to_string(), serde_json::Value::String(init));
                    }
                    metadata.insert(
                        "modifiers".to_string(),
                        serde_json::Value::String(modifiers.join(", ")),
                    );
                    metadata
                }),
                doc_comment: None,
                annotations: Vec::new(),
            },
        );
        if is_var {
            if let Some(init) = initializer_node {
                super::type_facts::record_new_expression_type(&mut self.base, &symbol.id, init);
            }
        } else if let Some(type_node) = type_node {
            super::type_facts::record_declared_type(&mut self.base, &symbol.id, type_node);
        }
        Some(symbol)
    }
}
