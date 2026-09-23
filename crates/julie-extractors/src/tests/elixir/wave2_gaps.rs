use crate::base::{
    ExtractionResults, IdentifierKind, RelationshipKind, SourceRegionKind, Symbol, SymbolKind,
};
use crate::extract_canonical;
use std::path::Path;

fn extract(path: &str, source: &str) -> ExtractionResults {
    extract_canonical(path, source, Path::new("/tmp/test")).expect("elixir extraction")
}

fn name_of(result: &ExtractionResults, id: &str) -> String {
    result
        .symbols
        .iter()
        .find(|s| s.id == id)
        .map(|s| s.name.clone())
        .unwrap_or_default()
}

fn symbols_named<'a>(result: &'a ExtractionResults, name: &str) -> Vec<&'a Symbol> {
    result.symbols.iter().filter(|s| s.name == name).collect()
}

fn edges(result: &ExtractionResults, kind: RelationshipKind) -> Vec<(String, String)> {
    let mut rows: Vec<_> = result
        .relationships
        .iter()
        .filter(|r| r.kind == kind)
        .map(|r| {
            (
                name_of(result, &r.from_symbol_id),
                name_of(result, &r.to_symbol_id),
            )
        })
        .collect();
    rows.sort();
    rows
}

fn pending_targets(result: &ExtractionResults) -> Vec<String> {
    let mut rows: Vec<_> = result
        .structured_pending_relationships
        .iter()
        .map(|p| p.target.display_name.clone())
        .collect();
    rows.sort();
    rows
}

fn idents(result: &ExtractionResults, kind: IdentifierKind) -> Vec<String> {
    result
        .identifiers
        .iter()
        .filter(|i| i.kind == kind)
        .map(|i| i.name.clone())
        .collect()
}

fn meta_str<'a>(symbol: &'a Symbol, key: &str) -> Option<&'a str> {
    symbol.metadata.as_ref()?.get(key)?.as_str()
}

fn type_of(result: &ExtractionResults, symbol: &Symbol) -> Option<String> {
    result
        .types
        .get(&symbol.id)
        .map(|info| info.resolved_type.clone())
}

fn pair(a: &str, b: &str) -> (String, String) {
    (a.to_string(), b.to_string())
}

#[test]
fn special_forms_and_attribute_names_are_not_calls() {
    let result = extract(
        "lib/flow.ex",
        "defmodule Flow do\n  @doc \"Classify.\"\n  @impl true\n  def classify(x) do\n    if x > 0 do\n      case x do\n        1 -> :one\n      end\n    else\n      cond do\n        true -> :neg\n      end\n    end\n  end\n  def each(xs) do\n    for x <- xs, do: x\n    with {:ok, v} <- {:ok, 1}, do: v\n    unless xs == [], do: :ok\n    try do\n      :ok\n    rescue\n      _ -> :error\n    end\n    receive do\n      m -> m\n    end\n    quote do: unquote(xs)\n  end\nend\n",
    );

    let calls = idents(&result, IdentifierKind::Call);
    assert!(calls.is_empty(), "{calls:?}");
    let pending = pending_targets(&result);
    assert!(pending.is_empty(), "{pending:?}");
}

#[test]
fn field_access_and_anonymous_calls_are_not_pending_calls() {
    let result = extract(
        "lib/cart.ex",
        "defmodule Shop.Cart do\n  def run(cart, mod) do\n    :ets.insert(:carts, {cart.id, cart})\n    apply_fn = fn x -> x * 2 end\n    apply_fn.(3)\n    mod.process(cart)\n    cart.user.name\n  end\nend\n",
    );

    let pending: Vec<_> = result
        .structured_pending_relationships
        .iter()
        .map(|p| {
            (
                p.target.display_name.clone(),
                p.target.receiver.clone(),
                p.target.namespace_path.clone(),
            )
        })
        .collect();
    assert_eq!(pending.len(), 2, "{pending:?}");
    assert!(
        pending.contains(&(":ets.insert".to_string(), None, vec!["ets".to_string()])),
        "{pending:?}"
    );
    assert!(
        pending.contains(&("mod.process".to_string(), Some("mod".to_string()), vec![])),
        "{pending:?}"
    );

    let mut members = idents(&result, IdentifierKind::MemberAccess);
    members.sort();
    assert_eq!(members, ["id", "insert", "name", "process", "user"]);
}

#[test]
fn local_function_captures_call_the_captured_function() {
    let result = extract(
        "lib/shop.ex",
        "defmodule Shop do\n  def run(items) do\n    Enum.each(items, &log/1)\n    Enum.map(items, &Remote.price/1)\n  end\n  defp log(i), do: i\nend\n",
    );

    assert!(
        edges(&result, RelationshipKind::Calls).contains(&pair("run", "log")),
        "{:?}",
        result.relationships
    );
    assert!(idents(&result, IdentifierKind::Call).contains(&"log".to_string()));
    assert!(!idents(&result, IdentifierKind::VariableRef).contains(&"log".to_string()));
    assert!(pending_targets(&result).contains(&"Remote.price".to_string()));
}

#[test]
fn defimpl_names_use_the_enclosing_module_and_external_protocols_stay_pending() {
    let result = extract(
        "lib/shape.ex",
        "defprotocol Shape do\n  def area(shape)\nend\ndefmodule Circle do\n  defimpl Shape do\n    def area(_), do: 1\n  end\nend\ndefimpl Shape, for: [Map, List] do\n  def area(_), do: 0\nend\ndefimpl Jason.Encoder, for: Circle do\n  def encode(c, _opts), do: c\nend\n",
    );

    let impls: Vec<_> = result
        .symbols
        .iter()
        .filter(|s| s.kind == SymbolKind::Class)
        .map(|s| s.name.as_str())
        .collect();
    assert_eq!(
        impls,
        ["Shape.Circle", "Shape.{Map, List}", "Jason.Encoder.Circle"]
    );

    let implements = edges(&result, RelationshipKind::Implements);
    assert_eq!(
        implements,
        [
            pair("Shape.Circle", "Shape"),
            pair("Shape.{Map, List}", "Shape")
        ]
    );
    let pending: Vec<_> = result
        .structured_pending_relationships
        .iter()
        .filter(|p| p.pending.kind == RelationshipKind::Implements)
        .map(|p| p.target.display_name.clone())
        .collect();
    assert_eq!(pending, ["Jason.Encoder"]);

    let types = idents(&result, IdentifierKind::TypeUsage);
    assert!(types.contains(&"Jason.Encoder".to_string()), "{types:?}");
}

#[test]
fn spec_return_types_match_module_name_and_arity() {
    let result = extract(
        "lib/users.ex",
        "defmodule Users do\n  @spec user(integer()) :: User.t()\n  def user(id), do: id\n  @spec fetch(integer()) :: User.t()\n  def fetch(id), do: id\n  @spec fetch(integer(), keyword()) :: Account.t()\n  def fetch(id, _opts), do: id\n  @spec count :: non_neg_integer()\n  def count, do: 0\n  @spec pick(x) :: x when x: term()\n  def pick(x), do: x\n  def render(user) do\n    user = normalize(user)\n    user\n  end\n  def normalize(x), do: x\nend\ndefmodule Other do\n  def user(id), do: id\nend\n",
    );

    let typed: Vec<_> = result
        .symbols
        .iter()
        .filter_map(|s| type_of(&result, s).map(|t| (s.name.clone(), s.kind.clone(), t)))
        .collect();
    assert_eq!(
        typed,
        [
            (
                "user".to_string(),
                SymbolKind::Function,
                "User.t".to_string()
            ),
            (
                "fetch".to_string(),
                SymbolKind::Function,
                "User.t".to_string()
            ),
            (
                "fetch".to_string(),
                SymbolKind::Function,
                "Account.t".to_string()
            ),
            (
                "count".to_string(),
                SymbolKind::Function,
                "non_neg_integer".to_string()
            ),
        ]
    );
}

#[test]
fn defdelegate_emits_a_call_to_its_target() {
    let result = extract(
        "lib/my_app.ex",
        "defmodule MyApp do\n  defdelegate list_users(opts), to: MyApp.Accounts\n  defdelegate fetch(id), to: MyApp.Billing.Invoice, as: :get\n  defdelegate local(x), to: MyApp.Local\nend\ndefmodule MyApp.Local do\n  def local(x), do: x\nend\n",
    );

    let pending: Vec<_> = result
        .structured_pending_relationships
        .iter()
        .map(|p| {
            (
                name_of(&result, &p.pending.from_symbol_id),
                p.target.display_name.clone(),
            )
        })
        .collect();
    assert_eq!(
        pending,
        [
            pair("list_users", "MyApp.Accounts.list_users"),
            pair("fetch", "MyApp.Billing.Invoice.get"),
        ]
    );
    assert_eq!(
        edges(&result, RelationshipKind::Calls),
        [pair("local", "local")]
    );
    let fetch = &symbols_named(&result, "fetch")[0];
    assert_eq!(
        meta_str(fetch, "delegate_to"),
        Some("MyApp.Billing.Invoice")
    );
    assert_eq!(meta_str(fetch, "delegate_as"), Some("get"));
}

#[test]
fn module_attributes_are_constants_and_reads_name_the_attribute() {
    let result = extract(
        "lib/poller.ex",
        "defmodule Poller do\n  @moduledoc false\n  @interval 5_000\n  @default_opts [retries: 3]\n  def interval, do: @interval\n  def schedule(opts \\\\ @default_opts) do\n    Process.send_after(self(), :poll, @interval)\n    opts\n  end\nend\n",
    );

    let constants: Vec<_> = result
        .symbols
        .iter()
        .filter(|s| s.kind == SymbolKind::Constant)
        .map(|s| {
            (
                s.name.as_str(),
                name_of(&result, s.parent_id.as_deref().unwrap_or("")),
            )
        })
        .collect();
    assert_eq!(
        constants,
        [
            ("@interval", "Poller".to_string()),
            ("@default_opts", "Poller".to_string())
        ]
    );

    let params: Vec<_> = result
        .symbols
        .iter()
        .filter(|s| s.kind == SymbolKind::Variable)
        .map(|s| s.name.as_str())
        .collect();
    assert_eq!(params, ["opts"]);

    let calls = idents(&result, IdentifierKind::Call);
    assert_eq!(calls, ["self"]);
    let reads = idents(&result, IdentifierKind::VariableRef);
    assert_eq!(reads, ["@interval", "@default_opts", "@interval", "opts"]);

    let names: Vec<_> = result
        .structural_facts
        .iter()
        .filter(|f| f.pattern_id == "elixir.module_attribute.v1")
        .map(|f| {
            f.metadata
                .as_ref()
                .and_then(|m| m.get("attribute_name"))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string()
        })
        .collect();
    assert_eq!(
        names,
        [
            "moduledoc",
            "interval",
            "default_opts",
            "interval",
            "default_opts",
            "interval"
        ]
    );
}

#[test]
fn ecto_schema_defines_a_struct_with_fields() {
    let result = extract(
        "lib/user.ex",
        "defmodule MyApp.User do\n  use Ecto.Schema\n  schema \"users\" do\n    field :email, :string\n    field :age, :integer, default: 0\n    belongs_to :org, MyApp.Org\n    has_many :posts, MyApp.Post\n    embeds_one :profile, MyApp.Profile\n    timestamps()\n  end\nend\ndefmodule MyApp.Address do\n  use Ecto.Schema\n  embedded_schema do\n    field :street, :string\n  end\nend\n",
    );

    let user = result
        .symbols
        .iter()
        .find(|s| s.kind == SymbolKind::Struct && s.name == "MyApp.User")
        .expect("schema struct");
    assert_eq!(meta_str(user, "table"), Some("users"));
    let fields: Vec<_> = result
        .symbols
        .iter()
        .filter(|s| s.kind == SymbolKind::Field && s.parent_id.as_deref() == Some(&user.id))
        .map(|s| s.name.as_str())
        .collect();
    assert_eq!(
        fields,
        [
            "email",
            "age",
            "org",
            "org_id",
            "posts",
            "profile",
            "inserted_at",
            "updated_at"
        ]
    );
    assert!(
        result
            .symbols
            .iter()
            .any(|s| s.kind == SymbolKind::Struct && s.name == "MyApp.Address")
    );
    assert!(symbols_named(&result, "street")[0].kind == SymbolKind::Field);
    let types = idents(&result, IdentifierKind::TypeUsage);
    assert!(types.contains(&"MyApp.Org".to_string()), "{types:?}");
}

#[test]
fn typespec_names_drop_parameters_and_macrocallbacks_are_callbacks() {
    let result = extract(
        "lib/repo.ex",
        "defmodule Repo2 do\n  @type result(t) :: {:ok, t} | {:error, term()}\n  @type id() :: pos_integer()\n  @macrocallback build(term()) :: Macro.t()\n  @spec map_ok(Enumerable.t(User.t())) :: MapSet.t(term())\n  def map_ok(e), do: e\nend\n",
    );

    let types: Vec<_> = result
        .symbols
        .iter()
        .filter(|s| s.kind == SymbolKind::Type)
        .map(|s| s.name.as_str())
        .collect();
    assert_eq!(types, ["result", "id"]);
    let build = &symbols_named(&result, "build")[0];
    assert_eq!(build.kind, SymbolKind::Function);
    let meta = build.metadata.as_ref().expect("callback metadata");
    assert_eq!(meta.get("macro"), Some(&serde_json::Value::Bool(true)));
    assert_eq!(meta.get("callback"), Some(&serde_json::Value::Bool(true)));

    let generics: Vec<_> = result
        .type_argument_usages
        .iter()
        .filter_map(|usage| {
            let generic = result
                .identifiers
                .iter()
                .find(|i| i.id == usage.identifier_id)?;
            let first = usage.arguments.first()?;
            Some((generic.name.clone(), first.type_name.clone()))
        })
        .collect();
    assert_eq!(generics, [pair("t", "User.t"), pair("t", "term")]);
}

#[test]
fn alias_arguments_of_ordinary_calls_are_type_usages() {
    let result = extract(
        "lib/app_web/router.ex",
        "defmodule AppWeb.Router do\n  use Phoenix.Router\n  alias AppWeb.Helpers\n  @behaviour Plug\n  scope \"/\", AppWeb do\n    get \"/\", PageController, :home\n  end\n  plug AppWeb.Plugs.RequireAdmin\n  def show(conn, _), do: render(conn, AppWeb.UserView)\nend\n",
    );

    let mut types = idents(&result, IdentifierKind::TypeUsage);
    types.sort();
    assert_eq!(
        types,
        [
            "AppWeb",
            "AppWeb.Plugs.RequireAdmin",
            "AppWeb.UserView",
            "PageController"
        ]
    );
}

#[test]
fn test_roles_follow_exunit() {
    let lib = extract(
        "lib/db.ex",
        "defmodule Db do\n  def test_connection(conn), do: conn\nend\n",
    );
    let test_connection = &symbols_named(&lib, "test_connection")[0];
    assert_eq!(meta_str(test_connection, "test_role"), None);

    let result = extract(
        "test/user_test.exs",
        "defmodule MyApp.UserTest do\n  use ExUnit.Case\n  use ExUnitProperties\n  doctest MyApp.User\n  setup :create_user\n  setup [:a, :b]\n  property \"reverse twice\" do\n    check all list <- list_of(integer()) do\n      assert list\n    end\n  end\n  defp create_user(_), do: %{}\n  defp a(_), do: %{}\n  defp b(_), do: %{}\nend\n",
    );
    let module = &symbols_named(&result, "MyApp.UserTest")[0];
    assert_eq!(meta_str(module, "test_role"), Some("test_container"));
    let property = &symbols_named(&result, "reverse twice")[0];
    assert_eq!(meta_str(property, "test_role"), Some("test_case"));
    let doctest = &symbols_named(&result, "doctest MyApp.User")[0];
    assert_eq!(meta_str(doctest, "test_role"), Some("test_case"));
    assert_eq!(
        edges(&result, RelationshipKind::Calls),
        [
            pair("setup", "a"),
            pair("setup", "b"),
            pair("setup", "create_user")
        ]
    );
}

#[test]
fn parameter_counts_include_every_head_argument() {
    let result = extract(
        "lib/heads.ex",
        "defmodule Heads do\n  @ttl 1_000\n  def put(server \\\\ __MODULE__, key, value, ttl \\\\ @ttl), do: {server, key, value, ttl}\n  def handle_call({:get, key}, _from, state), do: {:reply, key, state}\n  def update(%{id: id} = user, attrs), do: {id, user, attrs}\n  defmacro trace(expr), do: expr\n  def count, do: 0\nend\n",
    );

    let counts: Vec<_> = result
        .complexity_metrics
        .iter()
        .filter_map(|m| {
            m.symbol_id
                .as_deref()
                .map(|id| (name_of(&result, id), m.parameter_count))
        })
        .collect();
    assert_eq!(
        counts,
        [
            ("put".to_string(), Some(4)),
            ("handle_call".to_string(), Some(3)),
            ("update".to_string(), Some(2)),
            ("trace".to_string(), Some(1)),
            ("count".to_string(), Some(0)),
        ]
    );
    let put_params: Vec<_> = result
        .symbols
        .iter()
        .filter(|s| {
            s.kind == SymbolKind::Variable
                && name_of(&result, s.parent_id.as_deref().unwrap_or("")) == "put"
        })
        .map(|s| s.name.as_str())
        .collect();
    assert_eq!(put_params, ["server", "key", "value", "ttl"]);
}

#[test]
fn sigils_produce_regions() {
    let result = extract(
        "lib/page.ex",
        "defmodule Page do\n  def render(assigns) do\n    ~H\"\"\"\n    <div>{@title}</div>\n    \"\"\"\n  end\n  def email?(s), do: s =~ ~r/^[^@]+@[^@]+$/\n  def path, do: ~p\"/users/new\"\nend\n",
    );

    let regions: Vec<_> = result
        .source_regions
        .iter()
        .map(|r| (r.start_line, r.kind.clone()))
        .collect();
    assert_eq!(
        regions,
        [
            (3, SourceRegionKind::Embedded),
            (7, SourceRegionKind::StringLiteral),
            (8, SourceRegionKind::StringLiteral),
        ]
    );
    let embedded = &result.source_regions[0];
    assert_eq!(
        embedded
            .metadata
            .as_ref()
            .and_then(|m| m.get("embedded_language"))
            .and_then(|v| v.as_str()),
        Some("heex")
    );
}

fn facts(
    result: &ExtractionResults,
    pattern: &str,
) -> Vec<serde_json::Map<String, serde_json::Value>> {
    result
        .structural_facts
        .iter()
        .filter(|f| f.pattern_id == pattern)
        .map(|f| f.metadata.clone().unwrap_or_default().into_iter().collect())
        .collect()
}

fn text(fact: &serde_json::Map<String, serde_json::Value>, key: &str) -> String {
    fact.get(key)
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string()
}

#[test]
fn phoenix_nested_resources_live_routes_and_scope_options() {
    let result = extract(
        "lib/shop_web/router.ex",
        "defmodule ShopWeb.Router do\n  use ShopWeb, :router\n  scope \"/api/v1\", ShopWeb.V1 do\n    resources \"/users\", UserController, only: [:index] do\n      resources \"/posts\", PostController\n      get \"/avatar\", AvatarController, :show\n    end\n    live \"/dash\", DashLive.Index, :index\n  end\n  scope path: \"/beta\", alias: ShopWeb.Beta do\n    get \"/feature\", FeatureController, :index\n  end\nend\n",
    );

    let routes: Vec<_> = facts(&result, "phoenix.route.v1")
        .iter()
        .map(|f| {
            (
                text(f, "normalized_route_template"),
                text(f, "verb"),
                text(f, "controller_module"),
                text(f, "handler_kind"),
            )
        })
        .collect();
    let row = |a: &str, b: &str, c: &str, d: &str| {
        (a.to_string(), b.to_string(), c.to_string(), d.to_string())
    };
    assert_eq!(
        routes,
        [
            row(
                "/api/v1/users/:user_id/avatar",
                "GET",
                "ShopWeb.V1.AvatarController",
                ""
            ),
            row(
                "/api/v1/dash",
                "GET",
                "ShopWeb.V1.DashLive.Index",
                "live_view"
            ),
            row("/beta/feature", "GET", "ShopWeb.Beta.FeatureController", ""),
        ]
    );

    let resources: Vec<_> = facts(&result, "phoenix.resource_route.v1")
        .iter()
        .map(|f| {
            (
                text(f, "normalized_resource_path"),
                text(f, "controller_module"),
            )
        })
        .collect();
    assert_eq!(
        resources,
        [
            pair("/api/v1/users", "ShopWeb.V1.UserController"),
            pair("/api/v1/users/:user_id/posts", "ShopWeb.V1.PostController"),
        ]
    );
}

#[test]
fn tesla_module_clients_join_the_base_url_and_aliases_resolve() {
    let result = extract(
        "lib/github.ex",
        "defmodule GitHub do\n  use Tesla\n  plug Tesla.Middleware.BaseUrl, \"https://api.github.com\"\n  def repos, do: get(\"/user/repos\")\nend\ndefmodule Other do\n  alias Req, as: R\n  alias MyApp.Fake, as: Req\n  def a, do: R.get!(\"https://example.com/real\")\n  def b, do: Req.get!(\"https://example.com/fake\")\n  def c, do: get(\"/not/tesla\")\nend\n",
    );

    let requests: Vec<_> = facts(&result, "http.client_request.v1")
        .iter()
        .map(|f| (text(f, "client"), text(f, "target_path")))
        .collect();
    assert_eq!(
        requests,
        [
            pair("tesla", "https://api.github.com/user/repos"),
            pair("req", "https://example.com/real"),
        ]
    );
}
