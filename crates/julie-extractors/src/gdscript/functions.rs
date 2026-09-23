//! Function and constructor extraction for GDScript

use super::helpers::{declaration_annotations, doc_comment, member_visibility};
use crate::base::{
    BaseExtractor, Symbol, SymbolKind, SymbolOptions, Visibility, normalize_annotations,
};
use crate::test_detection::apply_callable_test_metadata;
use std::collections::HashMap;
use tree_sitter::Node;

/// Extract constructor (_init) definition
pub(super) fn extract_constructor_definition(
    base: &mut BaseExtractor,
    node: Node,
    parent_id: Option<&String>,
) -> Option<Symbol> {
    let signature = synthetic_signature(base, node);
    let doc_comment = doc_comment(base, node);

    let mut symbol = base.create_symbol(
        &node,
        "_init".to_string(),
        SymbolKind::Constructor,
        SymbolOptions {
            signature: Some(signature),
            visibility: Some(Visibility::Public),
            parent_id: parent_id.cloned(),
            metadata: None,
            doc_comment: None,
            annotations: Vec::new(),
        },
    );
    symbol.doc_comment = doc_comment;
    Some(symbol)
}

/// Extract a `func` definition, or a named lambda, as a function or method.
pub(super) fn extract_function_definition(
    base: &mut BaseExtractor,
    node: Node,
    parent_id: Option<&String>,
    in_class: bool,
) -> Option<Symbol> {
    let name = base.get_node_text(&node.child_by_field_name("name")?);
    let annotations = declaration_annotations(base, node, false);
    let signature = annotations.signature(&synthetic_signature(base, node));
    let visibility = member_visibility(&name);
    let kind = function_kind(&name, in_class && node.kind() == "function_definition");
    let doc_comment = doc_comment(base, node);

    let mut metadata = HashMap::new();
    apply_callable_test_metadata(
        "gdscript",
        &name,
        &base.file_path,
        &kind,
        &[],
        doc_comment.as_deref(),
        &mut metadata,
    );

    let mut symbol = base.create_symbol(
        &node,
        name,
        kind,
        SymbolOptions {
            signature: Some(signature),
            visibility: Some(visibility),
            parent_id: parent_id.cloned(),
            metadata: if metadata.is_empty() {
                None
            } else {
                Some(metadata)
            },
            doc_comment: None,
            annotations: normalize_annotations(&annotations.all, "gdscript"),
        },
    );
    symbol.doc_comment = doc_comment;
    if node.child_by_field_name("body").is_none() {
        symbol.body_span = None;
        symbol.body_hash = None;
    }
    Some(symbol)
}

fn synthetic_signature(base: &BaseExtractor, node: Node) -> String {
    if let Some(body) = node.child_by_field_name("body") {
        base.content[node.start_byte()..body.start_byte()]
            .trim_end()
            .to_string()
    } else {
        base.get_node_text(&node).trim_end().to_string()
    }
}

fn function_kind(name: &str, in_class: bool) -> SymbolKind {
    if name == "_init" {
        SymbolKind::Constructor
    } else if in_class {
        SymbolKind::Method
    } else {
        SymbolKind::Function
    }
}

/// Attempt to recover a function declaration from an ERROR leaf node.
///
/// GDScript's indentation-sensitive parser can corrupt a `match` body when
/// pattern labels are bare identifiers (e.g. `NOTIFICATION_EXIT_TREE:`).  The
/// error-recovery pass sometimes folds the immediately following `func name`
/// into a childless ERROR leaf.  In that state the normal `function_definition`
/// and `func` traversal arms never fire, and the function is silently dropped.
///
/// This function detects the pattern — trimmed text starts with `"func "` and
/// is followed by a valid GDScript identifier — and synthesises a minimal symbol
/// for the recovered declaration.  If the next `arguments` sibling is present
/// (the `()` is frequently parsed as a standalone `arguments` node) its text is
/// included in the synthetic signature.
pub(super) fn try_recover_function_from_error(
    base: &mut BaseExtractor,
    node: Node,
    parent_id: Option<&String>,
    in_class: bool,
) -> Option<Symbol> {
    let text = base.get_node_text(&node);
    let trimmed = text.trim_start();
    let after_func = trimmed.strip_prefix("func ")?;

    // Collect the function name — all alphanumeric + underscore chars.
    let func_name: String = after_func
        .chars()
        .take_while(|c| c.is_alphanumeric() || *c == '_')
        .collect();
    if func_name.is_empty() {
        return None;
    }

    // Look for the `arguments` node that follows this ERROR sibling in the same
    // parent (common when `func name()` is absorbed into error-recovery).
    let params_text = node
        .parent()
        .and_then(|parent| {
            let mut found_self = false;
            let mut cursor = parent.walk();
            for sib in parent.children(&mut cursor) {
                if sib.id() == node.id() {
                    found_self = true;
                    continue;
                }
                if found_self && sib.kind() == "arguments" {
                    return Some(base.get_node_text(&sib));
                }
            }
            None
        })
        .unwrap_or_else(|| "()".to_string());

    let signature = format!("func {}{}", func_name, params_text);
    let visibility = member_visibility(&func_name);
    let kind = function_kind(&func_name, in_class);

    let mut metadata = HashMap::new();
    apply_callable_test_metadata(
        "gdscript",
        &func_name,
        &base.file_path,
        &kind,
        &[],
        None,
        &mut metadata,
    );

    Some(base.create_symbol(
        &node,
        func_name,
        kind,
        SymbolOptions {
            signature: Some(signature),
            visibility: Some(visibility),
            parent_id: parent_id.cloned(),
            metadata: if metadata.is_empty() {
                None
            } else {
                Some(metadata)
            },
            doc_comment: None,
            annotations: Vec::new(),
        },
    ))
}
