use crate::ExtractionResults;
use crate::base::{IdentifierKind, Symbol, SymbolKind, Visibility};
use crate::extract_canonical;
use std::path::Path;

fn extract(source: &str) -> ExtractionResults {
    extract_canonical("src/lib.rs", source, Path::new("/tmp/test"))
        .expect("canonical Rust extraction must succeed")
}

fn label(result: &ExtractionResults, id: Option<&str>) -> String {
    id.and_then(|id| result.symbols.iter().find(|symbol| symbol.id == id))
        .map(|symbol| format!("{}:{}@{}", symbol.name, symbol.kind, symbol.start_line))
        .unwrap_or_default()
}

fn find<'a>(result: &'a ExtractionResults, name: &str, line: u32) -> &'a Symbol {
    result
        .symbols
        .iter()
        .find(|symbol| symbol.name == name && symbol.start_line == line)
        .unwrap_or_else(|| {
            panic!(
                "{name}@{line} missing: {:#?}",
                result
                    .symbols
                    .iter()
                    .map(|s| format!("{}:{}@{}", s.name, s.kind, s.start_line))
                    .collect::<Vec<_>>()
            )
        })
}

fn parent(result: &ExtractionResults, name: &str, line: u32) -> String {
    label(result, find(result, name, line).parent_id.as_deref())
}

fn relationship_rows(result: &ExtractionResults) -> Vec<String> {
    result
        .relationships
        .iter()
        .map(|rel| {
            format!(
                "{} {}->{}",
                rel.kind,
                label(result, Some(&rel.from_symbol_id)),
                label(result, Some(&rel.to_symbol_id))
            )
        })
        .collect()
}

fn pending_rows(result: &ExtractionResults) -> Vec<String> {
    result
        .structured_pending_relationships
        .iter()
        .map(|pending| {
            format!(
                "{} {}->{} ns={:?}",
                pending.pending.kind,
                label(result, Some(&pending.pending.from_symbol_id)),
                pending.target.terminal_name,
                pending.target.namespace_path,
            )
        })
        .collect()
}

fn identifier_rows(result: &ExtractionResults) -> Vec<String> {
    result
        .identifiers
        .iter()
        .map(|identifier| {
            format!(
                "{} {} in={}",
                identifier.kind,
                identifier.name,
                label(result, identifier.containing_symbol_id.as_deref())
            )
        })
        .collect()
}

fn type_of(result: &ExtractionResults, name: &str, line: u32) -> Option<(String, bool)> {
    let symbol = find(result, name, line);
    result
        .types
        .get(&symbol.id)
        .map(|info| (info.resolved_type.clone(), info.is_inferred))
}

fn annotation_keys(result: &ExtractionResults, name: &str, line: u32) -> Vec<String> {
    find(result, name, line)
        .annotations
        .iter()
        .map(|marker| marker.annotation_key.clone())
        .collect()
}

fn facts(result: &ExtractionResults, pattern: &str) -> Vec<String> {
    result
        .structural_facts
        .iter()
        .filter(|fact| fact.pattern_id == pattern)
        .map(|fact| {
            let meta = fact.metadata.clone().unwrap_or_default();
            let get = |key: &str| {
                meta.get(key)
                    .map(|value| value.to_string().trim_matches('"').to_string())
                    .unwrap_or_default()
            };
            format!(
                "{} {} {} in={}",
                get("verb"),
                get("route_template"),
                get("mount_path"),
                label(result, fact.containing_symbol_id.as_deref())
            )
        })
        .collect()
}

fn assert_contains(rows: &[String], expected: &str) {
    assert!(
        rows.iter().any(|row| row == expected),
        "missing {expected:?} in {rows:#?}"
    );
}

#[test]
fn impl_associated_items_and_method_body_items_are_parented() {
    let result = extract(
        r#"pub struct S;
impl S {
    pub const MAX: u32 = 10;
    pub fn outer(&self) -> u32 {
        fn inner_helper(x: u32) -> u32 { x * 2 }
        struct Tmp { v: u32 }
        let t = Tmp { v: 1 };
        inner_helper(t.v)
    }
}
pub trait Shape { type Unit; }
impl Shape for S { type Unit = f64; }
pub mod a { pub struct Config; impl Config { pub fn load() -> Self { Config } } }
pub mod b { pub struct Config; impl Config { pub fn load() -> Self { Config } } }
"#,
    );

    assert_eq!(parent(&result, "MAX", 3), "S:struct@1");
    assert_eq!(parent(&result, "Unit", 12), "S:struct@1");
    assert_eq!(find(&result, "inner_helper", 5).kind, SymbolKind::Function);
    assert_eq!(parent(&result, "inner_helper", 5), "outer:method@4");
    assert!(find(&result, "inner_helper", 5).body_span.is_some());
    assert_eq!(parent(&result, "Tmp", 6), "outer:method@4");
    assert_eq!(parent(&result, "load", 14), "Config:struct@14");
    assert_eq!(parent(&result, "load", 13), "Config:struct@13");
    assert_contains(
        &relationship_rows(&result),
        "calls outer:method@4->inner_helper:function@5",
    );
    assert!(
        !pending_rows(&result)
            .iter()
            .any(|row| row.contains("inner_helper")),
        "{:#?}",
        pending_rows(&result)
    );
}

#[test]
fn doc_comments_follow_rustdoc_attachment() {
    let result = extract(
        r#"//! Crate docs for the parser module.

/// Token kinds.
pub enum Token { A }

/// Parses input.
// NOTE: keep in sync with the grammar
pub fn parse() {}

pub trait Store {
    /// Loads a record.
    /// Returns None when absent.
    fn load(&self) -> Option<u32>;
    /// The key type.
    type Key;
}

/**
 * Block docs.
 */
pub fn block() {}

extern "C" {
    /// Absolute value from libc.
    pub fn abs(x: i32) -> i32;
}
"#,
    );
    let doc = |name: &str, line: u32| find(&result, name, line).doc_comment.clone();

    assert_eq!(doc("Token", 4).as_deref(), Some("Token kinds."));
    assert_eq!(doc("parse", 8).as_deref(), Some("Parses input."));
    assert_eq!(
        doc("load", 13).as_deref(),
        Some("Loads a record.\nReturns None when absent.")
    );
    assert_eq!(doc("Key", 15).as_deref(), Some("The key type."));
    assert_eq!(doc("block", 21).as_deref(), Some("Block docs."));
    assert_eq!(doc("abs", 25).as_deref(), Some("Absolute value from libc."));
}

#[test]
fn inner_module_doc_never_attaches_to_the_first_item() {
    let result = extract("//! Crate docs.\n\npub fn first_item() {}\n");

    assert_eq!(find(&result, "first_item", 3).doc_comment, None);
}

#[test]
fn macro_invocations_emit_call_identifiers_and_call_edges() {
    let result = extract(
        r#"macro_rules! twice { ($e:expr) => { $e * 2 }; }
pub fn add(a: u32, b: u32) -> u32 { a + b }
pub fn app() -> u32 {
    println!("{}", 1);
    tracing::info!("phase {:?}", 2);
    twice!(add(1, 1))
}
"#,
    );
    let identifiers = identifier_rows(&result);

    assert_contains(&identifiers, "call twice in=app:function@3");
    assert_contains(&identifiers, "call println in=app:function@3");
    assert_contains(&identifiers, "call info in=app:function@3");
    assert!(
        !identifiers
            .iter()
            .any(|row| row.starts_with("type_usage info")),
        "{identifiers:#?}"
    );
    assert_contains(
        &relationship_rows(&result),
        "calls app:function@3->twice:function@1",
    );
    let pending = pending_rows(&result);
    assert_contains(&pending, "calls app:function@3->info ns=[\"tracing\"]");
    assert!(
        !pending.iter().any(|row| row.contains("->println")),
        "{pending:#?}"
    );
}

#[test]
fn restricted_visibility_maps_to_internal_and_members_inherit() {
    let result = extract(
        r#"pub(crate) struct CrateOnly;
pub(super) fn parent_only() {}
pub(in crate::a) fn in_path() {}
pub(crate) mod internal {}
use std::fmt;
pub use std::io::Read;
pub trait Api { fn required(&self); fn provided(&self) {} const K: u32; type T; }
pub struct Impl;
impl Api for Impl { fn required(&self) {} const K: u32 = 1; type T = u8; }
enum Private { A { x: u8 } }
"#,
    );
    let visibility = |name: &str, line: u32| find(&result, name, line).visibility.clone();

    assert_eq!(visibility("CrateOnly", 1), Some(Visibility::Internal));
    assert_eq!(visibility("parent_only", 2), Some(Visibility::Internal));
    assert_eq!(visibility("in_path", 3), Some(Visibility::Internal));
    assert_eq!(visibility("internal", 4), Some(Visibility::Internal));
    assert_eq!(visibility("fmt", 5), Some(Visibility::Private));
    assert_eq!(visibility("Read", 6), Some(Visibility::Public));
    for (name, line) in [("required", 7), ("provided", 7), ("K", 7), ("T", 7)] {
        assert_eq!(
            visibility(name, line),
            Some(Visibility::Public),
            "{name}@{line}"
        );
    }
    for (name, line) in [("required", 9), ("K", 9), ("T", 9)] {
        assert_eq!(
            visibility(name, line),
            Some(Visibility::Public),
            "{name}@{line}"
        );
    }
    assert_eq!(visibility("A", 10), Some(Visibility::Private));
    assert_eq!(visibility("x", 10), Some(Visibility::Private));
}

#[test]
fn trait_members_are_methods() {
    let result = extract("pub trait Api { fn required(&self); fn provided(&self) {} }\n");

    assert_eq!(find(&result, "required", 1).kind, SymbolKind::Method);
    assert_eq!(find(&result, "provided", 1).kind, SymbolKind::Method);
    assert_eq!(parent(&result, "required", 1), "Api:interface@1");
}

#[test]
fn return_and_const_types_are_declared_facts() {
    let result = extract(
        r#"pub struct User; pub struct Repo; pub struct DbError;
impl Repo {
    pub fn find(&self, id: u32) -> Result<User, DbError> { todo!() }
    pub fn maybe(&self) -> Option<User> { None }
    pub fn borrow_mut(&mut self) -> &mut User { todo!() }
    pub fn iter(&self) -> impl Iterator<Item = User> { std::iter::empty() }
    pub fn me() -> Self { Repo }
}
pub struct Config;
pub const DEFAULT: Config = Config;
pub static GLOBAL: Config = Config;
pub static mut MUTABLE: Config = Config;
"#,
    );
    let declared = |name: &str| Some((name.to_string(), false));

    assert_eq!(type_of(&result, "find", 3), declared("Result"));
    assert_eq!(type_of(&result, "maybe", 4), declared("Option"));
    assert_eq!(type_of(&result, "borrow_mut", 5), declared("User"));
    assert_eq!(type_of(&result, "iter", 6), declared("Iterator"));
    assert_eq!(type_of(&result, "me", 7), declared("Repo"));
    assert_eq!(type_of(&result, "DEFAULT", 10), declared("Config"));
    assert_eq!(type_of(&result, "GLOBAL", 11), declared("Config"));
    assert_eq!(type_of(&result, "MUTABLE", 12), declared("Config"));
}

#[test]
fn item_macros_define_their_items_instead_of_macro_named_symbols() {
    let result = extract(
        r#"lazy_static! {
    /// Global registry.
    pub static ref REGISTRY: Mutex<Vec<u32>> = Mutex::new(vec![]);
}
thread_local! { static CACHE: RefCell<u32> = RefCell::new(0); }
bitflags! { pub struct Flags: u32 { const A = 1; } }
cfg_if::cfg_if! { if #[cfg(unix)] { pub fn platform() -> &'static str { "unix" } } else { pub fn platform() -> &'static str { "other" } } }
pub fn use_them() { REGISTRY.lock().unwrap().push(1); }
#[cfg(test)]
mod tests { proptest! { #[test] fn prop_add(a in 0u32..10) { assert!(a < 10); } } }
"#,
    );
    let names: Vec<_> = result.symbols.iter().map(|s| s.name.as_str()).collect();
    for macro_name in [
        "lazy_static",
        "thread_local",
        "bitflags",
        "cfg_if",
        "proptest",
    ] {
        assert!(!names.contains(&macro_name), "{macro_name} in {names:#?}");
    }

    let registry = find(&result, "REGISTRY", 3);
    assert_eq!(registry.doc_comment.as_deref(), Some("Global registry."));
    assert_eq!(registry.visibility, Some(Visibility::Public));
    assert_eq!(find(&result, "CACHE", 5).kind, SymbolKind::Constant);
    assert_eq!(find(&result, "Flags", 6).kind, SymbolKind::Struct);
    assert_eq!(parent(&result, "A", 6), "Flags:struct@6");
    assert_eq!(
        result
            .symbols
            .iter()
            .filter(|s| s.name == "platform" && s.kind == SymbolKind::Function)
            .count(),
        2
    );
    let prop = find(&result, "prop_add", 10);
    assert_eq!(parent(&result, "prop_add", 10), "tests:namespace@10");
    assert_eq!(
        prop.metadata
            .as_ref()
            .and_then(|metadata| metadata.get("test_role"))
            .and_then(|role| role.as_str()),
        Some("test_case")
    );
    assert_contains(
        &identifier_rows(&result),
        "variable_ref REGISTRY in=use_them:function@8",
    );
}

#[test]
fn attributes_on_non_function_items_become_annotations() {
    let result = extract(
        r#"#[derive(Deserialize)]
pub struct Config {
    #[serde(rename = "db_url", default)]
    pub database_url: String,
}
#[derive(Serialize)]
pub enum Event { #[serde(rename = "created")] Created { id: u32 } }
#[async_trait]
pub trait Repo { async fn get(&self) -> u32; }
#[deprecated(note = "use v2")]
pub const OLD: u32 = 1;
#[derive(Clone, Copy)]
pub union U { a: u32 }
#[cfg(unix)]
pub type Alias = u32;
#[no_mangle]
pub static GLOBAL: u32 = 1;
"#,
    );

    assert_eq!(annotation_keys(&result, "database_url", 4), ["serde"]);
    assert_eq!(annotation_keys(&result, "Created", 7), ["serde"]);
    assert_eq!(annotation_keys(&result, "Repo", 9), ["async_trait"]);
    assert_eq!(annotation_keys(&result, "OLD", 11), ["deprecated"]);
    assert!(!annotation_keys(&result, "U", 13).is_empty());
    assert_eq!(annotation_keys(&result, "Alias", 15), ["cfg"]);
    assert_eq!(annotation_keys(&result, "GLOBAL", 17), ["no_mangle"]);
}

#[test]
fn rocket_attribute_routes_and_mounts_emit_facts() {
    let result = extract(
        r#"#[macro_use] extern crate rocket;
use rocket::serde::json::Json;
#[get("/hello/<name>/<age>")]
fn hello(name: &str, age: u8) -> String { format!("{} {}", name, age) }
#[post("/users", data = "<user>")]
fn create(user: Json<User>) -> Status { Status::Created }
#[launch]
fn rocket() -> _ { rocket::build().mount("/api", routes![hello, create]) }
"#,
    );
    let routes = facts(&result, "rocket.route.v1");

    assert_contains(&routes, "GET /hello/<name>/<age>  in=hello:function@4");
    assert_contains(&routes, "POST /users  in=create:function@6");
    assert_contains(
        &facts(&result, "rocket.mount.v1"),
        "  /api in=rocket:function@8",
    );
}

#[test]
fn axum_path_qualified_method_routers_emit_routes() {
    let result = extract(
        r#"use axum::{routing, routing::get, Router};
pub fn app() -> Router {
    Router::new()
        .route("/a", routing::get(a))
        .route("/b", axum::routing::post(b))
        .route("/c/{id}", get(show))
}
"#,
    );
    let routes = facts(&result, "axum.route.v1");

    assert_contains(&routes, "GET /a  in=app:function@2");
    assert_contains(&routes, "POST /b  in=app:function@2");
    assert_contains(&routes, "GET /c/{id}  in=app:function@2");
}

#[test]
fn match_arms_guards_and_let_else_count_as_decisions() {
    let result = extract(
        r#"pub fn code(n: i32) -> &'static str {
    match n { 0 => "zero", 1 => "one", 2 => "two", x if x < 0 => "neg", _ => "many" }
}
pub fn guarded(a: Option<u32>) -> Result<u32, String> {
    let Some(w) = a else { return Err("x".into()) };
    Ok(w)
}
"#,
    );
    let decisions = |name: &str, line: u32| {
        let id = &find(&result, name, line).id;
        result
            .complexity_metrics
            .iter()
            .find(|metric| metric.symbol_id.as_ref() == Some(id))
            .map(|metric| metric.decision_count)
    };

    assert_eq!(decisions("code", 1), Some(6));
    assert_eq!(decisions("guarded", 4), Some(1));
}

#[test]
fn lifetimes_placeholders_keywords_and_attribute_paths_are_not_identifiers() {
    let result = extract(
        r#"#[tokio::main]
async fn main() {}
pub async fn f(client: Client, items: Vec<u32>) -> Result<(), E> {
    let v = items.into_iter().collect::<Vec<_>>();
    let x = client.fetch().await.unwrap();
    matches!(x, Some(ref y) if y > 0);
    Ok(())
}
pub struct P<'a> { s: &'a str }
"#,
    );
    let identifiers = identifier_rows(&result);

    for garbage in [
        "type_usage main",
        "type_usage _",
        "variable_ref a ",
        "variable_ref ref ",
    ] {
        assert!(
            !identifiers.iter().any(|row| row.starts_with(garbage)),
            "{garbage:?} in {identifiers:#?}"
        );
    }
    assert!(
        !result
            .identifiers
            .iter()
            .any(|identifier| identifier.kind == IdentifierKind::TypeUsage
                && identifier.name == "tokio"),
        "{identifiers:#?}"
    );
}
