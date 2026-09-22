use crate::base::{
    ExtractionResults, IdentifierKind, RelationshipKind, Symbol, SymbolKind, Visibility,
};
use crate::extract_canonical;
use std::path::Path;

fn extract(path: &str, code: &str) -> ExtractionResults {
    extract_canonical(path, code, Path::new("/tmp/test")).expect("swift extraction")
}

fn symbol<'a>(result: &'a ExtractionResults, name: &str, kind: SymbolKind) -> &'a Symbol {
    result
        .symbols
        .iter()
        .find(|s| s.name == name && s.kind == kind)
        .unwrap_or_else(|| panic!("no {kind:?} {name}: {:#?}", names(result)))
}

fn names(result: &ExtractionResults) -> Vec<String> {
    result
        .symbols
        .iter()
        .map(|s| format!("{}:{:?}", s.name, s.kind))
        .collect()
}

fn name_of(result: &ExtractionResults, id: Option<&String>) -> String {
    id.and_then(|id| result.symbols.iter().find(|s| &s.id == id))
        .map(|s| s.name.clone())
        .unwrap_or_default()
}

fn calls_from(result: &ExtractionResults, caller: &str) -> Vec<String> {
    let mut out: Vec<String> = result
        .relationships
        .iter()
        .filter(|r| r.kind == RelationshipKind::Calls)
        .filter(|r| name_of(result, Some(&r.from_symbol_id)) == caller)
        .map(|r| name_of(result, Some(&r.to_symbol_id)))
        .collect();
    out.extend(
        result
            .structured_pending_relationships
            .iter()
            .filter(|p| p.pending.kind == RelationshipKind::Calls)
            .filter(|p| name_of(result, Some(&p.pending.from_symbol_id)) == caller)
            .map(|p| format!("pending:{}", p.target.terminal_name)),
    );
    out
}

fn pending_receivers(result: &ExtractionResults) -> Vec<(String, String)> {
    result
        .structured_pending_relationships
        .iter()
        .filter(|p| p.pending.kind == RelationshipKind::Calls)
        .map(|p| {
            (
                p.target.terminal_name.clone(),
                p.target.receiver.clone().unwrap_or_default(),
            )
        })
        .collect()
}

fn sorted(mut values: Vec<String>) -> Vec<String> {
    values.sort();
    values
}

fn visibility(result: &ExtractionResults, name: &str, kind: SymbolKind) -> Option<Visibility> {
    symbol(result, name, kind).visibility.clone()
}

fn test_role(symbol: &Symbol) -> String {
    symbol
        .metadata
        .as_ref()
        .and_then(|m| m.get("test_role"))
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string()
}

#[test]
fn call_targets_come_from_the_callee_node() {
    let code = "final class Loader {\n  func load() {}\n  func around(_ f: () -> Void) {}\n  func start() {\n    Task { await self.load() }\n    around { items.append(1) }\n    items.removeAll { $0 < 0 }\n    api.client().fetch(id: x)\n    let v = try await s.request(\"x\").validate().value\n    let b = Box<User>(value: 1)\n  }\n}\n";
    let result = extract("Sources/Loader.swift", code);
    let calls = sorted(calls_from(&result, "start"));
    for expected in [
        "around",
        "load",
        "pending:Task",
        "pending:append",
        "pending:removeAll",
        "pending:client",
        "pending:fetch",
        "pending:request",
        "pending:validate",
        "pending:Box",
    ] {
        assert!(
            calls.contains(&expected.to_string()),
            "{expected}: {calls:?}"
        );
    }
    let receivers = pending_receivers(&result);
    for expected in [
        ("removeAll", "items"),
        ("fetch", "api.client()"),
        ("request", "s"),
        ("validate", "s.request(\"x\")"),
    ] {
        assert!(
            receivers.contains(&(expected.0.to_string(), expected.1.to_string())),
            "{expected:?}: {receivers:?}"
        );
    }
    assert!(
        result
            .structured_pending_relationships
            .iter()
            .all(|p| !p.target.terminal_name.contains(['{', ' '])),
        "{receivers:?}"
    );
}

#[test]
fn a_call_on_a_call_result_never_resolves_to_the_caller() {
    let code = "final class Store {\n  func save(_ x: Int) {\n    builder().save(x)\n  }\n}\n";
    let result = extract("Sources/Store.swift", code);
    assert_eq!(
        sorted(calls_from(&result, "save")),
        vec!["pending:builder", "pending:save"]
    );
}

#[test]
fn subscripts_and_defer_are_not_calls() {
    let code = "final class Repo {\n  private var cache: [String: Int] = [:]\n  func load(id: String) {\n    if let cached = cache[id] { _ = cached }\n    defer { cleanup() }\n  }\n  func cleanup() {}\n}\n";
    let result = extract("Sources/Repo.swift", code);
    assert_eq!(calls_from(&result, "load"), vec!["cleanup"]);
    let calls: Vec<&str> = result
        .identifiers
        .iter()
        .filter(|i| i.kind == IdentifierKind::Call)
        .map(|i| i.name.as_str())
        .collect();
    assert_eq!(calls, vec!["cleanup"]);
    assert!(
        result
            .identifiers
            .iter()
            .any(|i| i.name == "cache" && i.kind == IdentifierKind::VariableRef)
    );
}

#[test]
fn accessor_observer_lazy_and_deinit_bodies_are_callers() {
    let code = "final class Session {\n  var token: String = \"\" { didSet { persist() } }\n  var isValid: Bool { validate() }\n  lazy var client: Client = { makeClient() }()\n  deinit { teardown() }\n  private func persist() {}\n  private func validate() -> Bool { true }\n  private func makeClient() -> Client { Client() }\n  private func teardown() {}\n}\n";
    let result = extract("Sources/Session.swift", code);
    assert_eq!(calls_from(&result, "token"), vec!["persist"]);
    assert_eq!(calls_from(&result, "isValid"), vec!["validate"]);
    assert_eq!(calls_from(&result, "client"), vec!["makeClient"]);
    assert_eq!(calls_from(&result, "deinit"), vec!["teardown"]);
    assert!(calls_from(&result, "Session").is_empty());
    let is_valid = symbol(&result, "isValid", SymbolKind::Property);
    assert!(
        result
            .complexity_metrics
            .iter()
            .any(|m| m.symbol_id.as_ref() == Some(&is_valid.id))
    );
    let stored = symbol(&result, "client", SymbolKind::Property);
    assert!(stored.body_span.is_some());
}

#[test]
fn conformances_emit_edges_for_protocols_and_extensions() {
    let code = "protocol Base {}\nprotocol Derived: Base { func go() }\nextension User: Derived { func go() {} }\nextension Order: PaymentConvertible, Auditable {}\npublic struct Invoice: PaymentConvertible {}\n@MainActor enum Mode: ModeProtocol { case a }\n";
    let result = extract("Sources/Rels.swift", code);
    let derived = symbol(&result, "Derived", SymbolKind::Interface);
    let base = symbol(&result, "Base", SymbolKind::Interface);
    assert!(
        result
            .relationships
            .iter()
            .any(|r| r.kind == RelationshipKind::Extends
                && r.from_symbol_id == derived.id
                && r.to_symbol_id == base.id)
    );
    let user = symbol(&result, "User", SymbolKind::Module);
    assert!(
        result
            .relationships
            .iter()
            .any(|r| r.kind == RelationshipKind::Implements
                && r.from_symbol_id == user.id
                && r.to_symbol_id == derived.id)
    );
    let pending: Vec<(String, RelationshipKind)> = result
        .structured_pending_relationships
        .iter()
        .filter(|p| p.pending.kind != RelationshipKind::Calls)
        .map(|p| {
            (
                format!(
                    "{}->{}",
                    name_of(&result, Some(&p.pending.from_symbol_id)),
                    p.target.terminal_name
                ),
                p.pending.kind.clone(),
            )
        })
        .collect();
    for expected in [
        "Order->PaymentConvertible",
        "Order->Auditable",
        "Invoice->PaymentConvertible",
        "Mode->ModeProtocol",
    ] {
        assert!(
            pending.contains(&(expected.to_string(), RelationshipKind::Implements)),
            "{expected}: {pending:?}"
        );
    }
}

#[test]
fn every_name_in_multi_name_declarations_is_a_symbol() {
    let code = "enum HTTPMethod: String { case get = \"GET\", post = \"POST\", put, delete }\nenum CodingKeys: String, CodingKey { case id, name, createdAt = \"created_at\" }\nstruct Frame {\n  var x, y: Double\n  let width = 10, height = 20\n  func split() { let (head, tail) = (1, 2); _ = (head, tail) }\n}\n";
    let result = extract("Sources/Enums.swift", code);
    for name in ["get", "post", "put", "delete", "id", "name", "createdAt"] {
        symbol(&result, name, SymbolKind::EnumMember);
    }
    assert_eq!(
        symbol(&result, "id", SymbolKind::EnumMember)
            .signature
            .as_deref(),
        Some("id")
    );
    assert_eq!(
        symbol(&result, "createdAt", SymbolKind::EnumMember)
            .signature
            .as_deref(),
        Some("createdAt = \"created_at\"")
    );
    for name in ["x", "y", "width", "height"] {
        symbol(&result, name, SymbolKind::Property);
    }
    for name in ["head", "tail"] {
        symbol(&result, name, SymbolKind::Variable);
    }
    assert!(result.symbols.iter().all(|s| !s.name.starts_with('(')));
}

#[test]
fn signatures_and_type_facts_keep_declared_return_types() {
    let code = "protocol Loader { func load(id: String) async throws -> Item? }\nstruct Item {}\nfinal class Repo {\n  func names() -> [String] { [] }\n  func find(_ id: String) -> Item? { nil }\n  func run() async throws -> Item { Item() }\n  subscript(i: Int) -> Item? { nil }\n}\nstruct Screen { var body: some Equatable { 1 }; var any: any Loader }\n";
    let result = extract("Sources/Ret.swift", code);
    let signature = |name: &str| {
        result
            .symbols
            .iter()
            .find(|s| s.name == name)
            .and_then(|s| s.signature.clone())
            .unwrap_or_default()
    };
    assert!(
        signature("names").ends_with("-> [String]"),
        "{}",
        signature("names")
    );
    assert!(
        signature("find").ends_with("-> Item?"),
        "{}",
        signature("find")
    );
    assert!(
        signature("run").contains("async throws -> Item"),
        "{}",
        signature("run")
    );
    assert!(
        signature("load").contains("async throws -> Item?"),
        "{}",
        signature("load")
    );
    let fact = |name: &str| {
        result
            .symbols
            .iter()
            .find(|s| s.name == name)
            .and_then(|s| result.types.get(&s.id))
            .map(|t| (t.resolved_type.clone(), t.is_inferred))
    };
    assert_eq!(fact("names"), None);
    assert_eq!(fact("find"), Some(("Item".to_string(), false)));
    assert_eq!(fact("load"), Some(("Item".to_string(), false)));
    assert_eq!(fact("subscript"), Some(("Item".to_string(), false)));
    assert_eq!(fact("body"), Some(("Equatable".to_string(), false)));
    assert_eq!(fact("any"), Some(("Loader".to_string(), false)));
}

#[test]
fn declaration_markers_are_never_type_facts() {
    let code = "import SwiftUI\nprotocol Loader {}\nstruct Svc {}\nextension Lock { }\nfinal class M { init() {} ; deinit {} ; var isLoading = false }\nfunc t() { let vm = Remote(api: 1) }\n";
    let result = extract("Sources/M.swift", code);
    for fact in result.types.values() {
        assert!(
            ![
                "class",
                "struct",
                "protocol",
                "import",
                "initializer",
                "extension",
                "deinitializer",
                "Any"
            ]
            .contains(&fact.resolved_type.as_str()),
            "{fact:?}"
        );
    }
}

#[test]
fn access_levels_follow_swift_defaults() {
    let code = "struct Plain {\n    func b() {}\n    private init() { }\n    fileprivate init(z: Int) { }\n}\nprivate extension Plain { func privExt() {} }\npublic extension Plain { func pubExt() {} }\nprivate protocol Hidden { func req() }\nprivate enum Mode { case on }\nfunc f() { let item = Plain() }\n";
    let result = extract("Sources/Vis.swift", code);
    assert_eq!(
        visibility(&result, "Plain", SymbolKind::Struct),
        Some(Visibility::Internal)
    );
    assert_eq!(
        visibility(&result, "b", SymbolKind::Method),
        Some(Visibility::Internal)
    );
    let inits: Vec<Option<Visibility>> = result
        .symbols
        .iter()
        .filter(|s| s.kind == SymbolKind::Constructor)
        .map(|s| s.visibility.clone())
        .collect();
    assert_eq!(
        inits,
        vec![Some(Visibility::Private), Some(Visibility::FilePrivate)]
    );
    assert_eq!(
        visibility(&result, "privExt", SymbolKind::Method),
        Some(Visibility::FilePrivate)
    );
    assert_eq!(
        visibility(&result, "pubExt", SymbolKind::Method),
        Some(Visibility::Public)
    );
    assert_eq!(
        visibility(&result, "req", SymbolKind::Method),
        Some(Visibility::Private)
    );
    assert_eq!(
        visibility(&result, "on", SymbolKind::EnumMember),
        Some(Visibility::Private)
    );
    assert_eq!(visibility(&result, "item", SymbolKind::Variable), None);
}

#[test]
fn xctest_subclasses_of_project_base_cases_get_test_roles() {
    let code = "import XCTest\nclass SessionBaseTestCase: XCTestCase { func makeSession() -> Session { Session() } }\nfinal class SessionTestCase: SessionBaseTestCase {\n    override func setUp() { super.setUp() }\n    func testDefaultSessionIsCreated() { XCTAssertNotNil(makeSession()) }\n}\nfinal class RequestTestCase: BaseTestCase {\n    func testRequestIsBuilt() { XCTAssertTrue(true) }\n    func testHelper(_ x: Int) {}\n}\n";
    let result = extract("Tests/SessionTests.swift", code);
    let role = |name: &str| {
        test_role(
            result
                .symbols
                .iter()
                .find(|s| s.name == name)
                .unwrap_or_else(|| panic!("{name}")),
        )
    };
    assert_eq!(role("SessionTestCase"), "test_container");
    assert_eq!(role("RequestTestCase"), "test_container");
    assert_eq!(role("setUp"), "fixture_setup");
    assert_eq!(role("testDefaultSessionIsCreated"), "test_case");
    assert_eq!(role("testRequestIsBuilt"), "test_case");
    assert_eq!(role("testHelper"), "");
}
