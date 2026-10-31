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

    for expected in [
        "x",
        "count",
        "seed",
        "conn",
        "is_user_type",
        "total",
        "visibility_unknown",
    ] {
        assert!(
            var_refs.contains(&expected),
            "expected variable_ref for {expected}; got {var_refs:?}"
        );
    }

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
    assert_eq!(
        identifiers
            .iter()
            .filter(|id| id.name == "GraphTraversal" && id.kind == IdentifierKind::VariableRef)
            .count(),
        0,
        "GraphTraversal must have no variable_ref rows"
    );

    for forbidden in [
        "unused_param",
        "ghost_token",
        "evaluate",
        "status",
        "filter_items",
        "mode",
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

    let mut keys: Vec<(String, u32, u32)> = identifiers
        .iter()
        .filter(|id| id.kind == IdentifierKind::VariableRef)
        .map(|id| (id.name.clone(), id.start_byte, id.end_byte))
        .collect();
    let before = keys.len();
    keys.sort();
    keys.dedup();
    assert_eq!(before, keys.len(), "duplicate variable_ref rows detected");

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

    assert!(
        identifiers
            .iter()
            .any(|id| id.name == "compute" && id.kind == IdentifierKind::Call)
    );
    for expected in [
        "bonus", "kv", "whole", "@limit", "items", "item", "shift", "q",
    ] {
        assert!(
            var_refs.contains(&expected),
            "expected variable_ref for {expected}; got {var_refs:?}"
        );
    }

    for forbidden in ["integer", "run", "helper", "go"] {
        assert!(
            !var_refs.contains(&forbidden),
            "{forbidden} must NOT be a variable_ref; got {var_refs:?}"
        );
    }

    assert_eq!(
        identifiers
            .iter()
            .filter(|id| id.name == "item" && id.kind == IdentifierKind::VariableRef)
            .count(),
        2,
        "the generator binding and body read both emit"
    );
    assert_eq!(
        identifiers
            .iter()
            .filter(|id| id.name == "q" && id.kind == IdentifierKind::VariableRef)
            .count(),
        2,
        "the function clause binding and body read both emit"
    );
    assert_eq!(
        variable_access_at(&identifiers, code.find("item <- items").unwrap()),
        Some("write")
    );
    assert_eq!(
        variable_access_at(
            &identifiers,
            code.find("use_item(item)").unwrap() + "use_item(".len()
        ),
        Some("read")
    );
    assert_eq!(
        variable_access_at(&identifiers, code.find("fn q ->").unwrap() + "fn ".len()),
        Some("write")
    );
    assert_eq!(
        variable_access_at(&identifiers, code.find("q + shift").unwrap()),
        Some("read")
    );
}

fn variable_access_at(identifiers: &[Identifier], start: usize) -> Option<&str> {
    identifiers
        .iter()
        .find(|identifier| identifier.start_byte as usize == start)?
        .metadata
        .as_ref()?
        .get("access")?
        .as_str()
}

#[test]
fn variable_access_facts_follow_elixir_match_and_pattern_syntax() {
    let code = r#"
defmodule Sample do
  def consume(value), do: value
  def update(output, input, conn) do
    output = input
    output = output + 1
    conn.value = input
    ^output = input
    case input do
      captured -> captured
    end
    for generated <- [input], do: consume(generated)
    consume(output)
  end
end
"#;
    let (_, identifiers) = extract_all(code);

    assert_eq!(
        variable_access_at(&identifiers, code.find("output = input").unwrap()),
        Some("write")
    );
    assert_eq!(
        variable_access_at(
            &identifiers,
            code.find("output = output + 1").unwrap() + "output = ".len()
        ),
        Some("read")
    );
    assert_eq!(
        variable_access_at(
            &identifiers,
            code.find("conn.value = input").unwrap() + "conn.".len()
        ),
        Some("write")
    );
    assert_eq!(
        variable_access_at(&identifiers, code.find("conn.value = input").unwrap()),
        Some("read")
    );
    assert_eq!(
        variable_access_at(&identifiers, code.find("^output").unwrap() + 1),
        Some("read")
    );
    assert_eq!(
        variable_access_at(
            &identifiers,
            code.find("case input").unwrap() + "case ".len()
        ),
        Some("read")
    );
    assert_eq!(
        variable_access_at(&identifiers, code.find("captured ->").unwrap()),
        Some("write")
    );
    assert_eq!(
        variable_access_at(
            &identifiers,
            code.find("-> captured").unwrap() + "-> ".len()
        ),
        Some("read")
    );
    assert_eq!(
        variable_access_at(&identifiers, code.find("generated <-").unwrap()),
        Some("write")
    );
    assert_eq!(
        variable_access_at(
            &identifiers,
            code.find("consume(generated)").unwrap() + "consume(".len()
        ),
        Some("read")
    );
    assert_eq!(
        variable_access_at(
            &identifiers,
            code.find("consume(output)").unwrap() + "consume(".len()
        ),
        Some("read")
    );
}
