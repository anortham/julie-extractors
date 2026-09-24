use crate::base::{Symbol, SymbolKind, TypeInfo};
use crate::factory::extract_symbols_and_relationships;
use std::collections::HashMap;
use std::path::PathBuf;

struct Extraction {
    symbols: Vec<Symbol>,
    types: HashMap<String, TypeInfo>,
}

fn extract(source: &str) -> Extraction {
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_elixir::LANGUAGE.into())
        .unwrap();
    let tree = parser.parse(source, None).unwrap();
    let results = extract_symbols_and_relationships(
        &tree,
        "lib/app.ex",
        source,
        "elixir",
        &PathBuf::from("/tmp/test"),
    )
    .unwrap();
    Extraction {
        symbols: results.symbols,
        types: results.types,
    }
}

impl Extraction {
    fn local(&self, name: &str) -> &Symbol {
        self.symbols
            .iter()
            .find(|s| {
                s.name == name
                    && s.kind == SymbolKind::Variable
                    && s.metadata
                        .as_ref()
                        .and_then(|m| m.get("role"))
                        .is_none_or(|role| role != &serde_json::json!("parameter"))
            })
            .unwrap_or_else(|| panic!("missing local `{name}`"))
    }

    fn local_type(&self, name: &str) -> Option<&TypeInfo> {
        self.types.get(&self.local(name).id)
    }

    fn inferred(&self, name: &str) -> &str {
        let fact = self
            .local_type(name)
            .unwrap_or_else(|| panic!("missing type fact for `{name}`"));
        assert!(fact.is_inferred, "`{name}` fact must be inferred");
        assert_eq!(fact.language, "elixir");
        &fact.resolved_type
    }

    fn function_type(&self, name: &str) -> Option<&str> {
        let function = self
            .symbols
            .iter()
            .find(|s| s.name == name && s.kind == SymbolKind::Function)
            .unwrap_or_else(|| panic!("missing function `{name}`"));
        self.types
            .get(&function.id)
            .map(|fact| fact.resolved_type.as_str())
    }
}

#[test]
fn local_call_takes_the_spec_return_type() {
    let x = extract(
        r#"
defmodule App do
  @spec load() :: Workspace.t()
  def load, do: %Workspace{}

  def run do
    ws = load()
    ws
  end
end
"#,
    );

    assert_eq!(x.inferred("ws"), "Workspace.t");
}

#[test]
fn spec_written_after_the_definition_still_applies() {
    let x = extract(
        r#"
defmodule App do
  def run do
    ws = load()
    ws
  end

  def load, do: %Workspace{}
  @spec load() :: Workspace.t()
end
"#,
    );

    assert_eq!(x.inferred("ws"), "Workspace.t");
    assert_eq!(x.function_type("load"), Some("Workspace.t"));
}

#[test]
fn module_qualified_calls_to_same_file_modules_take_the_spec_return_type() {
    let x = extract(
        r#"
defmodule App.Store do
  @spec open() :: Conn.t()
  def open, do: nil
end

defmodule App.Web do
  alias App.Store

  @spec build(integer()) :: Page.t()
  def build(n), do: n

  def run do
    aliased = Store.open()
    qualified = App.Store.open()
    own = __MODULE__.build(1)
    {aliased, qualified, own}
  end
end
"#,
    );

    assert_eq!(x.inferred("aliased"), "Conn.t");
    assert_eq!(x.inferred("qualified"), "Conn.t");
    assert_eq!(x.inferred("own"), "Page.t");
}

#[test]
fn nested_module_is_reachable_by_its_short_name() {
    let x = extract(
        r#"
defmodule Outer do
  defmodule Repo do
    @spec get() :: Row.t()
    def get, do: nil
  end

  def run do
    row = Repo.get()
    row
  end
end
"#,
    );

    assert_eq!(x.inferred("row"), "Row.t");
}

#[test]
fn piped_call_counts_the_piped_argument() {
    let x = extract(
        r#"
defmodule App do
  @spec parse(String.t()) :: Doc.t()
  def parse(text), do: text

  def run(text) do
    doc = text |> parse()
    doc
  end
end
"#,
    );

    assert_eq!(x.inferred("doc"), "Doc.t");
}

#[test]
fn call_with_defaults_omitted_reaches_the_full_arity_spec() {
    let x = extract(
        r#"
defmodule App do
  @spec fetch(integer(), keyword()) :: Item.t()
  def fetch(id, opts \\ []), do: {id, opts}

  def run do
    item = fetch(1)
    item
  end
end
"#,
    );

    assert_eq!(x.inferred("item"), "Item.t");
}

#[test]
fn multi_clause_definition_shares_its_spec() {
    let x = extract(
        r#"
defmodule App do
  @spec fact(integer()) :: integer()
  def fact(0), do: 1
  def fact(n), do: n * fact(n - 1)

  def run do
    total = fact(5)
    total
  end
end
"#,
    );

    assert_eq!(x.inferred("total"), "integer");
}

#[test]
fn ok_tuple_match_binds_the_ok_payload_type() {
    let x = extract(
        r#"
defmodule App do
  @spec load() :: {:ok, Workspace.t()} | {:error, term()} | :disabled
  def load, do: {:ok, %Workspace{}}

  def run do
    {:ok, ws} = load()
    ws
  end
end
"#,
    );

    let run = x
        .symbols
        .iter()
        .find(|s| s.name == "run")
        .expect("missing run");
    let ws = x.local("ws");
    assert_eq!(ws.parent_id.as_deref(), Some(run.id.as_str()));
    assert_eq!(x.inferred("ws"), "Workspace.t");
}

#[test]
fn call_outside_the_file_records_nothing() {
    let x = extract(
        r#"
defmodule App do
  import Other

  def run(mod) do
    imported = load()
    remote = Other.load()
    dynamic = mod.load()
    {imported, remote, dynamic}
  end
end
"#,
    );

    for name in ["imported", "remote", "dynamic"] {
        assert!(x.local_type(name).is_none(), "`{name}` must have no fact");
    }
}

#[test]
fn alias_to_another_file_shadows_a_same_file_module_name() {
    let x = extract(
        r#"
defmodule Store do
  @spec open() :: Conn.t()
  def open, do: nil
end

defmodule Web do
  alias External.Store

  def run do
    conn = Store.open()
    conn
  end
end
"#,
    );

    assert!(x.local_type("conn").is_none());
}

#[test]
fn call_with_an_arity_no_spec_covers_records_nothing() {
    let x = extract(
        r#"
defmodule App do
  import Other

  @spec load(String.t()) :: Workspace.t()
  def load(path), do: path

  def run do
    ws = load()
    ws
  end
end
"#,
    );

    assert!(x.local_type("ws").is_none());
}

#[test]
fn conflicting_specs_for_one_head_record_nothing() {
    let x = extract(
        r#"
defmodule App do
  @spec pick(integer()) :: integer()
  @spec pick(atom()) :: atom()
  def pick(v), do: v

  def run do
    picked = pick(1)
    picked
  end
end
"#,
    );

    assert!(x.local_type("picked").is_none());
    assert_eq!(x.function_type("pick"), None);
}

#[test]
fn type_variable_return_records_nothing() {
    let x = extract(
        r#"
defmodule App do
  @spec same(a) :: a when a: term()
  def same(v), do: v

  @spec wrap(a) :: {:ok, a} when a: term()
  def wrap(v), do: {:ok, v}

  def run do
    plain = same(1)
    {:ok, wrapped} = wrap(1)
    {plain, wrapped}
  end
end
"#,
    );

    assert!(x.local_type("plain").is_none());
    assert!(x.local_type("wrapped").is_none());
}

#[test]
fn function_of_an_enclosing_module_is_not_a_local_call() {
    let x = extract(
        r#"
defmodule Outer do
  @spec load() :: Workspace.t()
  def load, do: nil

  defmodule Inner do
    def run do
      ws = load()
      ws
    end
  end
end
"#,
    );

    assert!(x.local_type("ws").is_none());
}

#[test]
fn macro_call_records_nothing() {
    let x = extract(
        r#"
defmodule App do
  @spec build() :: Macro.t()
  defmacro build, do: quote(do: 1)

  def run do
    built = build()
    built
  end
end
"#,
    );

    assert!(x.local_type("built").is_none());
}

#[test]
fn ok_tuple_match_needs_every_alternative_to_be_a_literal_or_agreeing_ok_tuple() {
    let x = extract(
        r#"
defmodule App do
  @type result :: {:ok, Other.t()}

  @spec open_union() :: {:ok, Workspace.t()} | result()
  def open_union, do: nil

  @spec split() :: {:ok, Workspace.t()} | {:ok, Other.t()}
  def split, do: nil

  @spec plain() :: Workspace.t()
  def plain, do: nil

  @spec listed() :: {:ok, [Workspace.t()]}
  def listed, do: nil

  def run do
    {:ok, a} = open_union()
    {:ok, b} = split()
    {:ok, c} = plain()
    {:ok, d} = listed()
    {a, b, c, d}
  end
end
"#,
    );

    for name in ["a", "b", "c", "d"] {
        assert!(x.local_type(name).is_none(), "`{name}` must have no fact");
    }
}

#[test]
fn plain_match_on_an_ok_tuple_spec_records_nothing() {
    let x = extract(
        r#"
defmodule App do
  @spec load() :: {:ok, Workspace.t()} | {:error, term()}
  def load, do: nil

  def run do
    result = load()
    result
  end
end
"#,
    );

    assert!(x.local_type("result").is_none());
}

#[test]
fn ok_tuple_match_on_a_struct_literal_records_nothing() {
    let x = extract(
        r#"
defmodule App do
  def run do
    {:ok, job} = %Job{}
    job
  end
end
"#,
    );

    assert!(x.local_type("job").is_none());
}
