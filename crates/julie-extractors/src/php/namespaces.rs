// PHP Extractor - Namespace and import declarations, variable assignments

use super::{PhpExtractor, find_child};
use crate::base::{Symbol, SymbolKind, SymbolOptions, Visibility};
use std::collections::HashMap;
use tree_sitter::Node;

/// Extract PHP namespace declarations
pub(super) fn extract_namespace(
    extractor: &mut PhpExtractor,
    node: Node,
    parent_id: Option<&str>,
) -> Option<Symbol> {
    let base = extractor.get_base();
    let name = find_child(extractor, &node, "namespace_name").map(|n| base.get_node_text(&n))?;

    let mut metadata = HashMap::new();
    metadata.insert(
        "type".to_string(),
        serde_json::Value::String("namespace".to_string()),
    );

    // Extract PHPDoc comment
    let doc_comment = extractor.get_base().find_doc_comment(&node);

    Some(extractor.get_base_mut().create_symbol(
        &node,
        name.clone(),
        SymbolKind::Namespace,
        SymbolOptions {
            signature: Some(format!("namespace {}", name)),
            visibility: Some(Visibility::Public),
            parent_id: parent_id.map(|s| s.to_string()),
            metadata: Some(metadata),
            doc_comment,
            annotations: Vec::new(),
        },
    ))
}

/// Extract PHP use/import declarations.
///
/// Returns a Vec because grouped use declarations like `use App\{A, B, C};`
/// produce multiple Import symbols from a single AST node.
pub(super) fn extract_use(
    extractor: &mut PhpExtractor,
    node: Node,
    parent_id: Option<&str>,
) -> Vec<Symbol> {
    // Check for grouped use: `use Prefix\{A, B, C};`
    // AST: namespace_use_declaration -> namespace_name + namespace_use_group
    if let Some(group) = find_child(extractor, &node, "namespace_use_group") {
        return extract_grouped_use(extractor, &node, &group, parent_id);
    }

    // Non-grouped: `use App\Models\User;`
    // AST: namespace_use_declaration -> namespace_use_clause -> qualified_name
    find_child(extractor, &node, "namespace_use_clause")
        .and_then(|clause| extract_single_use_clause(extractor, &node, &clause, parent_id))
        .into_iter()
        .collect()
}

/// Extract symbols from a grouped use declaration like `use App\Models\{User, Post as BlogPost};`
fn extract_grouped_use(
    extractor: &mut PhpExtractor,
    decl_node: &Node,
    group_node: &Node,
    parent_id: Option<&str>,
) -> Vec<Symbol> {
    // Get the prefix from namespace_name (e.g., "App\Models")
    let prefix = find_child(extractor, decl_node, "namespace_name")
        .map(|n| extractor.get_base().get_node_text(&n))
        .unwrap_or_default();

    let doc_comment = extractor.get_base().find_doc_comment(decl_node);
    let mut symbols = Vec::new();

    // Iterate all namespace_use_clause children inside the group
    let mut cursor = group_node.walk();
    for child in group_node.children(&mut cursor) {
        if child.kind() != "namespace_use_clause" {
            continue;
        }

        // Each clause has `name` children. First name = the imported symbol.
        // If there's an `as` keyword, the name after it is the alias.
        let mut names = Vec::new();
        let mut has_as = false;
        let mut inner_cursor = child.walk();
        for clause_child in child.children(&mut inner_cursor) {
            match clause_child.kind() {
                "name" => names.push(extractor.get_base().get_node_text(&clause_child)),
                "as" => has_as = true,
                _ => {}
            }
        }

        let clause_name = match names.first() {
            Some(n) => n.clone(),
            None => continue,
        };

        let alias = if has_as && names.len() >= 2 {
            Some(names[1].clone())
        } else {
            None
        };

        // Build fully-qualified name: prefix + clause name
        let full_name = if prefix.is_empty() {
            clause_name
        } else {
            format!("{}\\{}", prefix, clause_name)
        };

        let mut signature = format!("use {}", full_name);
        if let Some(alias_text) = &alias {
            signature.push_str(&format!(" as {}", alias_text));
        }

        let mut metadata = HashMap::new();
        metadata.insert(
            "type".to_string(),
            serde_json::Value::String("use".to_string()),
        );
        if let Some(alias_text) = alias {
            metadata.insert("alias".to_string(), serde_json::Value::String(alias_text));
        }

        symbols.push(extractor.get_base_mut().create_symbol(
            decl_node,
            full_name,
            SymbolKind::Import,
            SymbolOptions {
                signature: Some(signature),
                visibility: Some(Visibility::Public),
                parent_id: parent_id.map(|s| s.to_string()),
                metadata: Some(metadata),
                doc_comment: doc_comment.clone(),
                annotations: Vec::new(),
            },
        ));
    }

    symbols
}

/// Extract a single (non-grouped) namespace_use_clause.
/// AST: namespace_use_clause -> qualified_name, possibly followed by `as` + name
fn extract_single_use_clause(
    extractor: &mut PhpExtractor,
    decl_node: &Node,
    clause: &Node,
    parent_id: Option<&str>,
) -> Option<Symbol> {
    let qualified_name = find_child(extractor, clause, "qualified_name")?;
    let name = extractor.get_base().get_node_text(&qualified_name);

    // Check for alias: `as` + `name` children inside the clause
    let alias = extract_clause_alias(extractor, clause);

    create_use_symbol(extractor, decl_node, name, alias, parent_id)
}

/// Extract legacy use_declaration format
/// Extract alias from a namespace_use_clause by looking for `as` + `name` children
fn extract_clause_alias(extractor: &PhpExtractor, clause: &Node) -> Option<String> {
    let mut cursor = clause.walk();
    let mut found_as = false;
    for child in clause.children(&mut cursor) {
        if child.kind() == "as" {
            found_as = true;
        } else if found_as && child.kind() == "name" {
            return Some(extractor.get_base().get_node_text(&child));
        }
    }
    None
}

/// Create a single Import symbol with given name and optional alias
fn create_use_symbol(
    extractor: &mut PhpExtractor,
    node: &Node,
    name: String,
    alias: Option<String>,
    parent_id: Option<&str>,
) -> Option<Symbol> {
    let mut signature = format!("use {}", name);
    if let Some(alias_text) = &alias {
        signature.push_str(&format!(" as {}", alias_text));
    }

    let mut metadata = HashMap::new();
    metadata.insert(
        "type".to_string(),
        serde_json::Value::String("use".to_string()),
    );
    if let Some(alias_text) = alias {
        metadata.insert("alias".to_string(), serde_json::Value::String(alias_text));
    }

    let doc_comment = extractor.get_base().find_doc_comment(node);

    Some(extractor.get_base_mut().create_symbol(
        node,
        name,
        SymbolKind::Import,
        SymbolOptions {
            signature: Some(signature),
            visibility: Some(Visibility::Public),
            parent_id: parent_id.map(|s| s.to_string()),
            metadata: Some(metadata),
            doc_comment,
            annotations: Vec::new(),
        },
    ))
}

/// Create the variable symbol for one assignment target `$name`.
pub(super) fn extract_variable_assignment(
    extractor: &mut PhpExtractor,
    node: Node,
    variable_name_node: Node,
    parent_id: Option<&str>,
) -> Option<Symbol> {
    let name_node = find_child(extractor, &variable_name_node, "name")?;
    let var_name = extractor.get_base().get_node_text(&name_node);
    let value_text = node
        .child_by_field_name("right")
        .map(|right| extractor.get_base().get_node_text(&right))
        .unwrap_or_default();
    let target_text = extractor.get_base().get_node_text(&variable_name_node);
    let is_whole_left = node
        .child_by_field_name("left")
        .is_some_and(|left| left.id() == variable_name_node.id());
    let signature = format!("{target_text} = {value_text}");

    let mut metadata = HashMap::new();
    metadata.insert("value".to_string(), serde_json::Value::String(value_text));

    let doc_comment = extractor.get_base().find_doc_comment(&node);
    let anchor = if is_whole_left {
        node
    } else {
        variable_name_node
    };

    Some(extractor.get_base_mut().create_symbol(
        &anchor,
        var_name,
        SymbolKind::Variable,
        SymbolOptions {
            signature: Some(signature),
            visibility: Some(Visibility::Public),
            parent_id: parent_id.map(|s| s.to_string()),
            metadata: Some(metadata),
            doc_comment,
            annotations: Vec::new(),
        },
    ))
}
