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
fn nested_class_trait_and_anonymous_members_record_return_type() {
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
    new Runnable {
      def make(): Session = ???
      def run(): Unit = {
        val s = this.make()
      }
    }
    val c = Repo.current
  }
}
object Repo {
  def current: Workspace = ???
}
"#;
    assert_eq!(inferred(source, "cached"), workspace());
    assert_eq!(inferred(source, "w"), workspace());
    assert_eq!(inferred(source, "s"), Some(("Session".to_string(), true)));
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
        "new Runnable {\n      def run(): Unit = {\n        val a = this.load()\n        val b = load()\n      }\n    }",
    );
    assert_eq!(inferred(&source, "a"), None);
    assert_eq!(inferred(&source, "b"), None);
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
