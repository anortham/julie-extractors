use crate::ExtractionResults;
use crate::base::{RelationshipKind, Visibility};
use crate::extract_canonical;
use std::path::Path;

fn extract(path: &str, source: &str) -> ExtractionResults {
    extract_canonical(path, source, Path::new("/tmp/test")).expect("scala extraction")
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

fn symbol<'a>(result: &'a ExtractionResults, name: &str) -> &'a crate::base::Symbol {
    result
        .symbols
        .iter()
        .find(|s| s.name == name)
        .unwrap_or_else(|| panic!("missing symbol {name}"))
}

fn pending_full(result: &ExtractionResults) -> Vec<(String, String, String, Vec<String>)> {
    result
        .structured_pending_relationships
        .iter()
        .filter(|p| p.pending.kind == RelationshipKind::Calls)
        .map(|p| {
            (
                symbol_name(result, &p.pending.from_symbol_id),
                p.target.terminal_name.clone(),
                p.target.receiver.clone().unwrap_or_default(),
                p.target.namespace_path.clone(),
            )
        })
        .collect()
}

#[test]
fn calls_in_local_val_initializers_belong_to_the_enclosing_method() {
    let result = extract(
        "Pipeline.scala",
        r#"object Pipeline {
  val built = compute(1)
  def load(path: String): List[String] = Nil
  def compute(x: Int): Int = x * 2
  def run(path: String): Int = {
    val lines = load(path)
    lazy val extra = compute(lines.size)
    var total = remote.fetch(path)
    extra + total
  }
}
"#,
    );
    let calls = resolved(&result, RelationshipKind::Calls);
    assert!(calls.contains(&pair("run", "load")), "{calls:?}");
    assert!(calls.contains(&pair("run", "compute")), "{calls:?}");
    assert!(calls.contains(&pair("built", "compute")), "{calls:?}");
    assert!(pending(&result, RelationshipKind::Calls).contains(&triple("run", "fetch", "remote")));
}

#[test]
fn chained_call_targets_take_only_the_receiver_spine() {
    let result = extract(
        "Chains.scala",
        r#"object Chains {
  def a(xs: List[String]): List[Int] = xs.map(x => parse(x)).filter(_ > 0)
  def b(): String = ConfigFactory.load().getString("app.url")
  def c(repo: Repo, user: User) = repo.find(user.id).map(_.name).getOrElse("none")
  def d(): Int = com.acme.Util.twice(1)
}
class Child extends Base {
  override def greet(): String = super.greet() + "!"
}
"#,
    );
    let calls = pending_full(&result);
    let find = |from: &str, name: &str| {
        calls
            .iter()
            .find(|(f, t, _, _)| f == from && t == name)
            .unwrap_or_else(|| panic!("{from}->{name} missing from {calls:?}"))
            .clone()
    };
    assert_eq!(
        find("a", "filter"),
        ("a".into(), "filter".into(), "".into(), vec![])
    );
    assert_eq!(
        find("a", "map"),
        ("a".into(), "map".into(), "xs".into(), vec![])
    );
    assert_eq!(
        find("b", "getString"),
        ("b".into(), "getString".into(), "".into(), vec![])
    );
    assert_eq!(
        find("b", "load"),
        ("b".into(), "load".into(), "ConfigFactory".into(), vec![])
    );
    assert_eq!(
        find("c", "getOrElse"),
        ("c".into(), "getOrElse".into(), "".into(), vec![])
    );
    assert_eq!(
        find("d", "twice"),
        (
            "d".into(),
            "twice".into(),
            "Util".into(),
            vec!["com".to_string(), "acme".to_string()]
        )
    );
    let greet = result
        .structured_pending_relationships
        .iter()
        .find(|p| p.target.terminal_name == "greet")
        .expect("super.greet pending");
    assert_eq!(greet.receiver_type.as_deref(), Some("Base"));
}

#[test]
fn generic_calls_emit_edges() {
    let result = extract(
        "Codec.scala",
        r#"object Codec {
  def decode[T](s: String): T = ???
  def use(): Int = decode[Int]("1")
  def remote(): Any = Json.parse[User]("{}")
}
"#,
    );
    assert!(resolved(&result, RelationshipKind::Calls).contains(&pair("use", "decode")));
    assert!(pending(&result, RelationshipKind::Calls).contains(&triple("remote", "parse", "Json")));
}

#[test]
fn new_expressions_emit_constructor_calls() {
    let result = extract(
        "Worker.scala",
        r#"class Helper(x: Int)
class Worker(helper: Helper) {
  def process(): Unit = {
    val h = new Helper(1)
    val r = new UserRepo()
    val m = new scala.collection.mutable.HashMap[String, Int]()
  }
}
"#,
    );
    assert!(resolved(&result, RelationshipKind::Calls).contains(&pair("process", "Helper")));
    let calls = pending(&result, RelationshipKind::Calls);
    assert!(
        calls.contains(&triple("process", "UserRepo", "")),
        "{calls:?}"
    );
    assert!(
        calls.contains(&triple("process", "HashMap", "mutable")),
        "{calls:?}"
    );
    assert!(
        result
            .identifiers
            .iter()
            .any(|i| i.name == "UserRepo" && i.kind == crate::base::IdentifierKind::TypeUsage)
    );
}

#[test]
fn qualified_access_modifiers_set_visibility() {
    let result = extract(
        "src/main/scala/com/acme/core/Internal.scala",
        r#"package com.acme.core

private[core] class Internal {
  private[this] def secret(): Int = 1
  protected[core] def shared(): Int = secret()
  private[acme] val cfg: String = "x"
  protected def prot(): Int = 2
}
"#,
    );
    let visibility = |name: &str| symbol(&result, name).visibility.clone();
    assert_eq!(visibility("Internal"), Some(Visibility::Private));
    assert_eq!(visibility("secret"), Some(Visibility::Private));
    assert_eq!(visibility("shared"), Some(Visibility::Protected));
    assert_eq!(visibility("cfg"), Some(Visibility::Private));
    assert_eq!(visibility("prot"), Some(Visibility::Protected));
    assert_eq!(
        symbol(&result, "secret").metadata.as_ref().unwrap()["modifiers"],
        serde_json::json!("private[this]")
    );
    assert!(
        !result
            .identifiers
            .iter()
            .any(|i| matches!(i.name.as_str(), "core" | "acme" | "this")),
        "access qualifiers leaked into identifiers"
    );
}

#[test]
fn braceless_bodies_do_not_swallow_the_next_doc_comment() {
    let source = r#"object Service:
  /** Doc f. */
  def f(): Int =
    val a = 1
    a + 1

  /** Doc g. */
  def g(): Int =
    f() * 2

  /** Doc k. */
  val k: Int = 3

class A:
  def x: Int = 1

/** Doc for B. */
class B
"#;
    let result = extract("Service.scala", source);
    assert_eq!(
        symbol(&result, "g").doc_comment.as_deref(),
        Some("/** Doc g. */")
    );
    assert_eq!(
        symbol(&result, "k").doc_comment.as_deref(),
        Some("/** Doc k. */")
    );
    assert_eq!(
        symbol(&result, "B").doc_comment.as_deref(),
        Some("/** Doc for B. */")
    );
    assert_eq!(symbol(&result, "f").end_line, 5);
    assert_eq!(symbol(&result, "g").end_line, 9);
    assert_eq!(symbol(&result, "A").end_line, 15);
    let f_body = symbol(&result, "f").body_span.unwrap();
    assert_eq!(
        &source[f_body.start_byte as usize..f_body.end_byte as usize],
        "val a = 1\n    a + 1"
    );
}

#[test]
fn production_methods_named_like_tests_get_no_role() {
    let result = extract(
        "src/main/scala/db/Pool.scala",
        r#"package db

class ConnectionPool {
  def testConnection(conn: java.sql.Connection): Boolean = conn.isValid(5)
  def testOnBorrow: Boolean = true
  def beforeAll(): Unit = ()
}
object Features {
  def it(name: String)(body: => Unit): Unit = body
  it("warmup") { println("x") }
}
"#,
    );
    for name in ["testConnection", "testOnBorrow", "beforeAll", "warmup"] {
        assert_eq!(test_role(symbol(&result, name)), None, "{name}");
    }
}

#[test]
fn suite_classes_are_containers_and_keep_their_cases() {
    let result = extract(
        "src/test/scala/SuitesSpec.scala",
        r#"class JsonSuite extends munit.FunSuite {
  test("parses") { assert(true) }
}
class ValidatedSpec extends AnyFlatSpec {
  "Validated" should "combine" in { assert(true) }
}
class LegacySuite extends AnyFunSuite {
  def testLegacy(): Unit = ()
  override def beforeAll(): Unit = ()
}
"#,
    );
    for name in ["JsonSuite", "ValidatedSpec", "LegacySuite"] {
        assert_eq!(
            test_role(symbol(&result, name)),
            Some("test_container"),
            "{name}"
        );
    }
    assert_eq!(test_role(symbol(&result, "parses")), Some("test_case"));
    assert_eq!(
        test_role(symbol(&result, "Validated should combine")),
        Some("test_case")
    );
    assert_eq!(
        test_role(symbol(&result, "beforeAll")),
        Some("fixture_setup")
    );
}

#[test]
fn scalatest_word_free_feature_and_munit_styles_emit_tests() {
    let result = extract(
        "src/test/scala/StackSpec.scala",
        r#"class StackWordSpec extends AnyWordSpec {
  "A Stack" when {
    "empty" should {
      "have size 0" in { assert(true) }
    }
  }
}
class StackFreeSpec extends AnyFreeSpec {
  "A Cart" - {
    "adds items" in { assert(true) }
  }
}
class StackFeatureSpec extends AnyFeatureSpec {
  Feature("Stack") {
    Scenario("push then pop") { assert(true) }
  }
}
class CartMunit extends munit.FunSuite {
  test("skipped".ignore) { assert(true) }
  test("slow".tag(Slow)) { assert(true) }
  tmp.test("with fixture") { s => assertEquals(s, "x") }
}
class CartFunSpec extends AnyFunSpec {
  describe("Cart") { ignore("not yet") { fail() } }
}
"#,
    );
    for name in ["A Stack when", "empty should", "A Cart", "Stack"] {
        assert_eq!(
            test_role(symbol(&result, name)),
            Some("test_container"),
            "{name}"
        );
    }
    for name in [
        "have size 0",
        "adds items",
        "push then pop",
        "skipped",
        "slow",
        "with fixture",
        "not yet",
    ] {
        assert_eq!(
            test_role(symbol(&result, name)),
            Some("test_case"),
            "{name}"
        );
    }
}
