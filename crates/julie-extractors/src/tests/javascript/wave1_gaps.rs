use crate::base::{ExtractionResults, IdentifierKind, SymbolKind};
use crate::extract_canonical;
use std::path::Path;

fn extract(path: &str, source: &str) -> ExtractionResults {
    extract_canonical(path, source, Path::new("/tmp/test")).expect("extraction succeeds")
}

fn symbol_name(results: &ExtractionResults, id: &str) -> String {
    results
        .symbols
        .iter()
        .find(|symbol| symbol.id == id)
        .map(|symbol| symbol.name.clone())
        .unwrap_or_else(|| format!("<{id}>"))
}

fn pending_rows(results: &ExtractionResults) -> Vec<String> {
    results
        .structured_pending_relationships
        .iter()
        .map(|pending| {
            format!(
                "{:?} {} -> {} recv={} imp={}",
                pending.pending.kind,
                symbol_name(results, &pending.pending.from_symbol_id),
                pending.target.display_name,
                pending.target.receiver.as_deref().unwrap_or(""),
                pending.target.import_context.as_deref().unwrap_or(""),
            )
        })
        .collect()
}

fn relationship_rows(results: &ExtractionResults) -> Vec<String> {
    results
        .relationships
        .iter()
        .map(|relationship| {
            format!(
                "{:?} {} -> {}",
                relationship.kind,
                symbol_name(results, &relationship.from_symbol_id),
                symbol_name(results, &relationship.to_symbol_id),
            )
        })
        .collect()
}

fn symbols_named<'a>(results: &'a ExtractionResults, name: &str) -> Vec<&'a crate::base::Symbol> {
    results
        .symbols
        .iter()
        .filter(|symbol| symbol.name == name)
        .collect()
}

#[test]
fn function_valued_bindings_emit_one_callable_symbol() {
    let source = r#"const add = (a, b) => a + b;
const mul = function (a, b) { return a * b; };
function run() { return add(1, 2) + mul(2, 3); }
const api = { post: function (body) { return body; } };
Animal.prototype.speak = function (words) { return words; };
export const exported = () => 1;
"#;
    let results = extract("src/dup.js", source);
    for (name, kind) in [
        ("add", SymbolKind::Function),
        ("mul", SymbolKind::Function),
        ("post", SymbolKind::Method),
        ("speak", SymbolKind::Method),
        ("exported", SymbolKind::Function),
    ] {
        let callables: Vec<_> = results
            .symbols
            .iter()
            .filter(|symbol| {
                matches!(symbol.kind, SymbolKind::Function | SymbolKind::Method)
                    && (symbol.name == name || symbol.name.ends_with(&format!(".{name}")))
            })
            .collect();
        assert_eq!(callables.len(), 1, "{name}: {callables:#?}");
        assert_eq!(callables[0].kind, kind, "{name}");
    }
    let add = symbols_named(&results, "add")[0];
    let add_params: Vec<&str> = results
        .symbols
        .iter()
        .filter(|symbol| symbol.parent_id.as_deref() == Some(&add.id))
        .map(|symbol| symbol.name.as_str())
        .collect();
    assert_eq!(add_params, vec!["a", "b"]);
    for (method, parameter) in [("post", "body"), ("speak", "words")] {
        let method = symbols_named(&results, method)[0];
        assert!(
            results.symbols.iter().any(|symbol| symbol.name == parameter
                && symbol.parent_id.as_deref() == Some(&method.id)),
            "{parameter} parented to {}",
            method.name
        );
    }
    let add_metrics = results
        .complexity_metrics
        .iter()
        .filter(|metric| metric.symbol_id.as_deref() == Some(&add.id))
        .count();
    assert_eq!(add_metrics, 1);

    let relationships = relationship_rows(&results);
    for expected in ["Calls run -> add", "Calls run -> mul"] {
        assert!(
            relationships.iter().any(|row| row == expected),
            "{expected}: {relationships:#?}"
        );
    }
    let pending = pending_rows(&results);
    assert!(
        !pending.iter().any(|row| row.starts_with("Calls run")),
        "{pending:#?}"
    );
}

#[test]
fn calls_inside_function_expressions_and_generators_emit_pending_rows() {
    let source = r#"import { fetchPage } from "./api";
export function* paginate() { yield fetchPage(1); }
export const loader = function () { return fetchPage(2); };
function wrap() { (function iife() { fetchPage(3); })(); }
describe('cart', function () {
  it('function callback', function () { fetchPage(4); });
});
"#;
    let results = extract("test/pending_scope.test.js", source);
    let pending = pending_rows(&results);
    for caller in ["paginate", "loader", "iife", "function callback"] {
        let expected = format!("Calls {caller} -> fetchPage recv= imp=fetchPage");
        assert!(pending.contains(&expected), "{expected}: {pending:#?}");
    }
    assert!(
        !pending.iter().any(|row| row.starts_with("Calls wrap")),
        "{pending:#?}"
    );
}

#[test]
fn member_calls_on_imports_are_not_suppressed_by_same_named_locals() {
    let source = r#"import * as db from "./db";
import * as api from "./api";
async function getUser(id) {
  const user = await db.user(id);
  return user;
}
function other(id) {
  send({ fetchUser: true });
  return api.fetchUser(id);
}
"#;
    let results = extract("src/shadow.js", source);
    let pending = pending_rows(&results);
    for expected in [
        "Calls getUser -> db.user recv=db imp=db",
        "Calls other -> api.fetchUser recv=api imp=api",
    ] {
        assert!(
            pending.iter().any(|row| row == expected),
            "{expected}: {pending:#?}"
        );
    }
}

#[test]
fn bare_calls_do_not_bind_to_methods_or_shadowed_locals() {
    let source = r#"var resolve = require('node:path').resolve;
class Loader { resolve(name) { return name; } }
function load(root, name) { return resolve(root, name); }
function useHook(state) { const { toggle } = state; toggle(); }
class Panel { toggle() {} }
function helper() {}
function caller() { helper(); }
"#;
    let results = extract("src/bare.js", source);
    let relationships = relationship_rows(&results);
    assert!(
        !relationships
            .iter()
            .any(|row| row.contains("-> resolve") || row.contains("-> toggle")),
        "{relationships:#?}"
    );
    assert!(
        relationships
            .iter()
            .any(|row| row == "Calls caller -> helper"),
        "{relationships:#?}"
    );
    let pending = pending_rows(&results);
    assert!(
        !pending
            .iter()
            .any(|row| row.contains("-> resolve") || row.contains("-> toggle")),
        "{pending:#?}"
    );
}

#[test]
fn jsx_component_elements_are_call_sites() {
    let source = r#"import TodoItem from './TodoItem';
import * as UI from '../ui';
function EmptyState({ message }) { return <p>{message}</p>; }
export function List({ items }) {
  if (!items.length) return <EmptyState message="none" />;
  return (<UI.Panel>{items.map((i) => <TodoItem key={i.id} todo={i} />)}</UI.Panel>);
}
"#;
    let results = extract("src/List.jsx", source);
    let calls: Vec<String> = results
        .identifiers
        .iter()
        .filter(|identifier| identifier.kind == IdentifierKind::Call)
        .map(|identifier| identifier.name.clone())
        .collect();
    for name in ["EmptyState", "Panel", "TodoItem"] {
        assert!(calls.iter().any(|call| call == name), "{name}: {calls:?}");
    }
    assert!(!calls.iter().any(|call| call == "p"), "{calls:?}");
    let relationships = relationship_rows(&results);
    assert!(
        relationships
            .iter()
            .any(|row| row == "Calls List -> EmptyState"),
        "{relationships:#?}"
    );
    let pending = pending_rows(&results);
    for expected in [
        "Calls List -> TodoItem recv= imp=TodoItem",
        "Calls List -> UI.Panel recv=UI imp=UI",
    ] {
        assert!(
            pending.iter().any(|row| row == expected),
            "{expected}: {pending:#?}"
        );
    }
}

#[test]
fn destructured_requires_are_imports_and_destructured_bindings_are_symbols() {
    let source = r#"const { validate, audit: log } = require("./validate");
const Router = require("./router").Router;
function handle(req) { validate(req); log(req); return new Router(); }
const { data: user, status = 200, meta: { total }, ...rest } = response;
const [first, , [third], ...others] = list;
function Button({ label, onClick: handler }, [x]) { return label + handler + x; }
class Form { submit = async (event) => event; }
"#;
    let results = extract("src/cjs.js", source);
    for (name, imported, source_path) in [
        ("validate", "validate", "./validate"),
        ("log", "audit", "./validate"),
        ("Router", "Router", "./router"),
    ] {
        let symbol = symbols_named(&results, name)
            .into_iter()
            .find(|symbol| symbol.kind == SymbolKind::Import)
            .unwrap_or_else(|| panic!("{name} import missing"));
        let metadata = symbol.metadata.as_ref().unwrap();
        assert_eq!(
            metadata["importedName"],
            serde_json::json!(imported),
            "{name}"
        );
        assert_eq!(metadata["source"], serde_json::json!(source_path), "{name}");
        assert_eq!(metadata["isCommonJS"], serde_json::json!(true), "{name}");
    }
    let pending = pending_rows(&results);
    for expected in [
        "Calls handle -> validate recv= imp=validate",
        "Calls handle -> log recv= imp=log",
    ] {
        assert!(
            pending.iter().any(|row| row == expected),
            "{expected}: {pending:#?}"
        );
    }
    for name in [
        "user", "status", "total", "rest", "first", "third", "others",
    ] {
        assert!(
            symbols_named(&results, name)
                .iter()
                .any(|symbol| symbol.kind == SymbolKind::Variable),
            "{name} variable missing"
        );
    }
    for (callable, parameters) in [
        ("Button", vec!["label", "handler", "x"]),
        ("submit", vec!["event"]),
    ] {
        let owner = symbols_named(&results, callable)
            .into_iter()
            .find(|symbol| matches!(symbol.kind, SymbolKind::Function | SymbolKind::Method))
            .unwrap();
        let found: Vec<&str> = results
            .symbols
            .iter()
            .filter(|symbol| {
                symbol.parent_id.as_deref() == Some(&owner.id)
                    && symbol
                        .metadata
                        .as_ref()
                        .and_then(|metadata| metadata.get("role"))
                        == Some(&serde_json::json!("parameter"))
            })
            .map(|symbol| symbol.name.as_str())
            .collect();
        assert_eq!(found, parameters, "{callable}");
    }
}

#[test]
fn doc_comments_reach_exported_and_member_assigned_declarations() {
    let source = r#"/** Load a user. @returns {User} */
export function loadUser(id) { return id; }
/** Service class. */
export class UserService {}
/** @type {Config} */
export const config = {};
/** Proto method. */
Thing.prototype.legacy = function legacy(x) {};
/** Member assigned. */
exports.helper = function helper() {};
"#;
    let results = extract("src/docs.js", source);
    for (name, kind, doc) in [
        (
            "loadUser",
            SymbolKind::Function,
            "/** Load a user. @returns {User} */",
        ),
        ("UserService", SymbolKind::Class, "/** Service class. */"),
        ("config", SymbolKind::Variable, "/** @type {Config} */"),
        ("legacy", SymbolKind::Method, "/** Proto method. */"),
        ("helper", SymbolKind::Method, "/** Member assigned. */"),
    ] {
        let symbol = symbols_named(&results, name)
            .into_iter()
            .find(|symbol| symbol.kind == kind)
            .unwrap_or_else(|| panic!("{name} {kind:?} missing"));
        assert_eq!(symbol.doc_comment.as_deref(), Some(doc), "{name}");
    }
    for (name, resolved) in [("loadUser", "User"), ("config", "Config")] {
        let symbol = symbols_named(&results, name)
            .into_iter()
            .find(|symbol| symbol.kind != SymbolKind::Export)
            .unwrap();
        assert_eq!(
            results
                .types
                .get(&symbol.id)
                .map(|info| info.resolved_type.as_str()),
            Some(resolved),
            "{name}"
        );
    }
    let documented: Vec<(u32, String)> = results
        .source_regions
        .iter()
        .filter(|region| region.kind.as_str() == "doc_comment")
        .map(|region| {
            (
                region.start_line,
                symbol_name(
                    &results,
                    region.containing_symbol_id.as_deref().unwrap_or(""),
                ),
            )
        })
        .collect();
    let expected: Vec<(u32, String)> = [
        (1, "loadUser"),
        (3, "UserService"),
        (5, "config"),
        (7, "legacy"),
        (9, "helper"),
    ]
    .iter()
    .map(|(line, name)| (*line, name.to_string()))
    .collect();
    assert_eq!(documented, expected);
}

#[test]
fn required_helpers_named_like_hooks_are_not_lifecycle_symbols() {
    let source = r#"var after = require('after');
describe('counter', function () {
  after(function () {});
  it('waits for both', function (done) {
    var cb = after(2, done);
    cb(); cb();
  });
});
"#;
    let results = extract("test/counter.js", source);
    assert!(
        !results
            .symbols
            .iter()
            .any(|symbol| symbol.name == "after" && symbol.kind == SymbolKind::Function),
        "{:#?}",
        results.symbols
    );

    let hooks = r#"describe('app', function () {
  after(function () {});
  afterEach(cleanup);
  it('runs', function () { var cb = after(2, done); });
});
"#;
    let results = extract("test/app.js", hooks);
    let teardowns: Vec<u32> = results
        .symbols
        .iter()
        .filter(|symbol| {
            symbol
                .metadata
                .as_ref()
                .and_then(|metadata| metadata.get("test_role"))
                == Some(&serde_json::json!("fixture_teardown"))
        })
        .map(|symbol| symbol.start_line)
        .collect();
    assert_eq!(teardowns, vec![2, 3]);
}
