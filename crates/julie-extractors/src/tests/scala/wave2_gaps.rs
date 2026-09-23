use crate::ExtractionResults;
use crate::base::{IdentifierKind, LiteralKind, RelationshipKind, SymbolKind, Visibility};
use crate::extract_canonical;
use crate::tests::helpers::metadata_str;
use std::path::Path;

fn extract(path: &str, source: &str) -> ExtractionResults {
    extract_canonical(path, source, Path::new("/tmp/test")).expect("scala extraction")
}

fn names(result: &ExtractionResults) -> Vec<String> {
    result
        .symbols
        .iter()
        .map(|symbol| format!("{}:{:?}", symbol.name, symbol.kind))
        .collect()
}

fn symbol<'a>(result: &'a ExtractionResults, name: &str) -> &'a crate::base::Symbol {
    result
        .symbols
        .iter()
        .find(|symbol| symbol.name == name)
        .unwrap_or_else(|| panic!("no symbol {name}: {:?}", names(result)))
}

fn symbol_name(result: &ExtractionResults, id: &str) -> String {
    result
        .symbols
        .iter()
        .find(|symbol| symbol.id == id)
        .map(|symbol| symbol.name.clone())
        .unwrap_or_default()
}

fn parent_name(result: &ExtractionResults, symbol: &crate::base::Symbol) -> String {
    symbol
        .parent_id
        .as_deref()
        .map(|id| symbol_name(result, id))
        .unwrap_or_default()
}

fn identifiers(result: &ExtractionResults, kind: IdentifierKind) -> Vec<String> {
    result
        .identifiers
        .iter()
        .filter(|identifier| identifier.kind == kind)
        .map(|identifier| identifier.name.clone())
        .collect()
}

fn resolved(result: &ExtractionResults, kind: RelationshipKind) -> Vec<(String, String, u32)> {
    result
        .relationships
        .iter()
        .filter(|relationship| relationship.kind == kind)
        .map(|relationship| {
            (
                symbol_name(result, &relationship.from_symbol_id),
                symbol_name(result, &relationship.to_symbol_id),
                relationship.line_number,
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
                pending.target.namespace_path.join("."),
            )
        })
        .collect()
}

fn text(value: &str) -> String {
    value.to_string()
}

fn meta<'a>(symbol: &'a crate::base::Symbol, key: &str) -> Option<&'a str> {
    symbol.metadata.as_ref()?.get(key)?.as_str()
}

fn facts(result: &ExtractionResults, pattern_id: &str, key: &str) -> Vec<(String, String)> {
    result
        .structural_facts
        .iter()
        .filter(|fact| fact.pattern_id == pattern_id)
        .map(|fact| {
            (
                metadata_str(fact, "verb").unwrap_or_default().to_string(),
                metadata_str(fact, key).unwrap_or_default().to_string(),
            )
        })
        .collect()
}

#[test]
fn constructor_delegation_calls_the_class() {
    let result = extract(
        "Local.scala",
        r#"
class Local(x: Int) {
  def this() = this(0)
}
"#,
    );

    assert_eq!(
        resolved(&result, RelationshipKind::Calls),
        vec![(text("Local"), text("Local"), 3)]
    );
    assert!(
        result
            .structured_pending_relationships
            .iter()
            .all(|pending| pending.target.terminal_name != "this")
    );
}

#[test]
fn interpolated_strings_are_literals_with_holes_and_regions() {
    let result = extract(
        "Api.scala",
        r#"
object Api {
  def user(id: Long) = requests.get(s"https://api.example.com/users/$id")
  def byId(id: Long) = sql"select id from users where id = $id".query[User]
  def ws(client: WSClient) = client.url("https://api.example.com/ws").get()
  def sttp() = basicRequest.get(uri"https://api.example.com/sttp")
}
"#,
    );

    let mut literals = result.literals.clone();
    crate::language_policy::classify_literals_by_carrier(&mut literals);
    let classified: Vec<(LiteralKind, &str)> = literals
        .iter()
        .map(|literal| (literal.kind.clone(), literal.literal_text.as_str()))
        .collect();
    assert_eq!(
        classified,
        vec![
            (LiteralKind::Url, "https://api.example.com/users/{}"),
            (LiteralKind::Sql, "select id from users where id = {}"),
            (LiteralKind::Url, "https://api.example.com/ws"),
            (LiteralKind::Url, "https://api.example.com/sttp"),
        ]
    );
    let string_regions = result
        .source_regions
        .iter()
        .filter(|region| region.kind == crate::base::SourceRegionKind::StringLiteral)
        .count();
    assert_eq!(string_regions, 4);
}

#[test]
fn packaging_blocks_and_package_objects_own_their_members() {
    let result = extract(
        "Util.scala",
        r#"
package com.acme

package object util {
  type Millis = Long
  def now(): Long = System.currentTimeMillis()
}

package http {
  class Router
}
"#,
    );

    let http = symbol(&result, "http");
    assert_eq!(http.kind, SymbolKind::Namespace);
    assert_eq!(parent_name(&result, symbol(&result, "Router")), "http");
    let util = symbol(&result, "util");
    assert_eq!(util.kind, SymbolKind::Namespace);
    assert_eq!(util.signature.as_deref(), Some("package object util"));
    assert_eq!(parent_name(&result, symbol(&result, "Millis")), "util");
    assert_eq!(parent_name(&result, symbol(&result, "now")), "util");
    assert!(!identifiers(&result, IdentifierKind::VariableRef).contains(&text("util")));
}

#[test]
fn pattern_and_multi_name_vals_bind_every_name() {
    let result = extract(
        "Settings.scala",
        r#"
object Settings {
  val (host, port) = ("localhost", 8080)
  val Config(user, pass) = load()
  val a, b = 1
  private val x, y: Int = 2
  def load(): Config = Config("u", "p")
}
"#,
    );

    for name in ["host", "port", "user", "pass", "a", "b"] {
        let binding = symbol(&result, name);
        assert_eq!(binding.kind, SymbolKind::Property, "{name}");
        assert_eq!(binding.visibility, Some(Visibility::Public), "{name}");
    }
    assert_eq!(symbol(&result, "x").visibility, Some(Visibility::Private));
    assert_eq!(
        symbol(&result, "y").signature.as_deref(),
        Some("val x, y: Int")
    );
    assert!(result.symbols.iter().all(|symbol| symbol.name != "Int"));
    let refs = identifiers(&result, IdentifierKind::VariableRef);
    assert!(
        ["a", "b", "x", "y"]
            .iter()
            .all(|name| !refs.contains(&text(name))),
        "{refs:?}"
    );
}

#[test]
fn enum_cases_keep_docs_parameters_and_extend_their_enum() {
    let result = extract(
        "Planet.scala",
        r#"
/** Planets. */
enum Planet(val mass: Double) {
  /** Closest to the sun. */
  case Mercury extends Planet(3.3e23)
  case Custom(name: String, m: Double) extends Planet(m)
}
"#,
    );

    assert_eq!(
        symbol(&result, "Mercury").doc_comment.as_deref(),
        Some("/** Closest to the sun. */")
    );
    assert_eq!(parent_name(&result, symbol(&result, "mass")), "Planet");
    assert_eq!(parent_name(&result, symbol(&result, "name")), "Custom");
    assert_eq!(parent_name(&result, symbol(&result, "m")), "Custom");
    let refs = identifiers(&result, IdentifierKind::VariableRef);
    assert!(!refs.contains(&text("Mercury")) && !refs.contains(&text("Custom")));
    let extends = resolved(&result, RelationshipKind::Extends);
    assert!(extends.contains(&(text("Mercury"), text("Planet"), 5)));
    assert!(extends.contains(&(text("Custom"), text("Planet"), 6)));
}

#[test]
fn member_vals_and_vars_are_properties_and_plain_parameters_are_private() {
    let result = extract(
        "Service.scala",
        r#"
class Service(repo: Repo, val name: String)(implicit ec: ExecutionContext) {
  var counter: Int = 0
  def run(): Unit = {
    val local = 1
  }
}
trait Named {
  val label: String
  var alias: String
}
case class Point(x: Int)
"#,
    );

    let repo = symbol(&result, "repo");
    assert_eq!(repo.visibility, Some(Visibility::Private));
    assert_eq!(repo.signature.as_deref(), Some("repo: Repo"));
    assert_eq!(meta(repo, "binding"), Some("none"));
    assert_eq!(symbol(&result, "name").visibility, Some(Visibility::Public));
    assert_eq!(parent_name(&result, symbol(&result, "ec")), "Service");
    assert_eq!(
        symbol(&result, "Service").signature.as_deref(),
        Some("class Service(repo: Repo, val name: String)(implicit ec: ExecutionContext)")
    );
    for name in ["counter", "label", "alias"] {
        assert_eq!(symbol(&result, name).kind, SymbolKind::Property, "{name}");
    }
    let local = symbol(&result, "local");
    assert_eq!(local.kind, SymbolKind::Variable);
    assert_eq!(local.visibility, None);
    let x = symbol(&result, "x");
    assert_eq!(x.visibility, Some(Visibility::Public));
    assert_eq!(x.signature.as_deref(), Some("val x: Int"));
}

#[test]
fn annotations_are_rows_not_modifiers() {
    let result = extract(
        "Api.scala",
        r#"
@deprecated("use V2", "2.0")
trait OldApi
@SerialVersionUID(1L)
enum Status {
  @deprecated("gone", "1.0") case Legacy
}
class Holder {
  @volatile var flag: Boolean = false
  @throws[java.io.IOException]
  def start(): Unit = ()
  @scala.annotation.tailrec
  final def loop(n: Int): Int = if (n <= 0) 0 else loop(n - 1)
}
"#,
    );

    let annotations = |name: &str| -> Vec<String> {
        symbol(&result, name)
            .annotations
            .iter()
            .map(|annotation| annotation.annotation.clone())
            .collect()
    };
    assert_eq!(annotations("OldApi"), vec!["deprecated"]);
    assert_eq!(annotations("Status"), vec!["SerialVersionUID"]);
    assert_eq!(annotations("Legacy"), vec!["deprecated"]);
    assert_eq!(annotations("flag"), vec!["volatile"]);
    assert_eq!(annotations("start"), vec!["throws"]);
    assert!(
        result.symbols.iter().all(|symbol| !symbol
            .signature
            .as_deref()
            .unwrap_or_default()
            .contains('@')),
        "{:?}",
        result
            .symbols
            .iter()
            .map(|symbol| symbol.signature.clone())
            .collect::<Vec<_>>()
    );
    let names: Vec<String> = facts(&result, "scala.annotation.v1", "annotation_name")
        .into_iter()
        .map(|(_, name)| name)
        .collect();
    assert!(names.contains(&text("tailrec")), "{names:?}");
    assert!(!names.contains(&text("scala")), "{names:?}");
}

#[test]
fn qualified_types_keep_paths_out_of_refs_and_record_type_arguments() {
    let result = extract(
        "Cache.scala",
        r#"
class Cache[K, V] extends java.io.Serializable {
  private val store: mutable.Map[K, V] = null
  val index: scala.collection.immutable.Map[String, List[Int]] = null
}
"#,
    );

    let refs = identifiers(&result, IdentifierKind::VariableRef);
    for segment in ["java", "io", "mutable", "scala", "collection", "immutable"] {
        assert!(!refs.contains(&text(segment)), "{segment}: {refs:?}");
    }
    let generic_heads: Vec<String> = result
        .type_argument_usages
        .iter()
        .filter_map(|usage| {
            result
                .identifiers
                .iter()
                .find(|identifier| identifier.id == usage.identifier_id)
                .map(|identifier| format!("{}@{}", identifier.name, identifier.start_line))
        })
        .collect();
    assert_eq!(generic_heads, vec![text("Map@3"), text("Map@4")]);
    assert_eq!(
        pending(&result, RelationshipKind::Extends),
        vec![(text("Cache"), text("Serializable"), text("java"))]
    );
}

#[test]
fn alphanumeric_infix_operators_are_method_calls() {
    let result = extract(
        "Math.scala",
        r#"
object Math {
  def sum(a: Vec, b: Vec): Vec = a plus b
  def both(xs: List[Int]): List[Int] = xs map double filter positive
  def add(a: Int, b: Int): Int = a + b
}
"#,
    );

    let mut calls = identifiers(&result, IdentifierKind::Call);
    calls.sort();
    assert_eq!(calls, vec![text("filter"), text("map"), text("plus")]);
    let mut calls: Vec<(String, String)> = result
        .structured_pending_relationships
        .iter()
        .map(|pending| {
            (
                pending.target.display_name.clone(),
                pending.target.receiver.clone().unwrap_or_default(),
            )
        })
        .collect();
    calls.sort();
    assert_eq!(
        calls,
        vec![
            (text("a.plus"), text("a")),
            (text("filter"), String::new()),
            (text("xs.map"), text("xs")),
        ]
    );
}

#[test]
fn flatspec_infix_words_do_not_call_out_of_their_test() {
    let result = extract(
        "src/test/scala/StackSpec.scala",
        r#"
class StackSpec extends AnyFlatSpec {
  "A Stack" should "pop values" in {
    check()
  }
}
"#,
    );

    let targets: Vec<String> = pending(&result, RelationshipKind::Calls)
        .into_iter()
        .map(|(_, target, _)| target)
        .collect();
    assert_eq!(targets, vec![text("check")]);
}

#[test]
fn givens_implement_their_type_and_extensions_name_their_type() {
    let result = extract(
        "Show.scala",
        r#"
trait Show[A]
case class Shape(r: Double)
given intShow: Show[Int] with
  def show(a: Int): String = a.toString
given Show[List[String]] with
  def show(a: List[String]): String = a.mkString
given ec: scala.concurrent.ExecutionContext = scala.concurrent.ExecutionContext.global
extension (s: Shape)
  def area: Double = s.r * s.r
"#,
    );

    let implements = resolved(&result, RelationshipKind::Implements);
    assert_eq!(
        implements
            .iter()
            .map(|(from, to, _)| (from.clone(), to.clone()))
            .collect::<Vec<_>>(),
        vec![
            (text("intShow"), text("Show")),
            (text("given_Show_List"), text("Show")),
        ]
    );
    let ec = symbol(&result, "ec");
    assert_eq!(
        result
            .types
            .get(&ec.id)
            .map(|fact| fact.resolved_type.as_str()),
        Some("ExecutionContext")
    );
    let extension = result
        .symbols
        .iter()
        .find(|symbol| symbol.kind == SymbolKind::Module)
        .expect("extension symbol");
    assert_eq!(extension.name, "Shape");
    assert_eq!(meta(extension, "extendedType"), Some("Shape"));
    assert_eq!(parent_name(&result, symbol(&result, "area")), "Shape");
    let given_types: Vec<(String, String)> =
        facts(&result, "scala.given_definition.v1", "given_type");
    assert_eq!(given_types.len(), 3);
    assert!(
        given_types
            .iter()
            .all(|(_, given_type)| !given_type.is_empty())
    );
}

#[test]
fn companion_objects_own_their_supertypes_and_qualified_targets_split() {
    let result = extract(
        "Money.scala",
        r#"
trait Codec[A]
class Money(val amount: BigDecimal) extends Ordered[Money]
object Money extends Codec[Money] with play.api.libs.json.Reads[Money]
object Main extends IOApp.Simple
"#,
    );

    assert_eq!(
        resolved(&result, RelationshipKind::Implements),
        vec![(text("Money"), text("Codec"), 4)]
    );
    let implements_reads = result
        .structured_pending_relationships
        .iter()
        .find(|pending| pending.target.terminal_name == "Reads")
        .expect("pending Reads");
    assert_eq!(implements_reads.pending.line_number, 4);
    assert_eq!(implements_reads.target.receiver.as_deref(), Some("json"));
    assert_eq!(
        implements_reads.target.namespace_path,
        vec!["play", "api", "libs"]
    );
    assert_eq!(
        pending(&result, RelationshipKind::Extends)
            .into_iter()
            .map(|(from, target, _)| (from, target))
            .collect::<Vec<_>>(),
        vec![
            (text("Money"), text("Ordered")),
            (text("Main"), text("Simple")),
        ]
    );
}

#[test]
fn akka_http_directives_emit_routes() {
    let result = extract(
        "UserRoutes.scala",
        r#"
import akka.http.scaladsl.server.Directives._

class UserRoutes(service: UserService) {
  val route =
    pathPrefix("users") {
      get { path(LongNumber) { id => complete(service.find(id)) } } ~
      post { entity(as[User]) { user => complete(service.create(user)) } } ~
      path("admin" / Segment) { name => delete { complete("gone") } }
    } ~
    path(dynamicPath) { get { complete("nope") } } ~
    get { complete("no path") }
}
"#,
    );

    assert_eq!(
        facts(&result, "akka_http.route.v1", "route_template"),
        vec![
            (text("GET"), text("/users/{LongNumber}")),
            (text("POST"), text("/users")),
            (text("DELETE"), text("/users/admin/{Segment}")),
        ]
    );
}

#[test]
fn http4s_patterns_emit_routes() {
    let result = extract(
        "Http4sRoutes.scala",
        r#"
import org.http4s._

object Http4sRoutes {
  val routes = HttpRoutes.of[IO] {
    case GET -> Root / "users" / IntVar(id) => Ok(s"user $id")
    case req @ POST -> Root / "users" => Created("ok")
    case GET -> Root / "search" :? QueryMatcher(q) => Ok(q)
    case GET -> Root => Ok("root")
    case other => NotFound()
  }
}
"#,
    );

    assert_eq!(
        facts(&result, "http4s.route.v1", "route_template"),
        vec![
            (text("GET"), text("/users/{id}")),
            (text("POST"), text("/users")),
            (text("GET"), text("/search")),
            (text("GET"), text("/")),
        ]
    );
}

#[test]
fn scala_http_clients_emit_client_requests() {
    let result = extract(
        "Client.scala",
        r#"
import play.api.libs.ws.WSClient
import sttp.client3._

class Client(ws: WSClient, other: Other) {
  def a() = requests.get("https://api.example.com/items")
  def b() = basicRequest.post(uri"https://api.example.com/post").send(backend)
  def c(id: Int) = basicRequest.get(uri"https://api.example.com/users/$id")
  def d() = ws.url("https://api.example.com/ws").delete()
  def e() = other.url("https://nope.example.com").get()
}
"#,
    );

    let requests: Vec<(String, String)> = result
        .structural_facts
        .iter()
        .filter(|fact| fact.pattern_id == "http.client_request.v1")
        .map(|fact| {
            (
                metadata_str(fact, "client").unwrap_or_default().to_string(),
                format!(
                    "{} {}",
                    metadata_str(fact, "verb").unwrap_or_default(),
                    metadata_str(fact, "target_path").unwrap_or_default()
                ),
            )
        })
        .collect();
    assert_eq!(
        requests,
        vec![
            (
                text("requests_scala"),
                text("GET https://api.example.com/items")
            ),
            (text("sttp"), text("POST https://api.example.com/post")),
            (text("play_ws"), text("DELETE https://api.example.com/ws")),
        ]
    );
}

#[test]
fn sbt_build_files_are_scala() {
    let result = extract(
        "build.sbt",
        r#"
lazy val core = (project in file("core"))
  .settings(name := "acme-core")
lazy val root = (project in file(".")).dependsOn(core)
"#,
    );

    assert_eq!(symbol(&result, "core").kind, SymbolKind::Constant);
    assert_eq!(symbol(&result, "root").kind, SymbolKind::Constant);
    assert!(identifiers(&result, IdentifierKind::Call).contains(&text("dependsOn")));
}
