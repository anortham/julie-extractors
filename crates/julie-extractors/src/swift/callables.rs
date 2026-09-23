use super::signatures::return_type_node;
use super::type_facts;
use crate::base::{Symbol, SymbolKind, SymbolOptions, Visibility};
use crate::test_detection::apply_callable_test_metadata;
use std::collections::HashMap;
use tree_sitter::Node;

use super::SwiftExtractor;

/// Extracts Swift callable members: functions, methods, initializers, and deinitializers
impl SwiftExtractor {
    /// Implementation of extractFunction method
    pub(super) fn extract_function(
        &mut self,
        node: Node,
        parent_id: Option<&str>,
    ) -> Option<Symbol> {
        let name_node = node.child_by_field_name("name")?;
        let name = self.base.get_node_text(&name_node);
        let is_operator = name_node.kind() != "simple_identifier";

        let modifiers = self.extract_modifiers(node);
        let annotations = self.extract_annotations(node);
        let annotation_keys: Vec<String> = annotations
            .iter()
            .map(|annotation| annotation.annotation_key.clone())
            .collect();
        let generic_params = self.extract_generic_parameters(node);
        let parameters = self.extract_parameters(node);
        let return_type = self.extract_return_type(node);

        let mut signature = format!("func {}", name);

        if !modifiers.is_empty() {
            signature = format!("{} {}", modifiers.join(" "), signature);
        }

        if let Some(ref generic_params) = generic_params {
            signature.push_str(generic_params);
        }

        let params_str = parameters.unwrap_or_else(|| "()".to_string());
        signature.push_str(&params_str);
        if let Some(effects) = self.extract_effects(node) {
            signature.push_str(&format!(" {effects}"));
        }
        if let Some(ref return_type) = return_type {
            signature.push_str(&format!(" -> {return_type}"));
        }
        if let Some(where_clause) = self.extract_where_clause(node) {
            signature.push_str(&format!(" {where_clause}"));
        }

        let symbol_kind = if is_operator {
            SymbolKind::Operator
        } else if self.is_type_member(node) {
            SymbolKind::Method
        } else {
            SymbolKind::Function
        };

        let mut metadata = HashMap::from([
            (
                "type".to_string(),
                serde_json::Value::String("function".to_string()),
            ),
            (
                "modifiers".to_string(),
                serde_json::Value::String(modifiers.join(", ")),
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

        // Extract Swift documentation comment
        let doc_comment = self.base.find_doc_comment(&node);

        apply_callable_test_metadata(
            "swift",
            &name,
            &self.base.file_path,
            &symbol_kind,
            &annotation_keys,
            doc_comment.as_deref(),
            &mut metadata,
        );

        let symbol = self.base.create_symbol(
            &node,
            name,
            symbol_kind,
            SymbolOptions {
                signature: Some(signature),
                visibility: Some(self.determine_visibility(&modifiers)),
                parent_id: parent_id.map(|s| s.to_string()),
                metadata: Some(metadata),
                doc_comment,
                annotations,
            },
        );
        if let Some(type_node) = return_type_node(node) {
            type_facts::record_declared_type(&mut self.base, &symbol.id, type_node);
        }
        Some(symbol)
    }

    /// Implementation of extractInitializer method
    pub(super) fn extract_initializer(&mut self, node: Node, parent_id: Option<&str>) -> Symbol {
        let name = "init".to_string();
        let modifiers = self.extract_modifiers(node);
        let annotations = self.extract_annotations(node);
        let annotation_keys: Vec<String> = annotations
            .iter()
            .map(|annotation| annotation.annotation_key.clone())
            .collect();
        let params_str = self
            .extract_parameters(node)
            .unwrap_or_else(|| "()".to_string());
        let mut signature = format!("init{}{}", failable_marker(node), params_str);
        if let Some(effects) = self.extract_effects(node) {
            signature.push_str(&format!(" {effects}"));
        }

        if !modifiers.is_empty() {
            signature = format!("{} {}", modifiers.join(" "), signature);
        }

        let mut metadata = HashMap::from([
            (
                "type".to_string(),
                serde_json::Value::String("initializer".to_string()),
            ),
            (
                "parameters".to_string(),
                serde_json::Value::String(params_str),
            ),
            (
                "modifiers".to_string(),
                serde_json::Value::String(modifiers.join(", ")),
            ),
        ]);

        // Extract Swift documentation comment
        let doc_comment = self.base.find_doc_comment(&node);

        apply_callable_test_metadata(
            "swift",
            &name,
            &self.base.file_path,
            &SymbolKind::Constructor,
            &annotation_keys,
            doc_comment.as_deref(),
            &mut metadata,
        );

        self.base.create_symbol(
            &node,
            name,
            SymbolKind::Constructor,
            SymbolOptions {
                signature: Some(signature),
                visibility: Some(self.determine_visibility(&modifiers)),
                parent_id: parent_id.map(|s| s.to_string()),
                metadata: Some(metadata),
                doc_comment,
                annotations,
            },
        )
    }

    /// A declaration directly inside a type, extension, or protocol body is a
    /// member; one inside a function body is a local.
    pub(super) fn is_type_member(&self, node: Node) -> bool {
        node.parent().is_some_and(|parent| {
            matches!(
                parent.kind(),
                "class_body" | "enum_class_body" | "protocol_body"
            )
        })
    }

    /// `macro name(...) -> T = #externalMacro(...)`: a function-like symbol
    /// whose signature stops before the `=` definition.
    pub(super) fn extract_macro(&mut self, node: Node, parent_id: Option<&str>) -> Option<Symbol> {
        let name_node = node
            .children(&mut node.walk())
            .find(|child| child.kind() == "simple_identifier")?;
        let name = self.base.get_node_text(&name_node);
        let modifiers = self.extract_modifiers(node);
        let annotations = self.extract_annotations(node);
        let header_end = node
            .child_by_field_name("definition")
            .map_or(node.end_byte(), |definition| definition.start_byte());
        let header = self.base.content[name_node.start_byte()..header_end]
            .trim_end()
            .trim_end_matches('=')
            .trim_end();
        let mut signature = format!("macro {header}");
        if !modifiers.is_empty() {
            signature = format!("{} {signature}", modifiers.join(" "));
        }
        let metadata = HashMap::from([
            (
                "type".to_string(),
                serde_json::Value::String("macro".to_string()),
            ),
            (
                "modifiers".to_string(),
                serde_json::Value::String(modifiers.join(", ")),
            ),
        ]);
        let doc_comment = self.base.find_doc_comment(&node);
        let mut symbol = self.base.create_symbol(
            &node,
            name,
            SymbolKind::Function,
            SymbolOptions {
                signature: Some(signature),
                visibility: Some(self.determine_visibility(&modifiers)),
                parent_id: parent_id.map(|s| s.to_string()),
                metadata: Some(metadata),
                doc_comment,
                annotations,
            },
        );
        super::set_body(
            &self.base,
            &mut symbol,
            node.child_by_field_name("definition"),
        );
        Some(symbol)
    }

    /// `infix operator <>: AdditionPrecedence` declares an operator's fixity.
    pub(super) fn extract_operator_declaration(
        &mut self,
        node: Node,
        parent_id: Option<&str>,
    ) -> Option<Symbol> {
        let name_node = node
            .children(&mut node.walk())
            .find(|child| child.kind() == "custom_operator")
            .or_else(|| {
                node.children(&mut node.walk())
                    .skip_while(|child| child.kind() != "operator")
                    .nth(1)
            })?;
        let name = self.base.get_node_text(&name_node);
        let signature = self
            .base
            .get_node_text(&node)
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        let metadata = HashMap::from([(
            "type".to_string(),
            serde_json::Value::String("operator_declaration".to_string()),
        )]);
        let doc_comment = self.base.find_doc_comment(&node);
        let mut symbol = self.base.create_symbol(
            &node,
            name,
            SymbolKind::Operator,
            SymbolOptions {
                signature: Some(signature),
                visibility: Some(Visibility::Internal),
                parent_id: parent_id.map(|s| s.to_string()),
                metadata: Some(metadata),
                doc_comment,
                annotations: Vec::new(),
            },
        );
        super::clear_body(&mut symbol);
        Some(symbol)
    }

    /// Implementation of extractDeinitializer method
    pub(super) fn extract_deinitializer(&mut self, node: Node, parent_id: Option<&str>) -> Symbol {
        let name = "deinit".to_string();
        let signature = "deinit".to_string();
        let annotations = self.extract_annotations(node);

        let metadata = HashMap::from([(
            "type".to_string(),
            serde_json::Value::String("deinitializer".to_string()),
        )]);

        // Extract Swift documentation comment
        let doc_comment = self.base.find_doc_comment(&node);

        self.base.create_symbol(
            &node,
            name,
            SymbolKind::Destructor,
            SymbolOptions {
                signature: Some(signature),
                visibility: Some(Visibility::Public),
                parent_id: parent_id.map(|s| s.to_string()),
                metadata: Some(metadata),
                doc_comment,
                annotations,
            },
        )
    }
}

/// `init?` and `init!` declare failable initializers.
fn failable_marker(node: Node) -> &'static str {
    let mut cursor = node.walk();
    node.children(&mut cursor)
        .take_while(|child| child.kind() != "parameter" && child.kind() != "(")
        .find_map(|child| match child.kind() {
            "?" => Some("?"),
            "bang" | "!" => Some("!"),
            _ => None,
        })
        .unwrap_or("")
}
