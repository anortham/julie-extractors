use crate::base::{ExtractionResults, Symbol, SymbolKind, Visibility};
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
        .unwrap_or_default()
}

fn find<'a>(results: &'a ExtractionResults, name: &str, kind: SymbolKind) -> &'a Symbol {
    results
        .symbols
        .iter()
        .find(|symbol| symbol.name == name && symbol.kind == kind)
        .unwrap_or_else(|| panic!("{name} {kind:?} in {:?}", symbol_rows(results)))
}

fn symbol_rows(results: &ExtractionResults) -> Vec<String> {
    results
        .symbols
        .iter()
        .map(|symbol| {
            format!(
                "{:?} {} parent={}",
                symbol.kind,
                symbol.name,
                symbol
                    .parent_id
                    .as_deref()
                    .map(|id| symbol_name(results, id))
                    .unwrap_or_default()
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
                "{} {:?} {}",
                symbol_name(results, &relationship.from_symbol_id),
                relationship.kind,
                symbol_name(results, &relationship.to_symbol_id)
            )
        })
        .collect()
}

fn pending_rows(results: &ExtractionResults) -> Vec<String> {
    results
        .structured_pending_relationships
        .iter()
        .map(|pending| {
            format!(
                "{} {:?} {}",
                symbol_name(results, &pending.pending.from_symbol_id),
                pending.pending.kind,
                pending.target.display_name,
            )
        })
        .collect()
}

fn resolved_type(results: &ExtractionResults, symbol: &Symbol) -> Option<String> {
    results
        .types
        .get(&symbol.id)
        .map(|info| info.resolved_type.clone())
}

#[test]
fn prototype_and_static_members_belong_to_the_constructor_function() {
    let source = r#"function Queue() { this.clear(); }
Queue.prototype.clear = function clear() { this.jobs = []; };
Queue.prototype.drain = function drain() { this.clear(); };
Queue.create = () => new Queue();
"#;
    let results = extract("queue.js", source);
    let clear = find(&results, "clear", SymbolKind::Method);
    assert_eq!(
        symbol_name(&results, clear.parent_id.as_deref().unwrap()),
        "Queue"
    );
    assert_eq!(
        clear.signature.as_deref(),
        Some("Queue.prototype.clear = function clear()")
    );
    let relationships = relationship_rows(&results);
    for expected in [
        "Queue Calls clear",
        "drain Calls clear",
        "create Instantiates Queue",
    ] {
        assert!(
            relationships.contains(&expected.to_string()),
            "{relationships:?}"
        );
    }
    assert!(
        pending_rows(&results).is_empty(),
        "{:?}",
        pending_rows(&results)
    );
    let this_call = results
        .identifiers
        .iter()
        .find(|identifier| identifier.name == "clear" && identifier.start_line == 3)
        .expect("this.clear() identifier");
    assert_eq!(this_call.receiver_type.as_deref(), Some("Queue"));
}

#[test]
fn commonjs_export_assignments_are_functions_named_by_the_export() {
    let source = r#"module.exports = function authMiddleware(req, res, next) { next(); };
exports.other = function () { return 1; };
module.exports.helper = (x) => x;
"#;
    let anonymous = extract(
        "plugin.js",
        "module.exports = function (app) { app.use(); };\n",
    );
    assert_eq!(
        find(&anonymous, "default", SymbolKind::Function).visibility,
        Some(Visibility::Public)
    );
    let results = extract("auth.js", source);
    let names: Vec<(SymbolKind, String)> = results
        .symbols
        .iter()
        .filter(|symbol| symbol.parent_id.is_none())
        .map(|symbol| (symbol.kind.clone(), symbol.name.clone()))
        .collect();
    assert_eq!(
        names,
        vec![
            (SymbolKind::Function, "authMiddleware".to_string()),
            (SymbolKind::Function, "other".to_string()),
            (SymbolKind::Function, "helper".to_string()),
        ]
    );
}

#[test]
fn jsdoc_and_constructor_assignments_give_type_facts() {
    let source = r#"/**
 * @param {UserRepo} repo
 * @returns {Promise<User>}
 */
async function loadUser(repo) { return repo.find(1); }
class Service {
  /** @type {Cache} */
  cache = null;
  repo = new Repo();
  constructor(store) { this.store = store; this.logger = new Logger(); }
}
/** @type {?Repo} */
let current = null;
"#;
    let results = extract("types.js", source);
    let repo_parameter = results
        .symbols
        .iter()
        .find(|symbol| symbol.name == "repo" && symbol.kind == SymbolKind::Variable)
        .unwrap();
    assert_eq!(
        resolved_type(&results, repo_parameter).as_deref(),
        Some("UserRepo")
    );
    assert_eq!(
        resolved_type(&results, find(&results, "loadUser", SymbolKind::Function)).as_deref(),
        Some("Promise")
    );
    assert_eq!(
        resolved_type(&results, find(&results, "cache", SymbolKind::Field)).as_deref(),
        Some("Cache")
    );
    assert_eq!(
        resolved_type(&results, find(&results, "repo", SymbolKind::Field)).as_deref(),
        Some("Repo")
    );
    let logger = find(&results, "logger", SymbolKind::Property);
    assert_eq!(
        symbol_name(&results, logger.parent_id.as_deref().unwrap()),
        "Service"
    );
    assert_eq!(resolved_type(&results, logger).as_deref(), Some("Logger"));
    assert_eq!(
        symbol_name(
            &results,
            find(&results, "store", SymbolKind::Property)
                .parent_id
                .as_deref()
                .unwrap()
        ),
        "Service"
    );
    assert_eq!(
        resolved_type(&results, find(&results, "current", SymbolKind::Variable)).as_deref(),
        Some("Repo")
    );
}

#[test]
fn module_visibility_follows_every_export_form() {
    let esm = extract(
        "esm.mjs",
        r#"import fs from "fs";
class Basket {}
export { Basket };
function hidden() { const local = 1; return local; }
export function shown() {}
export default Hidden;
class Hidden {}
"#,
    );
    let visibility = |results: &ExtractionResults, name: &str, kind: SymbolKind| {
        find(results, name, kind).visibility.clone()
    };
    assert_eq!(
        visibility(&esm, "Basket", SymbolKind::Class),
        Some(Visibility::Public)
    );
    assert_eq!(
        visibility(&esm, "Hidden", SymbolKind::Class),
        Some(Visibility::Public)
    );
    assert_eq!(
        visibility(&esm, "shown", SymbolKind::Function),
        Some(Visibility::Public)
    );
    assert_eq!(
        visibility(&esm, "hidden", SymbolKind::Function),
        Some(Visibility::Private)
    );
    assert_eq!(visibility(&esm, "local", SymbolKind::Variable), None);

    let cjs = extract(
        "cart.js",
        r#"class Cart {}
class Order {}
class Internal {}
module.exports = Cart;
module.exports.Order = Order;
"#,
    );
    assert_eq!(
        visibility(&cjs, "Cart", SymbolKind::Class),
        Some(Visibility::Public)
    );
    assert_eq!(
        visibility(&cjs, "Order", SymbolKind::Class),
        Some(Visibility::Public)
    );
    assert_eq!(
        visibility(&cjs, "Internal", SymbolKind::Class),
        Some(Visibility::Private)
    );

    let script = extract("global.js", "function globalFn() {}\nclass Widget {}\n");
    assert_eq!(
        visibility(&script, "globalFn", SymbolKind::Function),
        Some(Visibility::Public)
    );
    assert_eq!(
        visibility(&script, "Widget", SymbolKind::Class),
        Some(Visibility::Public)
    );
}

#[test]
fn export_statements_emit_one_row_per_exported_name() {
    let source = r#"function a() {} function b() {} const c = 1;
export { a, b as bee, c };
export * from "./all.js";
export * as ns from "./ns.js";
export { x, y as why } from "./xy.js";
export let m = 1, n = 2;
export function* gen() {}
export const { handlers, auth } = makeAuth();
export { default } from "./dflt.js";
"#;
    let results = extract("exports.js", source);
    let rows: Vec<String> = results
        .symbols
        .iter()
        .filter(|symbol| symbol.kind == SymbolKind::Export)
        .map(|symbol| {
            let source = symbol
                .metadata
                .as_ref()
                .and_then(|metadata| metadata.get("source"))
                .and_then(|value| value.as_str())
                .unwrap_or("-");
            format!("{} {source}", symbol.name)
        })
        .collect();
    assert_eq!(
        rows,
        vec![
            "a -",
            "bee -",
            "c -",
            "* ./all.js",
            "ns ./ns.js",
            "x ./xy.js",
            "why ./xy.js",
            "m -",
            "n -",
            "gen -",
            "handlers -",
            "auth -",
            "default ./dflt.js",
        ]
    );
    assert_eq!(find(&results, "gen", SymbolKind::Function).parent_id, None);
}

#[test]
fn object_literals_in_expressions_emit_no_property_symbols() {
    let source = r#"import * as api from "./api";
function load(id) {
  track("load", { fetchUser: true });
  return api.fetchUser(id);
}
export function Card({ title }) {
  return <div style={{ color: "red", padding: 4 }}>{title}</div>;
}
const config = { port: 3000, nested: { deep: 1 }, start() {}, stop: () => 0 };
register({ onReady() {} });
"#;
    let results = extract("card.jsx", source);
    let properties: Vec<String> = results
        .symbols
        .iter()
        .filter(|symbol| matches!(symbol.kind, SymbolKind::Property | SymbolKind::Method))
        .map(|symbol| symbol.name.clone())
        .collect();
    assert_eq!(
        properties,
        vec!["port", "nested", "deep", "start", "stop", "onReady"]
    );
    assert!(pending_rows(&results).contains(&"load Calls api.fetchUser".to_string()));
}

#[test]
fn class_expressions_are_classes_with_their_own_heritage() {
    let source = r#"class Local { ping() {} }
const Widget = class extends Local { render() { return this.ping(); } };
module.exports = class Repo extends Base { find() { return 1; } };
class Outer { make() { return class extends Local {}; } }
"#;
    let results = extract("classes.js", source);
    let widget = find(&results, "Widget", SymbolKind::Class);
    assert_eq!(widget.signature.as_deref(), Some("class extends Local"));
    let find_method = find(&results, "find", SymbolKind::Method);
    assert_eq!(
        symbol_name(&results, find_method.parent_id.as_deref().unwrap()),
        "Repo"
    );
    assert_eq!(relationship_rows(&results), vec!["Widget Extends Local"]);
    assert!(pending_rows(&results).contains(&"Repo Extends Base".to_string()));

    let anonymous = extract("anon.mjs", "export default class extends Base {}\n");
    let default_class = find(&anonymous, "default", SymbolKind::Class);
    assert_eq!(default_class.visibility, Some(Visibility::Public));
    assert_eq!(pending_rows(&anonymous), vec!["default Extends Base"]);
}

#[test]
fn class_field_decorators_are_annotations() {
    let source = r#"class Store {
  @observable items = [];
  @Input() name;
  @action.bound add(item) {}
}
"#;
    let results = extract("store.js", source);
    for (name, kind, key) in [
        ("items", SymbolKind::Field, "observable"),
        ("name", SymbolKind::Field, "input"),
        ("add", SymbolKind::Method, "action.bound"),
    ] {
        let symbol = find(&results, name, kind);
        assert_eq!(
            symbol
                .annotations
                .iter()
                .map(|annotation| annotation.annotation_key.as_str())
                .collect::<Vec<_>>(),
            vec![key]
        );
    }
}

#[test]
fn koa_and_hapi_routes_are_route_facts() {
    let source = r#"const Router = require('@koa/router');
const Hapi = require('@hapi/hapi');
const router = new Router({ prefix: '/api' });
router.get('/users/:id', async (ctx) => { ctx.body = {}; });
router.del('/users/:id', async (ctx) => {});
async function start() {
  const server = Hapi.server({ port: 3000 });
  server.route({ method: 'GET', path: '/items/{id}', handler: (r) => r.params.id });
  server.route([{ method: ['PUT', 'PATCH'], path: '/items', handler: () => 1 }]);
}
"#;
    let results = extract("server.js", source);
    let rows: Vec<String> = results
        .structural_facts
        .iter()
        .filter(|fact| matches!(fact.pattern_id.as_str(), "koa.route.v1" | "hapi.route.v1"))
        .map(|fact| {
            let metadata = fact.metadata.as_ref().unwrap();
            let text = |key: &str| {
                metadata
                    .get(key)
                    .and_then(|value| value.as_str())
                    .unwrap_or("-")
                    .to_string()
            };
            format!(
                "{} {} {}",
                fact.pattern_id,
                text("verb"),
                text("normalized_route_template")
            )
        })
        .collect();
    assert_eq!(
        rows,
        vec![
            "koa.route.v1 GET /api/users/:id",
            "koa.route.v1 DELETE /api/users/:id",
            "hapi.route.v1 GET /items/:id",
            "hapi.route.v1 PUT /items",
            "hapi.route.v1 PATCH /items",
        ]
    );
}

#[test]
fn garbage_callees_and_test_dsl_titles_are_never_targets() {
    let source = r#"import { describe, it, beforeEach, expect } from "vitest";
import { formatPrice } from "../src/format";
class A extends B { constructor() { super(); const lazy = require("./lazy"); } }
function wire(Component, handlers, name) {
  handlers[name]();
  (() => boot())();
  return connect(mapState)(Component);
}
describe("formatPrice", () => {
  beforeEach(() => {});
  it("formats cents", () => { expect(formatPrice(100)).toBe("$1.00"); });
});
"#;
    let results = extract("src/format.test.js", source);
    assert!(
        relationship_rows(&results).is_empty(),
        "{:?}",
        relationship_rows(&results)
    );
    assert_eq!(
        pending_rows(&results),
        vec![
            "A Extends B",
            "wire Calls boot",
            "wire Calls connect",
            "formats cents Calls formatPrice",
        ]
    );
}

#[test]
fn wrapped_components_own_their_render_calls() {
    let source = r#"import { forwardRef, memo } from "react";
import { formatPrice } from "./format";
function localFormat(v) { return v; }
export const Input = forwardRef((props, ref) => <input value={localFormat(props.value)} ref={ref} />);
export const Price = memo(({ value }) => <span>{formatPrice(value)}</span>);
"#;
    let results = extract("inputs.jsx", source);
    assert_eq!(relationship_rows(&results), vec!["Input Calls localFormat"]);
    assert_eq!(pending_rows(&results), vec!["Price Calls formatPrice"]);
}
