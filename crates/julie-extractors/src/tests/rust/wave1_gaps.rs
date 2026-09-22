use crate::ExtractionResults;
use crate::base::{IdentifierKind, SymbolKind};
use crate::extract_canonical;
use std::path::Path;

fn extract(source: &str) -> ExtractionResults {
    extract_canonical("src/lib.rs", source, Path::new("/tmp/test"))
        .expect("canonical Rust extraction must succeed")
}

fn symbol_name(result: &ExtractionResults, id: &str) -> String {
    result
        .symbols
        .iter()
        .find(|symbol| symbol.id == id)
        .map(|symbol| format!("{}:{}", symbol.name, symbol.kind))
        .unwrap_or_default()
}

fn relationship_rows(result: &ExtractionResults) -> Vec<String> {
    result
        .relationships
        .iter()
        .map(|rel| {
            format!(
                "{} {}->{}",
                rel.kind,
                symbol_name(result, &rel.from_symbol_id),
                symbol_name(result, &rel.to_symbol_id)
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
                "{} {}->{} ns={:?} recv={:?}",
                pending.pending.kind,
                symbol_name(result, &pending.pending.from_symbol_id),
                pending.target.terminal_name,
                pending.target.namespace_path,
                pending.target.receiver
            )
        })
        .collect()
}

fn parent_of(result: &ExtractionResults, name: &str) -> Option<String> {
    let symbol = result
        .symbols
        .iter()
        .find(|symbol| symbol.name == name && symbol.kind == SymbolKind::Method)
        .unwrap_or_else(|| panic!("method {name} missing: {:#?}", names(result)));
    symbol
        .parent_id
        .as_deref()
        .map(|id| symbol_name(result, id))
}

fn names(result: &ExtractionResults) -> Vec<String> {
    result
        .symbols
        .iter()
        .map(|symbol| format!("{}:{}", symbol.name, symbol.kind))
        .collect()
}

fn assert_contains(rows: &[String], expected: &str) {
    assert!(
        rows.iter().any(|row| row == expected),
        "missing {expected:?} in {rows:#?}"
    );
}

#[test]
fn methods_in_generic_reference_and_dyn_impls_are_extracted() {
    let result = extract(
        r#"pub fn helper() -> u32 { 1 }
pub struct Wrap<T>(T);
impl<T> Wrap<T> {
    pub fn go(&self) -> u32 { helper() + external::thing() }
}
pub struct Parser<'a> { s: &'a str }
impl<'a> Parser<'a> { pub fn new(s: &'a str) -> Self { Parser { s } } }
impl Clone for Wrap<u8> { fn clone(&self) -> Self { todo!() } }
pub struct A;
impl Iterator for &A { type Item = u8; fn next(&mut self) -> Option<u8> { None } }
pub trait Greeter {}
impl dyn Greeter { pub fn boxed_hi(&self) {} }
pub struct Buf<const N: usize>;
impl<const N: usize> Buf<N> { pub fn len(&self) -> usize { N } }
"#,
    );

    assert_eq!(parent_of(&result, "go").as_deref(), Some("Wrap:struct"));
    assert_eq!(parent_of(&result, "clone").as_deref(), Some("Wrap:struct"));
    assert_eq!(parent_of(&result, "new").as_deref(), Some("Parser:struct"));
    assert_eq!(parent_of(&result, "next").as_deref(), Some("A:struct"));
    assert_eq!(
        parent_of(&result, "boxed_hi").as_deref(),
        Some("Greeter:interface")
    );
    assert_eq!(parent_of(&result, "len").as_deref(), Some("Buf:struct"));

    assert_contains(
        &relationship_rows(&result),
        "calls go:method->helper:function",
    );
    assert_contains(
        &pending_rows(&result),
        "calls go:method->thing ns=[\"external\"] recv=None",
    );
}

#[test]
fn calls_inside_macro_arguments_emit_edges_and_call_identifiers() {
    let result = extract(
        r#"pub fn add(a: u32, b: u32) -> u32 { a + b }
pub fn render(x: u32) -> String { x.to_string() }
pub fn app() -> u32 {
    let v = vec![add(1, 2)];
    println!("{}", render(v[0]));
    let s = format!("{}", crate::util::shout("x"));
    twice!(add(1, 1))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn ignored() { assert_eq!(add(1, 2), 3); }
    #[test] fn documented() { assert!(render(5).starts_with('5')); }
}
"#,
    );

    let rels = relationship_rows(&result);
    assert_contains(&rels, "calls app:function->add:function");
    assert_contains(&rels, "calls app:function->render:function");
    assert_contains(&rels, "calls ignored:function->add:function");
    assert_contains(&rels, "calls documented:function->render:function");
    let pending = pending_rows(&result);
    assert_contains(
        &pending,
        "calls app:function->shout ns=[\"crate\", \"util\"] recv=None",
    );
    assert_contains(
        &pending,
        "calls documented:function->starts_with ns=[] recv=Some(\"render(5)\")",
    );

    for name in ["add", "render", "shout", "starts_with"] {
        assert!(
            result
                .identifiers
                .iter()
                .any(|ident| ident.name == name && ident.kind == IdentifierKind::Call),
            "{name} inside a macro argument must be a call identifier"
        );
        assert!(
            !result
                .identifiers
                .iter()
                .any(|ident| ident.name == name && ident.kind == IdentifierKind::VariableRef),
            "{name} inside a macro argument must not be a variable_ref"
        );
    }
}

#[test]
fn declarations_without_a_block_have_no_body_span() {
    let result = extract(
        r#"use axum::extract::{Path, State};
pub(crate) struct I { pub(crate) size: u32 }
pub trait Storage { fn load(&self, key: &str) -> Result<Option<Record>, Self::Error>; }
extern "C" { pub fn native_add(a: i32, b: i32) -> i32; }
pub static GLOBAL: Mutex<()> = Mutex::new(());
pub const LIMIT: u32 = (1 + 2);
pub type Callback = fn(u32);
pub fn f() { let expanded = foo(&path_str).to_string(); }
macro_rules! square { ($x:expr) => { $x * $x }; }
"#,
    );

    for name in [
        "Path",
        "State",
        "size",
        "load",
        "native_add",
        "GLOBAL",
        "LIMIT",
        "Callback",
        "expanded",
    ] {
        let symbol = result
            .symbols
            .iter()
            .find(|symbol| symbol.name == name)
            .unwrap_or_else(|| panic!("{name} missing: {:#?}", names(&result)));
        assert!(
            symbol.body_span.is_none() && symbol.body_hash.is_none(),
            "{name} has no block body but got {:?}",
            symbol.body_span
        );
    }
    for name in ["I", "Storage", "f", "square"] {
        let symbol = result
            .symbols
            .iter()
            .find(|symbol| symbol.name == name)
            .unwrap_or_else(|| panic!("{name} missing"));
        assert!(symbol.body_hash.is_some(), "{name} keeps its body");
    }
}

#[test]
fn trait_impls_and_supertraits_emit_implements_and_extends_edges() {
    let result = extract(
        r#"pub trait Base { fn id(&self) -> u32; }
pub trait Named: Base + Send { fn name(&self) -> String; }
pub trait Generic<T>: Base where T: Clone {}
pub struct User { pub id: u32 }
impl Base for User { fn id(&self) -> u32 { self.id } }
impl std::fmt::Display for User { fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result { Ok(()) } }
impl From<u32> for User { fn from(id: u32) -> Self { User { id } } }
impl<T: Clone> Generic<T> for User {}
impl Default for User { fn default() -> Self { User { id: 0 } } }
"#,
    );

    let rels = relationship_rows(&result);
    assert_contains(&rels, "implements User:struct->Base:interface");
    assert_contains(&rels, "implements User:struct->Generic:interface");
    assert_contains(&rels, "extends Named:interface->Base:interface");
    assert_contains(&rels, "extends Generic:interface->Base:interface");
    let pending = pending_rows(&result);
    assert_contains(
        &pending,
        "implements User:struct->Display ns=[\"std\", \"fmt\"] recv=None",
    );
    assert_contains(&pending, "implements User:struct->From ns=[] recv=None");
    assert_contains(&pending, "implements User:struct->Default ns=[] recv=None");
    assert_contains(&pending, "extends Named:interface->Send ns=[] recv=None");
}

#[test]
fn grouped_use_trees_emit_one_import_per_bound_name() {
    let result = extract(
        r#"use crate::{models::User, services::{auth::AuthService, mail::Mailer as M}};
use std::io::{self, Read};
use serde::Deserialize as De;
pub(in crate::a) use crate::b::Other;
extern crate serde_json as json;
use std::collections::*;
"#,
    );

    let imports: Vec<String> = result
        .symbols
        .iter()
        .filter(|symbol| symbol.kind == SymbolKind::Import)
        .map(|symbol| symbol.name.clone())
        .collect();
    assert_eq!(
        imports,
        [
            "User",
            "AuthService",
            "M",
            "io",
            "Read",
            "De",
            "Other",
            "json",
            "std::collections"
        ]
    );
    let alias = result
        .symbols
        .iter()
        .find(|symbol| symbol.name == "M")
        .unwrap();
    let metadata = alias.metadata.as_ref().unwrap();
    assert_eq!(metadata["importedName"], "Mailer");
    assert_eq!(metadata["alias"], "M");

    let pending: Vec<String> = result
        .structured_pending_relationships
        .iter()
        .map(|pending| {
            format!(
                "{}<-{}",
                pending.target.display_name,
                symbol_name(&result, &pending.pending.from_symbol_id)
            )
        })
        .collect();
    assert_eq!(
        pending,
        [
            "crate::models::User<-User:import",
            "crate::services::auth::AuthService<-AuthService:import",
            "crate::services::mail::Mailer<-M:import",
            "std::io<-io:import",
            "std::io::Read<-Read:import",
            "serde::Deserialize<-De:import",
            "crate::b::Other<-Other:import",
            "serde_json<-json:import",
            "std::collections<-std::collections:import",
        ]
    );
}

#[test]
fn turbofish_calls_emit_edges_without_type_arguments_in_the_path() {
    let result = extract(
        r#"pub fn parse_one<T>(s: &str) -> T { todo!() }
pub fn load(s: &str, items: Vec<u32>) {
    let c = serde_json::from_str::<Config>(s).unwrap();
    let n = parse_one::<u32>(s);
    let v = items.into_iter().collect::<Vec<_>>();
    let m = std::mem::size_of::<u64>();
    let w = Vec::<u32>::with_capacity(3);
}
"#,
    );

    assert_contains(
        &relationship_rows(&result),
        "calls load:function->parse_one:function",
    );
    let pending = pending_rows(&result);
    assert_contains(
        &pending,
        "calls load:function->from_str ns=[\"serde_json\"] recv=None",
    );
    assert_contains(
        &pending,
        "calls load:function->collect ns=[] recv=Some(\"items.into_iter()\")",
    );
    assert_contains(
        &pending,
        "calls load:function->size_of ns=[\"std\", \"mem\"] recv=None",
    );
    assert_contains(
        &pending,
        "calls load:function->with_capacity ns=[\"Vec\"] recv=None",
    );
}
