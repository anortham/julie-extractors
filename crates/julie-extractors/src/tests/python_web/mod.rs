use std::path::Path;

use crate::base::StructuralFact;
use crate::tests::helpers::{facts_with_pattern, metadata_str};

const FASTAPI_ROUTE_PATTERN_ID: &str = "fastapi.route.v1";
const FASTAPI_INCLUDE_ROUTER_PATTERN_ID: &str = "fastapi.include_router.v1";
const FLASK_ROUTE_PATTERN_ID: &str = "flask.route.v1";
const FLASK_BLUEPRINT_REGISTRATION_PATTERN_ID: &str = "flask.blueprint_registration.v1";
const DJANGO_URL_PATTERN_ID: &str = "django.url_pattern.v1";
const DJANGO_URL_INCLUDE_PATTERN_ID: &str = "django.url_include.v1";

fn extract(file_path: &str, source: &str) -> crate::ExtractionResults {
    crate::pipeline::extract_canonical(file_path, source, Path::new("/repo"))
        .expect("canonical extraction should succeed")
}

fn metadata_array<'a>(fact: &'a StructuralFact, key: &str) -> Vec<&'a str> {
    fact.metadata
        .as_ref()
        .and_then(|metadata| metadata.get(key))
        .and_then(|value| value.as_array())
        .map(|values| values.iter().filter_map(|value| value.as_str()).collect())
        .unwrap_or_default()
}

fn binding_symbol_name<'a>(
    results: &'a crate::ExtractionResults,
    fact: &StructuralFact,
) -> Option<&'a str> {
    let id = fact.containing_symbol_id.as_deref()?;
    results
        .symbols
        .iter()
        .find(|symbol| symbol.id == id)
        .map(|symbol| symbol.name.as_str())
}

#[test]
fn fastapi_decorators_prefixes_and_include_router_emit_boundary_facts() {
    let source = r#"
from fastapi import FastAPI, APIRouter

app = FastAPI()
router = APIRouter(prefix="/api")

@app.get("/health")
def health():
    pass

@router.get("/users/{user_id}")
def user(user_id: str):
    pass

@router.api_route("/items/{item_id}", methods=["GET", "POST"])
def item(item_id: str):
    pass

app.include_router(router, prefix="/v1")
"#;
    let results = extract("app/main.py", source);
    let routes = facts_with_pattern(&results, FASTAPI_ROUTE_PATTERN_ID);
    assert_eq!(routes.len(), 4, "{routes:#?}");

    let user = routes
        .iter()
        .find(|fact| metadata_str(fact, "route_template") == Some("/users/{user_id}"))
        .expect("router user route");
    assert_eq!(metadata_str(user, "framework"), Some("fastapi"));
    assert_eq!(metadata_str(user, "api_style"), Some("decorator_routing"));
    assert_eq!(metadata_str(user, "verb"), Some("GET"));
    assert_eq!(metadata_str(user, "verb_source"), Some("attested"));
    assert_eq!(metadata_str(user, "router_prefix"), Some("/api"));
    assert_eq!(
        metadata_str(user, "effective_route_template"),
        Some("/api/users/{user_id}")
    );
    assert_eq!(
        metadata_str(user, "normalized_route_template"),
        Some("/api/users/:user_id")
    );
    assert_eq!(metadata_array(user, "dynamic_segments"), vec!["user_id"]);
    assert_eq!(binding_symbol_name(&results, user), Some("user"));

    let item_verbs = routes
        .iter()
        .filter(|fact| metadata_str(fact, "route_template") == Some("/items/{item_id}"))
        .filter_map(|fact| metadata_str(fact, "verb"))
        .collect::<Vec<_>>();
    assert_eq!(item_verbs, vec!["GET", "POST"]);

    let includes = facts_with_pattern(&results, FASTAPI_INCLUDE_ROUTER_PATTERN_ID);
    assert_eq!(includes.len(), 1, "{includes:#?}");
    assert_eq!(metadata_str(includes[0], "mount_target"), Some("router"));
    assert_eq!(metadata_str(includes[0], "mount_path"), Some("/v1"));
    assert_eq!(
        metadata_str(includes[0], "normalized_mount_path"),
        Some("/v1")
    );
}

#[test]
fn fastapi_multiline_and_module_imports_emit_routes() {
    let multiline = r#"
from fastapi import (
    FastAPI,
    APIRouter,
)

app = FastAPI()
router = APIRouter(prefix="/api")

@router.get("/users/{user_id}")
def user(user_id: str):
    pass
"#;
    let multiline_results = extract("app/main.py", multiline);
    let multiline_routes = facts_with_pattern(&multiline_results, FASTAPI_ROUTE_PATTERN_ID);
    assert_eq!(multiline_routes.len(), 1, "{multiline_routes:#?}");
    assert_eq!(
        metadata_str(multiline_routes[0], "normalized_route_template"),
        Some("/api/users/:user_id")
    );

    let module_import = r#"
import fastapi

app = fastapi.FastAPI()

@app.get("/health")
def health():
    pass
"#;
    let module_results = extract("app/main.py", module_import);
    let module_routes = facts_with_pattern(&module_results, FASTAPI_ROUTE_PATTERN_ID);
    assert_eq!(module_routes.len(), 1, "{module_routes:#?}");
    assert_eq!(
        metadata_str(module_routes[0], "route_template"),
        Some("/health")
    );
}

#[test]
fn flask_routes_defaults_methods_and_blueprints_emit_boundary_facts() {
    let source = r#"
from flask import Flask, Blueprint

app = Flask(__name__)
bp = Blueprint("users", __name__, url_prefix="/api")

@app.route("/health")
def health():
    pass

@app.route("/submit", methods=["GET", "POST"])
def submit():
    pass

@bp.get("/users/<int:user_id>/")
def user(user_id):
    pass

app.register_blueprint(bp, url_prefix="/v1")
"#;
    let results = extract("app.py", source);
    let routes = facts_with_pattern(&results, FLASK_ROUTE_PATTERN_ID);
    assert_eq!(routes.len(), 4, "{routes:#?}");

    let health = routes
        .iter()
        .find(|fact| metadata_str(fact, "route_template") == Some("/health"))
        .expect("health route");
    assert_eq!(metadata_str(health, "verb"), Some("GET"));
    assert_eq!(metadata_str(health, "verb_source"), Some("default"));

    let submit_verbs = routes
        .iter()
        .filter(|fact| metadata_str(fact, "route_template") == Some("/submit"))
        .filter_map(|fact| metadata_str(fact, "verb"))
        .collect::<Vec<_>>();
    assert_eq!(submit_verbs, vec!["GET", "POST"]);

    let user = routes
        .iter()
        .find(|fact| metadata_str(fact, "route_template") == Some("/users/<int:user_id>/"))
        .expect("blueprint route");
    assert_eq!(metadata_str(user, "blueprint"), Some("users"));
    assert_eq!(metadata_str(user, "url_prefix"), Some("/api"));
    assert_eq!(
        metadata_str(user, "normalized_route_template"),
        Some("/api/users/:user_id/")
    );
    assert_eq!(metadata_array(user, "dynamic_segments"), vec!["user_id"]);
    assert_eq!(binding_symbol_name(&results, user), Some("user"));

    let registrations = facts_with_pattern(&results, FLASK_BLUEPRINT_REGISTRATION_PATTERN_ID);
    assert_eq!(registrations.len(), 1, "{registrations:#?}");
    assert_eq!(metadata_str(registrations[0], "mount_target"), Some("bp"));
    assert_eq!(metadata_str(registrations[0], "mount_path"), Some("/v1"));
}

#[test]
fn flask_keyword_spacing_and_methods_substring_keep_correct_verb_source() {
    let source = r#"
from flask import Flask, Blueprint

app = Flask(__name__)
bp = Blueprint("billing", __name__, url_prefix = "/api")

@app.route("/payment-methods")
def payment_methods():
    pass

@app.route("/submit", methods = ["POST"])
def submit():
    pass

@bp.get("/invoices")
def invoices():
    pass
"#;
    let results = extract("app.py", source);
    let routes = facts_with_pattern(&results, FLASK_ROUTE_PATTERN_ID);
    assert_eq!(routes.len(), 3, "{routes:#?}");

    let payment_methods = routes
        .iter()
        .find(|fact| metadata_str(fact, "route_template") == Some("/payment-methods"))
        .expect("payment-methods route");
    assert_eq!(metadata_str(payment_methods, "verb"), Some("GET"));
    assert_eq!(
        metadata_str(payment_methods, "verb_source"),
        Some("default")
    );

    let submit = routes
        .iter()
        .find(|fact| metadata_str(fact, "route_template") == Some("/submit"))
        .expect("submit route");
    assert_eq!(metadata_str(submit, "verb"), Some("POST"));
    assert_eq!(metadata_str(submit, "verb_source"), Some("attested"));

    let invoices = routes
        .iter()
        .find(|fact| metadata_str(fact, "route_template") == Some("/invoices"))
        .expect("blueprint route");
    assert_eq!(metadata_str(invoices, "url_prefix"), Some("/api"));
    assert_eq!(
        metadata_str(invoices, "normalized_route_template"),
        Some("/api/invoices")
    );
}

#[test]
fn flask_routes_survive_module_docstrings_with_apostrophes() {
    let source = r#"'''Routes for Bob's service.'''
from flask import Flask

app = Flask(__name__)

@app.route("/health")
def health():
    pass
"#;
    let results = extract("app.py", source);
    let routes = facts_with_pattern(&results, FLASK_ROUTE_PATTERN_ID);
    assert_eq!(routes.len(), 1, "{routes:#?}");
    assert_eq!(metadata_str(routes[0], "route_template"), Some("/health"));
}

#[test]
fn django_single_argument_path_calls_stay_silent() {
    let source = r#"
from django.urls import path

urlpatterns = [
    path("healthz"),
]
"#;
    let results = extract("project/urls.py", source);
    let routes = facts_with_pattern(&results, DJANGO_URL_PATTERN_ID);
    assert!(routes.is_empty(), "{routes:#?}");
}

#[test]
fn django_path_re_path_and_include_emit_boundary_facts() {
    let source = r#"
from django.urls import path, re_path, include
from . import views

urlpatterns = [
    path("users/<int:pk>/", views.detail, name="user-detail"),
    re_path(r"^legacy/(?P<slug>[-\\w]+)/$", views.legacy, name="legacy"),
    path("api/", include("app.urls"), namespace="api"),
]
"#;
    let results = extract("project/urls.py", source);
    let routes = facts_with_pattern(&results, DJANGO_URL_PATTERN_ID);
    assert_eq!(routes.len(), 2, "{routes:#?}");

    let path = routes
        .iter()
        .find(|fact| metadata_str(fact, "route_syntax") == Some("path"))
        .expect("path route");
    assert_eq!(
        metadata_str(path, "route_template"),
        Some("users/<int:pk>/")
    );
    assert_eq!(
        metadata_str(path, "normalized_route_template"),
        Some("/users/:pk/")
    );
    assert_eq!(metadata_array(path, "dynamic_segments"), vec!["pk"]);
    assert_eq!(metadata_str(path, "route_name"), Some("user-detail"));
    assert_eq!(metadata_str(path, "view_target"), Some("views.detail"));
    assert_eq!(metadata_str(path, "verb"), None);

    let regex = routes
        .iter()
        .find(|fact| metadata_str(fact, "route_syntax") == Some("regex"))
        .expect("regex route");
    assert_eq!(
        metadata_str(regex, "normalized_route_template"),
        Some("/legacy/:slug/")
    );
    assert_eq!(metadata_str(regex, "route_name"), Some("legacy"));

    let includes = facts_with_pattern(&results, DJANGO_URL_INCLUDE_PATTERN_ID);
    assert_eq!(includes.len(), 1, "{includes:#?}");
    assert_eq!(metadata_str(includes[0], "mount_path"), Some("api/"));
    assert_eq!(
        metadata_str(includes[0], "normalized_mount_path"),
        Some("/api/")
    );
    assert_eq!(
        metadata_str(includes[0], "included_module"),
        Some("app.urls")
    );
    assert_eq!(metadata_str(includes[0], "namespace"), Some("api"));
}
