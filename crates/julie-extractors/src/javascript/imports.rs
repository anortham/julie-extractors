//! Import statement extraction for JavaScript
//!
//! Handles extraction of ES6 import statements and CommonJS requires.

use crate::base::{Symbol, SymbolKind, SymbolOptions};
use serde_json::json;
use std::collections::HashMap;
use tree_sitter::Node;

impl super::JavaScriptExtractor {
    /// Create import symbol - direct Implementation of createImportSymbol
    pub(super) fn create_import_symbol(
        &mut self,
        node: Node,
        binding: &ImportBinding,
        parent_id: Option<String>,
    ) -> Symbol {
        let source = node.child_by_field_name("source");
        let source_path = source
            .map(|s| {
                self.base
                    .get_node_text(&s)
                    .replace(&['\'', '"', '`'][..], "")
            })
            .unwrap_or_default();

        let mut metadata = HashMap::new();
        metadata.insert("source".to_string(), json!(source_path));
        metadata.insert("specifier".to_string(), json!(binding.local_name));
        if let Some(ref imported) = binding.imported_name {
            metadata.insert("importedName".to_string(), json!(imported));
        }
        metadata.insert("isDefault".to_string(), json!(binding.is_default));
        metadata.insert("isNamespace".to_string(), json!(binding.is_namespace));
        metadata.insert("isTypeOnly".to_string(), json!(false));

        // Extract JSDoc comment
        let doc_comment = self.base.find_doc_comment(&node);

        self.base.create_symbol(
            &node,
            binding.local_name.clone(),
            SymbolKind::Import,
            SymbolOptions {
                signature: Some(self.base.get_node_text(&node)),
                visibility: None,
                parent_id,
                metadata: Some(metadata),
                doc_comment,
                annotations: Vec::new(),
            },
        )
    }

    /// Extract import bindings: local name + optional original imported name.
    pub(super) fn extract_import_specifiers(&self, node: &Node) -> Vec<ImportBinding> {
        let mut bindings = Vec::new();

        // Look for import clause which contains the specifiers (reference logic)
        let import_clause = node
            .children(&mut node.walk())
            .find(|c| c.kind() == "import_clause");
        if let Some(clause) = import_clause {
            for child in clause.children(&mut clause.walk()) {
                match child.kind() {
                    "import_specifier" => {
                        if let Some(binding) = self.import_specifier_binding(&child) {
                            bindings.push(binding);
                        }
                    }
                    "identifier" => {
                        // Default imports like React
                        let name = self.base.get_node_text(&child);
                        bindings.push(ImportBinding {
                            local_name: name,
                            imported_name: None,
                            is_default: true,
                            is_namespace: false,
                        });
                    }
                    "namespace_import" => {
                        if let Some(local_node) = child
                            .children(&mut child.walk())
                            .find(|c| c.kind() == "identifier")
                        {
                            let local = self.base.get_node_text(&local_node);
                            bindings.push(ImportBinding {
                                local_name: local,
                                imported_name: Some("*".to_string()),
                                is_default: false,
                                is_namespace: true,
                            });
                        }
                    }
                    "named_imports" => {
                        for named_child in child.children(&mut child.walk()) {
                            if named_child.kind() == "import_specifier"
                                && let Some(binding) = self.import_specifier_binding(&named_child)
                            {
                                bindings.push(binding);
                            }
                        }
                    }
                    _ => {}
                }
            }
        }

        bindings
    }

    fn import_specifier_binding(&self, child: &Node) -> Option<ImportBinding> {
        if let Some(alias_node) = child.child_by_field_name("alias") {
            let local = self.base.get_node_text(&alias_node);
            let imported = child
                .child_by_field_name("name")
                .map(|n| self.base.get_node_text(&n));
            Some(ImportBinding {
                local_name: local,
                imported_name: imported,
                is_default: false,
                is_namespace: false,
            })
        } else if let Some(name_node) = child.child_by_field_name("name") {
            let name = self.base.get_node_text(&name_node);
            Some(ImportBinding {
                local_name: name.clone(),
                imported_name: Some(name),
                is_default: false,
                is_namespace: false,
            })
        } else {
            None
        }
    }
}

/// One local import binding extracted from an import statement.
#[derive(Debug, Clone)]
pub(super) struct ImportBinding {
    pub local_name: String,
    pub imported_name: Option<String>,
    pub is_default: bool,
    pub is_namespace: bool,
}
