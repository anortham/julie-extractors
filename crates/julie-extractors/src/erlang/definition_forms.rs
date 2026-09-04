//! Function extraction. `tree-sitter-erlang` emits one `fun_decl` per clause,
//! so `foo(1) -> a; foo(2) -> b.` arrives as two sibling declarations. Erlang
//! identity is name/arity, so the clauses collapse into a single symbol whose
//! signature comes from the first clause head and whose span runs from the
//! first clause through the end of the last.

use std::collections::{HashMap, HashSet};

use serde_json::Value;
use tree_sitter::Node;

use super::ErlangExtractor;
use super::helpers::{NameArity, arg_count, find_child_by_type, first_atom_text};
use crate::base::body::body_hash;
use crate::base::{NormalizedSpan, Symbol, SymbolKind, SymbolOptions, Visibility};
use crate::test_detection::{apply_test_role, erlang_test_role};

pub(super) struct FunctionClause {
    pub(super) identity: NameArity,
    params: String,
    /// Start byte of this clause's `clause_body`. The `body` field lives on
    /// `function_clause`, one level below the `fun_decl` a function symbol
    /// spans, so nothing above the clause can reach it by field name.
    body_start: Option<u32>,
}

pub(super) fn function_clause(extractor: &ErlangExtractor, node: &Node) -> Option<FunctionClause> {
    let clause = find_child_by_type(node, "function_clause")?;
    let name = first_atom_text(&extractor.base, &clause)?;
    let args = clause.child_by_field_name("args")?;

    Some(FunctionClause {
        identity: (name, arg_count(&args)),
        params: extractor.base.get_node_text(&args),
        body_start: clause
            .child_by_field_name("body")
            .map(|body| body.start_byte() as u32),
    })
}

#[allow(clippy::too_many_arguments)]
pub(super) fn extract_function(
    extractor: &mut ErlangExtractor,
    node: &Node,
    extent: NormalizedSpan,
    clause: &FunctionClause,
    clause_count: usize,
    parent_id: Option<&str>,
    clauses: &[Node],
    same_file_records: &HashSet<String>,
) -> Vec<Symbol> {
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
        apply_test_role(&mut metadata, role);
    }

    let doc_comment = super::doc::doc_for(extractor, node);
    let annotations = super::doc::annotations_for(extractor, node);

    let mut symbol = extractor.base.create_symbol_from_span(
        node,
        extent,
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
    );
    apply_clause_body_span(extractor, &mut symbol, clause.body_start);
    let callable_id = symbol.id.clone();
    let mut seen = HashSet::new();
    let mut symbols = vec![symbol];
    symbols.extend(super::parameters::extract_parameter_symbols(
        extractor,
        clauses,
        &callable_id,
        &mut seen,
    ));
    symbols.extend(super::type_facts::extract_body_locals(
        extractor,
        clauses,
        &callable_id,
        same_file_records,
        &mut seen,
    ));
    symbols
}

/// Replace the inferred body span with the clause bodies the symbol actually
/// covers: from the first clause's `clause_body` through the end of the last
/// clause.
///
/// The generic inference cannot find it. `fun_decl` carries no `body` field and
/// no `BODY_NODE_KINDS` child, so it falls through to matching text and returns
/// the first brace run in the declaration — for `open(Id) -> #account{id = Id}.`
/// that is the record literal, not the body. Spanning to the declaration end
/// also makes the hash sensitive to every clause, so editing a later clause of a
/// multi-clause function changes it.
///
/// A clause whose body the grammar did not resolve keeps the inferred span
/// rather than losing body coverage outright.
fn apply_clause_body_span(
    extractor: &mut ErlangExtractor,
    symbol: &mut Symbol,
    body_start: Option<u32>,
) {
    let Some(body_start) = body_start else {
        return;
    };
    if body_start >= symbol.end_byte {
        return;
    }

    let Some(span) = NormalizedSpan::from_content_range_with_line_starts(
        &extractor.base.content,
        extractor.base.line_starts(),
        body_start as usize,
        symbol.end_byte as usize,
    ) else {
        return;
    };

    symbol.body_span = Some(span);
    symbol.body_hash = body_hash(&extractor.base.content, span, &extractor.base.language);
}
