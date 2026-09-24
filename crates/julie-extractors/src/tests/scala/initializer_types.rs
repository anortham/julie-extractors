use crate::base::TypeInfo;
use crate::scala::ScalaExtractor;
use std::path::PathBuf;

fn type_fact(source: &str, binding: &str) -> Option<TypeInfo> {
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_scala::LANGUAGE.into())
        .unwrap();
    let tree = parser.parse(source, None).unwrap();
    let mut extractor = ScalaExtractor::new(
        "scala".to_string(),
        "initializer_types.scala".to_string(),
        source.to_string(),
        &PathBuf::from("/tmp/test"),
    );
    let symbols = extractor.extract_symbols(&tree);
    let symbol = symbols
        .iter()
        .find(|s| s.name == binding)
        .unwrap_or_else(|| panic!("missing binding {binding}"));
    extractor.base.type_info.get(&symbol.id).cloned()
}

fn inferred(source: &str, binding: &str) -> Option<(String, bool)> {
    type_fact(source, binding).map(|fact| (fact.resolved_type, fact.is_inferred))
}

fn declared(source: &str, binding: &str) -> Option<String> {
    type_fact(source, binding)?
        .metadata?
        .get("declared")?
        .as_str()
        .map(str::to_string)
}

fn workspace() -> Option<(String, bool)> {
    Some(("Workspace".to_string(), true))
}

fn in_service(members: &str, body: &str) -> String {
    format!(
        "class Workspace\nclass Service {{\n{members}\n  def run(): Unit = {{\n    {body}\n  }}\n}}\n"
    )
}

#[test]
fn bare_call_to_method_of_enclosing_class_records_return_type() {
    let source = in_service("  def load(): Workspace = ???", "val w = load()");
    assert_eq!(inferred(&source, "w"), workspace());
}

#[test]
fn this_call_records_return_type() {
    let source = in_service("  def load(): Workspace = ???", "val w = this.load()");
    assert_eq!(inferred(&source, "w"), workspace());
}

#[test]
fn object_call_records_return_type() {
    let source = r#"
object Repo {
  def create(name: String): Workspace = ???
}
def run(): Unit = {
  val w = Repo.create("a")
}
"#;
    assert_eq!(inferred(source, "w"), workspace());
}

#[test]
fn top_level_and_local_defs_record_return_type() {
    let source = r#"
def open(): Workspace = ???
def run(): Unit = {
  def local(): Session = ???
  val w = open()
  val s = local()
}
"#;
    assert_eq!(inferred(source, "w"), workspace());
    assert_eq!(inferred(source, "s"), Some(("Session".to_string(), true)));
}

#[test]
fn generic_return_type_records_base_name_and_declared_text() {
    let source = in_service("  def all(): List[Workspace] = ???", "val ws = all()");
    assert_eq!(inferred(&source, "ws"), Some(("List".to_string(), true)));
    assert_eq!(declared(&source, "ws").as_deref(), Some("List[Workspace]"));
}

#[test]
fn option_and_try_calls_keep_the_wrapper_type() {
    let source = in_service(
        "  def find(): Option[Workspace] = ???\n  def attempt(): Try[Workspace] = ???",
        "val o = find()\n    val t = attempt()",
    );
    assert_eq!(inferred(&source, "o"), Some(("Option".to_string(), true)));
    assert_eq!(inferred(&source, "t"), Some(("Try".to_string(), true)));
}

#[test]
fn get_unwraps_option_and_try() {
    let source = in_service(
        "  def find(): Option[Workspace] = ???\n  def attempt(): scala.util.Try[Workspace] = ???",
        "val o = find().get\n    val t = this.attempt().get",
    );
    assert_eq!(inferred(&source, "o"), workspace());
    assert_eq!(inferred(&source, "t"), workspace());
}

#[test]
fn parameterless_def_reference_records_return_type() {
    let source = in_service(
        "  def current: Workspace = ???",
        "val w = current\n    val v = this.current",
    );
    assert_eq!(inferred(&source, "w"), workspace());
    assert_eq!(inferred(&source, "v"), workspace());
}

#[test]
fn implicit_using_and_curried_lists_record_return_type() {
    let source = in_service(
        "  def load(id: Int)(implicit ec: Ec): Workspace = ???\n  def withUsing(id: Int)(using ec: Ec): Workspace = ???\n  def curried(id: Int)(name: String): Workspace = ???",
        "val a = load(1)\n    val b = withUsing(1)\n    val c = curried(1)(\"n\")\n    val d = load(1)(ec)",
    );
    assert_eq!(inferred(&source, "a"), workspace());
    assert_eq!(inferred(&source, "b"), workspace());
    assert_eq!(inferred(&source, "c"), workspace());
    assert_eq!(inferred(&source, "d"), workspace());
}

#[test]
fn generic_def_with_concrete_return_records_return_type() {
    let source = in_service(
        "  def build[T: Show](seed: T): Workspace = ???\n  def open[F[_]](using ec: Ec): Workspace = ???",
        "val a = build(1)\n    val b = build[Int](1)\n    val c = open[IO]",
    );
    assert_eq!(inferred(&source, "a"), workspace());
    assert_eq!(inferred(&source, "b"), workspace());
    assert_eq!(inferred(&source, "c"), workspace());
}

#[test]
fn member_val_records_return_type() {
    let source = r#"
class Service {
  def load(): Workspace = ???
  val cached = load()
}
"#;
    assert_eq!(inferred(source, "cached"), workspace());
}

#[test]
fn nested_class_and_trait_members_record_return_type() {
    let source = r#"
trait Store {
  def load(): Workspace
  val cached = load()
}
class Outer {
  def open(): Workspace = ???
  class Inner {
    val w = open()
  }
  def run(): Unit = {
    val c = Repo.current
  }
}
object Repo {
  def current: Workspace = ???
}
"#;
    assert_eq!(inferred(source, "cached"), workspace());
    assert_eq!(inferred(source, "w"), workspace());
    assert_eq!(inferred(source, "c"), workspace());
}

#[test]
fn class_parameters_and_self_types_block_outer_defs() {
    let source = r#"
def load(): Workspace = ???
class Service(load: () => Session) {
  val a = load()
}
trait Plugin { self: Host =>
  val b = load()
}
"#;
    assert_eq!(inferred(source, "a"), None);
    assert_eq!(inferred(source, "b"), None);
}

#[test]
fn braceless_object_records_return_type() {
    let source = r#"
object Main:
  def load(): Workspace = ???
  def run(): Unit =
    val w = load()
"#;
    assert_eq!(inferred(source, "w"), workspace());
}

#[test]
fn companion_apply_with_class_return_type_records_class() {
    let source = r#"
class Session(id: Int)
object Session {
  def apply(name: String): Session = ???
}
object Maker {
  def apply(): Workspace = ???
}
def run(): Unit = {
  val s = Session("a")
  val w = Maker()
}
"#;
    assert_eq!(inferred(source, "s"), Some(("Session".to_string(), true)));
    assert_eq!(inferred(source, "w"), workspace());
}

#[test]
fn written_type_wins_over_call_inference() {
    let source = in_service("  def load(): Workspace = ???", "val w: Base = load()");
    assert_eq!(inferred(&source, "w"), Some(("Base".to_string(), false)));
}

#[test]
fn generic_and_abstract_type_returns_record_nothing() {
    let source = r#"
class Box[T] {
  type A
  def get(): T = ???
  def pick[U](): U = ???
  def pickAll[U](): List[U] = ???
  def wrapped(): Option[T] = ???
  def member(): A = ???
  def run(): Unit = {
    val g = get()
    val p = pick[Int]()
    val q = pick()
    val all = pickAll[Int]()
    val w = wrapped().get
    val m = member()
  }
}
"#;
    assert_eq!(inferred(source, "q"), None);
    assert_eq!(inferred(source, "p"), None);
    assert_eq!(inferred(source, "g"), None);
    assert_eq!(inferred(source, "all"), Some(("List".to_string(), true)));
    assert_eq!(inferred(source, "w"), None);
    assert_eq!(inferred(source, "m"), None);
}

#[test]
fn other_methods_at_the_end_of_a_chain_record_nothing() {
    let source = in_service(
        "  def find(): Option[Workspace] = ???\n  def table(): Map[String, Workspace] = ???",
        "val a = find().getOrElse(null)\n    val b = find().map(identity)\n    val c = table().get\n    val d = find().get()",
    );
    assert_eq!(inferred(&source, "a"), None);
    assert_eq!(inferred(&source, "b"), None);
    assert_eq!(inferred(&source, "c"), None);
    assert_eq!(inferred(&source, "d"), None);
}

#[test]
fn disagreeing_overloads_record_nothing() {
    let source = in_service(
        "  def load(): Workspace = ???\n  def load(id: Int): Session = ???",
        "val w = load()",
    );
    assert_eq!(inferred(&source, "w"), None);
}

#[test]
fn undeclared_return_type_records_nothing() {
    let source = in_service("  def load() = new Workspace", "val w = load()");
    assert_eq!(inferred(&source, "w"), None);
}

#[test]
fn inherited_or_unknown_receivers_record_nothing() {
    let source = r#"
def open(): Workspace = ???
class Service extends Base {
  def run(): Unit = {
    val a = this.inherited()
    val b = open()
    val c = other.load()
    val d = Remote.create()
  }
}
"#;
    assert_eq!(inferred(source, "a"), None);
    assert_eq!(inferred(source, "b"), None);
    assert_eq!(inferred(source, "c"), None);
    assert_eq!(inferred(source, "d"), None);
}

#[test]
fn case_class_and_any_members_do_not_resolve_to_outer_defs() {
    let source = r#"
def copy(): Workspace = ???
def toString(): Workspace = ???
case class Point(x: Int) {
  def run(): Unit = {
    val c = copy()
  }
}
class Plain {
  def run(): Unit = {
    val s = toString()
  }
}
"#;
    assert_eq!(inferred(source, "c"), None);
    assert_eq!(inferred(source, "s"), None);
}

#[test]
fn anonymous_class_body_records_nothing_for_outer_methods() {
    let source = in_service(
        "  def load(): Workspace = ???",
        "new Runnable {\n      def make(): Session = ???\n      def run(): Unit = {\n        val a = this.load()\n        val b = load()\n        val c = this.make()\n        val d = make()\n      }\n    }",
    );
    assert_eq!(inferred(&source, "a"), None);
    assert_eq!(inferred(&source, "b"), None);
    assert_eq!(inferred(&source, "c"), None);
    assert_eq!(inferred(&source, "d"), None);
}

#[test]
fn shadowing_bindings_record_nothing() {
    let source = r#"
object Repo {
  def load(): Workspace = ???
  def create(): Workspace = ???
  def withParam(load: () => Session): Unit = {
    val a = load()
  }
  def withLocal(): Unit = {
    val load = () => new Session
    if (ready) {
      val b = load()
    }
  }
  def withLambda(): Unit = {
    xs.foreach { load => val c = load() }
  }
  def withCase(): Unit = {
    xs match {
      case load => val d = load()
    }
  }
  def withObject(): Unit = {
    val Repo = other
    val e = Repo.create()
  }
}
"#;
    assert_eq!(inferred(source, "a"), None);
    assert_eq!(inferred(source, "b"), None);
    assert_eq!(inferred(source, "c"), None);
    assert_eq!(inferred(source, "d"), None);
    assert_eq!(inferred(source, "e"), None);
}

#[test]
fn argument_lists_that_do_not_match_the_def_record_nothing() {
    let source = in_service(
        "  def curried(id: Int)(name: String): Workspace = ???\n  def current: Workspace = ???\n  def load(): Workspace = ???",
        "val a = curried(1)\n    val b = current()\n    val c = load",
    );
    assert_eq!(inferred(&source, "a"), None);
    assert_eq!(inferred(&source, "b"), None);
    assert_eq!(inferred(&source, "c"), None);
}

#[test]
fn companion_apply_with_other_return_type_records_nothing() {
    let source = r#"
case class Session(id: Int)
object Session {
  def apply(name: String): Either[Error, Session] = ???
}
def run(): Unit = {
  val s = Session("a")
}
"#;
    assert_eq!(inferred(source, "s"), None);
}

#[test]
fn inherited_overloads_block_own_defs_of_inheriting_templates() {
    let source = r#"
trait Base { def load(): String = "" }
class Sub extends Base {
  def load(x: Int): Int = x
  def run(): Unit = { val inheritOverload = load(); val thisOverload = this.load() }
}
trait RBase { def create(): String = "" }
object Repo extends RBase { def create(x: Int): Int = x }
object Use { val objOverload = Repo.create() }
abstract class B2 { def load(): String = "" }
class Outer extends B2 { def load(x: Int): Int = x; class Inner { val nestedInherit = load() } }
"#;
    assert_eq!(inferred(source, "inheritOverload"), None);
    assert_eq!(inferred(source, "thisOverload"), None);
    assert_eq!(inferred(source, "objOverload"), None);
    assert_eq!(inferred(source, "nestedInherit"), None);
}

#[test]
fn for_enumerators_shadow_outer_defs() {
    let source = r#"
class Workspace; class Session
object Repo {
  def load(): Workspace = ???
  def a(xs: List[() => Session]): Unit = { for (load <- xs) { val parenFor = load() } }
  def b(xs: List[() => Session]): Unit = { for { load <- xs } { val braceFor = load() } }
  def c(xs: List[() => Session]) = for { load <- xs } yield { val yieldFor = load(); yieldFor }
  def d(xs: List[() => Session]) = for { x <- xs; load = x } yield { val eqFor = load(); eqFor }
  def e(xs: List[Int]) = for { x <- xs; y = load(); if load() != null } yield { val outer = load(); outer }
}
"#;
    assert_eq!(inferred(source, "parenFor"), None);
    assert_eq!(inferred(source, "braceFor"), None);
    assert_eq!(inferred(source, "yieldFor"), None);
    assert_eq!(inferred(source, "eqFor"), None);
    assert_eq!(inferred(source, "outer"), workspace());
}

#[test]
fn given_alias_shadows_outer_def() {
    let source = r#"
object G {
  def current: Int = 1
  def load(): Workspace = ???
  def f(): Unit = {
    given current: String = ""
    val givenShadow = current
  }
  def g(): Unit = {
    given load: (() => Session) = ???
    val gv = load()
  }
}
"#;
    assert_eq!(inferred(source, "givenShadow"), None);
    assert_eq!(inferred(source, "gv"), None);
}

#[test]
fn get_does_not_unwrap_a_same_file_class_named_like_a_wrapper() {
    let source = r#"
class Workspace
class Try[A] { def get: String = "" }
object U { def load(): Try[Workspace] = ???; val userTry = load().get }
"#;
    assert_eq!(inferred(source, "userTry"), None);
}

#[test]
fn companion_object_that_may_inherit_apply_records_nothing() {
    let source = r#"
class Worker
trait Factory { def apply(x: Int): String = "" }
object Worker extends Factory
class Job
object Job extends Factory { def apply(): Job = ??? }
object M { val inheritedApply = Worker(1); val ownApply = Job() }
"#;
    assert_eq!(inferred(source, "inheritedApply"), None);
    assert_eq!(inferred(source, "ownApply"), None);
}

#[test]
fn get_unwraps_only_the_scala_wrappers() {
    let source = r#"
import fastparse.Parsed
import fastparse.Parsed.Success
class Workspace
object P {
  def ok(): Parsed.Success[Workspace] = ???
  def ok2(): Success[Workspace] = ???
  def myopt(): mylib.Option[Workspace] = ???
  def std(): scala.Option[Workspace] = ???
  def rooted(): _root_.scala.util.Try[Workspace] = ???
  val qualifiedForeign = ok().get
  val importedForeign = ok2().get
  val otherPackage = myopt().get
  val scalaOption = std().get
  val rootedTry = rooted().get
}
"#;
    assert_eq!(inferred(source, "qualifiedForeign"), None);
    assert_eq!(inferred(source, "importedForeign"), None);
    assert_eq!(inferred(source, "otherPackage"), None);
    assert_eq!(inferred(source, "scalaOption"), workspace());
    assert_eq!(inferred(source, "rootedTry"), workspace());
}

#[test]
fn get_records_nothing_when_an_import_can_bring_another_wrapper() {
    let wildcard = "import fastparse.Parsed._\nclass Workspace\nobject P { def ok(): Option[Workspace] = ???; val w = ok().get }\n";
    let renamed = "import mylib.{Maybe => Option}\nclass Workspace\nobject P { def ok(): Option[Workspace] = ???; val w = ok().get }\n";
    let unimported_try =
        "class Workspace\nobject P { def ok(): Try[Workspace] = ???; val w = ok().get }\n";
    let scala_imports = "import scala.util.{Try, Success}\nimport scala.collection.mutable._\nclass Workspace\nobject P { def ok(): Try[Workspace] = ???; def fine(): Option[Workspace] = ???; val w = ok().get; val v = fine().get }\n";
    assert_eq!(inferred(wildcard, "w"), None);
    assert_eq!(inferred(renamed, "w"), None);
    assert_eq!(inferred(unimported_try, "w"), None);
    assert_eq!(inferred(scala_imports, "w"), workspace());
    assert_eq!(inferred(scala_imports, "v"), workspace());
}

#[test]
fn inner_case_class_hides_an_outer_def_of_the_same_name() {
    let source = r#"
class Tree
object Dsl {
  def Node(x: Int): Tree = ???
  def Leaf(x: Int): Tree = ???
  def Color(): Tree = ???
  object Inner {
    case class Node(x: Int)
    val n = Node(1)
  }
  def build = {
    case class Node(x: Int)
    val m = Node(2)
  }
  def plain = {
    class Leaf(x: Int)
    val l = Leaf(3)
  }
  def colors = {
    enum Color { case Red }
    val c = Color()
  }
}
"#;
    assert_eq!(inferred(source, "n"), Some(("Node".to_string(), true)));
    assert_eq!(inferred(source, "m"), Some(("Node".to_string(), true)));
    assert_eq!(inferred(source, "l"), None);
    assert_eq!(inferred(source, "c"), None);
}

#[test]
fn export_clauses_block_own_and_outer_defs() {
    let source = r#"
class Workspace
class Other
def load(): Workspace = ???
object B { def load(): Other = ???; def make(): Other = ??? }
object A {
  export B.load
  val x = load()
}
object A2 {
  export B.*
  def make(i: Int): Workspace = ???
  val y = make()
  val y2 = this.make()
}
object U { val z = A2.make() }
"#;
    assert_eq!(inferred(source, "x"), None);
    assert_eq!(inferred(source, "y"), None);
    assert_eq!(inferred(source, "y2"), None);
    assert_eq!(inferred(source, "z"), None);
}

#[test]
fn synthetic_companion_members_block_object_member_calls() {
    let source = r#"
class Other
case class Port(n: Int)
object Port {
  def apply(s: String): Option[Port] = ???
  def unapply(p: Port): Other = ???
  val inside = apply(8080)
  val insideThis = this.apply(8080)
}
case class Host(n: String)
object Host { def apply(i: Int): Host = ??? }
enum Color { case Red, Green }
object Color { def valueOf(i: Int): Other = ???; def fromOrdinal(s: String): Other = ???; def named(): Other = ??? }
object Use {
  val p2 = Port.apply(8080)
  val u = Port.unapply(null)
  val h = Host.apply(1)
  val c1 = Color.valueOf("Red")
  val c2 = Color.fromOrdinal(0)
  val c3 = Color.named()
}
"#;
    assert_eq!(inferred(source, "p2"), None);
    assert_eq!(inferred(source, "inside"), None);
    assert_eq!(inferred(source, "insideThis"), None);
    assert_eq!(inferred(source, "u"), None);
    assert_eq!(inferred(source, "c1"), None);
    assert_eq!(inferred(source, "c2"), None);
    assert_eq!(inferred(source, "h"), Some(("Host".to_string(), true)));
    assert_eq!(inferred(source, "c3"), Some(("Other".to_string(), true)));
}

#[test]
fn inherited_abstract_type_members_record_nothing() {
    let source = r#"
class Workspace
trait Base { type Out }
class C extends Base {
  object Helper { def load(): Out = ???; def all(): Option[Out] = ???; def ws(): Workspace = ??? }
  val x = Helper.load()
  val o = Helper.all().get
  val w = Helper.ws()
  def run = { def load2(): Out = ???; val y = load2() }
}
class Out
class Plain {
  object Helper { def load(): Out = ??? }
  val p = Helper.load()
}
"#;
    assert_eq!(inferred(source, "x"), None);
    assert_eq!(inferred(source, "o"), None);
    assert_eq!(inferred(source, "y"), None);
    assert_eq!(inferred(source, "w"), workspace());
    assert_eq!(inferred(source, "p"), Some(("Out".to_string(), true)));
}

#[test]
fn plain_argument_lists_do_not_fill_a_using_list() {
    let source = r#"
class Ctx
class Out { def apply(c: Ctx): Int = 1 }
object A {
  def load()(using c: Ctx): Out = new Out
  def cur(using c: Ctx): Out = new Out
  def first(using c: Ctx)(id: Int): Out = new Out
  def run(c: Ctx) = {
    val plainAfterEmpty = load()(c)
    val plainForUsing = cur(c)
    val explicitUsing = load()(using c)
    val summoned = load()
    val bareUsing = cur
    val usingThenPlain = first(1)
    val bothLists = first(using c)(1)
    val usingForExplicit = first(using c)(using c)
  }
}
object Repo2 {
  def create()(using c: Ctx): Out = ???
  val objUsing = Repo2.create()(ctx)
  val objExplicitUsing = Repo2.create()(using ctx)
}
"#;
    let out = Some(("Out".to_string(), true));
    assert_eq!(inferred(source, "plainAfterEmpty"), None);
    assert_eq!(inferred(source, "plainForUsing"), None);
    assert_eq!(inferred(source, "objUsing"), None);
    assert_eq!(inferred(source, "usingForExplicit"), None);
    assert_eq!(inferred(source, "explicitUsing"), out);
    assert_eq!(inferred(source, "summoned"), out);
    assert_eq!(inferred(source, "bareUsing"), out);
    assert_eq!(inferred(source, "usingThenPlain"), out);
    assert_eq!(inferred(source, "bothLists"), out);
    assert_eq!(inferred(source, "objExplicitUsing"), out);
}

#[test]
fn path_dependent_return_types_record_nothing() {
    let source = r#"
class Out
trait Repo { type Out }
object U {
  def open(r: Repo): r.Out = ???
  def run(repo: Repo) = { val depPath = open(repo) }
}
class C2 {
  type Out
  def load(): this.Out = ???
  val thisAbstract = load()
}
object Store { class Out; type Alias }
object Use {
  def fromObject(): Store.Out = ???
  def fromAlias(): Store.Alias = ???
  def fromJava(): java.util.UUID = ???
  val objectType = fromObject()
  val abstractAlias = fromAlias()
  val javaType = fromJava()
}
"#;
    assert_eq!(inferred(source, "depPath"), None);
    assert_eq!(inferred(source, "thisAbstract"), None);
    assert_eq!(inferred(source, "abstractAlias"), None);
    assert_eq!(
        inferred(source, "objectType"),
        Some(("Out".to_string(), true))
    );
    assert_eq!(
        inferred(source, "javaType"),
        Some(("UUID".to_string(), true))
    );
}

#[test]
fn own_def_of_an_inheriting_template_hides_a_same_named_class() {
    let source = r#"
class Tree
class Session
class Base
case class Node(x: Int)
class Leaf(x: Int)
object CaseDsl extends Base {
  def Node(x: Int): Session = ???
  val caseDsl = Node(1)
}
object PlainDsl extends Base {
  def Leaf(x: Int): Tree = ???
  val dslLeaf = Leaf(1)
}
object Inheriting extends Base {
  val inheritedClass = Leaf(2)
}
"#;
    assert_eq!(inferred(source, "caseDsl"), None);
    assert_eq!(inferred(source, "dslLeaf"), None);
    assert_eq!(
        inferred(source, "inheritedClass"),
        Some(("Leaf".to_string(), true))
    );
}
