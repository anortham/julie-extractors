use crate::base::{
    ExtractionResults, IdentifierKind, RelationshipKind, Symbol, SymbolKind, Visibility,
};
use crate::extract_canonical;
use std::path::Path;

fn extract(path: &str, code: &str) -> ExtractionResults {
    extract_canonical(path, code, Path::new("/tmp/test")).expect("dart extraction")
}

fn names(result: &ExtractionResults) -> Vec<String> {
    result
        .symbols
        .iter()
        .map(|s| format!("{}:{:?}", s.name, s.kind))
        .collect()
}

fn symbol<'a>(result: &'a ExtractionResults, name: &str, kind: SymbolKind) -> &'a Symbol {
    result
        .symbols
        .iter()
        .find(|s| s.name == name && s.kind == kind)
        .unwrap_or_else(|| panic!("no {kind:?} {name}: {:?}", names(result)))
}

fn name_of(result: &ExtractionResults, id: Option<&String>) -> String {
    id.and_then(|id| result.symbols.iter().find(|s| &s.id == id))
        .map(|s| s.name.clone())
        .unwrap_or_default()
}

fn parent_name(result: &ExtractionResults, symbol: &Symbol) -> String {
    name_of(result, symbol.parent_id.as_ref())
}

fn meta<'a>(symbol: &'a Symbol, key: &str) -> Option<&'a serde_json::Value> {
    symbol.metadata.as_ref()?.get(key)
}

fn edges(result: &ExtractionResults, kind: RelationshipKind) -> Vec<String> {
    let mut out: Vec<String> = result
        .relationships
        .iter()
        .filter(|r| r.kind == kind)
        .map(|r| {
            format!(
                "{}->{}",
                name_of(result, Some(&r.from_symbol_id)),
                name_of(result, Some(&r.to_symbol_id))
            )
        })
        .collect();
    out.extend(
        result
            .structured_pending_relationships
            .iter()
            .filter(|p| p.pending.kind == kind)
            .map(|p| {
                format!(
                    "{}->pending:{}",
                    name_of(result, Some(&p.pending.from_symbol_id)),
                    p.target.terminal_name
                )
            }),
    );
    out.sort();
    out
}

fn identifiers(result: &ExtractionResults, kind: IdentifierKind) -> Vec<String> {
    result
        .identifiers
        .iter()
        .filter(|i| i.kind == kind)
        .map(|i| format!("{}:{}", i.start_line, i.name))
        .collect()
}

fn pending_calls(result: &ExtractionResults) -> Vec<(String, String, String)> {
    result
        .structured_pending_relationships
        .iter()
        .filter(|p| p.pending.kind == RelationshipKind::Calls)
        .map(|p| {
            (
                p.target.terminal_name.clone(),
                p.target.receiver.clone().unwrap_or_default(),
                p.target.import_context.clone().unwrap_or_default(),
            )
        })
        .collect()
}

#[test]
fn extension_types_mixin_applications_and_unnamed_extensions_are_symbols() {
    let code = "extension type const UserId(int value) implements Object {\n  UserId.parse(String s) : this(int.parse(s));\n  bool get isValid => value > 0;\n}\nabstract class Base {}\nmixin Tracker {}\nclass Impl = Base with Tracker;\nextension on int { int get twice => this * 2; }\n";
    let result = extract("lib/ids.dart", code);
    let user_id = symbol(&result, "UserId", SymbolKind::Class);
    assert_eq!(
        meta(user_id, "isExtensionType"),
        Some(&serde_json::json!(true))
    );
    assert_eq!(
        meta(user_id, "representationType"),
        Some(&serde_json::json!("int"))
    );
    assert_eq!(
        user_id.signature.as_deref(),
        Some("extension type const UserId(int value) implements Object")
    );
    for (name, kind) in [
        ("UserId.parse", SymbolKind::Constructor),
        ("isValid", SymbolKind::Property),
        ("value", SymbolKind::Field),
    ] {
        assert_eq!(
            parent_name(&result, symbol(&result, name, kind)),
            "UserId",
            "{name}"
        );
    }
    let implements = edges(&result, RelationshipKind::Implements);
    assert_eq!(implements, vec!["UserId->pending:Object".to_string()]);

    let impl_class = symbol(&result, "Impl", SymbolKind::Class);
    assert_eq!(
        impl_class.signature.as_deref(),
        Some("class Impl = Base with Tracker")
    );
    assert!(edges(&result, RelationshipKind::Extends).contains(&"Impl->Base".to_string()));
    assert!(edges(&result, RelationshipKind::Uses).contains(&"Impl->Tracker".to_string()));

    let unnamed = symbol(&result, "<extension on int>", SymbolKind::Module);
    let twice = symbol(&result, "twice", SymbolKind::Property);
    assert_eq!(twice.parent_id.as_ref(), Some(&unnamed.id));
}

#[test]
fn operators_and_redirecting_factories_are_symbols() {
    let code = "class Point {\n  final int x;\n  Point(this.x);\n  Point operator +(Point other) => Point(add(x, other.x));\n  bool operator ==(Object other) => other is Point && other.x == x;\n  int operator [](int i) => lookup(i);\n  factory Point.redirect(int x) = Point;\n  const factory Point.frozen({required int x}) = _FrozenPoint;\n}\n";
    let result = extract("lib/point.dart", code);
    for name in ["+", "==", "[]"] {
        let op = symbol(&result, name, SymbolKind::Operator);
        assert_eq!(parent_name(&result, op), "Point", "{name}");
        assert!(op.body_span.is_some(), "{name}");
    }
    assert_eq!(
        symbol(&result, "+", SymbolKind::Operator)
            .signature
            .as_deref(),
        Some("Point operator +(Point other)")
    );
    let plus_calls: Vec<String> = result
        .structured_pending_relationships
        .iter()
        .filter(|p| name_of(&result, Some(&p.pending.from_symbol_id)) == "+")
        .map(|p| p.target.terminal_name.clone())
        .collect();
    assert!(plus_calls.contains(&"add".to_string()), "{plus_calls:?}");

    let redirect = symbol(&result, "Point.redirect", SymbolKind::Constructor);
    assert_eq!(
        redirect.signature.as_deref(),
        Some("factory Point.redirect(int x) = Point")
    );
    assert_eq!(meta(redirect, "isFactory"), Some(&serde_json::json!(true)));
    assert_eq!(
        meta(redirect, "redirectTarget"),
        Some(&serde_json::json!("Point"))
    );
    let frozen = symbol(&result, "Point.frozen", SymbolKind::Constructor);
    assert_eq!(meta(frozen, "isConst"), Some(&serde_json::json!(true)));
    assert_eq!(
        meta(frozen, "redirectTarget"),
        Some(&serde_json::json!("_FrozenPoint"))
    );
    let reads = identifiers(&result, IdentifierKind::VariableRef);
    assert!(
        !reads
            .iter()
            .any(|r| r.ends_with(":redirect") || r.ends_with(":frozen")),
        "{reads:?}"
    );
}

#[test]
fn mixin_and_enum_headers_publish_edges() {
    let code = "abstract class Base {}\nabstract class Api {}\nmixin Tracker on Base implements Api {}\nenum Mode with Tracker implements Api { a, b }\nenum Planet implements Comparable<Planet> { mercury, venus }\n";
    let result = extract("lib/modes.dart", code);
    let extends = edges(&result, RelationshipKind::Extends);
    assert!(
        extends.contains(&"Tracker->Base".to_string()),
        "{extends:?}"
    );
    let implements = edges(&result, RelationshipKind::Implements);
    for expected in ["Tracker->Api", "Mode->Api", "Planet->pending:Comparable"] {
        assert!(
            implements.contains(&expected.to_string()),
            "{expected}: {implements:?}"
        );
    }
    assert!(edges(&result, RelationshipKind::Uses).contains(&"Mode->Tracker".to_string()));
}

#[test]
fn underscore_names_are_private_for_every_declaration_kind() {
    let code = "class _Hidden { _Hidden(); _Hidden.named(); }\nenum _Kind { a }\nmixin _Mix {}\nextension _Ext on String {}\nclass Public { Public._internal(); factory Public._make() => Public._internal(); Public.open(); }\n";
    let result = extract("lib/hidden.dart", code);
    for (name, kind) in [
        ("_Hidden", SymbolKind::Class),
        ("_Hidden", SymbolKind::Constructor),
        ("_Hidden.named", SymbolKind::Constructor),
        ("_Kind", SymbolKind::Enum),
        ("_Mix", SymbolKind::Interface),
        ("_Ext", SymbolKind::Module),
        ("Public._internal", SymbolKind::Constructor),
        ("Public._make", SymbolKind::Constructor),
    ] {
        assert_eq!(
            symbol(&result, name, kind).visibility,
            Some(Visibility::Private),
            "{name}"
        );
    }
    for (name, kind) in [
        ("Public", SymbolKind::Class),
        ("Public.open", SymbolKind::Constructor),
    ] {
        assert_eq!(
            symbol(&result, name, kind).visibility,
            Some(Visibility::Public),
            "{name}"
        );
    }
}

#[test]
fn directives_publish_aliases_parts_and_library_names() {
    let code = "library app.models;\nimport 'package:http/http.dart' as http show get, post;\nimport 'src/heavy.dart' deferred as heavy;\nimport 'dart:convert' hide Codec;\npart 'models.g.dart';\nFuture<void> go() async { await heavy.loadLibrary(); http.get(Uri.parse('https://x.example.com')); }\n";
    let result = extract("lib/models.dart", code);
    let http = symbol(&result, "package:http/http.dart", SymbolKind::Import);
    assert_eq!(meta(http, "alias"), Some(&serde_json::json!("http")));
    assert_eq!(
        meta(http, "show"),
        Some(&serde_json::json!(["get", "post"]))
    );
    let heavy = symbol(&result, "src/heavy.dart", SymbolKind::Import);
    assert_eq!(meta(heavy, "deferred"), Some(&serde_json::json!(true)));
    assert_eq!(meta(heavy, "alias"), Some(&serde_json::json!("heavy")));
    let convert = symbol(&result, "dart:convert", SymbolKind::Import);
    assert_eq!(meta(convert, "hide"), Some(&serde_json::json!(["Codec"])));
    let part = symbol(&result, "models.g.dart", SymbolKind::Import);
    assert_eq!(meta(part, "type"), Some(&serde_json::json!("part")));
    symbol(&result, "app.models", SymbolKind::Namespace);
    let pending = pending_calls(&result);
    for expected in [
        ("loadLibrary", "heavy", "src/heavy.dart"),
        ("get", "http", "package:http/http.dart"),
    ] {
        assert!(
            pending.contains(&(
                expected.0.to_string(),
                expected.1.to_string(),
                expected.2.to_string()
            )),
            "{expected:?}: {pending:?}"
        );
    }

    let part_of = extract("lib/models.g.dart", "part of 'models.dart';\n");
    let owner = symbol(&part_of, "models.dart", SymbolKind::Import);
    assert_eq!(meta(owner, "type"), Some(&serde_json::json!("part_of")));
}

#[test]
fn cascades_and_generic_calls_keep_receivers_and_type_arguments() {
    let code = "void make(List<int> xs) {\n  final paint = Paint()..color = Colors.red..strokeWidth = 2;\n  xs..clear()..addAll([1]);\n  final buf = StringBuffer()..write('a');\n  getIt.registerSingleton<ApiClient>(ApiClient());\n  final list = List<int>.filled(3, 0);\n  helper<int>(3);\n}\n";
    let result = extract("lib/make.dart", code);
    let calls = identifiers(&result, IdentifierKind::Call);
    for expected in ["3:clear", "3:addAll", "4:write"] {
        assert!(
            calls.contains(&expected.to_string()),
            "{expected}: {calls:?}"
        );
    }
    let members = identifiers(&result, IdentifierKind::MemberAccess);
    for expected in ["2:color", "2:strokeWidth"] {
        assert!(
            members.contains(&expected.to_string()),
            "{expected}: {members:?}"
        );
    }
    let pending = pending_calls(&result);
    for expected in [
        ("clear", "xs"),
        ("addAll", "xs"),
        ("write", "StringBuffer()"),
        ("registerSingleton", "getIt"),
        ("filled", "List"),
    ] {
        assert!(
            pending
                .iter()
                .any(|(name, receiver, _)| name == expected.0 && receiver == expected.1),
            "{expected:?}: {pending:?}"
        );
    }
    let generic_uses: Vec<String> = result
        .type_argument_usages
        .iter()
        .map(|usage| {
            let name = result
                .identifiers
                .iter()
                .find(|i| i.id == usage.identifier_id)
                .map(|i| i.name.clone())
                .unwrap_or_default();
            let args: Vec<String> = usage
                .arguments
                .iter()
                .map(|a| a.type_name.clone())
                .collect();
            format!("{name}<{}>", args.join(","))
        })
        .collect();
    for expected in ["registerSingleton<ApiClient>", "List<int>", "helper<int>"] {
        assert!(
            generic_uses.contains(&expected.to_string()),
            "{expected}: {generic_uses:?}"
        );
    }
}

#[test]
fn async_and_static_flags_come_from_the_declaration_tokens() {
    let code = "class Loader {\n  void refresh() { final data = staticCache.read(); asyncValue.when(data: print); }\n  static Future<void> load() async { await x(); }\n  Stream<int> count() async* { yield 1; }\n}\nvoid schedule(Timer t) { t.onFire(() async { await work(); }); }\nIterable<int> gen() sync* { yield 2; }\n";
    let result = extract("lib/loader.dart", code);
    let refresh = symbol(&result, "refresh", SymbolKind::Method);
    assert_eq!(refresh.signature.as_deref(), Some("void refresh()"));
    assert_eq!(meta(refresh, "isAsync"), Some(&serde_json::json!(false)));
    assert_eq!(meta(refresh, "isStatic"), Some(&serde_json::json!(false)));
    let load = symbol(&result, "load", SymbolKind::Method);
    assert_eq!(meta(load, "isAsync"), Some(&serde_json::json!(true)));
    assert_eq!(meta(load, "isStatic"), Some(&serde_json::json!(true)));
    assert_eq!(
        meta(symbol(&result, "count", SymbolKind::Method), "isAsync"),
        Some(&serde_json::json!(true))
    );
    let schedule = symbol(&result, "schedule", SymbolKind::Function);
    assert_eq!(meta(schedule, "isAsync"), None);
    let modifiers: Vec<String> = result
        .structural_facts
        .iter()
        .filter(|f| f.pattern_id == "dart.async_modifier.v1")
        .map(|f| format!("{}:{}", f.start_line, f.node_kind))
        .collect();
    for expected in ["3:async", "4:async*", "7:sync*"] {
        assert!(
            modifiers.contains(&expected.to_string()),
            "{expected}: {modifiers:?}"
        );
    }
}

#[test]
fn declaration_names_are_not_references() {
    let code = "class Point {\n  static const origin = Point(0);\n  factory Point.fromJson(Map<String, dynamic> json) => Point(0);\n  const Point.constant(this.x);\n  Point(this.x);\n  final int x;\n}\nenum Planet { a(1); const Planet(this.mass); final int mass; }\nconst kPadding = 8.0;\n";
    let result = extract("lib/point.dart", code);
    let reads = identifiers(&result, IdentifierKind::VariableRef);
    for name in [
        "origin", "fromJson", "constant", "kPadding", "Planet", "Point",
    ] {
        assert!(
            !reads.iter().any(|r| r.ends_with(&format!(":{name}"))),
            "{name}: {reads:?}"
        );
    }
}

#[test]
fn signatures_keep_the_declared_header() {
    let code = "class Point {\n  final int x, y;\n  Point(this.x, this.y);\n  Point.origin() : this(0, 0);\n  factory Point.fromJson(Map<String, dynamic> json) => Point(0, 0);\n  set secret(int v) => print(v);\n}\ntypedef void LegacyCallback(String s);\n";
    let result = extract("lib/point.dart", code);
    let signatures: Vec<String> = result
        .symbols
        .iter()
        .filter(|s| {
            matches!(
                s.kind,
                SymbolKind::Constructor | SymbolKind::Property | SymbolKind::Type
            )
        })
        .map(|s| s.signature.clone().unwrap_or_default())
        .collect();
    for expected in [
        "Point(this.x, this.y)",
        "Point.origin()",
        "factory Point.fromJson(Map<String, dynamic> json)",
        "set secret(int v)",
        "typedef void LegacyCallback(String s)",
    ] {
        assert!(
            signatures.contains(&expected.to_string()),
            "{expected}: {signatures:?}"
        );
    }
    let legacy = symbol(&result, "LegacyCallback", SymbolKind::Type);
    assert_eq!(
        meta(legacy, "aliasedType"),
        Some(&serde_json::json!("void Function(String s)"))
    );
}

#[test]
fn pattern_bindings_are_locals() {
    let code = "(int, String) pair() => (1, 'a');\nvoid consume(Map<String, Object> json, List<(int, int)> pairs) {\n  final (count, label) = pair();\n  var (:x, :y) = (x: 1, y: 2);\n  if (json case {'name': String name, 'age': int age}) { print('$name $age'); }\n  for (final (a, b) in pairs) { print(a + b); }\n}\n";
    let result = extract("lib/consume.dart", code);
    for name in ["count", "label", "x", "y", "name", "age", "a", "b"] {
        let local = symbol(&result, name, SymbolKind::Variable);
        assert_eq!(parent_name(&result, local), "consume", "{name}");
    }
    let name_type = result
        .types
        .get(&symbol(&result, "name", SymbolKind::Variable).id)
        .map(|t| t.resolved_type.clone());
    assert_eq!(name_type.as_deref(), Some("String"));
    let reads = identifiers(&result, IdentifierKind::VariableRef);
    assert!(!reads.contains(&"3:count".to_string()), "{reads:?}");
}

#[test]
fn bloc_tests_are_cases_and_production_groups_are_not_containers() {
    let code = "import 'package:bloc_test/bloc_test.dart';\nvoid main() {\n  group('CounterCubit', () {\n    blocTest<CounterCubit, int>('emits [1] when increment is called', build: () => CounterCubit(), act: (c) => c.increment(), expect: () => [1]);\n    blocTest('emits nothing', build: () => CounterCubit(), expect: () => []);\n  });\n}\n";
    let result = extract("test/counter_cubit_test.dart", code);
    for name in ["emits [1] when increment is called", "emits nothing"] {
        let case = symbol(&result, name, SymbolKind::Function);
        assert_eq!(
            meta(case, "test_role"),
            Some(&serde_json::json!("test_case")),
            "{name}"
        );
        assert_eq!(parent_name(&result, case), "CounterCubit");
    }
    let production = extract(
        "lib/validator.dart",
        "void validate(Validator v) { group('admins', () => print('x')); }\n",
    );
    assert!(
        !production.symbols.iter().any(|s| s.name == "admins"),
        "{:?}",
        names(&production)
    );
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

fn fact_lines(result: &ExtractionResults, pattern_id: &str, keys: &[&str]) -> Vec<String> {
    facts(result, pattern_id)
        .iter()
        .map(|f| {
            keys.iter()
                .map(|key| fact_text(f, key))
                .collect::<Vec<_>>()
                .join(" ")
        })
        .collect()
}

const GO_ROUTER_APP: &str = "import 'package:go_router/go_router.dart';\nfinal router = GoRouter(routes: [\n  GoRoute(path: '/users/:id', name: 'user', builder: (c, s) => const UserPage(), routes: [\n    GoRoute(path: 'posts', builder: (c, s) => PostsPage()),\n  ]),\n  GoRoute(path: '/p/$slug', builder: (c, s) => Other()),\n]);\nvoid openUser(BuildContext context) {\n  context.go('/users/42');\n  context.pushNamed('user');\n  context.go(target);\n}\n";

#[test]
fn go_router_routes_join_nested_paths_and_name_their_builders() {
    let result = extract("lib/router.dart", GO_ROUTER_APP);
    assert_eq!(
        fact_lines(
            &result,
            "go_router.route_definition.v1",
            &[
                "effective_route_template",
                "normalized_route_template",
                "parent_route_path",
                "route_name",
                "route_component",
            ],
        ),
        vec![
            "/users/:id /users/:id  user UserPage",
            "/users/:id/posts /users/:id/posts /users/:id  PostsPage",
        ]
    );
    let without_import = extract(
        "lib/router.dart",
        &GO_ROUTER_APP.replacen(
            "package:go_router/go_router.dart",
            "package:other/other.dart",
            1,
        ),
    );
    assert!(facts(&without_import, "go_router.route_definition.v1").is_empty());
}

#[test]
fn go_router_navigation_calls_are_route_references() {
    let result = extract("lib/router.dart", GO_ROUTER_APP);
    assert_eq!(
        fact_lines(
            &result,
            "go_router.route_reference.v1",
            &[
                "navigation_method",
                "normalized_route_template",
                "route_name"
            ],
        ),
        vec!["go /users/42 ", "pushNamed  user"]
    );
}

#[test]
fn shelf_router_annotations_and_router_calls_are_routes() {
    let code = "import 'package:shelf_router/shelf_router.dart';\nclass Api {\n  @Route.get('/users/<id>')\n  Response fetch(Request r, String id) => Response.ok('x');\n  @Route('PATCH', '/users/<id>')\n  Response patch(Request r, String id) => Response.ok('x');\n}\nfinal app = Router()..get('/health', (Request req) => Response.ok('ok'));\nvoid f() {\n  app.post('/items', createItem);\n  cache.get('/not-a-route');\n}\n";
    let result = extract("bin/server.dart", code);
    assert_eq!(
        fact_lines(
            &result,
            "shelf_router.route.v1",
            &["api_style", "verb", "normalized_route_template", "handler"],
        ),
        vec![
            "annotation GET /users/:id fetch",
            "annotation PATCH /users/:id patch",
            "router_call GET /health ",
            "router_call POST /items createItem",
        ]
    );
}

#[test]
fn package_http_and_dio_requests_are_client_facts() {
    let code = "import 'package:http/http.dart' as http;\nimport 'package:dio/dio.dart';\nFuture<void> f(Dio dio) async {\n  await http.get(Uri.parse('https://api.example.com/users'));\n  await http.post(Uri.https('api.example.com', '/items'), body: '{}');\n  await dio.get<Map>('/users/1');\n  await _dio.delete('/users/1');\n  await cache.get('/ignored');\n  await http.get(Uri.parse('$base/x'));\n}\n";
    let result = extract("lib/api.dart", code);
    assert_eq!(
        fact_lines(
            &result,
            "http.client_request.v1",
            &["client", "verb", "target_path"],
        ),
        vec![
            "dart_http GET https://api.example.com/users",
            "dart_http POST https://api.example.com/items",
            "dio GET /users/1",
            "dio DELETE /users/1",
        ]
    );
    let bare = extract(
        "lib/api.dart",
        "import 'package:http/http.dart';\nFuture<void> f() => get(Uri.parse('https://api.example.com/a'));\n",
    );
    assert_eq!(
        fact_lines(&bare, "http.client_request.v1", &["client", "target_path"]),
        vec!["dart_http https://api.example.com/a"]
    );
}

#[test]
fn directives_bindings_and_aliases_have_no_body() {
    let code = "import 'package:http/http.dart' as http;\nlibrary_name_free() {}\ntypedef void Callback(String s);\nfinal list = List<int>.filled(3, 0);\nvoid run(List<(int, int)> pairs) {\n  final buffer = StringBuffer()..write('a');\n}\n";
    let result = extract("lib/run.dart", code);
    let with_body: Vec<String> = result
        .symbols
        .iter()
        .filter(|s| s.body_span.is_some() || s.body_hash.is_some())
        .map(|s| s.name.clone())
        .collect();
    assert_eq!(with_body, vec!["library_name_free", "run"]);
}

#[test]
fn uri_constructors_and_dio_fields_are_url_carriers() {
    let code = "Future<void> load(Dio _dio) async {\n  await http.get(Uri.parse('https://api.example.com/users'));\n  await _dio.patch('/v1/orders/7');\n  print('not a url');\n}\n";
    let mut literals = extract("lib/load.dart", code).literals;
    crate::classify_literals_by_carrier(&mut literals);
    let urls: Vec<(String, String)> = literals
        .iter()
        .map(|literal| {
            assert_eq!(literal.kind, crate::LiteralKind::Url, "{literal:?}");
            (
                literal.carrier.clone().unwrap_or_default(),
                literal.literal_text.clone(),
            )
        })
        .collect();
    assert_eq!(
        urls,
        vec![
            (
                "Uri.parse".to_string(),
                "https://api.example.com/users".to_string()
            ),
            ("_dio.patch".to_string(), "/v1/orders/7".to_string()),
        ]
    );
}
