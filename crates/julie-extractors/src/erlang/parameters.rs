//! Parameter symbols from Erlang clause-head variable patterns.

use std::collections::HashSet;

use tree_sitter::Node;

use super::ErlangExtractor;
use super::helpers::{first_atom_text, named_children};
use super::type_facts;
use crate::base::{Symbol, SymbolKind, SymbolOptions};
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};

pub(super) fn extract_parameter_symbols(
    extractor: &mut ErlangExtractor,
    clauses: &[Node],
    callable_id: &str,
    seen: &mut HashSet<String>,
) -> Vec<Symbol> {
    let mut symbols = Vec::new();
    for declaration in clauses {
        let Some(clause) = super::helpers::find_child_by_type(declaration, "function_clause")
        else {
            continue;
        };
        let Some(args) = clause
            .child_by_field_name("args")
            .or_else(|| super::helpers::find_child_by_type(&clause, "expr_args"))
            .or_else(|| super::helpers::find_child_by_type(&clause, "var_args"))
        else {
            continue;
        };
        for pattern in named_children(&args) {
            walk_pattern(
                extractor,
                pattern,
                callable_id,
                None,
                seen,
                &mut symbols,
                0,
            );
        }
    }
    symbols
}

fn walk_pattern(
    extractor: &mut ErlangExtractor,
    node: Node,
    callable_id: &str,
    declared_record: Option<&str>,
    seen: &mut HashSet<String>,
    symbols: &mut Vec<Symbol>,
    depth: u32,
) {
    if !should_visit_tree_depth(depth) {
        return;
    }

    match node.kind() {
        "var" => emit_parameter(
            extractor,
            node,
            callable_id,
            declared_record,
            seen,
            symbols,
        ),
        "match_expr" => {
            let lhs = node.child_by_field_name("lhs");
            let rhs = node.child_by_field_name("rhs");
            let rec_lhs = lhs.as_ref().and_then(|side| record_name(extractor, side));
            let rec_rhs = rhs.as_ref().and_then(|side| record_name(extractor, side));
            let Some(child_depth) = child_tree_depth(depth) else {
                return;
            };
            if let Some(lhs) = lhs {
                walk_pattern(
                    extractor,
                    lhs,
                    callable_id,
                    rec_rhs.as_deref().or(declared_record),
                    seen,
                    symbols,
                    child_depth,
                );
            }
            if let Some(rhs) = rhs {
                walk_pattern(
                    extractor,
                    rhs,
                    callable_id,
                    rec_lhs.as_deref().or(declared_record),
                    seen,
                    symbols,
                    child_depth,
                );
            }
        }
        _ => {
            let Some(child_depth) = child_tree_depth(depth) else {
                return;
            };
            for child in named_children(&node) {
                walk_pattern(
                    extractor,
                    child,
                    callable_id,
                    None,
                    seen,
                    symbols,
                    child_depth,
                );
            }
        }
    }
}

fn record_name(extractor: &ErlangExtractor, node: &Node) -> Option<String> {
    type_facts::record_expr_name(&extractor.base, node).or_else(|| {
        if node.kind() == "record_name" {
            first_atom_text(&extractor.base, node)
        } else {
            None
        }
    })
}

fn emit_parameter(
    extractor: &mut ErlangExtractor,
    var_node: Node,
    callable_id: &str,
    declared_record: Option<&str>,
    seen: &mut HashSet<String>,
    symbols: &mut Vec<Symbol>,
) {
    let name = extractor.base.get_node_text(&var_node);
    if name.is_empty() || name == "_" || !seen.insert(name.clone()) {
        return;
    }
    let signature = extractor.base.get_node_text(&var_node);
    let metadata = std::collections::HashMap::from([(
        "role".to_string(),
        serde_json::Value::String("parameter".to_string()),
    )]);
    let symbol = extractor.base.create_symbol(
        &var_node,
        name,
        SymbolKind::Variable,
        SymbolOptions {
            signature: Some(signature),
            parent_id: Some(callable_id.to_string()),
            metadata: Some(metadata),
            ..Default::default()
        },
    );
    if let Some(record_name) = declared_record {
        type_facts::record_record_fact(&mut extractor.base, &symbol.id, record_name, false);
    }
    symbols.push(symbol);
}
