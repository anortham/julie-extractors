use crate::base::{RelationshipKind, Symbol, SymbolKind};
use crate::{ExtractionResults, extract_canonical};
use std::path::Path;

fn extract(path: &str, source: &str) -> ExtractionResults {
    extract_canonical(path, source, Path::new("/tmp/test")).expect("python extraction")
}

fn named<'a>(result: &'a ExtractionResults, name: &str) -> Vec<&'a Symbol> {
    result.symbols.iter().filter(|s| s.name == name).collect()
}

fn one<'a>(result: &'a ExtractionResults, name: &str) -> &'a Symbol {
    let found = named(result, name);
    assert_eq!(
        found.len(),
        1,
        "expected one `{name}` symbol, got {found:#?}"
    );
    found[0]
}

fn resolved_type(result: &ExtractionResults, name: &str, kind: SymbolKind) -> Option<String> {
    let symbol = result
        .symbols
        .iter()
        .find(|s| s.name == name && s.kind == kind)
        .unwrap_or_else(|| panic!("missing {name}"));
    result
        .types
        .get(&symbol.id)
        .map(|t| t.resolved_type.clone())
}

fn role(symbol: &Symbol) -> Option<&str> {
    symbol.metadata.as_ref()?.get("test_role")?.as_str()
}

#[test]
fn lambda_emits_one_symbol_parented_to_the_enclosing_callable() {
    let result = extract(
        "lam.py",
        "def score(item):\n    return item.weight\n\n\ndef order(items):\n    return sorted(items, key=lambda item: score(item))\n",
    );
    let lambdas: Vec<_> = result
        .symbols
        .iter()
        .filter(|s| s.name.starts_with("lambda_"))
        .collect();
    assert_eq!(lambdas.len(), 1, "{lambdas:#?}");
    assert_eq!(
        lambdas[0].parent_id.as_deref(),
        Some(one(&result, "order").id.as_str())
    );
    assert!(lambdas[0].body_span.is_some());
}

#[test]
fn nested_definitions_are_parented_to_the_enclosing_function() {
    let source = r#"
class View:
    @classmethod
    def as_view(cls, name):
        def view(**kwargs):
            return cls().dispatch(**kwargs)
        return view

def retry(times):
    def decorate(fn):
        def wrapper(*args, **kwargs):
            return fn(*args, **kwargs)
        return wrapper
    return decorate

def make_app():
    class Middleware:
        def handle(self):
            pass
    return Middleware
"#;
    let result = extract("nested.py", source);
    let view = one(&result, "view");
    assert_eq!(view.kind, SymbolKind::Function);
    assert_eq!(
        view.parent_id.as_deref(),
        Some(one(&result, "as_view").id.as_str())
    );
    assert_eq!(
        one(&result, "decorate").parent_id.as_deref(),
        Some(one(&result, "retry").id.as_str())
    );
    assert_eq!(
        one(&result, "wrapper").parent_id.as_deref(),
        Some(one(&result, "decorate").id.as_str())
    );
    let middleware = one(&result, "Middleware");
    assert_eq!(
        middleware.parent_id.as_deref(),
        Some(one(&result, "make_app").id.as_str())
    );
    let handle = one(&result, "handle");
    assert_eq!(handle.kind, SymbolKind::Method);
    assert_eq!(handle.parent_id.as_deref(), Some(middleware.id.as_str()));
}

#[test]
fn nested_test_function_is_not_a_test_case() {
    let source = "def test_routes():\n    def test(x):\n        return x\n    assert test(1)\n";
    let result = extract("tests/test_nested.py", source);
    assert_eq!(role(one(&result, "test_routes")), Some("test_case"));
    assert_eq!(role(one(&result, "test")), None);
}

#[test]
fn optional_union_annotated_and_forward_reference_annotations_resolve_to_the_named_type() {
    let source = r#"
from typing import Optional, Annotated, ClassVar
from sqlalchemy.orm import Mapped

def handler(repo: Optional[UserRepository], other: UserRepository | None,
            svc: Annotated[UserService, Depends()], fwd: "UserService",
            either: None | Cache, pair: int | str) -> Optional[User]:
    local: HttpClient | None = None
    repo.find(1)

class Repo:
    flag: ClassVar[bool] = True
    user: Mapped["User"] = relationship()

    @property
    def session(self) -> Session: ...
"#;
    let result = extract("typed.py", source);
    let var = SymbolKind::Variable;
    assert_eq!(
        resolved_type(&result, "repo", var.clone()).as_deref(),
        Some("UserRepository")
    );
    assert_eq!(
        resolved_type(&result, "other", var.clone()).as_deref(),
        Some("UserRepository")
    );
    assert_eq!(
        resolved_type(&result, "svc", var.clone()).as_deref(),
        Some("UserService")
    );
    assert_eq!(
        resolved_type(&result, "fwd", var.clone()).as_deref(),
        Some("UserService")
    );
    assert_eq!(
        resolved_type(&result, "either", var.clone()).as_deref(),
        Some("Cache")
    );
    assert_eq!(resolved_type(&result, "pair", var.clone()), None);
    assert_eq!(
        resolved_type(&result, "local", var.clone()).as_deref(),
        Some("HttpClient")
    );
    assert_eq!(
        resolved_type(&result, "flag", var.clone()).as_deref(),
        Some("bool")
    );
    assert_eq!(resolved_type(&result, "user", var).as_deref(), Some("User"));
    assert_eq!(
        resolved_type(&result, "handler", SymbolKind::Function).as_deref(),
        Some("User")
    );
    assert_eq!(
        resolved_type(&result, "session", SymbolKind::Property).as_deref(),
        Some("Session")
    );
    let repo = result.symbols.iter().find(|s| s.name == "repo").unwrap();
    let declared = result.types[&repo.id].metadata.as_ref().unwrap()["declared"].clone();
    assert_eq!(declared, "Optional[UserRepository]");
}

#[test]
fn unannotated_signatures_and_values_record_no_type_fact() {
    let source = r#"
class Config:
    def get(self, key: str):
        return self.data[key]

def load(path: str):
    return open(path).read()

def cached(cache: "Cache" = None):
    pass

settings = {"debug": env.get("DEBUG") == "1", "port": 8000}
is_admin = lambda user: user.role == "admin"
"#;
    let result = extract("rettype.py", source);
    for (name, kind) in [
        ("get", SymbolKind::Method),
        ("load", SymbolKind::Function),
        ("settings", SymbolKind::Variable),
        ("is_admin", SymbolKind::Variable),
    ] {
        assert_eq!(resolved_type(&result, name, kind), None, "{name}");
    }
    assert_eq!(
        resolved_type(&result, "cache", SymbolKind::Variable).as_deref(),
        Some("Cache")
    );
    assert!(
        result
            .types
            .values()
            .all(|t| !t.resolved_type.contains(['(', ')', '"'])),
        "{:#?}",
        result.types
    );
}

#[test]
fn class_attribute_keeps_one_member_row() {
    let source = r#"
class Counter:
    count: int = 0

    def __init__(self):
        self.count = 0
        self.items, self.total = [], 0
        self._lock = None

    def reset(self):
        self.count = 0
        self._lock = None
"#;
    let result = extract("attrs.py", source);
    let counter = one(&result, "Counter");
    let count = one(&result, "count");
    assert_eq!(count.start_line, 3);
    assert_eq!(count.parent_id.as_deref(), Some(counter.id.as_str()));
    let lock = one(&result, "_lock");
    assert_eq!(lock.kind, SymbolKind::Property);
    assert_eq!(lock.start_line, 8);
    for name in ["items", "total"] {
        let member = one(&result, name);
        assert_eq!(member.kind, SymbolKind::Property);
        assert_eq!(member.parent_id.as_deref(), Some(counter.id.as_str()));
    }
}

#[test]
fn import_rows_carry_structured_metadata_and_pending_calls_carry_import_context() {
    let source = r#"from app.models import Order as O
import app.services.billing as billing
import os.path
from ..core import helpers
from app import *


def make():
    order = O()
    billing.charge(order)
    return helpers.slugify("x")
"#;
    let result = extract("alias.py", source);
    let meta = |name: &str| one(&result, name).metadata.clone().unwrap();
    let o = meta("O");
    assert_eq!(o["source"], "app.models");
    assert_eq!(o["importedName"], "Order");
    assert_eq!(o["specifier"], "O");
    assert_eq!(o["isWildcard"], false);
    let os = meta("os");
    assert_eq!(os["source"], "os.path");
    assert!(named(&result, "os.path").is_empty());
    let helpers = meta("helpers");
    assert_eq!(helpers["source"], "..core");
    assert_eq!(helpers["relativeLevel"], 2);
    assert_eq!(meta("*")["isWildcard"], true);
    for (terminal, context) in [("O", "O"), ("charge", "billing"), ("slugify", "helpers")] {
        let pending = result
            .structured_pending_relationships
            .iter()
            .find(|p| p.target.terminal_name == terminal)
            .unwrap_or_else(|| panic!("missing pending {terminal}"));
        assert_eq!(pending.target.import_context.as_deref(), Some(context));
    }
}

#[test]
fn non_callable_declarations_have_no_body_span() {
    let source = r#"import os.path as osp
from .models import User, Order as O
T = TypeVar("T")

class C:
    def __init__(self, store: dict = dict()):
        self._store: dict[str, User] = {}

NOTES = '''Bob's notebook: @flask_app.route("/fake-in-string") must stay silent.'''
"#;
    let result = extract("decls.py", source);
    for name in ["osp", "User", "O", "T", "_store", "NOTES", "store"] {
        let symbol = one(&result, name);
        assert_eq!(symbol.body_span, None, "{name}");
        assert_eq!(symbol.body_hash, None, "{name}");
    }
    assert!(one(&result, "__init__").body_span.is_some());
}

#[test]
fn imported_and_dotted_bases_emit_pending_extends() {
    let source = r#"import unittest
from django.db import models
from .base import BaseService, Mixin as AuditMixin

class Local: ...
class ApiTests(unittest.TestCase): ...
class Article(models.Model): ...
class OrderService(AuditMixin, BaseService, Local, Generic[T]): ...
class Typed(Local[int]): ...
"#;
    let result = extract("extends.py", source);
    let local_edges: Vec<_> = result
        .relationships
        .iter()
        .filter(|r| r.kind == RelationshipKind::Extends)
        .map(|r| (r.from_symbol_id.as_str(), r.to_symbol_id.as_str()))
        .collect();
    let local = one(&result, "Local").id.as_str();
    assert_eq!(
        local_edges,
        vec![
            (one(&result, "OrderService").id.as_str(), local),
            (one(&result, "Typed").id.as_str(), local),
        ]
    );
    let pending: Vec<_> = result
        .structured_pending_relationships
        .iter()
        .filter(|p| p.pending.kind == RelationshipKind::Extends)
        .map(|p| {
            (
                p.target.display_name.as_str(),
                p.target.import_context.as_deref(),
            )
        })
        .collect();
    assert_eq!(
        pending,
        vec![
            ("unittest.TestCase", Some("unittest")),
            ("models.Model", Some("models")),
            ("AuditMixin", Some("AuditMixin")),
            ("BaseService", Some("BaseService")),
            ("Generic", None),
        ]
    );
}

#[test]
fn django_testcase_variants_get_test_roles_outside_test_paths() {
    let source = r#"
class UserApiTests(APITestCase):
    def test_list_users(self): ...

class AsyncTests(unittest.IsolatedAsyncioTestCase):
    async def asyncSetUp(self): ...
    async def test_value(self): ...
    def helper(self): ...

class QuestionModelTests(TestCase):
    @classmethod
    def setUpTestData(cls): ...
"#;
    let result = extract("polls/checks.py", source);
    for name in ["UserApiTests", "AsyncTests", "QuestionModelTests"] {
        assert_eq!(role(one(&result, name)), Some("test_container"), "{name}");
    }
    assert_eq!(role(one(&result, "test_list_users")), Some("test_case"));
    assert_eq!(role(one(&result, "test_value")), Some("test_case"));
    assert_eq!(role(one(&result, "asyncSetUp")), Some("fixture_setup"));
    assert_eq!(role(one(&result, "setUpTestData")), Some("fixture_setup"));
    assert_eq!(role(one(&result, "helper")), None);
}

#[test]
fn django_tests_module_and_pytest_asyncio_fixture_get_roles() {
    let tests = extract("api/tests.py", "def test_ping():\n    pass\n");
    assert_eq!(role(one(&tests, "test_ping")), Some("test_case"));
    let conftest = extract(
        "src/conftest_helpers.py",
        "import pytest_asyncio\n\n@pytest_asyncio.fixture\nasync def client():\n    yield 1\n",
    );
    assert_eq!(role(one(&conftest, "client")), Some("fixture_setup"));
}

#[test]
fn flask_routes_on_imported_receivers_module_import_and_add_url_rule() {
    let routes = extract(
        "app/main/routes.py",
        r#"from flask import render_template
from app.main import bp

@bp.route("/index", methods=["GET", "POST"])
def index():
    return render_template("index.html")

@bp.get("/user/<username>")
def user(username):
    return username
"#,
    );
    let facts: Vec<(String, String)> = routes
        .structural_facts
        .iter()
        .filter(|f| f.pattern_id == "flask.route.v1")
        .map(|f| {
            let m = f.metadata.as_ref().unwrap();
            (
                m["route_template"].as_str().unwrap().to_string(),
                m["verb"].as_str().unwrap().to_string(),
            )
        })
        .collect();
    assert_eq!(
        facts,
        vec![
            ("/index".to_string(), "GET".to_string()),
            ("/index".to_string(), "POST".to_string()),
            ("/user/<username>".to_string(), "GET".to_string()),
        ]
    );

    let module = extract(
        "modimport.py",
        r#"import flask
app = flask.Flask(__name__)

@app.route("/health")
def health():
    return "ok"

def ping():
    return "pong"

app.add_url_rule("/ping", view_func=ping, methods=["POST"])
"#,
    );
    let facts: Vec<_> = module
        .structural_facts
        .iter()
        .filter(|f| f.pattern_id == "flask.route.v1")
        .map(|f| f.metadata.clone().unwrap())
        .collect();
    assert_eq!(facts.len(), 2, "{facts:#?}");
    assert_eq!(facts[0]["route_template"], "/health");
    assert_eq!(facts[1]["route_template"], "/ping");
    assert_eq!(facts[1]["verb"], "POST");
    assert_eq!(facts[1]["api_style"], "call_routing");
    assert_eq!(facts[1]["view_target"], "ping");
}
