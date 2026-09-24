use crate::gdscript::GDScriptExtractor;
use crate::tests::helpers::init_parser;
use std::path::PathBuf;

fn inferred_type(source: &str, variable: &str) -> Option<(String, bool)> {
    fact_with_declared(source, variable).map(|(resolved, inferred, _)| (resolved, inferred))
}

fn fact_with_declared(source: &str, variable: &str) -> Option<(String, bool, Option<String>)> {
    let tree = init_parser(source, "gdscript");
    let mut extractor = GDScriptExtractor::new(
        "gdscript".to_string(),
        "initializer_types.gd".to_string(),
        source.to_string(),
        &PathBuf::from("/tmp/test"),
    );
    let symbols = extractor.extract_symbols(&tree);
    let symbol = symbols
        .iter()
        .find(|s| s.name == variable)
        .unwrap_or_else(|| panic!("missing variable {variable}"));
    extractor.base.type_info.get(&symbol.id).map(|fact| {
        let declared = fact
            .metadata
            .as_ref()
            .and_then(|m| m.get("declared"))
            .and_then(|v| v.as_str())
            .map(str::to_string);
        (fact.resolved_type.clone(), fact.is_inferred, declared)
    })
}

const LOADERS: &str = r#"
class_name Sample
extends Node

class Inner extends Resource:
    static func build() -> Inner:
        return Inner.new()
    func peers() -> Array[Inner]:
        return []
    func run_inner():
        var inner_peers = peers()
        var inner_self_peers = self.peers()
        var inner_outer = load_thing()

func load_thing() -> Workspace:
    return null

static func make_sample() -> Sample:
    return null

func wait_done() -> Signal:
    return done

func no_return():
    pass

func nothing() -> void:
    pass
"#;

fn workspace_type(line: &str) -> Option<(String, bool)> {
    let source = format!("{LOADERS}\nfunc run():\n    {line}\n");
    inferred_type(&source, "workspace")
}

fn inferred(name: &str) -> Option<(String, bool)> {
    Some((name.to_string(), true))
}

#[test]
fn bare_call_to_same_class_function_records_return_type() {
    assert_eq!(
        workspace_type("var workspace = load_thing()"),
        inferred("Workspace")
    );
}

#[test]
fn walrus_bare_call_records_return_type() {
    assert_eq!(
        workspace_type("var workspace := load_thing()"),
        inferred("Workspace")
    );
}

#[test]
fn self_call_records_return_type() {
    assert_eq!(
        workspace_type("var workspace := self.load_thing()"),
        inferred("Workspace")
    );
}

#[test]
fn await_on_same_file_call_records_return_type() {
    assert_eq!(
        workspace_type("var workspace = await load_thing()"),
        inferred("Workspace")
    );
}

#[test]
fn parenthesized_call_records_return_type() {
    assert_eq!(
        workspace_type("var workspace = (self.load_thing())"),
        inferred("Workspace")
    );
}

#[test]
fn static_call_on_same_file_inner_class_records_return_type() {
    assert_eq!(
        workspace_type("var workspace = Inner.build()"),
        inferred("Inner")
    );
}

#[test]
fn static_call_on_script_class_name_records_return_type() {
    assert_eq!(
        workspace_type("var workspace = Sample.make_sample()"),
        inferred("Sample")
    );
}

#[test]
fn inner_class_call_resolves_against_inner_class_methods() {
    let expected = Some(("Array".to_string(), true, Some("Array[Inner]".to_string())));
    assert_eq!(fact_with_declared(LOADERS, "inner_peers"), expected);
    assert_eq!(fact_with_declared(LOADERS, "inner_self_peers"), expected);
}

#[test]
fn class_field_initializer_records_return_type() {
    let source = format!("{LOADERS}\nvar workspace = load_thing()\n");
    assert_eq!(inferred_type(&source, "workspace"), inferred("Workspace"));
}

#[test]
fn written_type_wins_over_call_initializer() {
    assert_eq!(
        workspace_type("var workspace: Node = load_thing()"),
        Some(("Node".to_string(), false))
    );
}

#[test]
fn same_file_new_initializer_still_records_class() {
    assert_eq!(
        workspace_type("var workspace = Inner.new()"),
        inferred("Inner")
    );
}

#[test]
fn inner_class_bare_call_to_outer_function_records_nothing() {
    assert_eq!(inferred_type(LOADERS, "inner_outer"), None);
}

#[test]
fn script_bare_call_to_inner_class_method_records_nothing() {
    assert_eq!(workspace_type("var workspace = peers()"), None);
}

#[test]
fn function_without_return_type_records_nothing() {
    assert_eq!(workspace_type("var workspace = no_return()"), None);
}

#[test]
fn void_return_records_nothing() {
    assert_eq!(workspace_type("var workspace = nothing()"), None);
}

#[test]
fn await_on_signal_return_records_nothing() {
    assert_eq!(workspace_type("var workspace = await wait_done()"), None);
}

#[test]
fn signal_return_without_await_records_signal() {
    assert_eq!(
        workspace_type("var workspace = wait_done()"),
        inferred("Signal")
    );
}

#[test]
fn chain_ending_in_unknown_method_records_nothing() {
    assert_eq!(workspace_type("var workspace = load_thing().open()"), None);
}

#[test]
fn foreign_receivers_record_nothing() {
    assert_eq!(workspace_type("var workspace = other.load_thing()"), None);
    assert_eq!(workspace_type("var workspace = super.load_thing()"), None);
    assert_eq!(workspace_type("var workspace = Other.load_thing()"), None);
    assert_eq!(workspace_type("var workspace = a.Inner.build()"), None);
}

#[test]
fn unknown_callee_records_nothing() {
    assert_eq!(workspace_type("var workspace = load(\"res://x.gd\")"), None);
}

#[test]
fn static_call_on_class_without_that_function_records_nothing() {
    assert_eq!(workspace_type("var workspace = Inner.load_thing()"), None);
}

#[test]
fn disagreeing_same_named_functions_record_nothing() {
    let source = format!(
        "{LOADERS}\nfunc twin() -> Workspace:\n    return null\nfunc twin() -> Node:\n    return null\nfunc run():\n    var workspace = twin()\n"
    );
    assert_eq!(inferred_type(&source, "workspace"), None);
}

#[test]
fn agreeing_same_named_functions_record_return_type() {
    let source = format!(
        "{LOADERS}\nfunc twin() -> Workspace:\n    return null\nfunc twin() -> Workspace:\n    return null\nfunc run():\n    var workspace = twin()\n"
    );
    assert_eq!(inferred_type(&source, "workspace"), inferred("Workspace"));
}
