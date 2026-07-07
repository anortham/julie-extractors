// Tests for the Elixir `variable_ref` complement arm (locked contract — see
// the doc comment in csharp/identifiers.rs).

use crate::base::{Identifier, IdentifierKind, Symbol};
use crate::elixir::ElixirExtractor;
use std::path::PathBuf;

fn extract_all(code: &str) -> (Vec<Symbol>, Vec<Identifier>) {
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_elixir::LANGUAGE.into())
        .unwrap();
    let tree = parser.parse(code, None).unwrap();
    let workspace_root = PathBuf::from("/tmp/test");
    let mut extractor = ElixirExtractor::new(
        "elixir".to_string(),
        "test.ex".to_string(),
        code.to_string(),
        &workspace_root,
    );
    let symbols = extractor.extract_symbols(&tree);
    let identifiers = extractor.extract_identifiers(&tree, &symbols);
    (symbols, identifiers)
}

#[test]
fn test_elixir_variable_ref_emission() {
    // Elixir boundary notes:
    // - A plain `=` match binds its LHS pattern (write); a pinned `^var` inside
    //   a pattern is a READ of the existing value.
    // - Module receivers (`GraphTraversal` in `GraphTraversal.reach()`) are
    //   `alias` nodes already emitted as type_usage by the dot arm — the
    //   variable_ref arm must NOT double-emit them. Variable receivers
    //   (`conn` in `conn.status`) are bare identifiers and are ours.
    // - Keyword-list keys (`mode: 5`) are atoms, never identifiers.
    let code = r#"
defmodule Sample do
  def evaluate(seed, unused_param) do
    count = 0
    count = count + 1
    x = 5
    x = 7
    total = seed
    g = GraphTraversal.reach()
    m = conn.status
    h = filter_items(is_user_type)
    f = configure(mode: 5, source: seed)
    ^total = seed
    # ghost_token appears only in this comment and must never be extracted
    if total > 0, do: total, else: visibility_unknown
  end
end
"#;

    let (symbols, identifiers) = extract_all(code);

    let var_refs: Vec<&str> = identifiers
        .iter()
        .filter(|id| id.kind == IdentifierKind::VariableRef)
        .map(|id| id.name.as_str())
        .collect();

    // Positive cases (rules 1/4)
    for expected in [
        "count",              // RHS read of the rebind
        "seed",               // RHS + keyword VALUE + pin-match RHS reads
        "conn",               // variable receiver of conn.status
        "is_user_type",       // bare argument read
        "total",              // pinned pattern read + condition/branch reads
        "visibility_unknown", // bare branch read
    ] {
        assert!(
            var_refs.contains(&expected),
            "expected variable_ref for {expected}; got {var_refs:?}"
        );
    }

    // Module receiver stays owned by the dot arm as type_usage — no double emission.
    assert!(
        !var_refs.contains(&"GraphTraversal"),
        "alias receiver must stay a type_usage, not variable_ref"
    );
    assert!(
        identifiers
            .iter()
            .any(|id| id.name == "GraphTraversal" && id.kind == IdentifierKind::TypeUsage),
        "GraphTraversal receiver must still yield a type_usage"
    );
    // NOTE: the pre-existing dot + standalone-alias arms both emit a module
    // receiver as type_usage (2 rows, same span) — visible in the checked-in
    // goldens (e.g. fixtures/extraction/elixir/http_client/expected.json). The
    // variable_ref arm must not add a THIRD row for it.
    assert_eq!(
        identifiers
            .iter()
            .filter(|id| id.name == "GraphTraversal" && id.kind == IdentifierKind::VariableRef)
            .count(),
        0,
        "GraphTraversal must have no variable_ref rows"
    );

    // Negative cases (rules 2/3/4/5)
    for forbidden in [
        "x",            // plain-match LHS only
        "unused_param", // parameter pattern in the def head
        "ghost_token",  // comment-only mention
        "evaluate",     // function head name (Call-arm/def territory, not a read)
        "status",       // accessed member name (dot right side)
        "filter_items", // call callee (owned by the Call arm)
        "mode",         // keyword-list key (atom)
    ] {
        assert!(
            !var_refs.contains(&forbidden),
            "{forbidden} must NOT be a variable_ref; got {var_refs:?}"
        );
    }
    assert!(
        !identifiers
            .iter()
            .any(|id| id.name == "ghost_token" && id.kind == IdentifierKind::VariableRef),
        "comment-only ghost_token must not be a variable_ref"
    );

    // No duplicate variable_ref rows: each (name, span) is unique. (Scoped to
    // this arm — the pre-existing dot/alias type_usage double emission noted
    // above is outside the variable_ref contract.)
    let mut keys: Vec<(String, u32, u32)> = identifiers
        .iter()
        .filter(|id| id.kind == IdentifierKind::VariableRef)
        .map(|id| (id.name.clone(), id.start_byte, id.end_byte))
        .collect();
    let before = keys.len();
    keys.sort();
    keys.dedup();
    assert_eq!(before, keys.len(), "duplicate variable_ref rows detected");

    // containing_symbol_id is populated on a variable_ref.
    let evaluate = symbols
        .iter()
        .find(|s| s.name == "evaluate")
        .expect("evaluate function extracted");
    let conn_ref = identifiers
        .iter()
        .find(|id| id.name == "conn" && id.kind == IdentifierKind::VariableRef)
        .expect("conn variable_ref");
    assert_eq!(
        conn_ref.containing_symbol_id.as_deref(),
        Some(evaluate.id.as_str()),
        "receiver variable_ref should be contained in evaluate"
    );
}

#[test]
fn test_elixir_variable_ref_pattern_and_typespec_exclusions() {
    let code = r#"
defmodule Deep do
  @limit 5

  @spec run(integer()) :: integer()
  def run(x) when x > 0, do: x + bonus

  defp helper(%{key: kv} = whole), do: {kv, whole}

  def go do
    f = &compute/1
    y = @limit
    for item <- items, do: use_item(item)
    fn q -> q + shift end
  end
end
"#;

    let (_symbols, identifiers) = extract_all(code);
    let var_refs: Vec<&str> = identifiers
        .iter()
        .filter(|id| id.kind == IdentifierKind::VariableRef)
        .map(|id| id.name.as_str())
        .collect();

    // Reads: do:-bodies of defs, capture operands, attribute reads,
    // comprehension sources, anonymous-fn bodies.
    for expected in [
        "bonus",   // def do:-body read
        "kv",      // defp do:-body tuple element read
        "whole",   // defp do:-body tuple element read
        "compute", // &compute/1 function-reference read
        "limit",   // @limit attribute read
        "items",   // comprehension source read
        "item",    // do:-body argument read
        "shift",   // anonymous-fn body read
        "q",       // anonymous-fn body read (LHS of +)
    ] {
        assert!(
            var_refs.contains(&expected),
            "expected variable_ref for {expected}; got {var_refs:?}"
        );
    }

    // Binds and typespec territory stay out. `x` appears as a def-head pattern
    // and guard/body reads — the head pattern is excluded, but body/guard reads
    // are separate spans; here we assert the pattern-only names.
    for forbidden in [
        "integer", // @spec typespec type (typespec walk territory)
        "run",     // function head name
        "helper",  // function head name
        "go",      // function head name (no-parens def)
    ] {
        assert!(
            !var_refs.contains(&forbidden),
            "{forbidden} must NOT be a variable_ref; got {var_refs:?}"
        );
    }

    // Pattern binds excluded: `item` in `item <- items` binds; the read of
    // `item` inside use_item(item) is a different span. Exactly one variable_ref
    // row for item proves the `<-` LHS was excluded.
    assert_eq!(
        identifiers
            .iter()
            .filter(|id| id.name == "item" && id.kind == IdentifierKind::VariableRef)
            .count(),
        1,
        "only the use_item(item) argument read may emit; the <- bind must not"
    );
    // fn-clause parameter `q` binds in the stab head; the body reads it once.
    assert_eq!(
        identifiers
            .iter()
            .filter(|id| id.name == "q" && id.kind == IdentifierKind::VariableRef)
            .count(),
        1,
        "only the body read of q may emit; the fn-clause parameter must not"
    );
}
