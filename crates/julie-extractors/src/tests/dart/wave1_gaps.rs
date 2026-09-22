use crate::base::{ExtractionResults, IdentifierKind, RelationshipKind, Symbol, SymbolKind};
use crate::extract_canonical;
use std::path::Path;

fn extract(path: &str, code: &str) -> ExtractionResults {
    extract_canonical(path, code, Path::new("/tmp/test")).expect("dart extraction")
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

fn containing(result: &ExtractionResults, name: &str) -> String {
    let identifier = result
        .identifiers
        .iter()
        .find(|i| i.name == name && i.kind == IdentifierKind::Call)
        .unwrap_or_else(|| panic!("no call identifier {name}"));
    name_of(result, identifier.containing_symbol_id.as_ref())
}

#[test]
fn calls_in_same_named_callables_keep_their_own_caller() {
    let code = "class A { void run() { helper(); remote(); } }\nclass B { void run() { helper(); remote(); } }\nvoid helper() {}\n";
    let result = extract("lib/dup.dart", code);
    assert_eq!(
        sorted(calls_from(&result, "run")),
        vec!["helper", "helper", "pending:remote", "pending:remote"]
    );
}

#[test]
fn member_calls_emit_edges_or_receiver_pending_rows() {
    let code = "class Cart {\n  final List<Item> items = [];\n  void add(Item item) { items.add(item); this.recalc(); recalc(); Cart.empty(); }\n  void recalc() {}\n  static Cart empty() => Cart();\n}\nclass Item {}\nvoid checkout(Cart cart, Store store) { cart.recalc(); store.add(Item()); DateTime.now(); }\n";
    let result = extract("lib/cart.dart", code);
    assert_eq!(
        sorted(calls_from(&result, "add")),
        vec!["empty", "pending:add", "recalc", "recalc"]
    );
    let checkout = sorted(calls_from(&result, "checkout"));
    for expected in ["pending:recalc", "pending:add", "pending:now", "Item"] {
        assert!(
            checkout.contains(&expected.to_string()),
            "{expected}: {checkout:?}"
        );
    }
    let receivers = pending_receivers(&result);
    for expected in [
        ("add", "items"),
        ("recalc", "cart"),
        ("add", "store"),
        ("now", "DateTime"),
    ] {
        assert!(
            receivers.contains(&(expected.0.to_string(), expected.1.to_string())),
            "{expected:?}: {receivers:?}"
        );
    }
}

#[test]
fn constructors_and_accessors_span_their_bodies() {
    let code = "abstract class Repo {\n  final Api api;\n  Repo(this.api) : retries = defaultRetries() { api.connect(); }\n  const Repo.small() : v = 4;\n  factory Repo.create() { return Impl(Api()); }\n  int load();\n  int get count { return api.count(); }\n  set count(int value) { api.store(value); }\n  int get fast => api.fast();\n}\nString get appName { return compute(); }\n";
    let result = extract("lib/repo.dart", code);
    let constructor = result
        .symbols
        .iter()
        .find(|s| s.name == "Repo" && s.kind == SymbolKind::Constructor)
        .unwrap();
    let body = constructor.body_span.expect("constructor body span");
    assert_eq!(
        &code[body.start_byte as usize..body.end_byte as usize],
        "{ api.connect(); }"
    );
    assert!(
        result
            .symbols
            .iter()
            .any(|s| s.name == "Repo.small" && s.kind == SymbolKind::Constructor)
    );
    for (name, caller) in [
        ("connect", "Repo"),
        ("defaultRetries", "Repo"),
        ("Impl", "Repo.create"),
        ("store", "count"),
        ("fast", "fast"),
        ("compute", "appName"),
    ] {
        assert_eq!(containing(&result, name), caller, "{name}");
        assert!(
            calls_from(&result, caller).contains(&format!("pending:{name}")),
            "{caller} -> {name}"
        );
    }
    let getter = result
        .symbols
        .iter()
        .find(|s| s.name == "count" && s.signature.as_deref() == Some("int get count"))
        .unwrap_or_else(|| panic!("{:#?}", names(&result)));
    assert!(getter.body_span.is_some());
    let load = symbol(&result, "load", SymbolKind::Method);
    assert!(load.body_span.is_none());
}

#[test]
fn arrow_closure_calls_are_calls() {
    let code = "class Counter {\n  void increment() {}\n  void wire(Button b, List<Map<String, dynamic>> rows) {\n    b.onPressed = () => increment();\n    final users = rows.map((e) => User.fromJson(e)).toList();\n    b.onDrag = (d) => handler.handle(d);\n  }\n}\n";
    let result = extract("lib/counter.dart", code);
    let calls: Vec<String> = result
        .identifiers
        .iter()
        .filter(|i| i.kind == IdentifierKind::Call)
        .map(|i| i.name.clone())
        .collect();
    for name in ["increment", "fromJson", "handle", "map", "toList"] {
        assert!(calls.contains(&name.to_string()), "{name}: {calls:?}");
    }
    assert!(calls_from(&result, "wire").contains(&"increment".to_string()));
    let receivers = pending_receivers(&result);
    assert!(
        receivers.contains(&("fromJson".to_string(), "User".to_string())),
        "{receivers:?}"
    );
    assert!(
        receivers.contains(&("handle".to_string(), "handler".to_string())),
        "{receivers:?}"
    );
    assert!(
        result
            .structured_pending_relationships
            .iter()
            .all(|p| !p.target.display_name.starts_with('(')),
        "{receivers:?}"
    );
}

#[test]
fn top_level_variables_and_all_field_declarators_are_symbols() {
    let code = "class Config {\n  static const defaultTimeout = Duration(seconds: 30);\n  static const String apiKey = 'k';\n  final controller = TextEditingController();\n  var counter = 0;\n  String first = 'a', second = 'b';\n  Config._();\n}\nconst kPadding = 8.0;\nfinal logger = Logger('app');\nString? currentUser;\n";
    let result = extract("lib/config.dart", code);
    let config = symbol(&result, "Config", SymbolKind::Class);
    for name in [
        "defaultTimeout",
        "apiKey",
        "controller",
        "counter",
        "first",
        "second",
    ] {
        let field = symbol(&result, name, SymbolKind::Field);
        assert_eq!(field.parent_id.as_ref(), Some(&config.id), "{name}");
    }
    assert!(
        result
            .symbols
            .iter()
            .any(|s| s.name == "kPadding" && s.kind == SymbolKind::Constant)
    );
    assert!(
        result
            .symbols
            .iter()
            .any(|s| s.name == "logger" && s.kind == SymbolKind::Constant)
    );
    assert!(
        result
            .symbols
            .iter()
            .any(|s| s.name == "currentUser" && s.kind == SymbolKind::Variable)
    );
    assert_eq!(
        calls_from(&result, "controller"),
        vec!["pending:TextEditingController"]
    );
    assert_eq!(calls_from(&result, "logger"), vec!["pending:Logger"]);
    assert_eq!(containing(&result, "Duration"), "defaultTimeout");
}

#[test]
fn type_facts_come_from_declared_return_types() {
    let code = "class Repo {\n  Future<User> load() async => User();\n  User? find(String id) => null;\n  User get current => User();\n}\nclass User {}\nMap<String, User> index() => {};\n";
    let result = extract("lib/types.dart", code);
    let fact = |name: &str| {
        result
            .symbols
            .iter()
            .find(|s| s.name == name)
            .and_then(|s| result.types.get(&s.id))
            .map(|t| (t.resolved_type.clone(), t.is_inferred))
    };
    assert_eq!(fact("Repo"), None);
    assert_eq!(fact("User"), None);
    assert_eq!(fact("load"), Some(("Future".to_string(), false)));
    assert_eq!(fact("find"), Some(("User".to_string(), false)));
    assert_eq!(fact("current"), Some(("User".to_string(), false)));
    assert_eq!(fact("index"), Some(("Map".to_string(), false)));
}

#[test]
fn undocumented_members_do_not_inherit_the_container_doc() {
    let code = "/// Class doc.\nclass Account {\n  /// The balance in cents.\n  int balance = 0;\n  final String owner;\n  void noDoc() {}\n}\n/// Increments a worker id.\nint helper(int value) => value + 1;\n/// Status values.\nenum Status { active, inactive }\n";
    let result = extract("lib/docs.dart", code);
    let doc = |name: &str| {
        result
            .symbols
            .iter()
            .find(|s| s.name == name)
            .unwrap()
            .doc_comment
            .clone()
    };
    assert_eq!(doc("Account").as_deref(), Some("/// Class doc."));
    assert_eq!(doc("balance").as_deref(), Some("/// The balance in cents."));
    assert_eq!(
        doc("helper").as_deref(),
        Some("/// Increments a worker id.")
    );
    for name in ["owner", "noDoc", "value", "active", "inactive"] {
        assert_eq!(doc(name), None, "{name}");
    }
}

#[test]
fn const_and_new_instantiations_are_calls() {
    let code = "class Spacing {\n  const Spacing(this.v);\n  const Spacing.small() : v = 4;\n  final double v;\n}\nWidget build() {\n  final a = const Spacing(8);\n  final b = new Spacing(2);\n  final d = const Spacing.small();\n  return const Padding(padding: EdgeInsets.all(8));\n}\n";
    let result = extract("lib/ctors.dart", code);
    assert_eq!(
        sorted(calls_from(&result, "build")),
        vec![
            "Spacing",
            "Spacing",
            "Spacing.small",
            "pending:Padding",
            "pending:all"
        ]
    );
    let calls: Vec<&str> = result
        .identifiers
        .iter()
        .filter(|i| i.kind == IdentifierKind::Call)
        .map(|i| i.name.as_str())
        .collect();
    assert_eq!(
        calls.iter().filter(|n| **n == "Spacing").count(),
        2,
        "{calls:?}"
    );
    assert!(
        calls.contains(&"small") && calls.contains(&"Padding"),
        "{calls:?}"
    );
}

#[test]
fn test_closure_calls_belong_to_the_test_symbol() {
    let code = "void main() {\n  late Counter counter;\n  group('Counter', () {\n    setUp(() { counter = Counter(); });\n    test('increments', () { counter.increment(); expect(counter.value, 1); });\n  });\n}\nvoid runExample(void Function() callback) { callback(); }\n";
    let result = extract("test/counter_test.dart", code);
    let increments = calls_from(&result, "increments");
    assert!(
        increments.contains(&"pending:increment".to_string()),
        "{increments:?}"
    );
    assert!(calls_from(&result, "setUp").contains(&"pending:Counter".to_string()));
    let to_names: Vec<String> = result
        .relationships
        .iter()
        .filter(|r| r.kind == RelationshipKind::Calls)
        .map(|r| name_of(&result, Some(&r.to_symbol_id)))
        .collect();
    for name in ["Counter", "setUp", "callback", "increments"] {
        assert!(
            !to_names.contains(&name.to_string()),
            "{name}: {to_names:?}"
        );
    }
}
