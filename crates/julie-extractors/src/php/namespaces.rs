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

/// Extract PHP use/import declarations: one Import symbol per clause, for
/// `use A;`, `use A, B\C;`, `use Prefix\{A, B as C};`, and the `use function`
/// and `use const` forms (recorded in `metadata.importKind`).
pub(super) fn extract_use(
    extractor: &mut PhpExtractor,
    node: Node,
    parent_id: Option<&str>,
) -> Vec<Symbol> {
    let prefix = find_child(extractor, &node, "namespace_name")
        .map(|prefix| extractor.get_base().get_node_text(&prefix));
    let declaration_kind = use_kind(extractor, &node);
    let clauses_parent = node.child_by_field_name("body").unwrap_or(node);
    let mut cursor = clauses_parent.walk();
    let clauses: Vec<Node> = clauses_parent
        .named_children(&mut cursor)
        .filter(|child| child.kind() == "namespace_use_clause")
        .collect();
    let single = clauses.len() == 1;
    clauses
        .into_iter()
        .filter_map(|clause| {
            let anchor = if single { node } else { clause };
            let target = clause_target(extractor, &clause)?;
            let name = match &prefix {
                Some(prefix) => format!("{prefix}\\{target}"),
                None => target,
            };
            let alias = clause
                .child_by_field_name("alias")
                .map(|alias| extractor.get_base().get_node_text(&alias));
            let kind = use_kind(extractor, &clause).or(declaration_kind);
            create_use_symbol(extractor, &node, &anchor, name, alias, kind, parent_id)
        })
        .collect()
}

/// The imported name of one clause: `Closure`, `Carbon\Carbon`, or a group
/// member such as `User`.
fn clause_target(extractor: &PhpExtractor, clause: &Node) -> Option<String> {
    let mut cursor = clause.walk();
    let target = clause
        .named_children(&mut cursor)
        .find(|child| matches!(child.kind(), "qualified_name" | "name"))?;
    Some(extractor.get_base().get_node_text(&target))
}

/// `function` or `const` for `use function` / `use const`; `None` for a class import.
fn use_kind(extractor: &PhpExtractor, node: &Node) -> Option<&'static str> {
    let kind = node.child_by_field_name("type")?;
    match extractor.get_base().get_node_text(&kind).as_str() {
        "function" => Some("function"),
        "const" => Some("const"),
        _ => None,
    }
}

/// Create a single Import symbol with given name and optional alias
fn create_use_symbol(
    extractor: &mut PhpExtractor,
    declaration: &Node,
    anchor: &Node,
    name: String,
    alias: Option<String>,
    kind: Option<&'static str>,
    parent_id: Option<&str>,
) -> Option<Symbol> {
    let mut signature = match kind {
        Some(kind) => format!("use {kind} {name}"),
        None => format!("use {name}"),
    };
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
    if let Some(kind) = kind {
        metadata.insert(
            "importKind".to_string(),
            serde_json::Value::String(kind.to_string()),
        );
    }

    let doc_comment = extractor.get_base().find_doc_comment(declaration);

    Some(extractor.get_base_mut().create_symbol(
        anchor,
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
