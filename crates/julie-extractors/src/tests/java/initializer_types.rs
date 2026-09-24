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
}
