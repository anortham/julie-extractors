use crate::base::{ExtractionResults, IdentifierKind, RelationshipKind, Symbol, SymbolKind};
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
        .unwrap_or_else(|| {
            let names: Vec<String> = result
                .symbols
                .iter()
                .map(|s| format!("{}:{:?}", s.name, s.kind))
                .collect();
            panic!("no {kind:?} {name}: {names:?}")
        })
}

fn parent_name(result: &ExtractionResults, symbol: &Symbol) -> String {
    symbol
        .parent_id
        .as_ref()
        .and_then(|id| result.symbols.iter().find(|s| &s.id == id))
        .map(|s| s.name.clone())
        .unwrap_or_default()
}

fn metadata_text<'a>(symbol: &'a Symbol, key: &str) -> Option<&'a str> {
    symbol.metadata.as_ref()?.get(key)?.as_str()
}

fn identifiers(result: &ExtractionResults, kind: IdentifierKind) -> Vec<&str> {
    result
        .identifiers
        .iter()
        .filter(|i| i.kind == kind)
        .map(|i| i.name.as_str())
        .collect()
}

fn pending_calls(result: &ExtractionResults) -> Vec<(String, String)> {
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

fn test_role(symbol: &Symbol) -> Option<&str> {
    metadata_text(symbol, "test_role")
}

#[test]
fn operator_functions_and_declarations_are_operator_symbols() {
    let code = "struct Money: Equatable {\n  let cents: Int\n  static func == (lhs: Money, rhs: Money) -> Bool { lhs.cents == rhs.cents }\n  static func + (lhs: Money, rhs: Money) -> Money { Money(cents: lhs.cents + rhs.cents) }\n  static prefix func - (m: Money) -> Money { Money(cents: -m.cents) }\n}\ninfix operator <>: AdditionPrecedence\nfunc <> (a: Money, b: Money) -> Money { a + b }\n";
    let result = extract("Sources/Money.swift", code);
    for name in ["==", "+", "-"] {
        let op = symbol(&result, name, SymbolKind::Operator);
        assert_eq!(parent_name(&result, op), "Money", "{name}");
        assert!(op.body_span.is_some(), "{name}");
    }
    assert_eq!(
        symbol(&result, "==", SymbolKind::Operator)
            .signature
            .as_deref(),
        Some("static func ==(lhs: Money, rhs: Money) -> Bool")
    );
    assert_eq!(
        symbol(&result, "-", SymbolKind::Operator)
            .signature
            .as_deref(),
        Some("static prefix func -(m: Money) -> Money")
    );
    let custom: Vec<&Symbol> = result
        .symbols
        .iter()
        .filter(|s| s.name == "<>" && s.kind == SymbolKind::Operator)
        .collect();
    let signatures: Vec<&str> = custom
        .iter()
        .filter_map(|s| s.signature.as_deref())
        .collect();
    assert!(
        signatures.contains(&"infix operator <>: AdditionPrecedence"),
        "{signatures:?}"
    );
    assert!(
        signatures.contains(&"func <>(a: Money, b: Money) -> Money"),
        "{signatures:?}"
    );
    let a = result
        .symbols
        .iter()
        .find(|s| s.name == "a")
        .expect("operator parameter");
    assert_eq!(parent_name(&result, a), "<>");
}

#[test]
fn macro_declarations_are_symbols_and_expansions_are_calls() {
    let code = "@freestanding(expression)\npublic macro stringify<T>(_ value: T) -> (T, String) = #externalMacro(module: \"M\", type: \"S\")\n/// Adds an init.\n@attached(member, names: named(init))\npublic macro AutoInit() = #externalMacro(module: \"M\", type: \"A\")\nfunc useMacro() { let (value, code) = #stringify(1 + 2) }\n";
    let result = extract("Sources/Macros.swift", code);
    let stringify = symbol(&result, "stringify", SymbolKind::Function);
    assert_eq!(metadata_text(stringify, "type"), Some("macro"));
    assert_eq!(
        stringify.signature.as_deref(),
        Some("@freestanding(expression) public macro stringify<T>(_ value: T) -> (T, String)")
    );
    let auto_init = symbol(&result, "AutoInit", SymbolKind::Function);
    assert_eq!(auto_init.doc_comment.as_deref(), Some("/// Adds an init."));
    assert!(identifiers(&result, IdentifierKind::Call).contains(&"stringify"));
    assert!(!identifiers(&result, IdentifierKind::VariableRef).contains(&"stringify"));
    assert!(
        result
            .relationships
            .iter()
            .any(|r| r.kind == RelationshipKind::Calls && r.to_symbol_id == stringify.id),
        "{:?}",
        pending_calls(&result)
    );
}

#[test]
fn failable_initializers_keep_their_marker() {
    let code = "final class Parser {\n  init?(json: [String: Any]) { return nil }\n  init!(raw: Int) {}\n}\n";
    let result = extract("Sources/Parser.swift", code);
    let signatures: Vec<&str> = result
        .symbols
        .iter()
        .filter(|s| s.kind == SymbolKind::Constructor)
        .filter_map(|s| s.signature.as_deref())
        .collect();
    assert_eq!(
        signatures,
        vec!["init?(json: [String: Any])", "init!(raw: Int)"]
    );
}

#[test]
fn type_signatures_take_where_clauses_only_from_their_own_constraints() {
    let code = "extension Array {\n  /// Returns elements where the predicate holds.\n  func matching(_ p: (Element) -> Bool) -> [Element] { filter(p) }\n}\nstruct Store { func load<T>(_ t: T.Type) -> T? where T: Decodable { nil } }\nfinal class Plain {\n  // skip rows where id is nil\n  let note = \"filter where x > 1\"\n  func run() {}\n}\nextension Box where Element: Equatable {}\n";
    let result = extract("Sources/Where.swift", code);
    assert_eq!(
        symbol(&result, "Array", SymbolKind::Module)
            .signature
            .as_deref(),
        Some("extension Array")
    );
    assert_eq!(
        symbol(&result, "Store", SymbolKind::Struct)
            .signature
            .as_deref(),
        Some("struct Store")
    );
    assert_eq!(
        symbol(&result, "Plain", SymbolKind::Class)
            .signature
            .as_deref(),
        Some("final class Plain")
    );
    assert_eq!(
        symbol(&result, "Box", SymbolKind::Module)
            .signature
            .as_deref(),
        Some("extension Box where Element: Equatable")
    );
    assert_eq!(
        symbol(&result, "load", SymbolKind::Method)
            .signature
            .as_deref(),
        Some("func load<T>(_ t: T.Type) -> T? where T: Decodable")
    );
}

#[test]
fn nested_functions_are_functions_not_methods() {
    let code = "func outer() -> Int {\n  func inner() -> Int { 1 }\n  return inner()\n}\nstruct S { func m() { func helper() {} } }\n";
    let result = extract("Sources/Outer.swift", code);
    let inner = symbol(&result, "inner", SymbolKind::Function);
    assert_eq!(parent_name(&result, inner), "outer");
    let helper = symbol(&result, "helper", SymbolKind::Function);
    assert_eq!(parent_name(&result, helper), "m");
    symbol(&result, "m", SymbolKind::Method);
}

#[test]
fn actor_signatures_use_the_actor_keyword() {
    let code = "protocol Base {}\nactor Worker: Base {}\n";
    let result = extract("Sources/Worker.swift", code);
    let worker = symbol(&result, "Worker", SymbolKind::Class);
    assert_eq!(worker.signature.as_deref(), Some("actor Worker: Base"));
    assert_eq!(metadata_text(worker, "type"), Some("actor"));
}

#[test]
fn implicit_member_and_self_calls_are_calls() {
    let code = "func go(session: Session) {\n  session.request(\"u\", interceptor: .retryPolicy(retryLimit: 3))\n  let v: Foo = .init(x: 1)\n  let s = Self.make()\n}\n";
    let result = extract("Sources/Go.swift", code);
    let calls = identifiers(&result, IdentifierKind::Call);
    for name in ["retryPolicy", "init", "make"] {
        assert!(calls.contains(&name), "{name}: {calls:?}");
    }
    let reads = identifiers(&result, IdentifierKind::VariableRef);
    assert!(!reads.contains(&"retryPolicy"), "{reads:?}");
    assert!(!reads.contains(&"init"), "{reads:?}");
    let pending = pending_calls(&result);
    for expected in [("retryPolicy", ""), ("init", "Foo"), ("make", "Self")] {
        assert!(
            pending.contains(&(expected.0.to_string(), expected.1.to_string())),
            "{expected:?}: {pending:?}"
        );
    }
}

#[test]
fn implicit_member_calls_never_resolve_to_free_functions() {
    let code = "func retryPolicy(retryLimit: Int) -> Int { retryLimit }\nfunc go(session: Session) { session.request(interceptor: .retryPolicy(retryLimit: 3)) }\n";
    let result = extract("Sources/Go.swift", code);
    assert!(
        !result
            .relationships
            .iter()
            .any(|r| r.kind == RelationshipKind::Calls),
        "{:?}",
        result.relationships
    );
}

#[test]
fn compiler_attributes_labels_and_dot_self_are_not_references() {
    let code = "@testable import App\n@main\nstruct Tool {\n  @available(iOS 17, *)\n  @discardableResult\n  static func main() -> Int { 0 }\n  @Published var name = \"\"\n  @MainActor func refresh() {}\n}\nenum Shape {\n  case circle(radius: Double)\n  case rect(width: Double, height: Double)\n}\nlet t = [User].self\n";
    let result = extract("Sources/Tool.swift", code);
    let types = identifiers(&result, IdentifierKind::TypeUsage);
    for noise in ["testable", "main", "available", "discardableResult"] {
        assert!(!types.contains(&noise), "{noise}: {types:?}");
    }
    for kept in ["Published", "MainActor", "User"] {
        assert!(types.contains(&kept), "{kept}: {types:?}");
    }
    let reads = identifiers(&result, IdentifierKind::VariableRef);
    for label in ["radius", "width", "height"] {
        assert!(!reads.contains(&label), "{label}: {reads:?}");
    }
    assert!(!identifiers(&result, IdentifierKind::MemberAccess).contains(&"self"));
}

#[test]
fn block_comments_are_regions_and_carry_markers() {
    let code = "/*\n License header block.\n */\n/**\n Computes a thing.\n */\nfunc compute(_ x: Int) -> Int {\n  /* TODO: handle overflow */\n  // FIXME: race\n  return x\n}\n";
    let result = extract("Sources/Compute.swift", code);
    let kinds: Vec<String> = result
        .source_regions
        .iter()
        .map(|r| format!("{}:{}", r.start_line, r.kind.as_str()))
        .collect();
    for expected in ["1:comment", "4:doc_comment", "8:comment", "9:comment"] {
        assert!(
            kinds.contains(&expected.to_string()),
            "{expected}: {kinds:?}"
        );
    }
    let markers: Vec<u32> = result
        .structural_facts
        .iter()
        .filter(|f| f.pattern_id == "code.marker.v1")
        .map(|f| f.start_line)
        .collect();
    assert_eq!(markers, vec![8, 9]);
    assert_eq!(
        symbol(&result, "compute", SymbolKind::Function).visibility,
        Some(crate::base::Visibility::Internal)
    );
}

#[test]
fn quick_spec_subclasses_are_test_containers() {
    let code = "import Quick\nimport Nimble\nfinal class CalculatorSpec: QuickSpec {\n  override class func spec() {\n    describe(\"Calculator\") { it(\"adds\") { expect(1 + 2).to(equal(3)) } }\n  }\n}\nfinal class AsyncCalculatorSpec: AsyncSpec {\n  override class func spec() { it(\"works\") { await expect(1).to(equal(1)) } }\n}\nfinal class Helper: NSObject {\n  override class func spec() {}\n}\n";
    let result = extract("Tests/CalculatorSpec.swift", code);
    for name in ["CalculatorSpec", "AsyncCalculatorSpec"] {
        assert_eq!(
            test_role(symbol(&result, name, SymbolKind::Class)),
            Some("test_container"),
            "{name}"
        );
    }
    assert_eq!(
        test_role(symbol(&result, "Helper", SymbolKind::Class)),
        None
    );
    let works = result
        .symbols
        .iter()
        .find(|s| s.name == "works")
        .expect("works example");
    assert_eq!(test_role(works), Some("test_case"));
}

fn facts<'a>(
    result: &'a ExtractionResults,
    pattern_id: &str,
) -> Vec<&'a crate::base::StructuralFact> {
    result
        .structural_facts
        .iter()
        .filter(|f| f.pattern_id == pattern_id)
        .collect()
}

fn fact_text(fact: &crate::base::StructuralFact, key: &str) -> String {
    fact.metadata
        .as_ref()
        .and_then(|m| m.get(key))
        .map(|v| match v {
            serde_json::Value::String(s) => s.clone(),
            other => other.to_string(),
        })
        .unwrap_or_default()
}

#[test]
fn vapor_routes_join_same_file_group_prefixes() {
    let code = "import Vapor\nfunc routes(_ app: Application) throws {\n  app.get(\"hello\") { req async -> String in \"Hello\" }\n  let api = app.grouped(\"api\", \"v1\")\n  api.post(\"todos\", use: createTodo)\n  app.group(\"admin\") { admin in admin.delete(\"users\", \":id\", use: remove) }\n  app.grouped(\"x\").on(.PATCH, \"y\", use: y)\n  let name = \"n\"\n  app.get(\"u\", \"\\(name)\", use: skip)\n  cache.get(\"key\")\n}\nstruct TodoController: RouteCollection {\n  func boot(routes: RoutesBuilder) throws {\n    let todos = routes.grouped(\"todos\")\n    todos.get(use: index)\n    todos.delete(\":todoID\", use: delete)\n  }\n}\n";
    let result = extract("Sources/App/routes.swift", code);
    let routes: Vec<String> = facts(&result, "vapor.route.v1")
        .iter()
        .map(|f| {
            format!(
                "{} {} {}",
                fact_text(f, "verb"),
                fact_text(f, "normalized_route_template"),
                fact_text(f, "handler")
            )
        })
        .collect();
    assert_eq!(
        routes,
        vec![
            "GET /hello ",
            "POST /api/v1/todos createTodo",
            "DELETE /admin/users/:id remove",
            "PATCH /x/y y",
            "GET /todos index",
            "DELETE /todos/:todoID delete",
        ]
    );
    let without_import = extract(
        "Sources/App/other.swift",
        &code.replacen("import Vapor", "import Foundation", 1),
    );
    assert!(facts(&without_import, "vapor.route.v1").is_empty());
}

#[test]
fn alamofire_and_urlsession_requests_are_client_facts() {
    let code = "import Alamofire\nfunc fetchRemote() async throws {\n  AF.request(\"https://api.example.com/todos\", method: .post).responseDecodable(of: [Todo].self) { _ in }\n  AF.request(\"https://httpbin.org/get\").responseJSON { _ in }\n  let task = URLSession.shared.dataTask(with: URL(string: \"https://api.example.com/users\")!) { _, _, _ in }\n  let (data, _) = try await session.data(for: URLRequest(url: URL(string: \"https://api.example.com/orders\")!))\n  let (more, _) = try await URLSession.shared.data(from: URL(string: \"https://x.example.com/\\(id)\")!)\n  cache.data(from: URL(string: \"https://ignored.example.com\")!)\n}\n";
    let result = extract("Sources/Net.swift", code);
    let requests: Vec<String> = facts(&result, "http.client_request.v1")
        .iter()
        .map(|f| {
            format!(
                "{} {} {} {}",
                fact_text(f, "client"),
                fact_text(f, "verb"),
                fact_text(f, "verb_source"),
                fact_text(f, "target_path")
            )
        })
        .collect();
    assert_eq!(
        requests,
        vec![
            "alamofire POST attested https://api.example.com/todos",
            "alamofire GET default https://httpbin.org/get",
            "urlsession GET default https://api.example.com/users",
            "urlsession GET default https://api.example.com/orders",
        ]
    );
}

#[test]
fn swiftpm_manifests_publish_package_products_targets_and_dependencies() {
    let code = "// swift-tools-version:5.9\nimport PackageDescription\nlet package = Package(name: \"Alamofire\",\n  products: [.library(name: \"Alamofire\", targets: [\"Alamofire\"])],\n  dependencies: [.package(url: \"https://github.com/apple/swift-log.git\", from: \"1.5.0\")],\n  targets: [.target(name: \"Alamofire\", dependencies: [.product(name: \"Logging\", package: \"swift-log\")], path: \"Source\"),\n            .testTarget(name: \"AlamofireTests\", dependencies: [\"Alamofire\"], path: \"Tests\")])\n";
    let result = extract("Package.swift", code);
    let package = facts(&result, "swiftpm.package.v1");
    assert_eq!(package.len(), 1);
    assert_eq!(fact_text(package[0], "name"), "Alamofire");
    let products: Vec<String> = facts(&result, "swiftpm.product.v1")
        .iter()
        .map(|f| {
            format!(
                "{} {} {}",
                fact_text(f, "product_kind"),
                fact_text(f, "name"),
                fact_text(f, "targets")
            )
        })
        .collect();
    assert_eq!(products, vec!["library Alamofire [\"Alamofire\"]"]);
    let targets: Vec<String> = facts(&result, "swiftpm.target.v1")
        .iter()
        .map(|f| {
            format!(
                "{} {} {} {}",
                fact_text(f, "target_kind"),
                fact_text(f, "name"),
                fact_text(f, "path"),
                fact_text(f, "dependencies")
            )
        })
        .collect();
    assert_eq!(
        targets,
        vec![
            "target Alamofire Source [\"Logging\"]",
            "testTarget AlamofireTests Tests [\"Alamofire\"]",
        ]
    );
    let dependencies: Vec<String> = facts(&result, "manifest.dependency.v1")
        .iter()
        .map(|f| {
            format!(
                "{} {} {} {}",
                fact_text(f, "ecosystem"),
                fact_text(f, "name"),
                fact_text(f, "version"),
                fact_text(f, "location")
            )
        })
        .collect();
    assert_eq!(
        dependencies,
        vec!["swiftpm swift-log from: \"1.5.0\" https://github.com/apple/swift-log.git"]
    );
    let not_manifest = extract("Sources/Config.swift", code);
    assert!(facts(&not_manifest, "swiftpm.package.v1").is_empty());
}
