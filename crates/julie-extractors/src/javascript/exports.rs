//! Export rows shared by the ECMAScript extractors.
//!
//! One `export` symbol per exported name. Every row carries `exportedName`,
//! `isDefault`, and `isNamed`; rows for a local binding add `localName`,
//! re-exports add `source`, `export * as ns` adds `isNamespace`, `export *`
//! adds `isStar`, and TypeScript type-only specifiers add `isTypeOnly`.
//! Rows for an exported declaration carry no signature or doc comment: the
//! declaration's own symbol has them.

use crate::base::{BaseExtractor, Symbol, SymbolKind, SymbolOptions};
use serde_json::json;
use std::collections::{HashMap, HashSet};
use tree_sitter::Node;

#[derive(Default)]
struct ExportRow {
    name: String,
    local_name: Option<String>,
    is_default: bool,
    is_namespace: bool,
    is_star: bool,
    is_type_only: bool,
}

/// The export rows of one `export_statement`.
pub(crate) fn extract_export_rows(
    base: &mut BaseExtractor,
    node: Node,
    parent_id: Option<&str>,
) -> Vec<Symbol> {
    let source = node.child_by_field_name("source").map(|source| {
        base.get_node_text(&source)
            .trim_matches(|c| c == '"' || c == '\'' || c == '`')
            .to_string()
    });
    let is_default = has_child_kind(node, "default");
    let declaration = node.child_by_field_name("declaration");
    let statement_type_only = has_child_kind(node, "type");

    let mut rows = Vec::new();
    if let Some(declaration) = declaration {
        for name in declared_names(base, declaration) {
            rows.push(ExportRow {
                local_name: Some(name.clone()),
                name,
                is_default,
                ..Default::default()
            });
        }
    } else if let Some(value) = node.child_by_field_name("value") {
        let local_name = match value.kind() {
            "identifier" => Some(base.get_node_text(&value)),
            "class" | "function_expression" | "arrow_function" | "generator_function" => Some(
                value
                    .child_by_field_name("name")
                    .map_or_else(|| "default".to_string(), |name| base.get_node_text(&name)),
            ),
            _ => value
                .child_by_field_name("name")
                .map(|name| base.get_node_text(&name)),
        };
        rows.push(ExportRow {
            name: local_name.clone().unwrap_or_else(|| "default".to_string()),
            local_name,
            is_default: true,
            ..Default::default()
        });
    } else {
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            match child.kind() {
                "export_clause" => {
                    let mut clause_cursor = child.walk();
                    for specifier in child.named_children(&mut clause_cursor) {
                        if specifier.kind() != "export_specifier" {
                            continue;
                        }
                        let Some(name_node) = specifier.child_by_field_name("name") else {
                            continue;
                        };
                        let local_name = base.get_node_text(&name_node);
                        let name = specifier
                            .child_by_field_name("alias")
                            .map(|alias| base.get_node_text(&alias))
                            .unwrap_or_else(|| local_name.clone());
                        rows.push(ExportRow {
                            is_default: name == "default",
                            name,
                            local_name: Some(local_name),
                            is_type_only: statement_type_only || has_child_kind(specifier, "type"),
                            ..Default::default()
                        });
                    }
                }
                "namespace_export" => {
                    let mut ns_cursor = child.walk();
                    if let Some(alias) = child
                        .named_children(&mut ns_cursor)
                        .find(|candidate| matches!(candidate.kind(), "identifier" | "string"))
                    {
                        rows.push(ExportRow {
                            name: base.get_node_text(&alias),
                            is_namespace: true,
                            ..Default::default()
                        });
                    }
                }
                // TypeScript `export = helper`.
                "identifier" if has_child_kind(node, "=") => {
                    let name = base.get_node_text(&child);
                    rows.push(ExportRow {
                        local_name: Some(name.clone()),
                        name,
                        is_default: true,
                        ..Default::default()
                    });
                }
                _ => {}
            }
        }
        if rows.is_empty() && has_child_kind(node, "*") && source.is_some() {
            rows.push(ExportRow {
                name: "*".to_string(),
                is_star: true,
                ..Default::default()
            });
        }
    }

    let names_code_elsewhere = declaration.is_some()
        || node
            .child_by_field_name("value")
            .is_some_and(|value| value.kind() != "identifier");
    let (signature, doc_comment) = if names_code_elsewhere {
        (None, None)
    } else {
        (
            Some(base.get_node_text(&node)),
            base.find_doc_comment(&node),
        )
    };

    rows.into_iter()
        .map(|row| {
            let mut metadata = HashMap::from([
                ("exportedName".to_string(), json!(row.name)),
                ("isDefault".to_string(), json!(row.is_default)),
                ("isNamed".to_string(), json!(!row.is_default)),
            ]);
            if let Some(local_name) = &row.local_name {
                metadata.insert("localName".to_string(), json!(local_name));
            }
            if let Some(source) = &source {
                metadata.insert("source".to_string(), json!(source));
            }
            if row.is_namespace {
                metadata.insert("isNamespace".to_string(), json!(true));
            }
            if row.is_star {
                metadata.insert("isStar".to_string(), json!(true));
            }
            if row.is_type_only {
                metadata.insert("isTypeOnly".to_string(), json!(true));
            }
            let mut row_symbol = base.create_symbol(
                &node,
                row.name,
                SymbolKind::Export,
                SymbolOptions {
                    signature: signature.clone(),
                    parent_id: parent_id.map(str::to_string),
                    metadata: Some(metadata),
                    doc_comment: doc_comment.clone(),
                    ..Default::default()
                },
            );
            row_symbol.doc_comment = doc_comment.clone();
            row_symbol
        })
        .collect()
}

/// The names an exported declaration binds. An overload signature binds
/// nothing: its implementation's export row names the function once.
fn declared_names(base: &BaseExtractor, declaration: Node) -> Vec<String> {
    let declaration = declaration_of(declaration);
    match declaration.kind() {
        "lexical_declaration" | "variable_declaration" => {
            let mut names = Vec::new();
            let mut cursor = declaration.walk();
            for declarator in declaration.named_children(&mut cursor) {
                if declarator.kind() != "variable_declarator" {
                    continue;
                }
                let Some(name) = declarator.child_by_field_name("name") else {
                    continue;
                };
                if name.kind() == "identifier" {
                    names.push(base.get_node_text(&name));
                } else {
                    let mut bindings = Vec::new();
                    super::variables::collect_pattern_bindings(name, None, &mut bindings);
                    names.extend(
                        bindings
                            .into_iter()
                            .map(|binding| base.get_node_text(&binding.name)),
                    );
                }
            }
            names
        }
        "function_signature" => declaration_name(base, declaration)
            .filter(|name| !is_overload_signature(base, declaration, name))
            .into_iter()
            .collect(),
        _ => declaration_name(base, declaration)
            .map(|name| {
                name.trim_matches(|c| c == '"' || c == '\'' || c == '`')
                    .to_string()
            })
            .into_iter()
            .collect(),
    }
}

fn has_child_kind(node: Node, kind: &str) -> bool {
    let mut cursor = node.walk();
    node.children(&mut cursor).any(|child| child.kind() == kind)
}

fn declaration_name(base: &BaseExtractor, declaration: Node) -> Option<String> {
    declaration
        .child_by_field_name("name")
        .map(|name| base.get_node_text(&name))
}

/// A TypeScript `function_signature` followed, in the same scope, by more
/// signatures of the same name and then the implementation.
pub(crate) fn is_overload_signature(base: &BaseExtractor, node: Node, name: &str) -> bool {
    let mut current = statement_of(node).next_named_sibling();
    while let Some(sibling) = current {
        let declaration = declaration_of(sibling);
        if declaration_name(base, declaration).as_deref() != Some(name) {
            return false;
        }
        match declaration.kind() {
            "function_declaration" | "generator_function_declaration" => return true,
            "function_signature" => current = sibling.next_named_sibling(),
            _ => return false,
        }
    }
    false
}

/// The overload signatures written directly before a function implementation,
/// in source order.
pub(crate) fn overload_signature_nodes<'t>(
    base: &BaseExtractor,
    node: Node<'t>,
    name: &str,
) -> Vec<Node<'t>> {
    let mut overloads = Vec::new();
    let mut current = statement_of(node).prev_named_sibling();
    while let Some(sibling) = current {
        let declaration = declaration_of(sibling);
        if declaration.kind() != "function_signature"
            || declaration_name(base, declaration).as_deref() != Some(name)
        {
            break;
        }
        overloads.push(declaration);
        current = sibling.prev_named_sibling();
    }
    overloads.reverse();
    overloads
}

fn statement_of(node: Node) -> Node {
    let mut statement = node;
    while let Some(parent) = statement
        .parent()
        .filter(|parent| matches!(parent.kind(), "export_statement" | "ambient_declaration"))
    {
        statement = parent;
    }
    statement
}

fn declaration_of(statement: Node) -> Node {
    let mut declaration = statement;
    loop {
        let inner = match declaration.kind() {
            "export_statement" => declaration.child_by_field_name("declaration"),
            "ambient_declaration" => {
                let mut cursor = declaration.walk();
                declaration.named_children(&mut cursor).next()
            }
            _ => None,
        };
        match inner {
            Some(inner) => declaration = inner,
            None => return declaration,
        }
    }
}

/// An import row for a dynamic `import('./m')` with a literal source, named by
/// the source and marked `isDynamic`. Computed sources record nothing.
pub(crate) fn dynamic_import_row(
    base: &mut BaseExtractor,
    call_node: Node,
    parent_id: Option<&str>,
) -> Option<Symbol> {
    let arguments = call_node.child_by_field_name("arguments")?;
    let mut cursor = arguments.walk();
    let source_node = arguments
        .named_children(&mut cursor)
        .next()
        .filter(|argument| argument.kind() == "string")?;
    let source = base
        .get_node_text(&source_node)
        .trim_matches(|c| c == '"' || c == '\'' || c == '`')
        .to_string();
    if source.is_empty() {
        return None;
    }
    let metadata = HashMap::from([
        ("source".to_string(), json!(source.clone())),
        ("isDynamic".to_string(), json!(true)),
    ]);
    Some(base.create_symbol(
        &call_node,
        source,
        SymbolKind::Import,
        SymbolOptions {
            signature: Some(base.get_node_text(&call_node)),
            parent_id: parent_id.map(str::to_string),
            metadata: Some(metadata),
            ..Default::default()
        },
    ))
}

/// Local names a module exports without an `export` wrapper on their
/// declaration: `export { a, b as c }` (no source), `export default a`, and
/// TypeScript `export = a`.
pub(crate) fn locally_exported_names(base: &BaseExtractor, root: Node) -> HashSet<String> {
    let mut names = HashSet::new();
    let mut cursor = root.walk();
    for statement in root.named_children(&mut cursor) {
        if statement.kind() != "export_statement"
            || statement.child_by_field_name("source").is_some()
            || statement.child_by_field_name("declaration").is_some()
        {
            continue;
        }
        if let Some(value) = statement
            .child_by_field_name("value")
            .filter(|value| value.kind() == "identifier")
        {
            names.insert(base.get_node_text(&value));
        }
        let mut statement_cursor = statement.walk();
        for child in statement.named_children(&mut statement_cursor) {
            match child.kind() {
                "export_clause" => {
                    let mut clause_cursor = child.walk();
                    for specifier in child.named_children(&mut clause_cursor) {
                        if let Some(name) = specifier.child_by_field_name("name") {
                            names.insert(base.get_node_text(&name));
                        }
                    }
                }
                "identifier" if has_child_kind(statement, "=") => {
                    names.insert(base.get_node_text(&child));
                }
                _ => {}
            }
        }
    }
    names
}
