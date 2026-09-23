//! Import and export statement extraction
//!
//! This module handles extraction of import and export statements,
//! including named imports/exports, default exports, and re-exports.

use crate::base::{Symbol, SymbolKind, SymbolOptions};
use crate::typescript::TypeScriptExtractor;
use serde_json::json;
use std::collections::HashMap;
use tree_sitter::Node;

/// Extract an import statement: one row per binding, one row named by the
/// source for a side-effect import (`import './polyfills'`), and a CommonJS
/// row for `import x = require('m')`.
pub(super) fn extract_import(extractor: &mut TypeScriptExtractor, node: Node) -> Vec<Symbol> {
    let source = import_source(extractor, node);
    let signature = extractor.base().get_node_text(&node);
    let doc_comment = extractor.base().find_doc_comment(&node);
    if let Some(require_clause) = node
        .children(&mut node.walk())
        .find(|child| child.kind() == "import_require_clause")
    {
        return import_require_row(extractor, node, require_clause, signature, doc_comment)
            .into_iter()
            .collect();
    }
    let bindings = extract_import_bindings(extractor, node);
    if bindings.is_empty() && !source.is_empty() && !has_import_clause(node) {
        let metadata = HashMap::from([
            ("source".to_string(), json!(source.clone())),
            ("isSideEffect".to_string(), json!(true)),
        ]);
        return vec![extractor.base_mut().create_symbol(
            &node,
            source,
            SymbolKind::Import,
            SymbolOptions {
                signature: Some(signature),
                metadata: Some(metadata),
                doc_comment,
                ..Default::default()
            },
        )];
    }
    bindings
        .into_iter()
        .map(|binding| {
            let mut metadata = HashMap::new();
            metadata.insert("source".to_string(), json!(source.clone()));
            metadata.insert("specifier".to_string(), json!(binding.local_name.clone()));
            metadata.insert("importedName".to_string(), json!(binding.imported_name));
            metadata.insert("isDefault".to_string(), json!(binding.is_default));
            metadata.insert("isNamespace".to_string(), json!(binding.is_namespace));
            metadata.insert("isTypeOnly".to_string(), json!(binding.is_type_only));

            extractor.base_mut().create_symbol(
                &node,
                binding.local_name,
                SymbolKind::Import,
                SymbolOptions {
                    signature: Some(signature.clone()),
                    metadata: Some(metadata),
                    doc_comment: doc_comment.clone(),
                    ..Default::default()
                },
            )
        })
        .collect()
}

fn has_import_clause(node: Node) -> bool {
    node.children(&mut node.walk())
        .any(|child| child.kind() == "import_clause")
}

fn import_require_row(
    extractor: &mut TypeScriptExtractor,
    node: Node,
    require_clause: Node,
    signature: String,
    doc_comment: Option<String>,
) -> Option<Symbol> {
    let binding = require_clause
        .children(&mut require_clause.walk())
        .find(|child| child.kind() == "identifier")?;
    let name = extractor.base().get_node_text(&binding);
    let source = require_clause
        .child_by_field_name("source")
        .map(|source| {
            extractor
                .base()
                .get_node_text(&source)
                .trim_matches(|c| c == '"' || c == '\'' || c == '`')
                .to_string()
        })
        .unwrap_or_default();
    let metadata = HashMap::from([
        ("source".to_string(), json!(source)),
        ("isCommonJS".to_string(), json!(true)),
    ]);
    Some(extractor.base_mut().create_symbol(
        &node,
        name,
        SymbolKind::Import,
        SymbolOptions {
            signature: Some(signature),
            metadata: Some(metadata),
            doc_comment,
            ..Default::default()
        },
    ))
}

/// `import('./m')`: a dynamic import with a literal source.
pub(super) fn is_dynamic_import(node: Node) -> bool {
    node.child_by_field_name("function")
        .is_some_and(|function| function.kind() == "import")
}

/// An import row for a dynamic `import('./m')`, named by its source.
pub(super) fn extract_dynamic_import(
    extractor: &mut TypeScriptExtractor,
    node: Node,
    parent_id: Option<&str>,
) -> Option<Symbol> {
    crate::javascript::exports::dynamic_import_row(extractor.base_mut(), node, parent_id)
}

#[derive(Debug)]
struct ImportBinding {
    local_name: String,
    imported_name: String,
    is_default: bool,
    is_namespace: bool,
    is_type_only: bool,
}

fn import_source(extractor: &TypeScriptExtractor, node: Node) -> String {
    node.child_by_field_name("source")
        .map(|source| {
            extractor
                .base()
                .get_node_text(&source)
                .trim_matches(|c| c == '"' || c == '\'' || c == '`')
                .to_string()
        })
        .unwrap_or_default()
}

fn extract_import_bindings(extractor: &TypeScriptExtractor, node: Node) -> Vec<ImportBinding> {
    let Some(clause) = node
        .children(&mut node.walk())
        .find(|child| child.kind() == "import_clause")
    else {
        return Vec::new();
    };

    let is_type_only = extractor
        .base()
        .get_node_text(&node)
        .trim_start()
        .starts_with("import type");
    let mut bindings = Vec::new();
    let mut cursor = clause.walk();
    for child in clause.children(&mut cursor) {
        match child.kind() {
            "identifier" => {
                let local_name = extractor.base().get_node_text(&child);
                bindings.push(ImportBinding {
                    imported_name: "default".to_string(),
                    local_name,
                    is_default: true,
                    is_namespace: false,
                    is_type_only,
                });
            }
            "named_imports" => extract_named_imports(extractor, child, is_type_only, &mut bindings),
            "namespace_import" => {
                if let Some(local_node) = child
                    .children(&mut child.walk())
                    .find(|candidate| candidate.kind() == "identifier")
                {
                    let local_name = extractor.base().get_node_text(&local_node);
                    bindings.push(ImportBinding {
                        imported_name: "*".to_string(),
                        local_name,
                        is_default: false,
                        is_namespace: true,
                        is_type_only,
                    });
                }
            }
            _ => {}
        }
    }
    bindings
}

fn extract_named_imports(
    extractor: &TypeScriptExtractor,
    named_imports: Node,
    statement_type_only: bool,
    bindings: &mut Vec<ImportBinding>,
) {
    let mut cursor = named_imports.walk();
    for specifier in named_imports.children(&mut cursor) {
        if specifier.kind() != "import_specifier" {
            continue;
        }
        let Some(name_node) = specifier.child_by_field_name("name") else {
            continue;
        };
        let imported_name = extractor.base().get_node_text(&name_node);
        let local_name = specifier
            .child_by_field_name("alias")
            .map(|alias| extractor.base().get_node_text(&alias))
            .unwrap_or_else(|| imported_name.clone());
        // Specifier-level `import { type X }` is type-only even when the
        // statement is a value import (`import { type X, y } from ...`).
        let specifier_type_only = statement_type_only
            || specifier
                .children(&mut specifier.walk())
                .any(|child| child.kind() == "type");
        bindings.push(ImportBinding {
            local_name,
            imported_name,
            is_default: false,
            is_namespace: false,
            is_type_only: specifier_type_only,
        });
    }
}

/// Extract an export statement: one row per exported name.
pub(super) fn extract_export(extractor: &mut TypeScriptExtractor, node: Node) -> Vec<Symbol> {
    crate::javascript::exports::extract_export_rows(extractor.base_mut(), node, None)
}
