use super::support::{extract, find, find_kind};
use crate::base::SymbolKind;

const DOCUMENTED: &str = r#"%% @doc Account bookkeeping primitives.
%% Balances are stored in whole cents.
-module(bank).
-moduledoc "Account ledger entry points.".

-spec open(integer()) -> ok.
%% @doc Open a new account.
open(_Id) ->
    ok.

-doc "Read the stored balance.".
balance(_Acct) ->
    0.

-doc "An opaque account handle.".
-opaque token() :: binary().

-doc "Start the behaviour.".
-callback init(term()) -> ok.
"#;

#[test]
fn attaches_edoc_comment_block_to_the_function_below_it() {
    let symbols = extract(DOCUMENTED);

    assert_eq!(
        find(&symbols, "open").doc_comment.as_deref(),
        Some("%% @doc Open a new account.")
    );
}

#[test]
fn attaches_multiline_edoc_block_to_the_module() {
    let symbols = extract(DOCUMENTED);

    assert_eq!(
        find(&symbols, "bank").doc_comment.as_deref(),
        Some("%% @doc Account bookkeeping primitives.\n%% Balances are stored in whole cents.")
    );
}

#[test]
fn falls_back_to_moduledoc_attribute_when_no_comment_precedes_the_module() {
    let symbols = extract("-module(bank).\n-moduledoc \"Ledger entry points.\".\n");

    assert_eq!(
        find(&symbols, "bank").doc_comment.as_deref(),
        Some("Ledger entry points.")
    );
}

#[test]
fn attaches_doc_attribute_to_functions_types_and_callbacks() {
    let symbols = extract(DOCUMENTED);

    assert_eq!(
        find(&symbols, "balance").doc_comment.as_deref(),
        Some("Read the stored balance.")
    );
    assert_eq!(
        find_kind(&symbols, "token", SymbolKind::Type)
            .doc_comment
            .as_deref(),
        Some("An opaque account handle.")
    );
    assert_eq!(
        find(&symbols, "init").doc_comment.as_deref(),
        Some("Start the behaviour.")
    );
}

#[test]
fn preceding_attributes_become_annotation_markers() {
    let symbols = extract(DOCUMENTED);
    let keys: Vec<_> = find(&symbols, "open")
        .annotations
        .iter()
        .map(|annotation| annotation.annotation_key.clone())
        .collect();

    assert_eq!(keys, vec!["spec".to_string()]);
    assert_eq!(
        find(&symbols, "balance")
            .annotations
            .iter()
            .map(|annotation| annotation.annotation_key.clone())
            .collect::<Vec<_>>(),
        vec!["doc".to_string()]
    );
}
