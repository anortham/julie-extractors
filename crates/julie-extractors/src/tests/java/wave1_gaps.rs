use crate::ExtractionResults;
use crate::base::{RelationshipKind, Visibility};
use crate::extract_canonical;
use std::path::Path;

fn extract(path: &str, source: &str) -> ExtractionResults {
    extract_canonical(path, source, Path::new("/tmp/test")).expect("java extraction")
}

fn symbol_name(result: &ExtractionResults, id: &str) -> String {
    result
        .symbols
        .iter()
        .find(|symbol| symbol.id == id)
        .map(|symbol| symbol.name.clone())
        .unwrap_or_default()
}

fn resolved(result: &ExtractionResults, kind: RelationshipKind) -> Vec<(String, String)> {
    result
        .relationships
        .iter()
        .filter(|relationship| relationship.kind == kind)
        .map(|relationship| {
            (
                symbol_name(result, &relationship.from_symbol_id),
                symbol_name(result, &relationship.to_symbol_id),
            )
        })
        .collect()
}

fn pending(result: &ExtractionResults, kind: RelationshipKind) -> Vec<(String, String, String)> {
    result
        .structured_pending_relationships
        .iter()
        .filter(|pending| pending.pending.kind == kind)
        .map(|pending| {
            (
                symbol_name(result, &pending.pending.from_symbol_id),
                pending.target.terminal_name.clone(),
                pending.target.receiver.clone().unwrap_or_default(),
            )
        })
        .collect()
}

fn test_role(symbol: &crate::base::Symbol) -> Option<&str> {
    symbol.metadata.as_ref()?.get("test_role")?.as_str()
}

fn pair(from: &str, to: &str) -> (String, String) {
    (from.to_string(), to.to_string())
}

fn triple(from: &str, terminal: &str, receiver: &str) -> (String, String, String) {
    (from.to_string(), terminal.to_string(), receiver.to_string())
}

#[test]
fn generic_and_qualified_supertypes_target_the_base_name() {
    let result = extract(
        "Super.java",
        r#"
class Base<T> {}
interface Handler<E> {}
class Repo extends Base<String> implements Handler<Integer> {}
public class Outer extends com.acme.base.AbstractThing implements java.io.Serializable, Map.Entry<String, Integer> {}
class Worker extends java.lang.Thread {}
"#,
    );

    let extends = resolved(&result, RelationshipKind::Extends);
    let implements = resolved(&result, RelationshipKind::Implements);
    assert!(extends.contains(&pair("Repo", "Base")), "{extends:?}");
    assert!(
        implements.contains(&pair("Repo", "Handler")),
        "{implements:?}"
    );

    let pending_extends = pending(&result, RelationshipKind::Extends);
    let pending_implements = pending(&result, RelationshipKind::Implements);
    assert!(pending_extends.contains(&triple("Outer", "AbstractThing", "base")));
    assert!(pending_extends.contains(&triple("Worker", "Thread", "lang")));
    assert!(pending_implements.contains(&triple("Outer", "Serializable", "io")));
    assert!(pending_implements.contains(&triple("Outer", "Entry", "Map")));
    assert!(
        result
            .structured_pending_relationships
            .iter()
            .all(|pending| !pending.target.terminal_name.contains('<')),
        "type arguments leaked into a pending target"
    );

    let outer = result.symbols.iter().find(|s| s.name == "Outer").unwrap();
    assert_eq!(
        outer.signature.as_deref(),
        Some(
            "public class Outer extends com.acme.base.AbstractThing implements java.io.Serializable, Map.Entry<String, Integer>"
        )
    );
    let worker = result.symbols.iter().find(|s| s.name == "Worker").unwrap();
    assert_eq!(
        worker.metadata.as_ref().unwrap()["base_types"],
        serde_json::json!(["java.lang.Thread"])
    );
}

#[test]
fn qualified_junit3_testcase_superclass_marks_container_and_cases() {
    let result = extract(
        "src/test/java/p/LegacyTest.java",
        "package p;\npublic class LegacyTest extends junit.framework.TestCase { public void testAdd() {} }\n",
    );
    let class = result
        .symbols
        .iter()
        .find(|s| s.name == "LegacyTest")
        .unwrap();
    let method = result.symbols.iter().find(|s| s.name == "testAdd").unwrap();
    assert_eq!(test_role(class), Some("test_container"), "{class:?}");
    assert_eq!(test_role(method), Some("test_case"), "{method:?}");
}

#[test]
fn interface_extends_clauses_emit_edges_and_base_types() {
    let result = extract(
        "Shapes.java",
        r#"
interface Shape { double area(); }
interface Polygon extends Shape, Comparable<Polygon> { int sides(); }
interface Remote extends java.rmi.Remote {}
interface OrderRepository extends JpaRepository<Order, Long>, OrderRepositoryCustom {}
"#,
    );

    assert!(resolved(&result, RelationshipKind::Extends).contains(&pair("Polygon", "Shape")));
    let pending_extends = pending(&result, RelationshipKind::Extends);
    for expected in [
        triple("Polygon", "Comparable", ""),
        triple("Remote", "Remote", "rmi"),
        triple("OrderRepository", "JpaRepository", ""),
        triple("OrderRepository", "OrderRepositoryCustom", ""),
    ] {
        assert!(
            pending_extends.contains(&expected),
            "{expected:?} missing from {pending_extends:?}"
        );
    }

    let repository = result
        .symbols
        .iter()
        .find(|s| s.name == "OrderRepository")
        .unwrap();
    assert_eq!(
        repository.metadata.as_ref().unwrap()["base_types"],
        serde_json::json!(["JpaRepository<Order, Long>", "OrderRepositoryCustom"])
    );
    let remote = result.symbols.iter().find(|s| s.name == "Remote").unwrap();
    assert_eq!(
        remote.signature.as_deref(),
        Some("interface Remote extends java.rmi.Remote")
    );
}

#[test]
fn supertype_edges_attach_to_the_declaring_nested_type() {
    let result = extract(
        "Nested.java",
        r#"
class Login { static class State extends LoginBase {} }
class Profile { static class State extends ProfileBase {} }
"#,
    );
    let states: Vec<_> = result
        .symbols
        .iter()
        .filter(|s| s.name == "State")
        .map(|s| s.id.clone())
        .collect();
    let owners: Vec<_> = result
        .structured_pending_relationships
        .iter()
        .filter(|p| p.pending.kind == RelationshipKind::Extends)
        .map(|p| {
            (
                p.pending.from_symbol_id.clone(),
                p.target.terminal_name.clone(),
            )
        })
        .collect();
    assert!(owners.contains(&(states[0].clone(), "LoginBase".to_string())));
    assert!(owners.contains(&(states[1].clone(), "ProfileBase".to_string())));
}

#[test]
fn method_references_emit_call_edges_and_call_identifiers() {
    let result = extract(
        "MethodRefs.java",
        r#"
class MethodRefs {
  void run(List<User> users) {
    users.forEach(this::handle);
    users.stream().map(User::getName).forEach(System.out::println);
    Supplier<MethodRefs> s = MethodRefs::new;
  }
  void handle(User u) {}
}
"#,
    );

    assert!(resolved(&result, RelationshipKind::Calls).contains(&pair("run", "handle")));
    let calls = pending(&result, RelationshipKind::Calls);
    assert!(
        calls.contains(&triple("run", "getName", "User")),
        "{calls:?}"
    );
    assert!(
        calls.contains(&triple("run", "println", "out")),
        "{calls:?}"
    );
    assert!(
        calls.contains(&triple("run", "MethodRefs", ""))
            || resolved(&result, RelationshipKind::Calls).contains(&pair("run", "MethodRefs")),
        "{calls:?}"
    );

    for name in ["handle", "getName", "println"] {
        let identifier = result
            .identifiers
            .iter()
            .find(|identifier| identifier.name == name)
            .unwrap_or_else(|| panic!("no identifier for {name}"));
        assert_eq!(identifier.kind, crate::base::IdentifierKind::Call, "{name}");
    }
    let handle = result
        .identifiers
        .iter()
        .find(|i| i.name == "handle")
        .unwrap();
    assert_eq!(handle.receiver_type.as_deref(), Some("MethodRefs"));
}

#[test]
fn initializer_calls_use_the_field_class_or_enum_constant_as_caller() {
    let result = extract(
        "Init.java",
        r#"
class Init {
  private static final Logger LOG = LoggerFactory.getLogger(Init.class);
  private final Helper helper = new Helper();
  static { Registry.register("init"); warmUp(); }
  { helper.prepare(); }
  static void warmUp() {}
  enum Op { ADD(Ops.plus()), SUB(Ops.minus()); Op(Object f) {} }
}
"#,
    );

    let calls = pending(&result, RelationshipKind::Calls);
    for expected in [
        triple("LOG", "getLogger", "LoggerFactory"),
        triple("helper", "Helper", ""),
        triple("Init", "register", "Registry"),
        triple("Init", "prepare", "helper"),
        triple("ADD", "plus", "Ops"),
        triple("SUB", "minus", "Ops"),
    ] {
        assert!(
            calls.contains(&expected),
            "{expected:?} missing from {calls:?}"
        );
    }
    assert!(resolved(&result, RelationshipKind::Calls).contains(&pair("Init", "warmUp")));
}

#[test]
fn constructor_calls_cover_generics_qualified_types_and_chaining() {
    let result = extract(
        "Ctor.java",
        r#"
import java.util.ArrayList;
class Point { Point(int x){} Point(){ this(0); } }
class Box<T> { static Box<String> of2(){ return new Box<String>("y"); } static Box<String> of(){ return new Box<>("x"); } }
class Child extends BaseService {
  Child(UserRepo repo) {
    super(repo);
    var list = new ArrayList<String>();
    Object e = new java.util.HashMap<String,Integer>();
    Outer.Inner i = new Outer.Inner();
  }
}
"#,
    );

    let calls = pending(&result, RelationshipKind::Calls);
    let resolved_calls = resolved(&result, RelationshipKind::Calls);
    let called = |from: &str, to: &str, receiver: &str| {
        calls.contains(&triple(from, to, receiver)) || resolved_calls.contains(&pair(from, to))
    };
    assert!(called("of", "Box", ""), "{calls:?} {resolved_calls:?}");
    assert!(called("of2", "Box", ""), "{calls:?} {resolved_calls:?}");
    assert!(called("Point", "Point", ""), "{calls:?} {resolved_calls:?}");
    assert!(
        calls.contains(&triple("Child", "BaseService", "")),
        "{calls:?}"
    );
    assert!(
        calls.contains(&triple("Child", "ArrayList", "")),
        "{calls:?}"
    );
    assert!(
        calls.contains(&triple("Child", "HashMap", "util")),
        "{calls:?}"
    );
    assert!(
        calls.contains(&triple("Child", "Inner", "Outer")),
        "{calls:?}"
    );
}

#[test]
fn implicit_and_package_private_visibility() {
    let result = extract(
        "Visibility.java",
        r#"
public interface Gateway {
  Result charge(long cents);
  default boolean enabled() { return true; }
  static Gateway noop() { return null; }
  private void hidden() {}
  record Result(String ref) {}
}
class PackageHelper { void helper() {} int count; private int secret; }
enum Color { RED; Color() {} }
"#,
    );
    let visibility = |name: &str| {
        result
            .symbols
            .iter()
            .find(|s| s.name == name)
            .unwrap_or_else(|| panic!("missing {name}"))
            .visibility
            .clone()
    };
    for name in ["charge", "enabled", "noop", "Result"] {
        assert_eq!(visibility(name), Some(Visibility::Public), "{name}");
    }
    assert_eq!(visibility("hidden"), Some(Visibility::Private));
    for name in ["PackageHelper", "helper", "count"] {
        assert_eq!(visibility(name), Some(Visibility::Internal), "{name}");
    }
    assert_eq!(visibility("secret"), Some(Visibility::Private));
    let enum_constructor = result
        .symbols
        .iter()
        .find(|s| s.name == "Color" && s.kind == crate::base::SymbolKind::Constructor)
        .unwrap();
    assert_eq!(enum_constructor.visibility, Some(Visibility::Private));
}
