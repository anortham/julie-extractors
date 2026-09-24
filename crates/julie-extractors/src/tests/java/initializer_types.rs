use crate::java::JavaExtractor;
use std::path::PathBuf;

fn inferred_type(source: &str, local: &str) -> Option<(String, bool)> {
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_java::LANGUAGE.into())
        .unwrap();
    let tree = parser.parse(source, None).unwrap();
    let mut extractor = JavaExtractor::new(
        "java".to_string(),
        "InitializerTypes.java".to_string(),
        source.to_string(),
        &PathBuf::from("/tmp/test"),
    );
    let symbols = extractor.extract_symbols(&tree);
    let local = symbols
        .iter()
        .find(|s| s.name == local)
        .unwrap_or_else(|| panic!("missing local {local}"));
    extractor
        .base
        .type_info
        .get(&local.id)
        .map(|fact| (fact.resolved_type.clone(), fact.is_inferred))
}

fn in_service(members: &str, body: &str) -> String {
    format!(
        "class Service {{\n  {members}\n  void run(String input) throws Exception {{\n    {body}\n  }}\n}}\n\
         class Registry {{\n  static Workspace open(String input) {{ return null; }}\n  \
         static Workspace[] openAll() {{ return null; }}\n}}\n"
    )
}

const LOADERS: &str = "Workspace load(String input) { return null; }\n  \
     static Workspace create() { return null; }\n  \
     List<Workspace> loadAll() { return null; }\n  \
     void reset() {}";

fn workspace_type(body: &str) -> Option<(String, bool)> {
    inferred_type(&in_service(LOADERS, body), "workspace")
}

fn inferred(name: &str) -> Option<(String, bool)> {
    Some((name.to_string(), true))
}

#[test]
fn var_from_unqualified_same_class_call_records_return_type() {
    assert_eq!(
        workspace_type("var workspace = load(input);"),
        inferred("Workspace")
    );
}

#[test]
fn var_from_this_call_records_return_type() {
    assert_eq!(
        workspace_type("var workspace = this.load(input);"),
        inferred("Workspace")
    );
}

#[test]
fn var_from_static_call_on_same_file_type_records_return_type() {
    assert_eq!(
        workspace_type("var workspace = Registry.open(input);"),
        inferred("Workspace")
    );
    assert_eq!(
        workspace_type("var workspace = Service.create();"),
        inferred("Workspace")
    );
}

#[test]
fn generic_return_type_records_its_base_name() {
    assert_eq!(
        workspace_type("var workspace = loadAll();"),
        inferred("List")
    );
}

#[test]
fn array_return_type_records_array_text() {
    assert_eq!(
        workspace_type("var workspace = Registry.openAll();"),
        inferred("Workspace[]")
    );
}

#[test]
fn var_resource_from_same_class_call_records_return_type() {
    assert_eq!(
        workspace_type("try (var workspace = load(input)) {}"),
        inferred("Workspace")
    );
}

#[test]
fn written_type_wins_over_call_inference() {
    assert_eq!(
        workspace_type("Project workspace = load(input);"),
        Some(("Project".to_string(), false))
    );
}

#[test]
fn var_new_expression_still_records_constructed_type() {
    assert_eq!(
        workspace_type("var workspace = new Workspace();"),
        inferred("Workspace")
    );
}

#[test]
fn void_call_records_no_fact() {
    assert_eq!(workspace_type("var workspace = reset();"), None);
}

#[test]
fn call_to_undeclared_method_records_no_fact() {
    assert_eq!(workspace_type("var workspace = fetch(input);"), None);
}

#[test]
fn call_on_other_receiver_records_no_fact() {
    assert_eq!(workspace_type("var workspace = loader.load(input);"), None);
    assert_eq!(
        workspace_type("var workspace = this.loader.load(input);"),
        None
    );
}

#[test]
fn chained_call_records_no_fact() {
    assert_eq!(
        workspace_type("var workspace = load(input).normalize();"),
        None
    );
    assert_eq!(workspace_type("var workspace = create().loadAll();"), None);
}

#[test]
fn super_call_records_no_fact() {
    assert_eq!(workspace_type("var workspace = super.load(input);"), None);
}

#[test]
fn static_call_on_type_without_that_method_records_no_fact() {
    assert_eq!(
        workspace_type("var workspace = Registry.load(input);"),
        None
    );
}

#[test]
fn static_call_on_other_file_type_records_no_fact() {
    assert_eq!(workspace_type("var workspace = Paths.open(input);"), None);
}

#[test]
fn call_with_no_arity_match_records_no_fact() {
    assert_eq!(workspace_type("var workspace = load();"), None);
}

#[test]
fn variadic_method_accepts_extra_arguments() {
    let members = "Workspace pick(String first, String... rest) { return null; }";
    assert_eq!(
        inferred_type(
            &in_service(members, "var workspace = pick(input, input, input);"),
            "workspace"
        ),
        inferred("Workspace")
    );
    assert_eq!(
        inferred_type(&in_service(members, "var workspace = pick();"), "workspace"),
        None
    );
}

#[test]
fn overloads_that_agree_record_the_shared_type() {
    let members =
        "Workspace load() { return null; }\n  Workspace load(String input) { return null; }";
    assert_eq!(
        inferred_type(&in_service(members, "var workspace = load();"), "workspace"),
        inferred("Workspace")
    );
}

#[test]
fn overloads_that_disagree_record_no_fact() {
    let members = "Workspace load(String input) { return null; }\n  Project load(Object input) { return null; }";
    assert_eq!(
        inferred_type(
            &in_service(members, "var workspace = load(input);"),
            "workspace"
        ),
        None
    );
}

#[test]
fn same_named_types_that_disagree_record_no_fact() {
    let source = "class Outer { static class Box { static Workspace make() { return null; } } }\n\
                  class Other { static class Box { static Project make() { return null; } } }\n\
                  class Service { void run() { var workspace = Box.make(); } }\n";
    assert_eq!(inferred_type(source, "workspace"), None);
}

#[test]
fn generic_method_return_records_no_fact() {
    let members = "<T> T load(String input) { return null; }";
    assert_eq!(
        inferred_type(
            &in_service(members, "var workspace = load(input);"),
            "workspace"
        ),
        None
    );
}

#[test]
fn class_type_parameter_return_records_no_fact() {
    let source = "class Holder<T> {\n  T get() { return null; }\n  T[] all() { return null; }\n  \
                  void run() { var workspace = this.get(); var every = all(); }\n}\n";
    assert_eq!(inferred_type(source, "workspace"), None);
    assert_eq!(inferred_type(source, "every"), None);
}

#[test]
fn outer_class_type_parameter_return_records_no_fact() {
    let source = "class Holder<T> {\n  class Inner {\n    T get() { return null; }\n    \
                  void run() { var workspace = get(); }\n  }\n}\n";
    assert_eq!(inferred_type(source, "workspace"), None);
}

#[test]
fn call_inside_nested_class_uses_only_the_innermost_class() {
    let source = "class Service {\n  Workspace load() { return null; }\n  class Inner {\n    \
                  void run() { var workspace = load(); }\n  }\n}\n";
    assert_eq!(inferred_type(source, "workspace"), None);
}

#[test]
fn call_inside_anonymous_class_records_no_fact() {
    let source = "class Service {\n  Workspace load() { return null; }\n  \
                  Runnable task = new Runnable() {\n    \
                  Workspace load() { return null; }\n    \
                  public void run() { var workspace = this.load(); }\n  };\n}\n";
    assert_eq!(inferred_type(source, "workspace"), None);
}

#[test]
fn old_style_array_return_records_no_fact() {
    let members = "Workspace load(String input)[] { return null; }";
    assert_eq!(
        inferred_type(
            &in_service(members, "var workspace = load(input);"),
            "workspace"
        ),
        None
    );
}

#[test]
fn enum_and_interface_methods_are_indexed() {
    let source = "enum Mode {\n  FAST;\n  Workspace load() { return null; }\n  \
                  void run() { var workspace = load(); }\n}\n\
                  interface Api {\n  static Project make() { return null; }\n  \
                  default void run() { var project = Api.make(); }\n}\n";
    assert_eq!(inferred_type(source, "workspace"), inferred("Workspace"));
    assert_eq!(inferred_type(source, "project"), inferred("Project"));
}

#[test]
fn type_qualified_call_to_instance_method_records_no_fact() {
    let source = "class Loader {\n  Workspace load() { return null; }\n}\n\
                  class Service {\n  Project Loader;\n  \
                  void run() { var workspace = Loader.load(); }\n}\n";
    assert_eq!(inferred_type(source, "workspace"), None);
    let inherited_field = "class Loader {\n  Workspace load() { return null; }\n}\n\
                           class Service extends Base {\n  \
                           void run() { var workspace = Loader.load(); }\n}\n";
    assert_eq!(inferred_type(inherited_field, "workspace"), None);
}

#[test]
fn unqualified_call_ignores_same_named_class_elsewhere_in_file() {
    let nested = "class A { static class Helper { Foo load() { return null; } } }\n\
                  class B {\n  Bar load() { return null; }\n  \
                  class Helper { void run() { var nested1 = load(); } }\n}\n";
    assert_eq!(inferred_type(nested, "nested1"), None);
    let local = "class C {\n  Foo load() { return null; }\n  \
                 void m1() { class Helper { Bar load() { return null; } } }\n  \
                 void m2() { class Helper { void run() { var local1 = load(); } } }\n}\n";
    assert_eq!(inferred_type(local, "local1"), None);
}

#[test]
fn unqualified_call_in_local_class_resolves_in_that_class() {
    let source = "class Service {\n  Project load() { return null; }\n  \
                  void run() {\n    class Local {\n      Workspace load() { return null; }\n      \
                  void go() { var workspace = load(); }\n    }\n  }\n}\n";
    assert_eq!(inferred_type(source, "workspace"), inferred("Workspace"));
}

#[test]
fn static_call_on_type_name_that_is_not_in_scope_records_no_fact() {
    let source = "class Outer2 { static class Helper { static Foo make() { return null; } } }\n\
                  class Other2 {\n  static class Helper extends Ext {}\n  \
                  void run() { var stat1 = Helper.make(); }\n}\n";
    assert_eq!(inferred_type(source, "stat1"), None);
}

#[test]
fn static_call_on_member_type_of_enclosing_class_records_return_type() {
    let source = "class Service {\n  static class Box { static Workspace make() { return null; } }\n  \
                  void run() { var workspace = Box.make(); }\n}\n";
    assert_eq!(inferred_type(source, "workspace"), inferred("Workspace"));
}

#[test]
fn same_arity_overload_in_same_file_superclass_records_no_fact() {
    let source = "class Base { Bar load(String s) { return null; } }\n\
                  class Sub extends Base {\n  Foo load(int i) { return null; }\n  \
                  void run() { var ov1 = load(\"s\"); var ov2 = this.load(\"s\"); }\n}\n";
    assert_eq!(inferred_type(source, "ov1"), None);
    assert_eq!(inferred_type(source, "ov2"), None);
}

#[test]
fn same_arity_static_overload_in_same_file_supertype_records_no_fact() {
    let source = "interface Base { static Bar make(String s) { return null; } }\n\
                  class Sub implements Base {\n  static Foo make(int i) { return null; }\n}\n\
                  class Service { void run() { var workspace = Sub.make(1); } }\n";
    assert_eq!(inferred_type(source, "workspace"), None);
}

#[test]
fn same_file_supertype_overload_that_agrees_keeps_the_fact() {
    let source = "class Base { Workspace load(String s) { return null; } }\n\
                  class Sub extends Base {\n  Workspace load(int i) { return null; }\n  \
                  void run() { var workspace = load(1); }\n}\n";
    assert_eq!(inferred_type(source, "workspace"), inferred("Workspace"));
}

#[test]
fn inherited_method_not_declared_in_the_class_records_no_fact() {
    let source = "class Base { Workspace load() { return null; } }\n\
                  class Sub extends Base { void run() { var workspace = load(); } }\n";
    assert_eq!(inferred_type(source, "workspace"), None);
}

#[test]
fn qualifier_bound_by_a_variable_records_no_fact() {
    let local = "class M { static Foo make() { return null; } }\n\
                 class Shadow { void run() { Other M = null; var shadow1 = M.make(); } }\n";
    assert_eq!(inferred_type(local, "shadow1"), None);
    let parameter = "class M { static Foo make() { return null; } }\n\
                     class Shadow { void run(Other M) { var shadow2 = M.make(); } }\n";
    assert_eq!(inferred_type(parameter, "shadow2"), None);
    let lambda = "class M { static Foo make() { return null; } }\n\
                  class Shadow { Object f = M -> { var shadow3 = M.make(); return null; }; }\n";
    assert_eq!(inferred_type(lambda, "shadow3"), None);
    let pattern = "class M { static Foo make() { return null; } }\n\
                   class Shadow { void run(Object o) {\n    \
                   if (o instanceof Other M) { var shadow4 = M.make(); }\n  } }\n";
    assert_eq!(inferred_type(pattern, "shadow4"), None);
    let constant = "class M { static Foo make() { return null; } }\n\
                    enum Shadow { M; void run() { var shadow5 = M.make(); } }\n";
    assert_eq!(inferred_type(constant, "shadow5"), None);
}

#[test]
fn call_inside_enum_constant_body_records_no_fact() {
    let source = "class X { Project load() { return null; } }\n\
                  enum E {\n  X {\n    Workspace load() { return null; }\n    \
                  void f() { var workspace = this.load(); }\n  };\n}\n";
    assert_eq!(inferred_type(source, "workspace"), None);
}

#[test]
fn record_component_accessor_records_component_type() {
    let source = "record Point(Workspace workspace, int[] cells) {\n  \
                  void f() { var copy = workspace(); var grid = this.cells(); }\n}\n";
    assert_eq!(inferred_type(source, "copy"), inferred("Workspace"));
    assert_eq!(inferred_type(source, "grid"), inferred("int[]"));
}

#[test]
fn record_type_parameter_accessor_records_no_fact() {
    let source = "record Rec<R>(R value) {\n  void run() { var rec2 = value(); }\n}\n";
    assert_eq!(inferred_type(source, "rec2"), None);
}

#[test]
fn enum_values_and_value_of_record_enum_types() {
    let source = "enum Color { RED }\n\
                  class Service { void run(String s) {\n    \
                  var all = Color.values(); var one = Color.valueOf(s); var none = Color.valueOf();\n  } }\n";
    assert_eq!(inferred_type(source, "all"), inferred("Color[]"));
    assert_eq!(inferred_type(source, "one"), inferred("Color"));
    assert_eq!(inferred_type(source, "none"), None);
}

#[test]
fn static_call_on_member_type_of_unrelated_class_records_no_fact() {
    let source = "class Outer { static class Box { static Workspace make() { return null; } } }\n\
                  class Service { void run() { var workspace = Box.make(); } }\n";
    assert_eq!(inferred_type(source, "workspace"), None);
}

#[test]
fn inherited_member_type_that_hides_top_level_type_records_no_fact() {
    let source = "class Box { static Workspace make() { return null; } }\n\
                  class Base { static class Box { static Project make() { return null; } } }\n\
                  class Sub extends Base { void run() { var workspace = Box.make(); } }\n";
    assert_eq!(inferred_type(source, "workspace"), None);
}

#[test]
fn local_class_declared_after_the_call_records_no_fact() {
    let after = "class Workspace {}\npublic class Main {\n  static void f() {\n    \
                 var wrongLocalAfter = String.valueOf(1);\n    \
                 java.lang.String check = wrongLocalAfter;\n    \
                 class String { static Workspace valueOf(int x) { return null; } }\n  }\n}\n";
    assert_eq!(inferred_type(after, "wrongLocalAfter"), None);
    let record_after = "class Workspace {}\npublic class Main {\n  static void f() {\n    \
                        var wrongRecordAfter = Helper.make();\n    \
                        record Helper() { static Workspace make() { return null; } }\n  }\n}\n";
    assert_eq!(inferred_type(record_after, "wrongRecordAfter"), None);
    let before = "class Workspace {}\npublic class Main {\n  static void f() {\n    \
                  class Helper { static Workspace make() { return null; } }\n    \
                  var localBefore = Helper.make();\n  }\n}\n";
    assert_eq!(inferred_type(before, "localBefore"), inferred("Workspace"));
}

#[test]
fn statically_imported_field_named_like_the_qualifier_records_no_fact() {
    let single = "import static java.lang.System.out;\nclass Workspace {}\n\
                  class out { static Workspace checkError() { return null; } }\n\
                  public class Main { static void f() { var wrongStaticImport = out.checkError(); } }\n";
    assert_eq!(inferred_type(single, "wrongStaticImport"), None);
    let on_demand = "import static java.util.Locale.*;\nclass Workspace {}\n\
                     class US { static Workspace getLanguage() { return null; } }\n\
                     public class Main { static void f() { var wrongOnDemand = US.getLanguage(); } }\n";
    assert_eq!(inferred_type(on_demand, "wrongOnDemand"), None);
    let unrelated = "import static java.lang.System.err;\nimport java.util.*;\nclass Workspace {}\n\
                     class US { static Workspace getLanguage() { return null; } }\n\
                     public class Main { static void f() { var unrelatedImport = US.getLanguage(); } }\n";
    assert_eq!(
        inferred_type(unrelated, "unrelatedImport"),
        inferred("Workspace")
    );
}

#[test]
fn on_demand_static_import_keeps_unqualified_calls() {
    let source = "import static java.util.Locale.*;\n\
                  class Service {\n  Workspace load() { return null; }\n  \
                  void run() { var workspace = load(); }\n}\n";
    assert_eq!(inferred_type(source, "workspace"), inferred("Workspace"));
}

#[test]
fn return_type_name_that_means_another_type_at_the_call_records_no_fact() {
    let source = "public class Main {\n  static class Inner { int outerMarker; }\n  \
                  static class Nested {\n    static class Inner { int nestedMarker; }\n    \
                  static Inner make() { return new Inner(); }\n  }\n  \
                  static void f() { var scopeShift = Nested.make(); }\n}\n";
    assert_eq!(inferred_type(source, "scopeShift"), None);
    let member_out_of_scope = "public class Main {\n  static class Nested {\n    \
                               static class Inner {}\n    static Inner make() { return null; }\n  }\n  \
                               static void f() { var memberOutOfScope = Nested.make(); }\n}\n";
    assert_eq!(inferred_type(member_out_of_scope, "memberOutOfScope"), None);
    let local_shadow = "public class Main {\n  static Workspace load() { return null; }\n  \
                        static void f() {\n    class Workspace {}\n    \
                        var localShadow = load();\n  }\n}\n";
    assert_eq!(inferred_type(local_shadow, "localShadow"), None);
    let type_parameter = "public class Main {\n  static Workspace load() { return null; }\n  \
                          static <Workspace> void f() { var typeParameter = load(); }\n}\n";
    assert_eq!(inferred_type(type_parameter, "typeParameter"), None);
    let shared = "public class Main {\n  static class Inner {}\n  \
                  static class Nested { static Inner make() { return null; } }\n  \
                  static void f() { var sharedScope = Nested.make(); }\n}\n";
    assert_eq!(inferred_type(shared, "sharedScope"), inferred("Inner"));
}

#[test]
fn overload_of_an_object_method_records_no_fact() {
    let equals = "class Workspace {}\npublic class Main {\n  \
                  static Workspace equals(String s) { return null; }\n  \
                  void f(Object o) { var objectOverload = equals(o); }\n}\n";
    assert_eq!(inferred_type(equals, "objectOverload"), None);
    let wait = "class Workspace {}\npublic class Main {\n  \
                static Workspace wait(int x) { return null; }\n  \
                void f() { var waitOverload = wait(1L); }\n}\n";
    assert_eq!(inferred_type(wait, "waitOverload"), None);
    let agrees = "public class Main {\n  boolean equals(String s) { return false; }\n  \
                  void f(Object o) { var sameAnswer = equals(o); }\n}\n";
    assert_eq!(inferred_type(agrees, "sameAnswer"), inferred("boolean"));
    let override_ = "public class Main {\n  public String toString() { return \"\"; }\n  \
                     void f() { var text = toString(); }\n}\n";
    assert_eq!(inferred_type(override_, "text"), inferred("String"));
}
