//! Class extraction for GDScript

use super::helpers::{doc_comment, doc_comment_after, find_child_by_type};
use crate::base::{BaseExtractor, Symbol, SymbolKind, SymbolOptions, Visibility};
use serde_json::Value;
use std::collections::HashMap;
use tree_sitter::Node;

/// The class a script file defines: named by its top-level `class_name`, else
/// by the file name when the script has a top-level `extends`. A file that
/// concatenates several `class_name` scripts yields one class per statement,
/// in source order.
pub(super) fn extract_script_classes(base: &mut BaseExtractor, root: Node) -> Vec<Symbol> {
    let mut cursor = root.walk();
    let children: Vec<Node> = root.named_children(&mut cursor).collect();
    let class_names: Vec<usize> = (0..children.len())
        .filter(|&index| children[index].kind() == "class_name_statement")
        .collect();
    let first_extends = children
        .iter()
        .copied()
        .find(|child| child.kind() == "extends_statement");

    if class_names.is_empty() {
        let Some(extends) = first_extends else {
            return Vec::new();
        };
        let Some(base_class) = extends_base_name(base, extends) else {
            return Vec::new();
        };
        let name = base
            .file_path
            .rsplit('/')
            .next()
            .unwrap_or("ImplicitClass")
            .trim_end_matches(".gd")
            .to_string();
        let signature = format!("extends {base_class}");
        let doc = doc_comment(base, extends).or_else(|| doc_comment_after(base, extends));
        return vec![create_class(
            base,
            extends,
            name,
            signature,
            Some(base_class),
            doc,
        )];
    }

    let single = class_names.len() == 1;
    class_names
        .into_iter()
        .filter_map(|index| {
            let node = children[index];
            let name = base.get_node_text(&find_child_by_type(&node, "name")?);
            let adjacent_extends = [index.checked_sub(1), Some(index + 1)]
                .into_iter()
                .flatten()
                .filter_map(|i| children.get(i).copied())
                .find(|child| child.kind() == "extends_statement");
            let extends = node
                .child_by_field_name("extends")
                .or(adjacent_extends)
                .or(first_extends.filter(|_| single));
            let base_class = extends.and_then(|extends| extends_base_name(base, extends));
            let last_header = extends
                .filter(|extends| extends.end_byte() > node.end_byte())
                .unwrap_or(node);
            let doc = doc_comment(base, node)
                .or_else(|| extends.and_then(|extends| doc_comment(base, extends)))
                .or_else(|| doc_comment_after(base, last_header));
            let signature = class_name_signature(base, node);
            Some(create_class(base, node, name, signature, base_class, doc))
        })
        .collect()
}

fn create_class(
    base: &mut BaseExtractor,
    anchor: Node,
    name: String,
    signature: String,
    base_class: Option<String>,
    doc: Option<String>,
) -> Symbol {
    let mut symbol = base.create_symbol(
        &anchor,
        name,
        SymbolKind::Class,
        SymbolOptions {
            signature: Some(signature),
            visibility: Some(Visibility::Public),
            parent_id: None,
            metadata: base_class_metadata(base_class),
            doc_comment: None,
            annotations: Vec::new(),
        },
    );
    symbol.doc_comment = doc;
    symbol
}

/// Extract an inner `class Name [extends Base]:` definition over its whole span.
pub(super) fn extract_inner_class(
    base: &mut BaseExtractor,
    node: Node,
    parent_id: Option<&String>,
) -> Option<Symbol> {
    let name = base.get_node_text(&node.child_by_field_name("name")?);
    let extends = node.child_by_field_name("extends").or_else(|| {
        let body = node.child_by_field_name("body")?;
        let mut cursor = body.walk();
        body.named_children(&mut cursor)
            .find(|child| child.kind() == "extends_statement")
    });
    let base_class = extends.and_then(|node| extends_base_name(base, node));
    let signature = match &base_class {
        Some(base_class) => format!("class {name} extends {base_class}:"),
        None => format!("class {name}:"),
    };
    let doc = doc_comment(base, node);

    let mut symbol = base.create_symbol(
        &node,
        name,
        SymbolKind::Class,
        SymbolOptions {
            signature: Some(signature),
            visibility: Some(Visibility::Public),
            parent_id: parent_id.cloned(),
            metadata: base_class_metadata(base_class),
            doc_comment: None,
            annotations: Vec::new(),
        },
    );
    symbol.doc_comment = doc;
    Some(symbol)
}

/// `class_name X` text, preceded by the script annotation (`@tool`, `@icon`)
/// found among the header lines above it.
fn class_name_signature(base: &BaseExtractor, node: Node) -> String {
    let signature = base.get_node_text(&node);
    let mut current = node.prev_named_sibling();
    while let Some(sibling) = current {
        match sibling.kind() {
            "annotation" => return format!("{}\n{signature}", base.get_node_text(&sibling)),
            "comment" | "extends_statement" => current = sibling.prev_named_sibling(),
            _ => break,
        }
    }
    signature
}

/// `baseClass` (string) and canonical `base_types` (array) for a declared base.
fn base_class_metadata(base_class: Option<String>) -> Option<HashMap<String, Value>> {
    let base_class = base_class?;
    Some(HashMap::from([
        (
            "base_types".to_string(),
            Value::Array(vec![Value::String(base_class.clone())]),
        ),
        ("baseClass".to_string(), Value::String(base_class)),
    ]))
}

/// The base named by an `extends` statement: a type name, or a script path
/// with its quotes removed.
pub(super) fn extends_base_name(base: &BaseExtractor, extends_node: Node) -> Option<String> {
    if let Some(type_node) = find_child_by_type(&extends_node, "type") {
        return Some(base.get_node_text(&type_node));
    }
    let text = base.get_node_text(&extends_node);
    let target = text.trim().strip_prefix("extends")?.trim();
    let target = target
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
        .or_else(|| {
            target
                .strip_prefix('\'')
                .and_then(|value| value.strip_suffix('\''))
        })
        .unwrap_or(target)
        .trim();
    (!target.is_empty()).then(|| target.to_string())
}
