use crate::base::{ExtractionResults, IdentifierKind, RelationshipKind, Symbol, SymbolKind};
use crate::extract_canonical;
use std::path::Path;

fn extract(path: &str, source: &str) -> ExtractionResults {
    extract_canonical(path, source, Path::new("/tmp/test")).expect("elixir extraction")
}

fn symbol<'a>(result: &'a ExtractionResults, name: &str, kind: SymbolKind) -> &'a Symbol {
    result
        .symbols
        .iter()
        .find(|s| s.name == name && s.kind == kind)
        .unwrap_or_else(|| panic!("missing {kind:?} {name}"))
}

fn name_of(result: &ExtractionResults, id: &str) -> String {
    result
        .symbols
        .iter()
        .find(|s| s.id == id)
        .map(|s| s.name.clone())
        .unwrap_or_default()
}

fn calls(result: &ExtractionResults) -> Vec<(String, String, u32)> {
    let mut edges: Vec<_> = result
        .relationships
        .iter()
        .filter(|r| r.kind == RelationshipKind::Calls)
        .map(|r| {
            (
                name_of(result, &r.from_symbol_id),
                name_of(result, &r.to_symbol_id),
                r.line_number,
            )
        })
        .collect();
    edges.sort();
    edges
}

fn pending(result: &ExtractionResults) -> Vec<(String, String, Vec<String>)> {
    let mut rows: Vec<_> = result
        .structured_pending_relationships
        .iter()
        .map(|p| {
            (
                name_of(result, &p.pending.from_symbol_id),
                p.target.terminal_name.clone(),
                p.target.namespace_path.clone(),
            )
        })
        .collect();
    rows.sort();
    rows
}

fn edge(from: &str, to: &str, line: u32) -> (String, String, u32) {
    (from.to_string(), to.to_string(), line)
}

fn row(from: &str, terminal: &str, namespace: &[&str]) -> (String, String, Vec<String>) {
    (
        from.to_string(),
        terminal.to_string(),
        namespace.iter().map(|s| s.to_string()).collect(),
    )
}

#[test]
fn multi_clause_and_name_colliding_functions_keep_their_call_edges() {
    let result = extract(
        "lib/math.ex",
        "defmodule MyApp.Math do\n  def fact(0), do: 1\n  def fact(n) when n > 0, do: n * fact(n - 1)\n  def total(list), do: list |> Enum.sum() |> fact()\nend\n\ndefmodule Cfg do\n  def config, do: Application.get_env(:app, :cfg)\n  def start do\n    config = config()\n    Logger.info(config)\n  end\nend\n",
    );

    let edges = calls(&result);
    assert!(edges.contains(&edge("fact", "fact", 3)), "{edges:?}");
    assert!(edges.contains(&edge("total", "fact", 4)), "{edges:?}");
    assert!(edges.contains(&edge("start", "config", 10)), "{edges:?}");

    let rows = pending(&result);
    assert!(
        rows.contains(&row("config", "get_env", &["Application"])),
        "{rows:?}"
    );
    assert!(rows.contains(&row("total", "sum", &["Enum"])), "{rows:?}");
    assert!(
        rows.iter()
            .all(|(_, terminal, _)| terminal != "fact" && terminal != "config")
    );
}

#[test]
fn test_setup_macro_and_guard_bodies_emit_calls() {
    let result = extract(
        "test/calc_test.exs",
        "defmodule CalcTest do\n  use ExUnit.Case\n  setup do\n    seed = build_seed()\n    {:ok, seed: seed}\n  end\n  test \"adds numbers\", %{seed: seed} do\n    assert Calc.add(seed, 2) == 3\n    assert helper() == :ok\n  end\n  defp helper, do: :ok\n  defp build_seed, do: 1\nend\n\ndefmodule Macros do\n  defmacro trace(expr) do\n    log_it(expr)\n    Macro.to_string(expr)\n  end\n  defmacrop secret(x), do: log_it(x)\n  defguard is_even(n) when is_integer(n) and rem(n, 2) == 0\n  defp log_it(e), do: e\nend\n",
    );

    let edges = calls(&result);
    for expected in [
        edge("setup", "build_seed", 4),
        edge("adds numbers", "helper", 9),
        edge("trace", "log_it", 17),
        edge("secret", "log_it", 20),
    ] {
        assert!(
            edges.contains(&expected),
            "missing {expected:?} in {edges:?}"
        );
    }

    let rows = pending(&result);
    for expected in [
        row("adds numbers", "add", &["Calc"]),
        row("trace", "to_string", &["Macro"]),
        row("is_even", "is_integer", &[]),
        row("is_even", "rem", &[]),
    ] {
        assert!(rows.contains(&expected), "missing {expected:?} in {rows:?}");
    }
}

#[test]
fn definition_heads_are_not_calls() {
    let result = extract(
        "lib/svc.ex",
        "defmodule Svc do\n  def run(id) do\n    id + 1\n  end\n\n  def show(%{name: name} = user), do: {name, user}\n  defp check(cs) when is_map(cs), do: cs\nend\n",
    );

    let edges = calls(&result);
    assert!(edges.is_empty(), "head self-loops: {edges:?}");
    let head_rows: Vec<_> = result
        .identifiers
        .iter()
        .filter(|i| {
            matches!(i.name.as_str(), "run" | "show" | "check")
                || (i.name == "name" && i.start_line == 6 && i.start_column < 25)
        })
        .map(|i| (i.name.clone(), i.kind.clone(), i.start_line))
        .collect();
    assert!(head_rows.is_empty(), "head identifiers: {head_rows:?}");
    assert!(
        result
            .identifiers
            .iter()
            .any(|i| i.name == "is_map" && i.kind == IdentifierKind::Call)
    );
}

#[test]
fn remote_calls_use_the_full_module_name_and_expand_aliases() {
    let result = extract(
        "lib/web.ex",
        "defmodule MyAppWeb.PageController do\n  alias MyApp.{Accounts, Repo}\n  alias MyApp.Billing.Invoice, as: Inv\n  alias MyApp.Users\n  def index(id) do\n    Accounts.get_user(id)\n    Inv.fetch(id)\n    Users.Admin.list()\n    MyApp.Other.go()\n    __MODULE__.helper(id)\n    Repo.all(id)\n  end\n  def helper(x), do: x\n  defmodule Entry do\n    def make, do: :ok\n  end\n  def build, do: Entry.make()\nend\n",
    );

    let rows = pending(&result);
    for expected in [
        row("index", "get_user", &["MyApp.Accounts"]),
        row("index", "fetch", &["MyApp.Billing.Invoice"]),
        row("index", "list", &["MyApp.Users.Admin"]),
        row("index", "go", &["MyApp.Other"]),
        row("index", "all", &["MyApp.Repo"]),
    ] {
        assert!(rows.contains(&expected), "missing {expected:?} in {rows:?}");
    }
    let edges = calls(&result);
    assert!(edges.contains(&edge("index", "helper", 10)), "{edges:?}");
    assert!(edges.contains(&edge("build", "make", 17)), "{edges:?}");

    symbol(&result, "MyAppWeb.PageController.Entry", SymbolKind::Module);
    let imports: Vec<_> = result
        .symbols
        .iter()
        .filter(|s| s.kind == SymbolKind::Import)
        .map(|s| {
            let alias = s
                .metadata
                .as_ref()
                .and_then(|m| m.get("alias"))
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string();
            (s.name.clone(), alias)
        })
        .collect();
    for expected in [
        ("MyApp.Accounts", "Accounts"),
        ("MyApp.Repo", "Repo"),
        ("MyApp.Billing.Invoice", "Inv"),
        ("MyApp.Users", "Users"),
    ] {
        assert!(
            imports.contains(&(expected.0.to_string(), expected.1.to_string())),
            "missing {expected:?} in {imports:?}"
        );
    }
}

#[test]
fn use_and_behaviour_targets_keep_the_full_module_name() {
    let result = extract(
        "lib/worker.ex",
        "defmodule Worker do\n  use MyApp.Web, :controller\n  @behaviour MyApp.Storage\nend\n",
    );
    let targets: Vec<_> = result
        .structured_pending_relationships
        .iter()
        .map(|p| {
            (
                p.target.terminal_name.clone(),
                p.target.namespace_path.clone(),
            )
        })
        .collect();
    assert!(
        targets.contains(&("MyApp.Web".to_string(), vec![])),
        "{targets:?}"
    );
    assert!(
        targets.contains(&("MyApp.Storage".to_string(), vec![])),
        "{targets:?}"
    );
}

#[test]
fn body_spans_cover_the_real_body() {
    let source = "defmodule Calc do\n  def add(x), do: x + 1\n  def double(x), do: x * 2\n  def head(opts \\\\ [])\n  defguard is_pos(n) when is_integer(n) and n > 0\n  defdelegate get(id), to: Store\n  @type t :: %{id: integer()}\n  def run(x) do\n    x\n  end\nend\n";
    let result = extract("lib/calc.ex", source);
    let body = |name: &str, kind: SymbolKind| {
        symbol(&result, name, kind)
            .body_span
            .map(|span| source[span.start_byte as usize..span.end_byte as usize].to_string())
    };
    assert_eq!(body("add", SymbolKind::Function).as_deref(), Some("x + 1"));
    assert_eq!(
        body("double", SymbolKind::Function).as_deref(),
        Some("x * 2")
    );
    assert_eq!(body("head", SymbolKind::Function), None);
    assert_eq!(
        body("is_pos", SymbolKind::Function).as_deref(),
        Some("is_integer(n) and n > 0")
    );
    assert_eq!(body("get", SymbolKind::Delegate), None);
    assert_eq!(
        body("t", SymbolKind::Type).as_deref(),
        Some("%{id: integer()}")
    );
    assert_eq!(
        body("run", SymbolKind::Function).as_deref(),
        Some("do\n    x\n  end")
    );
    assert_ne!(
        symbol(&result, "add", SymbolKind::Function).body_hash,
        symbol(&result, "double", SymbolKind::Function).body_hash
    );
}

#[test]
fn doc_attributes_attach_past_spec_and_impl() {
    let result = extract(
        "lib/outer.ex",
        "defmodule Outer do\n  defmodule Inner do\n    @moduledoc \"\"\"\n    Inner docs here.\n    Second line.\n    \"\"\"\n    @behaviour Plug\n  end\n\n  @doc \"\"\"\n  Adds numbers.\n  \"\"\"\n  @spec add(integer(), integer()) :: integer()\n  def add(a, b), do: a + b\n\n  @doc \"Sub with impl.\"\n  @impl true\n  def sub(a, b), do: a - b\n\n  @doc false\n  def hidden, do: :ok\n\n  @typedoc \"A user id.\"\n  @type id :: integer()\n\n  @doc \"Runs.\"\n  @callback run() :: :ok\nend\n",
    );
    let doc = |name: &str, kind: SymbolKind| symbol(&result, name, kind).doc_comment.clone();
    assert_eq!(doc("Outer", SymbolKind::Module), None);
    assert!(
        symbol(&result, "Outer", SymbolKind::Module)
            .annotations
            .is_empty()
    );
    assert_eq!(
        doc("Outer.Inner", SymbolKind::Module).as_deref(),
        Some("Inner docs here.\nSecond line.")
    );
    assert_eq!(
        doc("add", SymbolKind::Function).as_deref(),
        Some("Adds numbers.")
    );
    assert_eq!(
        doc("sub", SymbolKind::Function).as_deref(),
        Some("Sub with impl.")
    );
    assert_eq!(doc("hidden", SymbolKind::Function), None);
    assert_eq!(doc("id", SymbolKind::Type).as_deref(), Some("A user id."));
    assert_eq!(doc("run", SymbolKind::Function).as_deref(), Some("Runs."));
    let add_keys: Vec<_> = symbol(&result, "add", SymbolKind::Function)
        .annotations
        .iter()
        .map(|a| a.annotation_key.clone())
        .collect();
    assert_eq!(add_keys, vec!["doc", "spec"]);
}

#[test]
fn keyword_defstruct_fields_are_the_keys() {
    let source = "defmodule MyApp.Point do\n  defstruct x: 0, y: 0, label: :origin, tags: [:a, :b]\nend\ndefmodule MyApp.Mixed do\n  defstruct [:id, name: \"anon\", role: :user]\nend\ndefmodule MyApp.NotFound do\n  defexception message: \"not found\", status: :not_found\nend\n";
    let result = extract("lib/structs.ex", source);
    let fields = |owner: &str| {
        let owner_id = &symbol(&result, owner, SymbolKind::Struct).id;
        result
            .symbols
            .iter()
            .filter(|s| s.kind == SymbolKind::Field && s.parent_id.as_ref() == Some(owner_id))
            .map(|s| {
                (
                    s.name.clone(),
                    source[s.start_byte as usize..s.end_byte as usize].to_string(),
                )
            })
            .collect::<Vec<_>>()
    };
    let names = |owner: &str| {
        fields(owner)
            .into_iter()
            .map(|(n, _)| n)
            .collect::<Vec<_>>()
    };
    assert_eq!(names("MyApp.Point"), ["x", "y", "label", "tags"]);
    assert_eq!(names("MyApp.Mixed"), ["id", "name", "role"]);
    assert_eq!(names("MyApp.NotFound"), ["message", "status"]);
    assert_eq!(fields("MyApp.Mixed")[0].1, ":id");
}
