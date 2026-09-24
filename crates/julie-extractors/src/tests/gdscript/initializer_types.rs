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
fn disagreeing_duplicate_declarations_record_nothing() {
    let source = format!(
        "{LOADERS}\nfunc twin() -> Workspace:\n    return null\nfunc twin() -> Node:\n    return null\nfunc run():\n    var workspace = twin()\n"
    );
    assert_eq!(inferred_type(&source, "workspace"), None);
}

#[test]
fn agreeing_duplicate_declarations_record_return_type() {
    let source = format!(
        "{LOADERS}\nfunc twin() -> Workspace:\n    return null\nfunc twin() -> Workspace:\n    return null\nfunc run():\n    var workspace = twin()\n"
    );
    assert_eq!(inferred_type(&source, "workspace"), inferred("Workspace"));
}

const SAME_NAMED_CLASSES: &str = "extends Node
class Base:
\tfunc f() -> String:
\t\treturn \"\"
\tstatic func build() -> String:
\t\treturn \"\"
class Inner extends Base:
\tfunc g():
\t\tvar nested_collide = f()
class Outer:
\tclass Inner:
\t\tfunc f() -> int:
\t\t\treturn 1
\t\tstatic func build() -> int:
\t\t\treturn 1
\t\tfunc h():
\t\t\tvar own_nested = f()
\tfunc outer_run():
\t\tvar ambiguous_build = Inner.build()
func top():
\tvar static_collide = Inner.build()
";

#[test]
fn bare_call_in_class_does_not_use_same_named_nested_class() {
    assert_eq!(inferred_type(SAME_NAMED_CLASSES, "nested_collide"), None);
}

#[test]
fn static_call_does_not_use_same_named_nested_class() {
    assert_eq!(inferred_type(SAME_NAMED_CLASSES, "static_collide"), None);
}

#[test]
fn bare_call_in_nested_class_uses_its_own_functions() {
    assert_eq!(
        inferred_type(SAME_NAMED_CLASSES, "own_nested"),
        inferred("int")
    );
}

#[test]
fn static_call_on_class_name_with_two_visible_classes_records_nothing() {
    assert_eq!(inferred_type(SAME_NAMED_CLASSES, "ambiguous_build"), None);
}

const SCOPED_RETURNS: &str = "extends Node
class Result:
\tfunc top_only():
\t\tpass
class A:
\tclass Result:
\t\tfunc a_only():
\t\t\tpass
\tclass Local:
\t\tpass
\tstatic func make() -> Result:
\t\treturn Result.new()
\tstatic func make_local() -> Local:
\t\treturn Local.new()
\tstatic func make_self() -> A:
\t\treturn A.new()
\tstatic func count() -> int:
\t\treturn 1
\tstatic func peers() -> Array[A]:
\t\treturn []
\tclass Deep:
\t\tstatic func make_local() -> Local:
\t\t\treturn null
\tfunc a_run():
\t\tvar inside_leak = Deep.make_local()
func top():
\tvar scope_leak = A.make()
\tvar other_file_leak = A.make_local()
\tvar same_class = A.make_self()
\tvar builtin = A.count()
\tvar generic_same = A.peers()
";

#[test]
fn return_type_naming_a_different_class_at_the_call_site_records_nothing() {
    assert_eq!(inferred_type(SCOPED_RETURNS, "scope_leak"), None);
}

#[test]
fn return_type_naming_a_class_not_visible_at_the_call_site_records_nothing() {
    assert_eq!(inferred_type(SCOPED_RETURNS, "other_file_leak"), None);
}

#[test]
fn return_type_visible_from_both_scopes_records_it() {
    assert_eq!(inferred_type(SCOPED_RETURNS, "same_class"), inferred("A"));
    assert_eq!(inferred_type(SCOPED_RETURNS, "builtin"), inferred("int"));
    assert_eq!(
        fact_with_declared(SCOPED_RETURNS, "generic_same"),
        Some(("Array".to_string(), true, Some("Array[A]".to_string())))
    );
}

#[test]
fn return_type_from_nested_class_visible_at_outer_call_site_records_it() {
    assert_eq!(
        inferred_type(SCOPED_RETURNS, "inside_leak"),
        inferred("Local")
    );
}

const TYPE_MEMBERS: &str = "class_name Sample
extends Node
enum Kind { SCRIPT_A }
class Shape:
\tpass
class A:
\tenum Kind { INNER_X }
\tenum Shape { ROUND }
\tconst Res = preload(\"res://other.gd\")
\tstatic func kind() -> Kind:
\t\treturn Kind.INNER_X
\tstatic func res() -> Res:
\t\treturn null
\tstatic func shape() -> Shape:
\t\treturn Shape.ROUND
class B:
\tenum Kind { B_ONLY }
\tfunc g():
\t\tvar from_script = Sample.script_kind()
static func script_kind() -> Kind:
\treturn Kind.SCRIPT_A
func top():
\tvar enum_leak = A.kind()
\tvar const_leak = A.res()
\tvar shadowed_class = A.shape()
\tvar own_enum = script_kind()
";

#[test]
fn return_type_naming_another_class_enum_records_nothing() {
    assert_eq!(inferred_type(TYPE_MEMBERS, "enum_leak"), None);
    assert_eq!(inferred_type(TYPE_MEMBERS, "from_script"), None);
}

#[test]
fn return_type_naming_another_class_const_records_nothing() {
    assert_eq!(inferred_type(TYPE_MEMBERS, "const_leak"), None);
}

#[test]
fn return_type_naming_an_enum_that_shadows_an_outer_class_records_nothing() {
    assert_eq!(inferred_type(TYPE_MEMBERS, "shadowed_class"), None);
}

#[test]
fn return_type_naming_an_enum_of_the_same_scope_records_it() {
    assert_eq!(inferred_type(TYPE_MEMBERS, "own_enum"), inferred("Kind"));
}

const GLOBAL_NAMED_METHODS: &str = "extends Node
func str(v) -> int:
\treturn 1
func load(p) -> Dictionary:
\treturn {}
func max(a, b) -> Node:
\treturn null
func range(n) -> String:
\treturn \"\"
func Vector2(x) -> Node:
\treturn null
func use():
\tvar s = str(1)
\tvar l = load(\"x\")
\tvar m = max(1, 2)
\tvar r = range(3)
\tvar v = Vector2(1)
\tvar own_load = self.load(\"x\")
";

#[test]
fn bare_call_to_method_named_like_a_global_function_records_nothing() {
    for variable in ["s", "l", "m", "r", "v"] {
        assert_eq!(
            inferred_type(GLOBAL_NAMED_METHODS, variable),
            None,
            "{variable}"
        );
    }
}

#[test]
fn self_call_to_method_named_like_a_global_function_records_return_type() {
    assert_eq!(
        inferred_type(GLOBAL_NAMED_METHODS, "own_load"),
        inferred("Dictionary")
    );
}

const SHADOWED_RECEIVERS: &str = "extends Node
class Tools:
\tstatic func build() -> Dictionary:
\t\treturn {}
func plain():
\tvar unshadowed = Tools.build()
func shadow(Tools):
\tvar by_parameter = Tools.build()
\tvar new_by_parameter = Tools.new()
func typed(Tools: Object = null):
\tvar by_typed_parameter = Tools.build()
func local():
\tvar Tools = null
\tvar by_local = Tools.build()
func loop(items):
\tfor Tools in items:
\t\tvar by_loop = Tools.build()
class Inner:
\tconst Tools = preload(\"res://tools.gd\")
\tfunc g():
\t\tvar by_const = Tools.build()
\t\tvar new_by_const = Tools.new()
class Holder:
\tvar Tools
\tfunc g():
\t\tvar by_member = Tools.build()
";

#[test]
fn static_call_on_unshadowed_class_records_return_type() {
    assert_eq!(
        inferred_type(SHADOWED_RECEIVERS, "unshadowed"),
        inferred("Dictionary")
    );
}

#[test]
fn static_call_on_name_shadowed_by_parameter_or_local_records_nothing() {
    for variable in [
        "by_parameter",
        "new_by_parameter",
        "by_typed_parameter",
        "by_local",
        "by_loop",
    ] {
        assert_eq!(
            inferred_type(SHADOWED_RECEIVERS, variable),
            None,
            "{variable}"
        );
    }
}

#[test]
fn static_call_on_name_shadowed_by_class_member_records_nothing() {
    for variable in ["by_const", "new_by_const", "by_member"] {
        assert_eq!(
            inferred_type(SHADOWED_RECEIVERS, variable),
            None,
            "{variable}"
        );
    }
}

#[test]
fn chain_after_same_file_static_call_records_nothing() {
    assert_eq!(
        workspace_type("var workspace = Inner.build().peers()"),
        None
    );
    assert_eq!(workspace_type("var workspace = Inner.build().value"), None);
}

const ACCESSOR_SHADOWS: &str = "extends Node
class Foo:
\tstatic func make() -> int:
\t\treturn 1
var p1: int:
\tset(Foo):
\t\tvar set_param_call = Foo.make()
\t\tvar set_param_new = Foo.new()
\tget:
\t\tvar Foo = 3
\t\tvar get_local_call = Foo.make()
\t\tvar get_local_new = Foo.new()
\t\treturn 0
var p2: int:
\tset(value):
\t\tvar set_unshadowed = Foo.make()
";

#[test]
fn static_call_in_accessor_on_unshadowed_class_records_return_type() {
    assert_eq!(
        inferred_type(ACCESSOR_SHADOWS, "set_unshadowed"),
        inferred("int")
    );
}

#[test]
fn static_call_on_name_shadowed_by_accessor_parameter_or_local_records_nothing() {
    for variable in [
        "set_param_call",
        "set_param_new",
        "get_local_call",
        "get_local_new",
    ] {
        assert_eq!(
            inferred_type(ACCESSOR_SHADOWS, variable),
            None,
            "{variable}"
        );
    }
}

const INHERITED_MEMBERS: &str = "extends Node
class Item:
\tstatic func make() -> int:
\t\treturn 1
class Foo:
\tstatic func make() -> int:
\t\treturn 1
class Kind:
\tpass
class A:
\tstatic func k() -> Kind:
\t\treturn null
class Base:
\tclass Item:
\t\tstatic func make() -> String:
\t\t\treturn \"\"
\tclass Tool:
\t\tstatic func t() -> int:
\t\t\treturn 1
\tclass Kind:
\t\tpass
\tvar Foo = null
class Derived extends Base:
\tfunc use():
\t\tvar inherited_class = Item.make()
\t\tvar inherited_var_call = Foo.make()
\t\tvar inherited_var_new = Foo.new()
\t\tvar inherited_return_name = A.k()
\t\tvar inherited_tool = Tool.t()
\tclass Deep:
\t\tfunc use():
\t\t\tvar deep_inherited = Item.make()
class Grand extends Derived:
\tfunc use():
\t\tvar grand_inherited = Item.make()
class Plain extends Node:
\tfunc use():
\t\tvar plain_item = Item.make()
\t\tvar plain_kind = A.k()
class Broken extends Base.Missing:
\tfunc use():
\t\tvar broken_base = Item.make()
class Loop1 extends Loop2:
\tfunc use():
\t\tvar loop_base = Item.make()
class Loop2 extends Loop1:
\tpass
";

#[test]
fn static_call_on_name_that_a_same_file_base_class_declares_records_nothing() {
    for variable in [
        "inherited_class",
        "inherited_var_call",
        "inherited_var_new",
        "inherited_return_name",
        "deep_inherited",
        "grand_inherited",
    ] {
        assert_eq!(
            inferred_type(INHERITED_MEMBERS, variable),
            None,
            "{variable}"
        );
    }
}

#[test]
fn static_call_on_class_inherited_from_same_file_base_records_return_type() {
    assert_eq!(
        inferred_type(INHERITED_MEMBERS, "inherited_tool"),
        inferred("int")
    );
}

#[test]
fn static_call_in_class_with_outside_base_records_return_type() {
    assert_eq!(
        inferred_type(INHERITED_MEMBERS, "plain_item"),
        inferred("int")
    );
    assert_eq!(
        inferred_type(INHERITED_MEMBERS, "plain_kind"),
        inferred("Kind")
    );
}

#[test]
fn static_call_in_class_with_unresolved_or_looping_base_records_nothing() {
    for variable in ["broken_base", "loop_base"] {
        assert_eq!(
            inferred_type(INHERITED_MEMBERS, variable),
            None,
            "{variable}"
        );
    }
}
