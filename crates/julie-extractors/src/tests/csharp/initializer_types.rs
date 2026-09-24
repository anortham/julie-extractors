use crate::base::{SymbolKind, TypeInfo};
use crate::csharp::CSharpExtractor;
use std::path::PathBuf;

fn local_fact(source: &str, local: &str) -> Option<TypeInfo> {
    let mut parser = super::init_test_parser();
    let tree = parser.parse(source, None).unwrap();
    let mut extractor = CSharpExtractor::new(
        "csharp".to_string(),
        "initializer_types.cs".to_string(),
        source.to_string(),
        &PathBuf::from("/tmp/test"),
    );
    let symbols = extractor.extract_symbols(&tree);
    let symbol = symbols
        .iter()
        .find(|s| s.name == local && s.kind == SymbolKind::Variable)
        .unwrap_or_else(|| panic!("missing local {local}"));
    extractor.base.type_info.get(&symbol.id).cloned()
}

fn inferred(source: &str, local: &str) -> Option<(String, Option<String>)> {
    let fact = local_fact(source, local)?;
    assert!(fact.is_inferred, "{local} fact should be inferred");
    let declared = fact
        .metadata
        .as_ref()
        .and_then(|m| m.get("declared"))
        .and_then(|v| v.as_str())
        .map(str::to_string);
    Some((fact.resolved_type, declared))
}

fn resolved(source: &str, local: &str) -> Option<String> {
    inferred(source, local).map(|(resolved, _)| resolved)
}

#[test]
fn simple_name_call_to_same_class_method_infers_its_return_type() {
    let source = r#"
class Service {
  Widget Load() => null;
  void Run() { var widget = Load(); }
}
"#;
    assert_eq!(resolved(source, "widget").as_deref(), Some("Widget"));
}

#[test]
fn this_call_infers_the_enclosing_type_method_return() {
    let source = r#"
class Service {
  Widget Load() => null;
  void Run() { var widget = this.Load(); }
}
"#;
    assert_eq!(resolved(source, "widget").as_deref(), Some("Widget"));
}

#[test]
fn static_call_on_same_file_type_infers_its_return_type() {
    let source = r#"
static class Factory { public static Widget Create(string name) => null; }
class Service {
  void Run() { var widget = Factory.Create("a"); }
}
"#;
    assert_eq!(resolved(source, "widget").as_deref(), Some("Widget"));
}

#[test]
fn static_call_on_generic_type_uses_its_base_name() {
    let source = r#"
class Registry<T> { public static Registry<T> Empty() => null; }
class Service {
  void Run() { var registry = Registry<int>.Empty(); }
}
"#;
    assert_eq!(
        inferred(source, "registry"),
        Some(("Registry".to_string(), Some("Registry<T>".to_string())))
    );
}

#[test]
fn generic_return_with_concrete_base_keeps_declared_text() {
    let source = r#"
class Service {
  List<Widget> All() => null;
  void Run() { var widgets = All(); }
}
"#;
    assert_eq!(
        inferred(source, "widgets"),
        Some(("List".to_string(), Some("List<Widget>".to_string())))
    );
}

#[test]
fn nullable_return_records_the_base_type() {
    let source = r#"
class Service {
  Widget? Find() => null;
  void Run() { var widget = Find(); }
}
"#;
    assert_eq!(
        inferred(source, "widget"),
        Some(("Widget".to_string(), Some("Widget?".to_string())))
    );
}

#[test]
fn await_unwraps_task_of_t() {
    let source = r#"
class Service {
  async Task<Widget> LoadAsync() => null;
  async Task Run() { var widget = await LoadAsync(); }
}
"#;
    assert_eq!(
        inferred(source, "widget"),
        Some(("Widget".to_string(), None))
    );
}

#[test]
fn await_unwraps_value_task_and_qualified_task() {
    let source = r#"
class Service {
  ValueTask<Widget> LoadAsync() => default;
  System.Threading.Tasks.Task<Gadget> FetchAsync() => null;
  async Task Run() {
    var widget = await LoadAsync();
    var gadget = await this.FetchAsync();
  }
}
"#;
    assert_eq!(resolved(source, "widget").as_deref(), Some("Widget"));
    assert_eq!(resolved(source, "gadget").as_deref(), Some("Gadget"));
}

#[test]
fn await_unwraps_through_configure_await() {
    let source = r#"
class Service {
  Task<Widget> LoadAsync() => null;
  async Task Run() { var widget = await LoadAsync().ConfigureAwait(false); }
}
"#;
    assert_eq!(resolved(source, "widget").as_deref(), Some("Widget"));
}

#[test]
fn unawaited_task_call_records_the_task_type() {
    let source = r#"
class Service {
  Task<Widget> LoadAsync() => null;
  void Run() { var pending = LoadAsync(); }
}
"#;
    assert_eq!(
        inferred(source, "pending"),
        Some(("Task".to_string(), Some("Task<Widget>".to_string())))
    );
}

#[test]
fn null_forgiving_and_parentheses_keep_the_type() {
    let source = r#"
class Service {
  Widget? Find() => null;
  void Run() {
    var forgiven = Find()!;
    var wrapped = (Find());
  }
}
"#;
    assert_eq!(resolved(source, "forgiven").as_deref(), Some("Widget"));
    assert_eq!(resolved(source, "wrapped").as_deref(), Some("Widget"));
}

#[test]
fn local_function_in_scope_infers_its_return_type() {
    let source = r#"
class Service {
  void Run() {
    Widget Make() => null;
    var widget = Make();
  }
}
"#;
    assert_eq!(resolved(source, "widget").as_deref(), Some("Widget"));
}

#[test]
fn top_level_local_function_infers_its_return_type() {
    let source = r#"
var widget = Make();
Widget Make() => null;
"#;
    assert_eq!(resolved(source, "widget").as_deref(), Some("Widget"));
}

#[test]
fn overloads_resolve_by_argument_count() {
    let source = r#"
class Service {
  Widget Get() => null;
  Gadget Get(int id) => null;
  Gadget Find(int id, int limit = 10) => null;
  Gadget Many(params int[] ids) => null;
  void Run() {
    var widget = Get();
    var gadget = Get(1);
    var found = Find(1);
    var many = Many(1, 2, 3);
  }
}
"#;
    assert_eq!(resolved(source, "widget").as_deref(), Some("Widget"));
    assert_eq!(resolved(source, "gadget").as_deref(), Some("Gadget"));
    assert_eq!(resolved(source, "found").as_deref(), Some("Gadget"));
    assert_eq!(resolved(source, "many").as_deref(), Some("Gadget"));
}

#[test]
fn nested_type_without_base_reaches_outer_type_methods() {
    let source = r#"
class Outer {
  static Widget Load() => null;
  class Inner {
    void Run() { var widget = Load(); }
  }
}
"#;
    assert_eq!(resolved(source, "widget").as_deref(), Some("Widget"));
}

#[test]
fn using_declaration_infers_the_call_type() {
    let source = r#"
class Service {
  Stream Open() => null;
  void Run() { using var stream = Open(); }
}
"#;
    assert_eq!(resolved(source, "stream").as_deref(), Some("Stream"));
}

#[test]
fn written_type_wins_over_the_call_type() {
    let source = r#"
class Service {
  Widget Load() => null;
  void Run() { IWidget widget = Load(); }
}
"#;
    let fact = local_fact(source, "widget").expect("declared fact");
    assert_eq!(fact.resolved_type, "IWidget");
    assert!(!fact.is_inferred);
}

#[test]
fn call_to_method_in_another_file_records_nothing() {
    let source = r#"
class Service {
  void Run() { var widget = Load(); }
}
"#;
    assert_eq!(inferred(source, "widget"), None);
}

#[test]
fn disagreeing_same_arity_overloads_record_nothing() {
    let source = r#"
class Service {
  Widget Get(int id) => null;
  Gadget Get(string key) => null;
  void Run() { var item = Get(1); }
}
"#;
    assert_eq!(inferred(source, "item"), None);
}

#[test]
fn method_type_parameter_return_records_nothing() {
    let source = r#"
class Service {
  T Get<T>() => default;
  void Run() { var item = Get<Widget>(); }
}
"#;
    assert_eq!(inferred(source, "item"), None);
}

#[test]
fn class_type_parameter_return_records_nothing() {
    let source = r#"
class Box<T> {
  T Value() => default;
  T[] Values() => null;
  void Run() {
    var item = Value();
    var items = Values();
  }
}
"#;
    assert_eq!(inferred(source, "item"), None);
    assert_eq!(inferred(source, "items"), None);
}

#[test]
fn awaiting_a_non_generic_task_or_other_type_records_nothing() {
    let source = r#"
class Service {
  Task RunAsync() => null;
  Widget Load() => null;
  Lazy<Widget> Later() => null;
  async Task Run() {
    var done = await RunAsync();
    var widget = await Load();
    var later = await Later();
  }
}
"#;
    assert_eq!(inferred(source, "done"), None);
    assert_eq!(inferred(source, "widget"), None);
    assert_eq!(inferred(source, "later"), None);
}

#[test]
fn configure_await_without_await_records_nothing() {
    let source = r#"
class Service {
  Task<Widget> LoadAsync() => null;
  void Run() { var configured = LoadAsync().ConfigureAwait(false); }
}
"#;
    assert_eq!(inferred(source, "configured"), None);
}

#[test]
fn call_on_a_variable_or_chained_call_records_nothing() {
    let source = r#"
class Service {
  Widget Load() => null;
  Builder Builder() => null;
  void Run(Service other) {
    var widget = other.Load();
    var built = this.Builder().Load();
  }
}
"#;
    assert_eq!(inferred(source, "widget"), None);
    assert_eq!(inferred(source, "built"), None);
}

#[test]
fn this_call_only_sees_the_innermost_type() {
    let source = r#"
class Outer {
  Widget Load() => null;
  class Inner {
    void Run() { var widget = this.Load(); }
  }
}
"#;
    assert_eq!(inferred(source, "widget"), None);
}

#[test]
fn base_call_records_nothing() {
    let source = r#"
class Service : BaseService {
  Widget Load() => null;
  void Run() { var widget = base.Load(); }
}
"#;
    assert_eq!(inferred(source, "widget"), None);
}

#[test]
fn type_call_to_instance_method_records_nothing() {
    let source = r#"
class Factory { public Widget Create() => null; }
class Service {
  void Run() { var widget = Factory.Create(); }
}
"#;
    assert_eq!(inferred(source, "widget"), None);
}

#[test]
fn explicit_interface_implementation_is_not_a_simple_name_target() {
    let source = r#"
class Service : ILoader {
  Widget ILoader.Load() => null;
  void Run() { var widget = Load(); }
}
"#;
    assert_eq!(inferred(source, "widget"), None);
}

#[test]
fn type_with_base_list_stops_the_outer_search() {
    let source = r#"
class Outer {
  static Widget Load() => null;
  class Inner : Base {
    void Run() { var widget = Load(); }
  }
}
"#;
    assert_eq!(inferred(source, "widget"), None);
}

#[test]
fn partial_type_stops_the_outer_search() {
    let source = r#"
class Outer {
  static Widget Load() => null;
  partial class Inner {
    void Run() { var widget = Load(); }
  }
}
"#;
    assert_eq!(inferred(source, "widget"), None);
}

#[test]
fn argument_count_no_overload_accepts_records_nothing() {
    let source = r#"
class Service {
  Widget Load() => null;
  void Run() { var widget = Load(1); }
}
"#;
    assert_eq!(inferred(source, "widget"), None);
}

#[test]
fn local_function_out_of_scope_records_nothing() {
    let source = r#"
class Service {
  void Setup() { Widget Make() => null; }
  void Run() { var widget = Make(); }
}
"#;
    assert_eq!(inferred(source, "widget"), None);
}

#[test]
fn local_function_disagreeing_with_a_member_records_nothing() {
    let source = r#"
class Service {
  Gadget Make() => null;
  void Run() {
    Widget Make() => null;
    var item = Make();
  }
}
"#;
    assert_eq!(inferred(source, "item"), None);
}

#[test]
fn void_and_tuple_returns_record_nothing() {
    let source = r#"
class Service {
  void Reset() { }
  (Widget, Gadget) Pair() => default;
  void Run() {
    var reset = Reset();
    var pair = Pair();
  }
}
"#;
    assert_eq!(inferred(source, "reset"), None);
    assert_eq!(inferred(source, "pair"), None);
}

#[test]
fn same_named_nested_type_elsewhere_does_not_lend_its_members() {
    let source = r#"
class Outer {
  static Gadget Load() => null;
  class Node {
    void Run() { var item = Load(); }
  }
}
class Other {
  class Node { Widget Load() => null; }
}
"#;
    assert_eq!(resolved(source, "item").as_deref(), Some("Gadget"));
}

#[test]
fn static_call_on_disagreeing_same_named_types_records_nothing() {
    let source = r#"
namespace First { static class Factory { public static Widget Create() => null; } }
namespace Second { static class Factory { public static Gadget Create() => null; } }
class Service {
  void Run() { var item = Factory.Create(); }
}
"#;
    assert_eq!(inferred(source, "item"), None);
}

#[test]
fn local_bindings_named_like_the_method_record_nothing() {
    let source = r#"
class Shadows {
  int Load() => 1;
  void Local() { Func<string> Load = () => ""; var local = Load(); }
  void Cast() { var Load = (Func<string>)(() => ""); var cast = Load(); }
  void Parameter(Func<string> Load) { var parameter = Load(); }
  void Each(List<Func<string>> list) { foreach (var Load in list) { var each = Load(); } }
  void Lambda() { Run(Load => { var lambda = Load(); }); }
  void Pattern(object o) { if (o is Func<string> Load) { var pattern = Load(); } }
  void Out() { Get(out Func<string> Load); var output = Load(); }
  void Control() { var control = Load(); }
}
"#;
    for local in [
        "local",
        "cast",
        "parameter",
        "each",
        "lambda",
        "pattern",
        "output",
    ] {
        assert_eq!(inferred(source, local), None, "{local}");
    }
    assert_eq!(resolved(source, "control").as_deref(), Some("int"));
}

#[test]
fn nested_type_members_named_like_the_outer_method_record_nothing() {
    let source = r#"
class Outer {
  int Load() => 1;
  class WithProperty { Func<string> Load { get; } = null; void M() { var property = Load(); } }
  class WithField { Func<string> Load = null; void M() { var field = Load(); } }
  class WithEvent { event Func<string> Load; void M() { var evt = Load(); } }
  class WithPrimary(Func<string> Load) { void M() { var primary = Load(); } }
  class Middle { Func<string> Load = null; class Deep { void M() { var deep = Load(); } } }
  class Plain { void M() { var control = Load(); } }
}
"#;
    for local in ["property", "field", "evt", "primary", "deep"] {
        assert_eq!(inferred(source, local), None, "{local}");
    }
    assert_eq!(resolved(source, "control").as_deref(), Some("int"));
}

#[test]
fn object_member_names_do_not_reach_the_outer_type() {
    let source = r#"
class Widget {}
class Outer {
  public static Widget Equals(int a, int b) => null;
  public static Widget Make() => null;
  class Inner {
    void M() {
      var inherited = Equals(1, 2);
      var control = Make();
    }
  }
}
"#;
    assert_eq!(inferred(source, "inherited"), None);
    assert_eq!(resolved(source, "control").as_deref(), Some("Widget"));
}

#[test]
fn type_receiver_named_like_a_binding_records_nothing() {
    let source = r#"
class Stat { public static int Create() => 1; }
class Wrapper { public string Create() => ""; }
class FieldReceiver { Wrapper Stat = new Wrapper(); void M() { var field = Stat.Create(); } }
class PropertyReceiver { Wrapper Stat { get; } void M() { var property = Stat.Create(); } }
class ParameterReceiver { void M(Wrapper Stat) { var parameter = Stat.Create(); } }
class LocalReceiver { void M() { var Stat = new Wrapper(); var local = Stat.Create(); } }
class Plain { void M() { var control = Stat.Create(); } }
"#;
    for local in ["field", "property", "parameter", "local"] {
        assert_eq!(inferred(source, local), None, "{local}");
    }
    assert_eq!(resolved(source, "control").as_deref(), Some("int"));
}

#[test]
fn static_call_on_a_type_nested_in_another_type_records_nothing() {
    let source = r#"
class Outer { static class Helpers { public static int Make() => 1; } }
class Other { void M() { var n2 = Helpers.Make(); } }
"#;
    assert_eq!(inferred(source, "n2"), None);
}

#[test]
fn static_call_receiver_bound_to_a_nested_type_without_the_method_records_nothing() {
    let source = r#"
class Maker { public static int Make() => 1; }
class Svc {
  class Maker : Lib.MakerBase { }
  void M() { var m4 = Maker.Make(); }
}
"#;
    assert_eq!(inferred(source, "m4"), None);
}

#[test]
fn static_call_on_a_type_in_an_unrelated_namespace_records_nothing() {
    let source = r#"
namespace A { static class Factory { public static int Create() => 1; } }
namespace B { class User { void M() { var n1 = Factory.Create(); } } }
"#;
    assert_eq!(inferred(source, "n1"), None);
}

#[test]
fn static_call_resolves_through_nesting_and_namespace_scopes() {
    let source = r#"
namespace A {
  static class Factory { public static Widget Create() => null; }
  namespace B {
    static class Factory { public static Gadget Create() => null; }
    class User { void M() { var inner = Factory.Create(); } }
  }
  class Svc {
    static class Factory { public static Part Create() => null; }
    void M() { var nested = Factory.Create(); }
  }
  class Plain { void M() { var outer = Factory.Create(); } }
}
static class Registry { public static Widget Open() => null; }
namespace C.D { class User { void M() { var global = Registry.Open(); } } }
"#;
    assert_eq!(resolved(source, "inner").as_deref(), Some("Gadget"));
    assert_eq!(resolved(source, "nested").as_deref(), Some("Part"));
    assert_eq!(resolved(source, "outer").as_deref(), Some("Widget"));
    assert_eq!(resolved(source, "global").as_deref(), Some("Widget"));
}

#[test]
fn static_call_sees_types_of_a_file_scoped_namespace() {
    let source = r#"
namespace A.B;
static class Factory { public static Widget Create() => null; }
class User { void M() { var widget = Factory.Create(); } }
"#;
    assert_eq!(resolved(source, "widget").as_deref(), Some("Widget"));
}

#[test]
fn static_call_receiver_matches_the_type_argument_count() {
    let source = r#"
class Factory<T> { public static Gadget Create() => null; }
class Factory { public static Widget Create() => null; }
class User { void M() { var plain = Factory.Create(); var generic = Factory<int>.Create(); } }
"#;
    assert_eq!(resolved(source, "plain").as_deref(), Some("Widget"));
    assert_eq!(resolved(source, "generic").as_deref(), Some("Gadget"));
}

#[test]
fn static_call_receiver_named_like_a_type_parameter_records_nothing() {
    let source = r#"
class Factory { public static Widget Create() => null; }
class User<Factory> { void M() { var item = Factory.Create(); } }
"#;
    assert_eq!(inferred(source, "item"), None);
}

#[test]
fn call_with_arguments_into_a_type_with_a_base_list_records_nothing() {
    let source = r#"
class Base { protected string Load(string s) => ""; }
class D : Base {
  int Load(int x) => 1;
  void M() { var m1 = Load("s"); var m2 = this.Load("s"); }
}
"#;
    assert_eq!(inferred(source, "m1"), None);
    assert_eq!(inferred(source, "m2"), None);
}

#[test]
fn static_call_with_arguments_into_an_open_type_records_nothing() {
    let source = r#"
class Base { public static string Create(string s) => ""; }
class Factory : Base { public static int Create(int x) => 1; }
partial class Pp { public static int Build(int x) => 1; }
class User { void M() { var m3 = Factory.Create("s"); var m5 = Pp.Build("s"); } }
"#;
    assert_eq!(inferred(source, "m3"), None);
    assert_eq!(inferred(source, "m5"), None);
}

#[test]
fn parameterless_call_into_an_open_type_infers_its_return_type() {
    let source = r#"
class D : Base {
  Widget Load() => null;
  public static Widget Make() => null;
  void M() { var simple = Load(); var self = this.Load(); }
}
class User { void M() { var type = D.Make(); } }
"#;
    assert_eq!(resolved(source, "simple").as_deref(), Some("Widget"));
    assert_eq!(resolved(source, "self").as_deref(), Some("Widget"));
    assert_eq!(resolved(source, "type").as_deref(), Some("Widget"));
}

#[test]
fn open_type_candidates_with_optional_params_or_type_parameters_record_nothing() {
    let source = r#"
partial class D {
  Widget Optional(int x = 0) => null;
  Widget Many(params int[] xs) => null;
  Widget Generic<T>() => null;
  void M() { var optional = Optional(); var many = Many(); var generic = Generic(); }
}
"#;
    assert_eq!(inferred(source, "optional"), None);
    assert_eq!(inferred(source, "many"), None);
    assert_eq!(inferred(source, "generic"), None);
}

#[test]
fn local_function_with_arguments_inside_an_open_type_infers_its_return_type() {
    let source = r#"
class D : Base {
  void M() {
    Widget Make(int x) => null;
    var widget = Make(1);
  }
}
"#;
    assert_eq!(resolved(source, "widget").as_deref(), Some("Widget"));
}

#[test]
fn static_receiver_bound_by_a_using_alias_in_a_namespace_block_records_nothing() {
    let source = r#"
namespace Lib { class Other { public static string Create() => ""; } }
namespace A { using Factory = Lib.Other; class C { void M() { var aliasX = Factory.Create(); } } }
class Factory { public static int Create() => 1; }
"#;
    assert_eq!(inferred(source, "aliasX"), None);
}

#[test]
fn static_receiver_past_a_using_namespace_in_a_namespace_block_records_nothing() {
    let source = r#"
namespace Lib { static class Tool { public static string Make() => ""; } }
namespace App { using Lib; class C { void M() { var usingX = Tool.Make(); } } }
static class Tool { public static int Make() => 1; }
"#;
    assert_eq!(inferred(source, "usingX"), None);
}

#[test]
fn static_receiver_past_a_using_static_in_a_namespace_block_records_nothing() {
    let source = r#"
namespace Lib { static class Holder { public static class Tool { public static string Make() => ""; } } }
namespace App { using static Lib.Holder; class C { void M() { var staticX = Tool.Make(); } } }
static class Tool { public static int Make() => 1; }
"#;
    assert_eq!(inferred(source, "staticX"), None);
}

#[test]
fn static_receiver_in_the_namespace_block_binds_before_its_usings() {
    let source = r#"
namespace App {
  using Lib;
  using Tool = Lib.Other;
  static class Tool { public static Widget Make() => null; }
  class C { void M() { var widget = Tool.Make(); } }
}
"#;
    assert_eq!(resolved(source, "widget").as_deref(), Some("Widget"));
}

#[test]
fn static_receiver_inside_a_derived_type_records_nothing() {
    let source = r#"
class Base { protected class Maker { public static string Create() => ""; } }
class Maker { public static int Create() => 1; }
class Widget { public string Create() => ""; }
class Base2 { protected Widget Builder = new Widget(); }
class Builder { public static int Create() => 1; }
class C : Base { void M() { var inhNested = Maker.Create(); } }
class D : Base2 { void M() { var inhField = Builder.Create(); } }
"#;
    assert_eq!(inferred(source, "inhNested"), None);
    assert_eq!(inferred(source, "inhField"), None);
}

#[test]
fn static_receiver_nested_in_the_open_type_itself_infers_its_return_type() {
    let source = r#"
class C : Base {
  static class Maker { public static Widget Create() => null; }
  void M() { var widget = Maker.Create(); }
}
"#;
    assert_eq!(resolved(source, "widget").as_deref(), Some("Widget"));
}

#[test]
fn return_type_naming_a_nested_type_records_nothing_outside_its_declaring_type() {
    let source = r#"
namespace N {
  class Node { public int A; }
  static class Factory { public class Node { } public static Node Create() => new Node(); }
  class Other { void M() { var nestedRet = Factory.Create(); } }
}
"#;
    assert_eq!(inferred(source, "nestedRet"), None);
}

#[test]
fn outer_method_returning_a_nested_type_records_nothing_from_an_inner_type() {
    let source = r#"
class Outer {
  class Node { }
  static Node Make() => null;
  void Own() { var own = Make(); }
  class Inner {
    class Node { }
    void M() { var innerRet = Make(); }
  }
}
"#;
    assert_eq!(inferred(source, "innerRet"), None);
    assert_eq!(resolved(source, "own").as_deref(), Some("Node"));
}

#[test]
fn ref_returns_record_the_referenced_type() {
    let source = r#"
class R {
  int _v;
  ref int Get() => ref _v;
  ref readonly int GetRo() => ref _v;
  void M() { var refX = Get(); var refRo = GetRo(); }
}
"#;
    assert_eq!(inferred(source, "refX"), Some(("int".to_string(), None)));
    assert_eq!(inferred(source, "refRo"), Some(("int".to_string(), None)));
}
