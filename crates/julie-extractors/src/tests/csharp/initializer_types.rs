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
