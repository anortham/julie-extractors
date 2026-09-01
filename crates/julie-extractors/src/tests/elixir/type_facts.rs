#[cfg(test)]
mod elixir_type_fact_tests {
    use crate::base::{IdentifierKind, Symbol, SymbolKind};
    use crate::elixir::ElixirExtractor;
    use std::path::PathBuf;
    use tree_sitter::Parser;

    fn extract(code: &str) -> (Vec<Symbol>, ElixirExtractor, tree_sitter::Tree) {
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_elixir::LANGUAGE.into())
            .expect("Error loading Elixir grammar");
        let tree = parser.parse(code, None).expect("Error parsing code");
        let mut extractor = ElixirExtractor::new(
            "elixir".to_string(),
            "test.ex".to_string(),
            code.to_string(),
            &PathBuf::from("/tmp/test"),
        );
        let symbols = extractor.extract_symbols(&tree);
        (symbols, extractor, tree)
    }

    fn symbol<'a>(symbols: &'a [Symbol], name: &str) -> &'a Symbol {
        symbols
            .iter()
            .find(|s| s.name == name)
            .unwrap_or_else(|| panic!("missing symbol `{name}`"))
    }

    fn parameter_symbols<'a>(symbols: &'a [Symbol], name: &str) -> Vec<&'a Symbol> {
        symbols
            .iter()
            .filter(|s| {
                s.name == name
                    && s.metadata
                        .as_ref()
                        .and_then(|m| m.get("role"))
                        .map(|role| role == &serde_json::json!("parameter"))
                        .unwrap_or(false)
            })
            .collect()
    }

    fn local<'a>(symbols: &'a [Symbol], name: &str) -> &'a Symbol {
        symbols
            .iter()
            .find(|s| {
                s.name == name
                    && s.kind == SymbolKind::Variable
                    && s.metadata
                        .as_ref()
                        .and_then(|m| m.get("role"))
                        .map(|role| role != &serde_json::json!("parameter"))
                        .unwrap_or(true)
            })
            .unwrap_or_else(|| panic!("missing local `{name}`"))
    }

    #[test]
    fn struct_match_parameter_records_declared_fact() {
        let (symbols, extractor, _) = extract(
            r#"
defmodule App do
  def run(%Worker{} = w, n), do: {w, n}
end
"#,
        );

        let run = symbol(&symbols, "run");
        let params = parameter_symbols(&symbols, "w");
        assert_eq!(params.len(), 1);
        let w = params[0];
        assert_eq!(w.kind, SymbolKind::Variable);
        assert_eq!(w.parent_id.as_deref(), Some(run.id.as_str()));
        let fact = extractor
            .base
            .type_info
            .get(&w.id)
            .expect("missing type fact for `w`");
        assert_eq!(fact.resolved_type, "Worker");
        assert!(!fact.is_inferred);
        assert_eq!(fact.language, "elixir");

        let ns = parameter_symbols(&symbols, "n");
        assert_eq!(ns.len(), 1);
        let n = ns[0];
        assert_eq!(n.kind, SymbolKind::Variable);
        assert_eq!(n.parent_id.as_deref(), Some(run.id.as_str()));
        assert!(!extractor.base.type_info.contains_key(&n.id));
    }

    #[test]
    fn struct_literal_local_records_inferred_fact() {
        let (symbols, extractor, _) = extract(
            r#"
defmodule App do
  def go(x), do: y = %Job{id: x}
end
"#,
        );

        let go = symbol(&symbols, "go");
        let xs = parameter_symbols(&symbols, "x");
        assert_eq!(xs.len(), 1);
        assert_eq!(xs[0].parent_id.as_deref(), Some(go.id.as_str()));
        assert!(!extractor.base.type_info.contains_key(&xs[0].id));

        let y = local(&symbols, "y");
        assert_eq!(y.kind, SymbolKind::Variable);
        assert_eq!(y.parent_id.as_deref(), Some(go.id.as_str()));
        let fact = extractor
            .base
            .type_info
            .get(&y.id)
            .expect("missing type fact for `y`");
        assert_eq!(fact.resolved_type, "Job");
        assert!(fact.is_inferred);
        assert_eq!(fact.language, "elixir");
    }

    #[test]
    fn map_new_and_map_literal_locals_have_no_fact() {
        let (symbols, extractor, _) = extract(
            r#"
defmodule App do
  def go do
    z = Map.new()
    q = %{a: 1}
    {z, q}
  end
end
"#,
        );

        let go = symbol(&symbols, "go");
        for name in ["z", "q"] {
            let var = local(&symbols, name);
            assert_eq!(var.kind, SymbolKind::Variable);
            assert_eq!(var.parent_id.as_deref(), Some(go.id.as_str()));
            assert!(!extractor.base.type_info.contains_key(&var.id));
        }
    }

    #[test]
    fn struct_literal_emits_type_usage_identifier() {
        let (symbols, mut extractor, tree) = extract(
            r#"
defmodule App do
  def go, do: %Job{}
end
"#,
        );

        let identifiers = extractor.extract_identifiers(&tree, &symbols);
        let usages: Vec<_> = identifiers
            .iter()
            .filter(|id| id.kind == IdentifierKind::TypeUsage && id.name == "Job")
            .collect();
        assert_eq!(usages.len(), 1);
        assert!(usages[0].receiver_type.is_none());
    }

    #[test]
    fn unknown_call_local_has_no_fact() {
        let (symbols, extractor, _) = extract(
            r#"
defmodule App do
  def go, do: u = mystery()
end
"#,
        );

        let u = local(&symbols, "u");
        assert_eq!(u.kind, SymbolKind::Variable);
        assert!(!extractor.base.type_info.contains_key(&u.id));
    }

    #[test]
    fn qualified_struct_literal_local_has_no_fact() {
        let (symbols, extractor, _) = extract(
            r#"
defmodule App do
  def go, do: y = %Foo.Bar{}
end
"#,
        );

        let y = local(&symbols, "y");
        assert_eq!(y.kind, SymbolKind::Variable);
        assert!(!extractor.base.type_info.contains_key(&y.id));
    }

    #[test]
    fn module_struct_literal_local_has_no_fact_and_no_type_usage() {
        let (symbols, mut extractor, tree) = extract(
            r#"
defmodule App do
  defstruct [:id]
  def go, do: y = %__MODULE__{}
end
"#,
        );

        let y = local(&symbols, "y");
        assert_eq!(y.kind, SymbolKind::Variable);
        assert!(!extractor.base.type_info.contains_key(&y.id));

        let identifiers = extractor.extract_identifiers(&tree, &symbols);
        assert!(
            identifiers
                .iter()
                .all(|id| !(id.kind == IdentifierKind::TypeUsage && id.name == "__MODULE__"))
        );
    }

    #[test]
    fn variable_struct_literal_local_has_no_fact_and_no_type_usage() {
        let (symbols, mut extractor, tree) = extract(
            r#"
defmodule App do
  def go(mod), do: y = %mod{}
end
"#,
        );

        let y = local(&symbols, "y");
        assert_eq!(y.kind, SymbolKind::Variable);
        assert!(!extractor.base.type_info.contains_key(&y.id));

        let identifiers = extractor.extract_identifiers(&tree, &symbols);
        assert!(
            identifiers
                .iter()
                .all(|id| !(id.kind == IdentifierKind::TypeUsage && id.name == "mod"))
        );
    }

    #[test]
    fn quoted_assignments_inside_defmacro_are_not_macro_locals() {
        let (symbols, _, _) = extract(
            r#"
defmodule App do
  defmacro build(opts) do
    prefix = "x"
    quote do
      inner = unquote(opts)
      inner
    end
  end
end
"#,
        );

        let build = symbol(&symbols, "build");
        let prefix = local(&symbols, "prefix");
        assert_eq!(prefix.parent_id.as_deref(), Some(build.id.as_str()));
        assert!(symbols.iter().all(|s| s.name != "inner"));
    }
}

#[cfg(test)]
mod elixir_spec_type_tests {
    use crate::base::SymbolKind;
    use std::path::PathBuf;

    #[test]
    fn spec_return_types_reduce_to_base_names_in_the_artifact() {
        let source = r#"defmodule Fixture.Specs do
  @spec count() :: integer()
  def count, do: 1

  @spec start() :: GenServer.on_start()
  def start, do: :ok

  @spec pair() :: {:ok, integer()}
  def pair, do: {:ok, 1}

  @spec items() :: [term()]
  def items, do: []
end
"#;
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&tree_sitter_elixir::LANGUAGE.into())
            .unwrap();
        let tree = parser.parse(source, None).unwrap();
        let results = crate::factory::extract_symbols_and_relationships(
            &tree,
            "specs.ex",
            source,
            "elixir",
            &PathBuf::from("/tmp/test"),
        )
        .unwrap();
        let resolved = |name: &str| -> Option<String> {
            let symbol = results
                .symbols
                .iter()
                .find(|s| s.name == name && s.kind == SymbolKind::Function)
                .unwrap_or_else(|| panic!("missing function `{name}`"));
            results
                .types
                .get(&symbol.id)
                .map(|info| info.resolved_type.clone())
        };
        assert_eq!(resolved("count").as_deref(), Some("integer"));
        assert_eq!(resolved("start").as_deref(), Some("GenServer.on_start"));
        assert_eq!(resolved("pair"), None);
        assert_eq!(resolved("items"), None);
        for info in results.types.values() {
            let value = info.resolved_type.as_str();
            assert!(
                !value.contains(['[', '(', '{', '<', '?', ' ']),
                "non-base resolved_type {value}"
            );
        }
    }
}
