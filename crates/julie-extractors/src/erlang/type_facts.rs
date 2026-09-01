//! Record-derived type facts for Erlang variables.
//!
//! Head record patterns (`#foo{} = X`) are syntax-stated. Body record literals
//! (`X = #foo{}`) are inferred only when `foo` is a same-file `-record`.

use std::collections::HashSet;

use tree_sitter::Node;

use super::ErlangExtractor;
use super::helpers::{first_atom_text, named_children};
use crate::base::types::TypeNameRules;
use crate::base::{BaseExtractor, Symbol, SymbolKind, SymbolOptions};
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};

pub(super) const ERLANG_TYPE_NAME_RULES: TypeNameRules = TypeNameRules {
    nullable_suffixes: &[],
    reference_prefixes: &[],
    generic_open: &[],
};

pub(super) fn record_record_fact(
    base: &mut BaseExtractor,
    symbol_id: &str,
    record_name: &str,
    is_inferred: bool,
) {
    base.record_declared_type_fact_with_declared(
        symbol_id,
        record_name,
        record_name,
        &ERLANG_TYPE_NAME_RULES,
        is_inferred,
    );
}

pub(super) fn same_file_record_names(
    base: &BaseExtractor,
    declarations: &[Node],
) -> HashSet<String> {
    declarations
        .iter()
        .filter(|declaration| declaration.kind() == "record_decl")
        .filter_map(|declaration| first_atom_text(base, declaration))
        .collect()
}

pub(super) fn record_expr_name(base: &BaseExtractor, node: &Node) -> Option<String> {
    if node.kind() != "record_expr" {
        return None;
    }
    let name_node = node.child_by_field_name("name")?;
    first_atom_text(base, &name_node)
}

pub(super) fn extract_body_locals(
    extractor: &mut ErlangExtractor,
    clauses: &[Node],
    callable_id: &str,
    same_file_records: &HashSet<String>,
    seen: &mut HashSet<String>,
) -> Vec<Symbol> {
    let mut symbols = Vec::new();
    for declaration in clauses {
        let Some(clause) = super::helpers::find_child_by_type(declaration, "function_clause")
        else {
            continue;
        };
        let Some(body) = clause.child_by_field_name("body") else {
            continue;
        };
        walk_body(
            extractor,
            body,
            callable_id,
            same_file_records,
            seen,
            &mut symbols,
            0,
        );
    }
    symbols
}

fn walk_body(
    extractor: &mut ErlangExtractor,
    node: Node,
    callable_id: &str,
    same_file_records: &HashSet<String>,
    seen: &mut HashSet<String>,
    symbols: &mut Vec<Symbol>,
    depth: u32,
) {
    if !should_visit_tree_depth(depth) {
        return;
    }

    if node.kind() == "match_expr" {
        emit_match_local(
            extractor,
            node,
            callable_id,
            same_file_records,
            seen,
            symbols,
        );
    }

    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };
    for child in named_children(&node) {
        walk_body(
            extractor,
            child,
            callable_id,
            same_file_records,
            seen,
            symbols,
            child_depth,
        );
    }
}

fn emit_match_local(
    extractor: &mut ErlangExtractor,
    node: Node,
    callable_id: &str,
    same_file_records: &HashSet<String>,
    seen: &mut HashSet<String>,
    symbols: &mut Vec<Symbol>,
) {
    let Some(lhs) = node.child_by_field_name("lhs") else {
        return;
    };
    let Some(rhs) = node.child_by_field_name("rhs") else {
        return;
    };

    if lhs.kind() == "var" {
        emit_local(
            extractor,
            lhs,
            callable_id,
            record_expr_name(&extractor.base, &rhs)
                .filter(|name| same_file_records.contains(name)),
            seen,
            symbols,
        );
        return;
    }

    if rhs.kind() == "var" && record_expr_name(&extractor.base, &lhs).is_some() {
        emit_local(
            extractor,
            rhs,
            callable_id,
            record_expr_name(&extractor.base, &lhs)
                .filter(|name| same_file_records.contains(name)),
            seen,
            symbols,
        );
    }
}

fn emit_local(
    extractor: &mut ErlangExtractor,
    var_node: Node,
    callable_id: &str,
    inferred_record: Option<String>,
    seen: &mut HashSet<String>,
    symbols: &mut Vec<Symbol>,
) {
    let name = extractor.base.get_node_text(&var_node);
    if name.is_empty() || name == "_" || !seen.insert(name.clone()) {
        return;
    }
    let signature = name.clone();
    let symbol = extractor.base.create_symbol(
        &var_node,
        name,
        SymbolKind::Variable,
        SymbolOptions {
            signature: Some(signature),
            parent_id: Some(callable_id.to_string()),
            ..Default::default()
        },
    );
    if let Some(record_name) = inferred_record {
        record_record_fact(&mut extractor.base, &symbol.id, &record_name, true);
    }
    symbols.push(symbol);
}
