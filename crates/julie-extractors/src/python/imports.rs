/// Import statement extraction
/// Handles import, from...import, and aliased imports. Each binding carries
/// structured metadata: `source` (the module text, with leading dots for a
/// relative import), `importedName`, `specifier` (the bound local name),
/// `isWildcard`, and `relativeLevel` for relative imports.
use super::super::base::{Symbol, SymbolKind, SymbolOptions, Visibility};
use super::{PythonExtractor, helpers};
use serde_json::json;
use std::collections::HashMap;
use tree_sitter::Node;

struct ImportBinding {
    local_name: String,
    imported_name: String,
    source: String,
    signature: String,
}

/// Extract imports from an import or import_from statement
pub fn extract_imports(extractor: &mut PythonExtractor, node: Node) -> Vec<Symbol> {
    let mut bindings = Vec::new();

    match node.kind() {
        "import_statement" => {
            let mut cursor = node.walk();
            for child in node.children_by_field_name("name", &mut cursor) {
                match child.kind() {
                    "aliased_import" => {
                        if let Some((module_name, alias)) = extract_alias(extractor, &child) {
                            bindings.push(ImportBinding {
                                signature: format!("import {module_name} as {alias}"),
                                local_name: alias,
                                imported_name: module_name.clone(),
                                source: module_name,
                            });
                        }
                    }
                    "dotted_name" => {
                        let module_name = extractor.base().get_node_text(&child);
                        let bound_name = module_name
                            .split('.')
                            .next()
                            .unwrap_or(&module_name)
                            .to_string();
                        bindings.push(ImportBinding {
                            signature: format!("import {module_name}"),
                            local_name: bound_name.clone(),
                            imported_name: bound_name,
                            source: module_name,
                        });
                    }
                    _ => {}
                }
            }
        }
        "import_from_statement" => {
            if let Some(module_node) = node.child_by_field_name("module_name") {
                let module = extractor.base().get_node_text(&module_node);
                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    if Some(child.id()) == Some(module_node.id()) {
                        continue;
                    }
                    match child.kind() {
                        "dotted_name" => {
                            let name = extractor.base().get_node_text(&child);
                            bindings.push(ImportBinding {
                                signature: format!("from {module} import {name}"),
                                local_name: name.clone(),
                                imported_name: name,
                                source: module.clone(),
                            });
                        }
                        "aliased_import" => {
                            if let Some((name, alias)) = extract_alias(extractor, &child) {
                                bindings.push(ImportBinding {
                                    signature: format!("from {module} import {name} as {alias}"),
                                    local_name: alias,
                                    imported_name: name,
                                    source: module.clone(),
                                });
                            }
                        }
                        "wildcard_import" => bindings.push(ImportBinding {
                            signature: format!("from {module} import *"),
                            local_name: "*".to_string(),
                            imported_name: "*".to_string(),
                            source: module.clone(),
                        }),
                        _ => {}
                    }
                }
            }
        }
        _ => {}
    }

    bindings
        .into_iter()
        .map(|binding| create_import_symbol(extractor, &node, binding))
        .collect()
}

/// Extract alias from an aliased_import node
fn extract_alias(extractor: &PythonExtractor, node: &Node) -> Option<(String, String)> {
    let name = node.child_by_field_name("name")?;
    let alias = node.child_by_field_name("alias")?;
    Some((
        extractor.base().get_node_text(&name),
        extractor.base().get_node_text(&alias),
    ))
}

fn create_import_symbol(
    extractor: &mut PythonExtractor,
    node: &Node,
    binding: ImportBinding,
) -> Symbol {
    let doc_comment = extractor.base().find_doc_comment(node);
    let relative_level = binding.source.chars().take_while(|c| *c == '.').count();
    let mut metadata = HashMap::from([
        ("source".to_string(), json!(binding.source)),
        ("importedName".to_string(), json!(binding.imported_name)),
        ("specifier".to_string(), json!(binding.local_name)),
        ("isWildcard".to_string(), json!(binding.local_name == "*")),
    ]);
    if relative_level > 0 {
        metadata.insert("relativeLevel".to_string(), json!(relative_level));
    }

    helpers::without_body(extractor.base_mut().create_symbol(
        node,
        binding.local_name,
        SymbolKind::Import,
        SymbolOptions {
            signature: Some(binding.signature),
            visibility: Some(Visibility::Public),
            parent_id: None,
            metadata: Some(metadata),
            doc_comment,
            annotations: Vec::new(),
        },
    ))
}
