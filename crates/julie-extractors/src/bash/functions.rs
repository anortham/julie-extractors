//! Function extraction for Bash
//!
//! Handles extraction of function definitions and their positional parameters.

use crate::base::{Symbol, SymbolKind, SymbolOptions, Visibility};
use crate::test_detection::apply_callable_test_metadata;
use std::collections::{HashMap, HashSet};
use tree_sitter::Node;

impl super::BashExtractor {
    /// Extract a function definition from a function_definition node
    pub(super) fn extract_function(
        &mut self,
        node: Node,
        parent_id: Option<&str>,
    ) -> Option<Symbol> {
        let name_node = self.find_name_node(node)?;
        let name = self.base.get_node_text(&name_node);
        let doc_comment = self.base.find_doc_comment(&node);

        let mut metadata = HashMap::new();
        apply_callable_test_metadata(
            "bash",
            &name,
            &self.base.file_path,
            &SymbolKind::Function,
            &[],
            doc_comment.as_deref(),
            &mut metadata,
        );

        let options = SymbolOptions {
            signature: self.extract_function_signature(node),
            visibility: Some(Visibility::Public),
            parent_id: parent_id.map(|s| s.to_string()),
            doc_comment,
            metadata: if metadata.is_empty() {
                None
            } else {
                Some(metadata)
            },
            ..Default::default()
        };

        Some(
            self.base
                .create_symbol(&node, name, SymbolKind::Function, options),
        )
    }

    /// Extract positional parameters ($1, $2, etc.) from a function
    pub(super) fn extract_positional_parameters(
        &mut self,
        func_node: Node,
        parent_id: &str,
    ) -> Vec<Symbol> {
        let mut parameters = Vec::new();
        let mut seen_params = HashSet::new();

        for (node, number) in self.collect_parameter_nodes(func_node) {
            let param_name = format!("${number}");
            if !seen_params.insert(param_name.clone()) {
                continue;
            }
            let mut metadata = HashMap::new();
            metadata.insert("role".to_string(), serde_json::json!("parameter"));

            let options = SymbolOptions {
                signature: Some(format!("{param_name} (positional parameter)")),
                visibility: Some(Visibility::Public),
                parent_id: Some(parent_id.to_string()),
                metadata: Some(metadata),
                ..Default::default()
            };
            parameters.push(self.base.create_symbol(
                &node,
                param_name,
                SymbolKind::Variable,
                options,
            ));
        }

        parameters
    }
}
