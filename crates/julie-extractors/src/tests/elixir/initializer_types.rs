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

#[test]
fn do_block_counts_as_one_trailing_argument() {
    let x = extract(
        r#"
defmodule Locky do
  @spec with_lock() :: Zero.t()
  def with_lock, do: nil

  @spec with_lock(keyword()) :: One.t()
  def with_lock(opts), do: opts

  def run do
    with_block =
      with_lock do
        :ok
      end

    without_block = with_lock()
    {with_block, without_block}
  end
end
"#,
    );

    assert_eq!(x.inferred("with_block"), "One.t");
    assert_eq!(x.inferred("without_block"), "Zero.t");
}

#[test]
fn remote_spec_return_is_qualified_in_the_callee_module() {
    let x = extract(
        r#"
defmodule Shop.Store do
  alias Shop.Workspace, as: Ws

  defstruct []
  @type t :: %__MODULE__{}

  @spec new() :: t()
  def new, do: %__MODULE__{}

  @spec load() :: Ws.t()
  def load, do: nil

  def run do
    own = new()
    own
  end
end

defmodule Shop.Web do
  alias Shop.Store
  alias Other.Ws

  @type t :: %__MODULE__{}

  def run do
    remote_t = Store.new()
    remote_alias = Store.load()
    {remote_t, remote_alias}
  end
end
"#,
    );

    assert_eq!(x.inferred("own"), "t");
    assert_eq!(x.inferred("remote_t"), "Shop.Store.t");
    assert_eq!(x.inferred("remote_alias"), "Shop.Workspace.t");
}

#[test]
fn remote_spec_return_that_reads_differently_in_the_caller_records_nothing() {
    let x = extract(
        r#"
defmodule Shop.Store do
  use TypedStruct

  typedstruct do
    field :id, integer()
  end

  @spec new() :: t()
  def new, do: nil

  @spec load() :: Ws.t()
  def load, do: nil
end

defmodule Shop.Web do
  alias Shop.Store
  alias Other.Ws

  def run do
    generated_t = Store.new()
    shadowed = Store.load()
    {generated_t, shadowed}
  end
end
"#,
    );

    assert!(x.local_type("generated_t").is_none());
    assert!(x.local_type("shadowed").is_none());
}

#[test]
fn spec_inside_a_quote_does_not_attach_to_a_real_definition() {
    let x = extract(
        r#"
defmodule Base do
  def load, do: :real

  defmacro __using__(_) do
    quote do
      @spec load() :: Injected.t()
      def load, do: nil

      @spec only_quoted() :: Injected.t()
      def only_quoted, do: nil
    end
  end

  def run do
    using = load()
    quoted = only_quoted()
    {using, quoted}
  end
end
"#,
    );

    let loads: Vec<_> = x
        .symbols
        .iter()
        .filter(|s| s.name == "load" && s.kind == SymbolKind::Function)
        .map(|s| x.types.get(&s.id).map(|fact| fact.resolved_type.as_str()))
        .collect();
    assert_eq!(loads, [None, Some("Injected.t")]);
    assert!(x.local_type("using").is_none());
    assert!(x.local_type("quoted").is_none());
}

#[test]
fn alias_inside_a_function_body_does_not_reach_other_functions() {
    let x = extract(
        r#"
defmodule Shop.Store do
  @spec load() :: Shop.Ws.t()
  def load, do: nil
end

defmodule Web do
  def early do
    before_alias = Store.load()
    before_alias
  end

  def a do
    alias Shop.Store
    inside = Store.load()
    inside
  end

  def b do
    lexical = Store.load()
    lexical
  end
end
"#,
    );

    assert_eq!(x.inferred("inside"), "Shop.Ws.t");
    assert!(x.local_type("lexical").is_none());
    assert!(x.local_type("before_alias").is_none());
}

#[test]
fn module_alias_applies_only_after_its_position() {
    let x = extract(
        r#"
defmodule Shop.Store do
  @spec load() :: Shop.Ws.t()
  def load, do: nil
end

defmodule Web do
  def early do
    before_alias = Store.load()
    before_alias
  end

  alias Shop.Store

  def late do
    after_alias = Store.load()
    after_alias
  end
end
"#,
    );

    assert!(x.local_type("before_alias").is_none());
    assert_eq!(x.inferred("after_alias"), "Shop.Ws.t");
}

#[test]
fn call_inside_a_quote_records_nothing() {
    let x = extract(
        r#"
defmodule Base do
  @spec load() :: Base.Thing.t()
  def load, do: nil

  def run do
    outside = load()
    outside
  end

  defmacro __using__(_) do
    quote do
      def go do
        plain = load()
        self_call = __MODULE__.load()
        {plain, self_call}
      end
    end
  end
end
"#,
    );

    assert_eq!(x.inferred("outside"), "Base.Thing.t");
    assert!(x.local_type("plain").is_none());
    assert!(x.local_type("self_call").is_none());
}

#[test]
fn same_module_spec_name_that_an_alias_reads_differently_records_nothing() {
    let x = extract(
        r#"
defmodule A do
  @spec load() :: Ws.t()
  def load, do: nil

  def before do
    before_alias = load()
    before_alias
  end

  alias X.Ws

  def run do
    after_alias = load()
    after_alias
  end

  def run2 do
    alias Y.Ws
    fn_alias = load()
    fn_alias
  end
end
"#,
    );

    assert_eq!(x.inferred("before_alias"), "Ws.t");
    assert!(x.local_type("after_alias").is_none());
    assert!(x.local_type("fn_alias").is_none());
}
