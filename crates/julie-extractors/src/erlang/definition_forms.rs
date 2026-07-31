//! Function extraction. `tree-sitter-erlang` emits one `fun_decl` per clause,
//! so `foo(1) -> a; foo(2) -> b.` arrives as two sibling declarations. Erlang
//! identity is name/arity, so the clauses collapse into a single symbol whose
//! signature comes from the first clause head.

use std::collections::HashMap;

use serde_json::Value;
use tree_sitter::Node;

use super::ErlangExtractor;
use super::helpers::{NameArity, arg_count, find_child_by_type, first_atom_text};
use crate::base::{Symbol, SymbolKind, SymbolOptions, Visibility};
use crate::test_detection::{ErlangTestRole, erlang_test_role};

pub(super) struct FunctionClause {
    pub(super) identity: NameArity,
    params: String,
}

pub(super) fn function_clause(extractor: &ErlangExtractor, node: &Node) -> Option<FunctionClause> {
    let clause = find_child_by_type(node, "function_clause")?;
    let name = first_atom_text(&extractor.base, &clause)?;
    let args = find_child_by_type(&clause, "expr_args")?;

    Some(FunctionClause {
        identity: (name, arg_count(&args)),
        params: extractor.base.get_node_text(&args),
    })
}

pub(super) fn extract_function(
    extractor: &mut ErlangExtractor,
    node: &Node,
    clause: &FunctionClause,
    clause_count: usize,
    parent_id: Option<&str>,
) -> Symbol {
    let (name, arity) = clause.identity.clone();
    let signature = format!("{}/{}{}", name, arity, clause.params);
    let exported =
        extractor.exports_everything || extractor.exported_functions.contains(&clause.identity);
    let visibility = if exported {
        Visibility::Public
    } else {
        Visibility::Private
    };

    let mut metadata = HashMap::new();
    metadata.insert("arity".to_string(), Value::Number(arity.into()));
    metadata.insert(
        "clause_count".to_string(),
        Value::Number((clause_count as u64).into()),
    );
    if let Some(role) = erlang_test_role(extractor.test_module, &name, arity, exported) {
        metadata.insert("is_test".to_string(), Value::Bool(true));
        if role == ErlangTestRole::Lifecycle {
            metadata.insert("test_lifecycle".to_string(), Value::Bool(true));
        }
    }

    let doc_comment = super::doc::doc_for(extractor, node);
    let annotations = super::doc::annotations_for(extractor, node);

    extractor.base.create_symbol(
        node,
        name,
        SymbolKind::Function,
        SymbolOptions {
            signature: Some(signature),
            visibility: Some(visibility),
            parent_id: parent_id.map(String::from),
            metadata: Some(metadata),
            doc_comment,
            annotations,
        },
    )
}
