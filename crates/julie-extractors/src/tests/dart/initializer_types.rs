use crate::base::{Symbol, SymbolKind, TypeInfo};
use crate::dart::DartExtractor;
use std::path::PathBuf;

fn extract(source: &str) -> (Vec<Symbol>, DartExtractor) {
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_dart::LANGUAGE.into())
        .unwrap();
    let tree = parser.parse(source, None).unwrap();
    let mut extractor = DartExtractor::new(
        "dart".to_string(),
        "initializer_types.dart".to_string(),
        source.to_string(),
        &PathBuf::from("/tmp/test"),
    );
    let symbols = extractor.extract_symbols(&tree);
    (symbols, extractor)
}

fn local<'a>(symbols: &'a [Symbol], name: &str) -> &'a Symbol {
    symbols
        .iter()
        .find(|s| s.name == name && s.kind == SymbolKind::Variable)
        .unwrap_or_else(|| panic!("missing local {name}"))
}

fn fact<'a>(extractor: &'a DartExtractor, symbols: &[Symbol], name: &str) -> Option<&'a TypeInfo> {
    extractor.base.type_info.get(&local(symbols, name).id)
}

fn declared(info: &TypeInfo) -> Option<&str> {
    info.metadata
        .as_ref()
        .and_then(|m| m.get("declared"))
        .and_then(|v| v.as_str())
}

fn assert_inferred(source: &str, name: &str, resolved: &str, declared_text: Option<&str>) {
    let (symbols, extractor) = extract(source);
    let info =
        fact(&extractor, &symbols, name).unwrap_or_else(|| panic!("missing type fact for {name}"));
    assert_eq!(info.resolved_type, resolved, "resolved type of {name}");
    assert!(info.is_inferred, "{name} fact must be inferred");
    assert_eq!(declared(info), declared_text, "declared text of {name}");
}

fn assert_no_fact(source: &str, names: &[&str]) {
    let (symbols, extractor) = extract(source);
    for name in names {
        assert!(
            fact(&extractor, &symbols, name).is_none(),
            "unexpected type fact for {name}: {:?}",
            fact(&extractor, &symbols, name)
        );
    }
}

#[test]
fn top_level_function_call_records_its_return_type() {
    let source = r#"
class Item {}
Item load() => Item();
void run() {
  var item = load();
}
"#;
    assert_inferred(source, "item", "Item", None);
}

#[test]
fn function_declared_after_use_records_its_return_type() {
    let source = r#"
void run() {
  final item = load();
}
Item load() => Item();
"#;
    assert_inferred(source, "item", "Item", None);
}

#[test]
fn future_return_without_await_records_the_future() {
    let source = r#"
Future<Item> fetch() async => Item();
void run() {
  var pending = fetch();
}
"#;
    assert_inferred(source, "pending", "Future", Some("Future<Item>"));
}

#[test]
fn await_unwraps_future_and_future_or() {
    let source = r#"
Future<Item> fetch() async => Item();
FutureOr<Item> maybeSync() => Item();
Future<void> run() async {
  final a = await fetch();
  final b = (await fetch());
  final c = await maybeSync();
}
"#;
    for name in ["a", "b", "c"] {
        assert_inferred(source, name, "Item", None);
    }
}

#[test]
fn await_of_nullable_future_keeps_nullability() {
    let source = r#"
Future<Item>? maybe() => null;
Future<void> run() async {
  final item = await maybe();
}
"#;
    assert_inferred(source, "item", "Item", Some("Item?"));
}

#[test]
fn null_assertion_removes_nullability() {
    let source = r#"
Item? find() => null;
Future<Item?> fetch() async => null;
Future<void> run() async {
  final nullable = find();
  final asserted = find()!;
  final awaited = (await fetch())!;
}
"#;
    assert_inferred(source, "nullable", "Item", Some("Item?"));
    assert_inferred(source, "asserted", "Item", None);
    assert_inferred(source, "awaited", "Item", None);
}

#[test]
fn this_and_unqualified_calls_resolve_to_the_enclosing_class_member() {
    let source = r#"
class Repo {
  Item? find() => null;
  Other load() => Other();
  void run() {
    final viaThis = this.find();
    final bare = load();
    final inClosure = () {
      final nested = this.load();
    };
  }
}
Item load() => Item();
"#;
    assert_inferred(source, "viaThis", "Item", Some("Item?"));
    assert_inferred(source, "bare", "Other", None);
    assert_inferred(source, "nested", "Other", None);
}

#[test]
fn unqualified_call_in_class_without_that_member_uses_the_top_level_function() {
    let source = r#"
class Repo {
  void run() {
    final item = load();
  }
}
Item load() => Item();
"#;
    assert_inferred(source, "item", "Item", None);
}

#[test]
fn static_calls_on_same_file_types_record_the_declared_return_type() {
    let source = r#"
class Repo {
  static Repo create() => Repo();
  static Item make() => Item();
  factory Repo.fromJson(Object json) => Repo();
}
enum Color {
  red;
  static Color parse(String text) => red;
}
extension RepoTools on Repo {
  static Item helper() => Item();
}
void run() {
  var created = Repo.create();
  var made = Repo.make();
  var decoded = Repo.fromJson(1);
  var color = Color.parse('red');
  var helped = RepoTools.helper();
}
"#;
    assert_inferred(source, "created", "Repo", None);
    assert_inferred(source, "made", "Item", None);
    assert_inferred(source, "decoded", "Repo", None);
    assert_inferred(source, "color", "Color", None);
    assert_inferred(source, "helped", "Item", None);
}

#[test]
fn extension_members_resolve_inside_the_extension() {
    let source = r#"
extension RepoTools on Repo {
  Item pick() => Item();
  void run() {
    final bare = pick();
    final viaThis = this.pick();
  }
}
"#;
    assert_inferred(source, "bare", "Item", None);
    assert_inferred(source, "viaThis", "Item", None);
}

#[test]
fn cascade_and_explicit_type_arguments_keep_the_call_type() {
    let source = r#"
Item load() => Item();
List<T> many<T>() => [];
void run() {
  final cascaded = load()..touch();
  final listed = many<int>();
}
"#;
    assert_inferred(source, "cascaded", "Item", None);
    assert_inferred(source, "listed", "List", Some("List<T>"));
}

#[test]
fn written_type_wins_over_the_call_type() {
    let source = r#"
Item load() => Item();
void run() {
  Object item = load();
}
"#;
    let (symbols, extractor) = extract(source);
    let info = fact(&extractor, &symbols, "item").expect("missing type fact");
    assert_eq!(info.resolved_type, "Object");
    assert!(!info.is_inferred);
}

#[test]
fn generic_returns_record_nothing() {
    let source = r#"
T pick<T>() => throw 1;
Future<T> later<T>() async => throw 1;
class Box<T> {
  T get() => throw 1;
  void run() {
    final boxed = this.get();
  }
}
Future<void> run() async {
  final picked = pick<int>();
  final awaited = await later<int>();
}
"#;
    assert_no_fact(source, &["boxed", "picked", "awaited"]);
}

#[test]
fn chains_ending_in_an_unknown_method_record_nothing() {
    let source = r#"
Item load() => Item();
Item? find() => null;
Future<Item> fetch() async => Item();
void run() {
  final chained = load().run();
  final then = fetch().then((item) => item);
  final nullAware = find()?.run();
  final property = load().name;
}
"#;
    assert_no_fact(source, &["chained", "then", "nullAware", "property"]);
}

#[test]
fn await_of_a_non_future_records_nothing() {
    let source = r#"
Item load() => Item();
List<Item> many() => [];
Future<dynamic> loose() async => 1;
Future<void> run() async {
  final plain = await load();
  final listed = await many();
  final dynamicResult = await loose();
}
"#;
    assert_no_fact(source, &["plain", "listed", "dynamicResult"]);
}

#[test]
fn void_and_dynamic_returns_record_nothing() {
    let source = r#"
void touch() {}
dynamic anything() => 1;
void run() {
  final nothing = touch();
  final loose = anything();
}
"#;
    assert_no_fact(source, &["nothing", "loose"]);
}

#[test]
fn disagreeing_same_named_functions_record_nothing() {
    let source = r#"
Item load() => Item();
Other load() => Other();
void run() {
  final ambiguous = load();
}
"#;
    assert_no_fact(source, &["ambiguous"]);
}

#[test]
fn parameters_and_local_functions_shadow_same_file_functions() {
    let source = r#"
Item load() => Item();
Item make() => Item();
void run(Other Function() load) {
  final fromParameter = load();
}
void other() {
  Other make() => Other();
  final fromLocalFunction = make();
}
"#;
    assert_no_fact(source, &["fromParameter", "fromLocalFunction"]);
}

#[test]
fn class_getters_and_fields_shadow_top_level_functions() {
    let source = r#"
Item load() => Item();
Item make() => Item();
class Repo {
  Other Function() get load => () => Other();
  final Other Function() make = () => Other();
  void run() {
    final viaGetter = load();
    final viaField = make();
  }
}
"#;
    assert_no_fact(source, &["viaGetter", "viaField"]);
}

#[test]
fn receivers_other_than_this_or_a_same_file_type_record_nothing() {
    let source = r#"
class Base {
  Item load() => Item();
}
class Repo extends Base {
  Item find() => Item();
  void run(Repo other) {
    final inherited = this.load();
    final viaSuper = super.load();
    final viaSuperOverride = super.find();
    final viaOther = other.find();
    final viaPrefix = prefix.find();
  }
}
void top() {
  final noClass = this.find();
}
"#;
    assert_no_fact(
        source,
        &[
            "inherited",
            "viaSuper",
            "viaSuperOverride",
            "viaOther",
            "viaPrefix",
            "noClass",
        ],
    );
}

#[test]
fn class_members_are_not_in_scope_outside_their_class() {
    let source = r#"
class Repo {
  Item find() => Item();
}
void run() {
  final outside = find();
}
"#;
    assert_no_fact(source, &["outside"]);
}

#[test]
fn static_callable_field_on_a_same_file_type_records_nothing() {
    let source = r#"
class Repo {
  static final Item Function() build = () => Item();
}
void run() {
  final built = Repo.build();
}
"#;
    assert_no_fact(source, &["built"]);
}

#[test]
fn calls_inside_an_unnamed_extension_record_nothing() {
    let source = r#"
Item pick() => Item();
extension on Repo {
  Other pick() => Other();
  void run() {
    final bare = pick();
    final viaThis = this.pick();
  }
}
"#;
    assert_no_fact(source, &["bare", "viaThis"]);
}

#[test]
fn mixin_members_resolve_inside_the_mixin() {
    let source = r#"
mixin Loader on Base {
  Item pick() => Item();
  void run() {
    final bare = pick();
    final viaThis = this.pick();
  }
}
"#;
    assert_inferred(source, "bare", "Item", None);
    assert_inferred(source, "viaThis", "Item", None);
}

#[test]
fn extension_type_members_resolve_under_the_type_name() {
    let source = r#"
extension type const Id<T>._(T value) {
  Item wrap() => Item();
  static Id<int> zero() => Id._(0);
  void run() {
    final wrapped = this.wrap();
  }
}
void top() {
  final made = Id.zero();
}
"#;
    assert_inferred(source, "wrapped", "Item", None);
    assert_inferred(source, "made", "Id", Some("Id<int>"));
}

#[test]
fn extension_type_parameters_and_representation_record_nothing() {
    let source = r#"
Item value() => Item();
extension type const Id<T>._(T value) {
  T raw() => value;
  void run() {
    final viaThis = this.raw();
    final bare = raw();
    final representation = value();
  }
}
"#;
    assert_no_fact(source, &["viaThis", "bare", "representation"]);
}

#[test]
fn abstract_external_and_covariant_callable_fields_record_nothing() {
    let source = r#"
class Item {}
class Other {}
Item a1() => Item();
Item a2() => Item();
Item c1() => Item();
abstract class R {
  abstract final Other Function() a1;
  external Other Function() a2;
  covariant late final Other Function() c1;
  external static Other Function() e1;
  void go() {
    var abs = a1();
    var ext = a2();
    var cov = c1();
  }
}
void outside() {
  var extStatic = R.e1();
}
"#;
    assert_no_fact(source, &["abs", "ext", "cov", "extStatic"]);
}

#[test]
fn callable_enum_constants_record_nothing() {
    let source = r#"
class Item {}
class Other {}
Item fetch() => Item();
enum E {
  a, fetch;
  Other call() => Other();
  void go() {
    var enumConstBare = fetch();
  }
}
void outside() {
  var enumConstQualified = E.fetch();
}
"#;
    assert_no_fact(source, &["enumConstBare", "enumConstQualified"]);
}
