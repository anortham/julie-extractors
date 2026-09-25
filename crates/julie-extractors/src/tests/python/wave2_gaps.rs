use crate::base::{IdentifierKind, RelationshipKind, Symbol, SymbolKind, Visibility};
use crate::{ExtractionResults, extract_canonical};
use std::path::Path;

fn extract(path: &str, source: &str) -> ExtractionResults {
    extract_canonical(path, source, Path::new("/tmp/test")).expect("python extraction")
}

fn one<'a>(result: &'a ExtractionResults, name: &str) -> &'a Symbol {
    let found: Vec<_> = result.symbols.iter().filter(|s| s.name == name).collect();
    assert_eq!(found.len(), 1, "expected one `{name}`, got {found:#?}");
    found[0]
}

fn idents<'a>(result: &'a ExtractionResults, name: &str) -> Vec<&'a crate::base::Identifier> {
    result
        .identifiers
        .iter()
        .filter(|i| i.name == name)
        .collect()
}

#[test]
fn type_alias_statement_is_a_type_symbol_and_not_a_usage() {
    let result = extract(
        "alias.py",
        "from app.models import User\n\ntype UserId = int\ntype Pair[T] = tuple[T, T]\ntype Lookup = dict[str, User]\n",
    );
    let pair = one(&result, "Pair");
    assert_eq!(pair.kind, SymbolKind::Type);
    assert_eq!(
        pair.signature.as_deref(),
        Some("type Pair[T] = tuple[T, T]")
    );
    assert_eq!(one(&result, "UserId").kind, SymbolKind::Type);
    assert_eq!(one(&result, "Lookup").kind, SymbolKind::Type);
    assert!(idents(&result, "Lookup").is_empty());
    assert!(idents(&result, "Pair").is_empty());
    let user = idents(&result, "User");
    assert_eq!(user.len(), 1);
    assert_eq!(user[0].kind, IdentifierKind::TypeUsage);
}

#[test]
fn generic_classes_and_functions_keep_type_parameters_in_signatures() {
    let result = extract(
        "generic.py",
        "class Box[T]: ...\n\ndef first[T](items: list[T]) -> T: ...\n",
    );
    assert_eq!(
        one(&result, "Box").signature.as_deref(),
        Some("class Box[T]")
    );
    assert_eq!(
        one(&result, "first").signature.as_deref(),
        Some("def first[T](items: list[T]) -> T")
    );
}

#[test]
fn super_call_carries_the_first_base_and_resolves_in_file() {
    let result = extract(
        "sup.py",
        "class Parent:\n    def save(self):\n        return 1\n\nclass Child(Parent):\n    def save(self):\n        return super().save()\n\n    @classmethod\n    def build(cls):\n        return cls()\n",
    );
    assert!(
        result
            .structured_pending_relationships
            .iter()
            .all(|p| p.target.terminal_name != "super"
                && p.target.receiver.as_deref() != Some("super()")),
        "{:#?}",
        result.structured_pending_relationships
    );
    let parent_save = result
        .symbols
        .iter()
        .find(|s| s.name == "save" && s.start_line == 2)
        .unwrap();
    let child_save = result
        .symbols
        .iter()
        .find(|s| s.name == "save" && s.start_line == 6)
        .unwrap();
    assert!(
        result
            .relationships
            .iter()
            .any(|r| r.kind == RelationshipKind::Calls
                && r.from_symbol_id == child_save.id
                && r.to_symbol_id == parent_save.id),
        "{:#?}",
        result.relationships
    );
    let save_call = result
        .identifiers
        .iter()
        .find(|i| i.name == "save" && i.kind == IdentifierKind::Call)
        .unwrap();
    assert_eq!(save_call.receiver_type.as_deref(), Some("Parent"));
    let cls_call = result
        .structured_pending_relationships
        .iter()
        .find(|p| p.pending.line_number == 11)
        .expect("cls() pending");
    assert_eq!(cls_call.target.terminal_name, "Child");
    assert_eq!(cls_call.receiver_type.as_deref(), Some("Child"));
}

#[test]
fn a_local_cls_binding_is_not_the_enclosing_class() {
    let result = extract(
        "app.py",
        "class App:\n    def test_client(self):\n        cls = self.client_class\n        if cls is None:\n            from .testing import Client as cls\n        return cls(self)\n\n    @classmethod\n    def build(cls):\n        def make():\n            return cls()\n        return make()\n",
    );
    let terminal_at = |line: u32| {
        result
            .structured_pending_relationships
            .iter()
            .find(|p| p.pending.line_number == line)
            .map(|p| p.target.terminal_name.clone())
    };
    assert_eq!(terminal_at(6).as_deref(), Some("cls"));
    assert_eq!(terminal_at(11).as_deref(), Some("App"));
}

#[test]
fn super_call_to_an_imported_base_stays_pending_with_receiver_type() {
    let result = extract(
        "view.py",
        "from django.views import View\n\nclass Home(View):\n    def dispatch(self, request):\n        return super().dispatch(request)\n",
    );
    let pending = result
        .structured_pending_relationships
        .iter()
        .find(|p| p.target.terminal_name == "dispatch")
        .expect("dispatch pending");
    assert_eq!(pending.receiver_type.as_deref(), Some("View"));
    assert_eq!(pending.target.receiver.as_deref(), Some("super"));
}

#[test]
fn only_typing_protocol_and_enum_bases_change_the_class_kind() {
    let result = extract(
        "kinds.py",
        "import asyncio\nimport enum\nimport typing\nfrom django.db import models\n\nclass ProtocolError(Exception):\n    pass\n\nclass H2Error(ProtocolError):\n    pass\n\nclass EchoServer(asyncio.Protocol):\n    pass\n\nclass BaseEnumerable:\n    pass\n\nclass Seq(BaseEnumerable):\n    pass\n\nclass Reader(typing.Protocol):\n    pass\n\nclass Getter(Protocol[T]):\n    pass\n\nclass Status(str, enum.Enum):\n    active = \"active\"\n\nclass Perm(enum.IntFlag):\n    READ = 1\n\nclass Kind(models.TextChoices):\n    DRAFT = \"DF\", \"Draft\"\n",
    );
    assert_eq!(one(&result, "H2Error").kind, SymbolKind::Class);
    assert_eq!(one(&result, "EchoServer").kind, SymbolKind::Class);
    assert_eq!(one(&result, "Seq").kind, SymbolKind::Class);
    assert_eq!(one(&result, "Reader").kind, SymbolKind::Interface);
    assert_eq!(one(&result, "Getter").kind, SymbolKind::Interface);
    assert_eq!(one(&result, "Status").kind, SymbolKind::Enum);
    assert_eq!(one(&result, "Perm").kind, SymbolKind::Enum);
    assert_eq!(one(&result, "Kind").kind, SymbolKind::Enum);
    assert_eq!(one(&result, "active").kind, SymbolKind::EnumMember);
    assert_eq!(one(&result, "DRAFT").kind, SymbolKind::EnumMember);
}

#[test]
fn every_plain_enum_body_assignment_is_an_enum_member() {
    let result = extract(
        "color.py",
        "from enum import Enum, auto\n\nclass Color(Enum):\n    RED = 1\n    green = 2\n    Blue = auto()\n    X = 3\n    _ignore_ = ['tmp']\n    __doc__ = 'colors'\n\n    def describe(self):\n        LIMIT = 3\n        return LIMIT\n",
    );
    for name in ["RED", "green", "Blue", "X"] {
        let member = one(&result, name);
        assert_eq!(member.kind, SymbolKind::EnumMember, "{name}");
        assert_eq!(
            member.parent_id.as_deref(),
            Some(one(&result, "Color").id.as_str())
        );
    }
    assert_ne!(one(&result, "_ignore_").kind, SymbolKind::EnumMember);
    assert_ne!(one(&result, "__doc__").kind, SymbolKind::EnumMember);
    assert_eq!(one(&result, "LIMIT").kind, SymbolKind::Constant);
}

#[test]
fn builtin_generic_subscripts_record_type_arguments() {
    let result = extract(
        "generics.py",
        "from typing import List\nfrom app.models import User\n\ndef names(users: list[User], index: dict[str, User], legacy: List[User]) -> tuple[User, ...]:\n    return users[0]\n",
    );
    let heads: Vec<(String, Vec<String>)> = result
        .type_argument_usages
        .iter()
        .map(|usage| {
            let ident = result
                .identifiers
                .iter()
                .find(|i| i.id == usage.identifier_id)
                .unwrap();
            (
                ident.name.clone(),
                usage
                    .arguments
                    .iter()
                    .map(|a| a.type_name.clone())
                    .collect(),
            )
        })
        .collect();
    assert!(
        heads.contains(&("list".to_string(), vec!["User".to_string()])),
        "{heads:?}"
    );
    assert!(heads.contains(&(
        "dict".to_string(),
        vec!["str".to_string(), "User".to_string()]
    )));
    assert!(heads.contains(&("List".to_string(), vec!["User".to_string()])));
    assert!(heads.contains(&(
        "tuple".to_string(),
        vec!["User".to_string(), "...".to_string()]
    )));
    assert!(idents(&result, "int").is_empty());
}

#[test]
fn match_class_patterns_and_dotted_value_patterns_are_references() {
    let result = extract(
        "match.py",
        "from app.shapes import Point, Circle\nfrom app.colors import Color\n\ndef describe(shape):\n    match shape:\n        case Point(x=0, y=0):\n            return \"origin\"\n        case Circle(radius=r) if r > 10:\n            return \"big\"\n        case Color.RED:\n            return \"red\"\n        case other:\n            return str(other)\n",
    );
    let describe = one(&result, "describe").id.clone();
    let kind_of = |name: &str| -> Vec<IdentifierKind> {
        idents(&result, name)
            .iter()
            .map(|i| i.kind.clone())
            .collect()
    };
    assert_eq!(kind_of("Point"), vec![IdentifierKind::TypeUsage]);
    assert_eq!(kind_of("Circle"), vec![IdentifierKind::TypeUsage]);
    assert_eq!(kind_of("Color"), vec![IdentifierKind::VariableRef]);
    assert_eq!(kind_of("RED"), vec![IdentifierKind::MemberAccess]);
    assert!(
        idents(&result, "Point")
            .iter()
            .all(|i| i.containing_symbol_id.as_deref() == Some(describe.as_str()))
    );
    assert_eq!(kind_of("other"), vec![IdentifierKind::VariableRef]);
}

#[test]
fn forward_reference_strings_emit_type_usages_at_their_spans() {
    let source = "from app.models import User\n\ndef find(repo: \"Repo\", uid: int) -> \"User | None\":\n    return repo.get(uid)\n\nclass Owner:\n    addresses: Mapped[List[\"Address\"]]\n    user: Mapped[\"User\"]\n    state: Literal[\"active\", \"idle\"]\n";
    let result = extract("fwd.py", source);
    for name in ["Repo", "Address"] {
        let found = idents(&result, name);
        assert_eq!(found.len(), 1, "{name}: {found:#?}");
        assert_eq!(found[0].kind, IdentifierKind::TypeUsage);
        let start = found[0].start_byte as usize;
        assert_eq!(&source[start..found[0].end_byte as usize], name);
    }
    assert_eq!(idents(&result, "User").len(), 2);
    assert!(idents(&result, "None").is_empty());
    assert!(idents(&result, "active").is_empty());
    let arg_names: Vec<String> = result
        .type_argument_usages
        .iter()
        .flat_map(|u| u.arguments.iter())
        .flat_map(|a| std::iter::once(a).chain(a.children.iter()))
        .map(|a| a.type_name.clone())
        .collect();
    assert!(arg_names.contains(&"User".to_string()), "{arg_names:?}");
    assert!(arg_names.contains(&"Address".to_string()), "{arg_names:?}");
    let quoted: Vec<&String> = arg_names.iter().filter(|n| n.contains('"')).collect();
    assert_eq!(quoted, vec!["\"active\"", "\"idle\""], "{arg_names:?}");
}

#[test]
fn docstring_is_only_the_first_statement_with_prefix_stripped() {
    let result = extract(
        "doc.py",
        "class Account:\n    r\"\"\"Raw docstring\n    over two lines.\"\"\"\n\n    def withdraw(self, amount):\n        self.balance -= amount\n        \"not a docstring\"\n\n    def total(self):\n        f\"\"\"f-string is not a docstring {self}\"\"\"\n        return 1\n\n    def clean(self):\n        # leading comment\n        \"\"\"Real docstring.\"\"\"\n        return 2\n\nclass Settings:\n    debug: bool = False\n    \"\"\"Whether debug mode is on.\"\"\"\n",
    );
    assert_eq!(
        one(&result, "Account").doc_comment.as_deref(),
        Some("Raw docstring\n    over two lines.")
    );
    assert_eq!(one(&result, "withdraw").doc_comment, None);
    assert_eq!(one(&result, "total").doc_comment, None);
    assert_eq!(
        one(&result, "clean").doc_comment.as_deref(),
        Some("Real docstring.")
    );
    assert_eq!(one(&result, "Settings").doc_comment, None);
    assert_eq!(
        one(&result, "debug").doc_comment.as_deref(),
        Some("Whether debug mode is on.")
    );
}

#[test]
fn sphinx_attribute_comments_document_constants_and_fields() {
    let result = extract(
        "consts.py",
        "# Maximum number of retries before giving up.\nMAX_RETRIES = 3\n\n#: Default timeout in seconds.\n#: Applies to every request.\nDEFAULT_TIMEOUT: float = 5.0\n\nclass Config:\n    #: The port to bind.\n    port: int = 80\n",
    );
    assert_eq!(one(&result, "MAX_RETRIES").doc_comment, None);
    assert_eq!(
        one(&result, "DEFAULT_TIMEOUT").doc_comment.as_deref(),
        Some("Default timeout in seconds.\nApplies to every request.")
    );
    assert_eq!(
        one(&result, "port").doc_comment.as_deref(),
        Some("The port to bind.")
    );
}

#[test]
fn signatures_keep_splat_parameters_and_separators() {
    let result = extract(
        "sig.py",
        "def request(method, url, /, *args, timeout: float = 5.0, retries=3, **kwargs):\n    pass\n\ndef typed(*values: int, **options: str) -> None:\n    pass\n\ndef kwonly(a, *, strict: bool, b=1):\n    pass\n",
    );
    assert_eq!(
        one(&result, "request").signature.as_deref(),
        Some("def request(method, url, /, *args, timeout: float = 5.0, retries=3, **kwargs)")
    );
    assert_eq!(
        one(&result, "typed").signature.as_deref(),
        Some("def typed(*values: int, **options: str) -> None")
    );
    assert_eq!(
        one(&result, "kwonly").signature.as_deref(),
        Some("def kwonly(a, *, strict: bool, b=1)")
    );
}

#[test]
fn underscore_classes_are_private() {
    let result = extract(
        "priv.py",
        "class _InternalCache:\n    pass\n\nclass Cache:\n    pass\n",
    );
    assert_eq!(
        one(&result, "_InternalCache").visibility,
        Some(Visibility::Private)
    );
    assert_eq!(one(&result, "Cache").visibility, Some(Visibility::Public));
}

#[test]
fn decorator_calls_belong_to_the_decorated_definition() {
    let result = extract(
        "deco.py",
        "def retry(times):\n    return lambda f: f\n\n@retry(3)\ndef fetch():\n    pass\n\n@login_required\n@require_role(\"admin\")\ndef dashboard(request):\n    pass\n\nclass Report:\n    @cached(ttl=60)\n    def totals(self):\n        pass\n",
    );
    let fetch = one(&result, "fetch").id.clone();
    let retry = one(&result, "retry").id.clone();
    let dashboard = one(&result, "dashboard").id.clone();
    let totals = one(&result, "totals").id.clone();
    assert!(
        result
            .relationships
            .iter()
            .any(|r| r.kind == RelationshipKind::Calls
                && r.from_symbol_id == fetch
                && r.to_symbol_id == retry),
        "{:#?}",
        result.relationships
    );
    let container = |name: &str| {
        idents(&result, name)[0]
            .containing_symbol_id
            .clone()
            .unwrap_or_default()
    };
    assert_eq!(container("retry"), fetch);
    assert_eq!(container("login_required"), dashboard);
    assert_eq!(container("require_role"), dashboard);
    assert_eq!(container("cached"), totals);
    let pending_from = |name: &str| {
        result
            .structured_pending_relationships
            .iter()
            .find(|p| p.target.terminal_name == name)
            .map(|p| p.pending.from_symbol_id.clone())
    };
    assert_eq!(pending_from("require_role"), Some(dashboard));
    assert_eq!(pending_from("cached"), Some(totals));
}

fn facts<'a>(result: &'a ExtractionResults, pattern: &str) -> Vec<&'a crate::base::StructuralFact> {
    crate::tests::helpers::facts_with_pattern(result, pattern)
}

fn meta<'a>(fact: &'a crate::base::StructuralFact, key: &str) -> Option<&'a serde_json::Value> {
    fact.metadata.as_ref()?.get(key)
}

fn meta_str<'a>(fact: &'a crate::base::StructuralFact, key: &str) -> Option<&'a str> {
    meta(fact, key)?.as_str()
}

#[test]
fn http_clients_constructed_in_scope_imported_verbs_and_url_keywords_emit_requests() {
    let result = extract(
        "src/clients.py",
        "import httpx\nimport requests\nfrom requests import get\n\nasync def fetch_users():\n    async with httpx.AsyncClient(base_url=\"https://api.example.com\") as client:\n        return await client.get(\"/users\")\n\ndef sync_session():\n    session = requests.Session()\n    session.post(\"https://api.example.com/login\")\n    get(\"https://api.example.com/plain\")\n    requests.post(url=\"https://api.example.com/c\")\n\ndef other(session):\n    session.post(\"https://unproven.example.com\")\n",
    );
    let requests = facts(&result, "http.client_request.v1");
    let targets: Vec<(&str, &str, &str)> = requests
        .iter()
        .map(|fact| {
            (
                meta_str(fact, "client").unwrap(),
                meta_str(fact, "verb").unwrap(),
                meta_str(fact, "target_path").unwrap(),
            )
        })
        .collect();
    assert_eq!(
        targets,
        vec![
            ("httpx", "GET", "/users"),
            ("requests", "POST", "https://api.example.com/login"),
            ("requests", "GET", "https://api.example.com/plain"),
            ("requests", "POST", "https://api.example.com/c"),
        ]
    );
}

#[test]
fn annotated_flask_and_fastapi_receivers_keep_their_routes() {
    let result = extract(
        "app.py",
        "from flask import Flask\nfrom fastapi import APIRouter\n\napp: Flask = Flask(__name__)\ntyped_router: APIRouter = APIRouter(prefix=\"/t\")\n\n@app.route(\"/health\")\ndef health():\n    return \"ok\"\n\n@typed_router.get(\"/typed\")\ndef typed():\n    pass\n",
    );
    assert_eq!(facts(&result, "flask.route.v1").len(), 1);
    let fastapi = facts(&result, "fastapi.route.v1");
    assert_eq!(fastapi.len(), 1);
    assert_eq!(
        meta_str(fastapi[0], "effective_route_template"),
        Some("/t/typed")
    );
}

#[test]
fn drf_router_registrations_actions_and_api_views_emit_facts() {
    let result = extract(
        "api/urls.py",
        "from rest_framework import routers, viewsets\nfrom rest_framework.decorators import action, api_view\nfrom . import views\n\nrouter = routers.DefaultRouter()\nrouter.register(r\"users\", views.UserViewSet, basename=\"user\")\n\nclass UserViewSet(viewsets.ModelViewSet):\n    @action(detail=True, methods=[\"post\"], url_path=\"set-password\")\n    def set_password(self, request, pk=None):\n        pass\n\n    @action(detail=False)\n    def recent(self, request):\n        pass\n\n@api_view([\"GET\", \"POST\"])\ndef health(request):\n    pass\n",
    );
    let registrations = facts(&result, "drf.router_registration.v1");
    assert_eq!(registrations.len(), 1);
    assert_eq!(meta_str(registrations[0], "resource_name"), Some("users"));
    assert_eq!(
        meta_str(registrations[0], "viewset"),
        Some("views.UserViewSet")
    );
    assert_eq!(meta_str(registrations[0], "basename"), Some("user"));
    let actions = facts(&result, "drf.viewset_action.v1");
    assert_eq!(actions.len(), 2);
    assert_eq!(meta_str(actions[0], "url_path"), Some("set-password"));
    assert_eq!(meta(actions[0], "detail"), Some(&serde_json::json!(true)));
    assert_eq!(
        meta(actions[0], "verbs"),
        Some(&serde_json::json!(["POST"]))
    );
    assert_eq!(meta_str(actions[1], "url_path"), Some("recent"));
    assert_eq!(meta(actions[1], "verbs"), Some(&serde_json::json!(["GET"])));
    let views = facts(&result, "drf.api_view.v1");
    assert_eq!(views.len(), 1);
    assert_eq!(
        meta(views[0], "verbs"),
        Some(&serde_json::json!(["GET", "POST"]))
    );
    assert_eq!(
        views[0].containing_symbol_id.as_deref(),
        Some(one(&result, "health").id.as_str())
    );
}

#[test]
fn django_regex_routes_keep_raw_backslashes_and_normalize_by_policy() {
    let result = extract(
        "project/urls.py",
        "from django.urls import include, path, re_path\nfrom . import views\n\nurlpatterns = [\n    re_path(r\"^blog/(page-(\\d+)/)?$\", views.blog),\n    re_path(r\"^users/(\\d+)/$\", views.user),\n    re_path(r\"^articles/(?P<year>[0-9]{4})/(?P<month>[0-9]{2})/?$\", views.month),\n    re_path(r\"^feed\\.xml$\", views.feed),\n    re_path(r\"^(rss|atom)/$\", views.feeds),\n    path(\"api/v1/\", include((\"api.urls\", \"api\"), namespace=\"v1\")),\n    path(\"api/\", include(router.urls)),\n]\n",
    );
    let routes = facts(&result, "django.url_pattern.v1");
    let shapes: Vec<(Option<&str>, Option<&str>)> = routes
        .iter()
        .map(|fact| {
            (
                meta_str(fact, "route_template"),
                meta_str(fact, "normalized_route_template"),
            )
        })
        .collect();
    assert_eq!(
        shapes,
        vec![
            (Some("^blog/(page-(\\d+)/)?$"), None),
            (Some("^users/(\\d+)/$"), Some("/users/:arg1/")),
            (
                Some("^articles/(?P<year>[0-9]{4})/(?P<month>[0-9]{2})/?$"),
                Some("/articles/:year/:month/")
            ),
            (Some("^feed\\.xml$"), Some("/feed.xml")),
            (Some("^(rss|atom)/$"), None),
        ]
    );
    let includes = facts(&result, "django.url_include.v1");
    assert_eq!(meta_str(includes[0], "included_module"), Some("api.urls"));
    assert_eq!(meta_str(includes[0], "namespace"), Some("v1"));
    assert_eq!(
        meta_str(includes[1], "included_module"),
        Some("router.urls")
    );
}

#[test]
fn an_annotation_without_a_value_has_no_equals_sign() {
    let result = extract(
        "app.py",
        "class App:\n    default_config: dict[str, int]\n    name: str = \"app\"\n",
    );
    assert_eq!(
        one(&result, "default_config").signature.as_deref(),
        Some("default_config: dict[str, int]")
    );
    assert_eq!(
        one(&result, "name").signature.as_deref(),
        Some("name: str = \"app\"")
    );
}

#[test]
fn decorator_arguments_stay_in_the_signature_on_one_line() {
    let long_argument = "x".repeat(120);
    let result = extract(
        "blog.py",
        &format!(
            "@bp.route(\n    \"/create\",\n    methods=(\"GET\", \"POST\"),\n)\n@login_required\ndef create():\n    pass\n\n@pytest.mark.parametrize(\"v\", [\"{long_argument}\"])\ndef test_long(v):\n    pass\n"
        ),
    );
    assert_eq!(
        one(&result, "create").signature.as_deref(),
        Some("@bp.route( \"/create\", methods=(\"GET\", \"POST\"), ) @login_required def create()")
    );
    let long = one(&result, "test_long").signature.clone().unwrap();
    assert!(
        long.starts_with("@pytest.mark.parametrize(\"v\", [\"xxx")
            && long.contains("… def test_long(v)"),
        "{long}"
    );
}

#[test]
fn isinstance_and_issubclass_classes_are_type_usages() {
    let result = extract(
        "cli.py",
        "import flask\n\ndef check(app, cls):\n    return isinstance(app, Flask) or isinstance(app, (flask.Blueprint, Scaffold)) or issubclass(cls, Base) or isinstance(Flask, str)\n",
    );
    for name in ["Flask", "Blueprint", "Scaffold", "Base"] {
        let kinds: Vec<_> = idents(&result, name)
            .iter()
            .map(|i| i.kind.clone())
            .collect();
        assert!(
            kinds.contains(&IdentifierKind::TypeUsage),
            "{name}: {kinds:?}"
        );
    }
    let app_kinds: Vec<_> = idents(&result, "app")
        .iter()
        .map(|i| i.kind.clone())
        .collect();
    assert!(
        !app_kinds.contains(&IdentifierKind::TypeUsage),
        "{app_kinds:?}"
    );
}
