use crate::base::SymbolKind;
use crate::typescript::TypeScriptExtractor;
use std::path::PathBuf;

fn fact_of(source: &str, local: &str) -> Option<(String, bool, Option<String>)> {
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into())
        .unwrap();
    let tree = parser.parse(source, None).unwrap();
    let mut extractor = TypeScriptExtractor::new(
        "typescript".to_string(),
        "initializer_types.ts".to_string(),
        source.to_string(),
        &PathBuf::from("/tmp/test"),
    );
    let symbols = extractor.extract_symbols(&tree);
    let local = symbols
        .iter()
        .find(|s| s.name == local && s.kind != SymbolKind::Export)
        .unwrap_or_else(|| panic!("missing local {local}"));
    extractor.base.type_info.get(&local.id).map(|fact| {
        (
            fact.resolved_type.clone(),
            fact.is_inferred,
            fact.metadata
                .as_ref()
                .and_then(|m| m.get("declared"))
                .and_then(|d| d.as_str())
                .map(str::to_string),
        )
    })
}

fn inferred_type(source: &str, local: &str) -> Option<(String, bool)> {
    fact_of(source, local).map(|(resolved, inferred, _)| (resolved, inferred))
}

fn inferred(name: &str) -> Option<(String, bool)> {
    Some((name.to_string(), true))
}

const LOADERS: &str = r#"
class User {}
function loadUser(id: string): User { return new User(); }
async function fetchUser(id: string): Promise<User> { return new User(); }
function findUser(id: string): User | undefined { return undefined; }
function findUserOrNull(id: string): null | User { return null; }
async function fetchMaybeUser(id: string): Promise<User | null> { return null; }
function listUsers(): User[] { return []; }
function untyped(id: string) { return new User(); }
function save(value: User): void {}
"#;

fn user_type(body: &str) -> Option<(String, bool)> {
    inferred_type(
        &format!("{LOADERS}\nasync function run(id: string) {{\n    {body}\n}}\n"),
        "user",
    )
}

#[test]
fn same_file_function_call_records_return_type_as_inferred() {
    assert_eq!(user_type("const user = loadUser(id);"), inferred("User"));
}

#[test]
fn let_and_var_declarations_also_record_the_return_type() {
    assert_eq!(user_type("let user = loadUser(id);"), inferred("User"));
    assert_eq!(user_type("var user = loadUser(id);"), inferred("User"));
}

#[test]
fn awaited_promise_call_records_the_promised_type() {
    assert_eq!(
        user_type("const user = await fetchUser(id);"),
        inferred("User")
    );
}

#[test]
fn unawaited_promise_call_records_the_promise_with_declared_text() {
    let source = format!("{LOADERS}\nfunction run(id: string) {{ const user = fetchUser(id); }}");
    assert_eq!(
        fact_of(&source, "user"),
        Some((
            "Promise".to_string(),
            true,
            Some("Promise<User>".to_string())
        ))
    );
}

#[test]
fn non_null_assertion_removes_null_and_undefined() {
    assert_eq!(user_type("const user = findUser(id)!;"), inferred("User"));
    assert_eq!(
        user_type("const user = findUserOrNull(id)!;"),
        inferred("User")
    );
    assert_eq!(
        user_type("const user = (await fetchMaybeUser(id))!;"),
        inferred("User")
    );
}

#[test]
fn non_null_assertion_on_a_plain_type_keeps_it() {
    assert_eq!(user_type("const user = loadUser(id)!;"), inferred("User"));
}

#[test]
fn parenthesized_call_records_the_return_type() {
    assert_eq!(user_type("const user = (loadUser(id));"), inferred("User"));
}

#[test]
fn array_return_type_records_the_array_type() {
    assert_eq!(user_type("const user = listUsers();"), inferred("User[]"));
}

#[test]
fn arrow_and_function_expression_consts_with_return_types_are_callees() {
    let source = r#"
class User {}
const makeUser = (id: string): User => new User();
const buildUser = function (id: string): User { return new User(); };
function run() {
    const made = makeUser("a");
    const built = buildUser("b");
}
"#;
    assert_eq!(inferred_type(source, "made"), inferred("User"));
    assert_eq!(inferred_type(source, "built"), inferred("User"));
}

const REPO: &str = r#"
class Repo {
    load(id: string): User { return new User(); }
    async fetch(id: string): Promise<User> { return new User(); }
    loadLater = (id: string): User => new User();
    static create(): Repo { return new Repo(); }
    get current(): User { return new User(); }
    with(): this { return this; }
    #secret(): User { return new User(); }
    BODY
}
"#;

fn in_repo(body: &str, local: &str) -> Option<(String, bool)> {
    inferred_type(&REPO.replace("BODY", body), local)
}

#[test]
fn this_method_call_records_the_method_return_type() {
    assert_eq!(
        in_repo("run() { const user = this.load(\"a\"); }", "user"),
        inferred("User")
    );
}

#[test]
fn awaited_this_method_call_records_the_promised_type() {
    assert_eq!(
        in_repo(
            "async run() { const user = await this.fetch(\"a\"); }",
            "user"
        ),
        inferred("User")
    );
}

#[test]
fn this_call_to_a_class_field_arrow_records_its_return_type() {
    assert_eq!(
        in_repo("run() { const user = this.loadLater(\"a\"); }", "user"),
        inferred("User")
    );
}

#[test]
fn this_call_to_a_private_method_records_its_return_type() {
    assert_eq!(
        in_repo("run() { const user = this.#secret(); }", "user"),
        inferred("User")
    );
}

#[test]
fn this_call_inside_an_arrow_function_keeps_the_class() {
    assert_eq!(
        in_repo(
            "run() { return [1].map(() => { const user = this.load(\"a\"); }); }",
            "user"
        ),
        inferred("User")
    );
}

#[test]
fn this_return_type_records_the_enclosing_class() {
    assert_eq!(
        in_repo("run() { const same = this.with(); }", "same"),
        inferred("Repo")
    );
}

#[test]
fn static_call_on_a_same_file_class_records_the_return_type() {
    let source = REPO.replace("BODY", "") + "function run() { const repo = Repo.create(); }";
    assert_eq!(inferred_type(&source, "repo"), inferred("Repo"));
}

#[test]
fn this_call_inside_a_static_method_resolves_static_methods() {
    assert_eq!(
        in_repo("static run() { const repo = this.create(); }", "repo"),
        inferred("Repo")
    );
}

#[test]
fn written_type_wins_over_call_inference() {
    assert_eq!(
        user_type("const user: Person = loadUser(id);"),
        Some(("Person".to_string(), false))
    );
}

#[test]
fn written_union_type_blocks_call_inference() {
    assert_eq!(user_type("const user: User | Admin = loadUser(id);"), None);
}

#[test]
fn constructor_inference_still_records_the_class() {
    assert_eq!(user_type("const user = new User();"), inferred("User"));
}

#[test]
fn callee_without_a_return_type_records_nothing() {
    assert_eq!(user_type("const user = untyped(id);"), None);
}

#[test]
fn void_return_type_records_nothing() {
    assert_eq!(user_type("const user = save(new User());"), None);
}

#[test]
fn callee_from_another_file_records_nothing() {
    let source = r#"
import { loadUser } from "./users";
function run() { const user = loadUser("a"); }
"#;
    assert_eq!(inferred_type(source, "user"), None);
}

#[test]
fn nullable_return_without_non_null_assertion_records_nothing() {
    assert_eq!(user_type("const user = findUser(id);"), None);
    assert_eq!(user_type("const user = await fetchMaybeUser(id);"), None);
}

#[test]
fn await_on_a_non_promise_return_records_nothing() {
    let source = r#"
type Pending = Promise<User>;
function load(): Pending { return Promise.resolve(new User()); }
function list(): Array<User> { return []; }
async function run() {
    const user = await load();
    const listed = await list();
}
"#;
    assert_eq!(inferred_type(source, "user"), None);
    assert_eq!(inferred_type(source, "listed"), None);
}

#[test]
fn methods_after_a_call_record_nothing() {
    for chain in [
        "loadUser(id).clone()",
        "fetchUser(id).then((u) => u)",
        "listUsers().map((u) => u)",
    ] {
        assert_eq!(
            user_type(&format!("const user = {chain};")),
            None,
            "{chain}"
        );
    }
}

#[test]
fn optional_calls_record_nothing() {
    assert_eq!(user_type("const user = loadUser?.(id);"), None);
    assert_eq!(
        in_repo("run() { const user = this?.load(\"a\"); }", "user"),
        None
    );
}

#[test]
fn generic_return_types_record_nothing() {
    let source = r#"
function first<T>(items: T[]): T { return items[0]; }
async function later<T>(value: T): Promise<T> { return value; }
function outer<T>(value: T) {
    const inner = (): T => value;
    const fromInner = inner();
    return fromInner;
}
async function run() {
    const one = first([1]);
    const awaited = await later(1);
}
class Box<T> {
    get(): T { return null!; }
    run() { const item = this.get(); }
}
"#;
    for local in ["one", "awaited", "fromInner", "item"] {
        assert_eq!(inferred_type(source, local), None, "{local}");
    }
}

#[test]
fn overloads_with_different_return_types_record_nothing() {
    let source = r#"
function load(id: string): User;
function load(id: number): Admin;
function load(id: string | number): User | Admin { return null!; }
function run() { const user = load("a"); }
"#;
    assert_eq!(inferred_type(source, "user"), None);
}

#[test]
fn same_named_functions_with_different_return_types_record_nothing() {
    let source = r#"
function load(): User { return null!; }
function other() {
    function load(): Admin { return null!; }
    const user = load();
    return user;
}
"#;
    assert_eq!(inferred_type(source, "user"), None);
}

#[test]
fn a_nested_function_does_not_reach_calls_outside_its_scope() {
    let source = r#"
function load(): User { return null!; }
function other() {
    function load(): Admin { return null!; }
    return load();
}
function run() { const user = load(); }
"#;
    assert_eq!(inferred_type(source, "user"), inferred("User"));
}

#[test]
fn a_nested_function_is_a_callee_inside_its_own_scope() {
    let source = r#"
function other() {
    function load(): Admin { return null!; }
    const admin = load();
    return admin;
}
"#;
    assert_eq!(inferred_type(source, "admin"), inferred("Admin"));
}

#[test]
fn an_out_of_scope_function_does_not_type_a_global_call() {
    let source = r#"
export function helper() {
    function open(): Door { return new Door(); }
    return open();
}
export const b1 = open("https://x");
"#;
    assert_eq!(inferred_type(source, "b1"), None);
}

#[test]
fn a_function_expression_name_shadows_the_outer_function_in_its_body() {
    let source = r#"
export function load(): User { return null!; }
export const g = function load(): Admin {
    const a1 = load();
    return a1;
};
"#;
    assert_eq!(inferred_type(source, "a1"), None);
}

#[test]
fn an_import_alias_blocks_a_nested_function_of_the_same_name() {
    let source = r#"
export function outer() {
    function load(): User { return null!; }
    return load();
}
import load = Other.load;
export const d1 = load();
"#;
    assert_eq!(inferred_type(source, "d1"), None);
}

#[test]
fn a_var_in_a_nested_block_shadows_the_function_in_the_whole_function() {
    let source = r#"
function load(): User { return null!; }
function run(flag: boolean) {
    if (flag) { var load = () => null; }
    const user = load();
}
"#;
    assert_eq!(inferred_type(source, "user"), None);
}

#[test]
fn a_namespace_or_enum_of_the_same_name_blocks_a_function_call() {
    for binding in ["namespace load { }", "enum load { A }"] {
        let source =
            format!("function load(): User {{ return null!; }}\n{binding}\nconst user = load();\n");
        assert_eq!(inferred_type(&source, "user"), None, "{binding}");
    }
}

#[test]
fn same_named_functions_that_agree_record_the_type() {
    let source = r#"
function load(): User { return null!; }
function other() {
    function load(): User { return null!; }
    return load();
}
function run() { const user = load(); }
"#;
    assert_eq!(inferred_type(source, "user"), inferred("User"));
}

#[test]
fn a_parameter_that_shadows_the_function_name_records_nothing() {
    let source = r#"
function load(): User { return null!; }
function run(load: () => Admin) { const user = load(); }
"#;
    assert_eq!(inferred_type(source, "user"), None);
}

#[test]
fn a_local_variable_that_shadows_the_function_name_records_nothing() {
    let source = r#"
function load(): User { return null!; }
function run() {
    const { load } = loaders;
    const user = load();
}
"#;
    assert_eq!(inferred_type(source, "user"), None);
}

#[test]
fn an_annotated_function_const_is_not_a_callee() {
    let source = r#"
const load: Loader = (): User => null!;
function run() { const user = load(); }
"#;
    assert_eq!(inferred_type(source, "user"), None);
}

#[test]
fn this_call_inside_a_nested_function_records_nothing() {
    assert_eq!(
        in_repo(
            "run() { return function () { const user = this.load(\"a\"); }; }",
            "user"
        ),
        None
    );
}

#[test]
fn this_call_inside_an_object_literal_method_records_nothing() {
    let source = r#"
class Repo { load(): User { return null!; } }
const handlers = { load(): Admin { return null!; }, run() { const user = this.load(); } };
"#;
    assert_eq!(inferred_type(source, "user"), None);
}

#[test]
fn this_call_inside_an_object_literal_in_a_class_method_records_nothing() {
    assert_eq!(
        in_repo(
            "run() { return { go() { const user = this.load(\"a\"); } }; }",
            "user"
        ),
        None
    );
}

#[test]
fn an_import_that_shadows_a_nested_function_name_records_nothing() {
    let source = r#"
import { load } from "./loaders";
function helper() {
    function load(): Admin { return null!; }
    return load();
}
function run() { const user = load(); }
"#;
    assert_eq!(inferred_type(source, "user"), None);
}

#[test]
fn this_call_to_a_getter_records_nothing() {
    assert_eq!(
        in_repo("run() { const user = this.current(); }", "user"),
        None
    );
}

#[test]
fn this_call_to_another_class_method_records_nothing() {
    let source = r#"
class Other { load(): User { return null!; } }
class Repo { run() { const user = this.load(); } }
"#;
    assert_eq!(inferred_type(source, "user"), None);
}

#[test]
fn this_call_to_a_static_method_from_an_instance_method_records_nothing() {
    assert_eq!(
        in_repo("run() { const repo = this.create(); }", "repo"),
        None
    );
}

#[test]
fn static_call_to_an_instance_method_records_nothing() {
    let source = REPO.replace("BODY", "") + "function run() { const user = Repo.load(\"a\"); }";
    assert_eq!(inferred_type(&source, "user"), None);
}

#[test]
fn static_call_on_a_shadowed_class_name_records_nothing() {
    let source =
        REPO.replace("BODY", "") + "function run(Repo: Factory) { const repo = Repo.create(); }";
    assert_eq!(inferred_type(&source, "repo"), None);
}

#[test]
fn call_on_another_receiver_records_nothing() {
    assert_eq!(
        in_repo("run() { const user = this.repo.load(\"a\"); }", "user"),
        None
    );
}

#[test]
fn static_call_through_a_namespace_does_not_reach_an_out_of_scope_class() {
    let source = r#"
namespace Repo { export function create(): Admin { return null!; } }
export function h() {
    class Repo { static create(): User { return null!; } }
    return Repo;
}
export const f1 = Repo.create();
"#;
    assert_eq!(inferred_type(source, "f1"), None);
}

#[test]
fn static_call_on_a_class_merged_with_a_namespace_records_nothing() {
    let class = REPO.replace("BODY", "");
    let namespace = "namespace Repo { export const x = 1; }\n";
    for declarations in [format!("{class}{namespace}"), format!("{namespace}{class}")] {
        let source = format!("{declarations}const repo = Repo.create();");
        assert_eq!(inferred_type(&source, "repo"), None, "{declarations}");
    }
}

#[test]
fn static_call_on_a_local_class_records_its_return_type() {
    let source = r#"
export function mk() {
    class Impl { static create(): User { return null!; } }
    const made = Impl.create();
    return made;
}
"#;
    assert_eq!(inferred_type(source, "made"), inferred("User"));
}

#[test]
fn static_call_on_a_class_expression_const_records_its_return_type() {
    let source = r#"
const Repo = class { static create(): User { return null!; } };
const repo = Repo.create();
"#;
    assert_eq!(inferred_type(source, "repo"), inferred("User"));
}

#[test]
fn same_named_classes_keep_their_own_methods() {
    let source = r#"
class Base { load(): Admin { return null!; } }
export function mkA() {
    class Impl extends Base {
        f() { const c1 = this.load(); return c1; }
    }
    return Impl;
}
export function mkB() {
    class Impl {
        load(): User { return null!; }
        g() { const c2 = this.load(); return c2; }
    }
    return Impl;
}
"#;
    assert_eq!(inferred_type(source, "c1"), None);
    assert_eq!(inferred_type(source, "c2"), inferred("User"));
}

#[test]
fn an_annotated_field_is_not_mixed_with_a_same_named_class_method() {
    let source = r#"
export function mkA() {
    class Impl {
        load: () => Admin = () => null!;
        f() { const c1 = this.load(); return c1; }
    }
    return Impl;
}
export function mkB() {
    class Impl { load(): User { return null!; } }
    return Impl;
}
"#;
    assert_eq!(inferred_type(source, "c1"), None);
}

#[test]
fn super_and_namespace_qualified_calls_record_nothing() {
    let source = r#"
class Base { load(): User { return null!; } }
namespace Ns { export function load(): User { return null!; } }
class Repo extends Base {
    load(): User { const viaSuper = super.load(); return viaSuper; }
}
const viaNamespace = Ns.load();
"#;
    assert_eq!(inferred_type(source, "viaSuper"), None);
    assert_eq!(inferred_type(source, "viaNamespace"), None);
}

#[test]
fn a_generic_callee_keeps_the_base_type_and_the_declared_text() {
    let source = r#"
export function make<K>(): Map<K, User> { return new Map(); }
const made = make<string>();
"#;
    assert_eq!(
        fact_of(source, "made"),
        Some(("Map".to_string(), true, Some("Map<K, User>".to_string())))
    );
}

const OUTER_AND_NAMESPACE_LOADERS: &str = r#"
class User {}
class Admin {}
function load(): User { return null!; }
class Repo { static create(): User { return null!; } }
namespace A {
    export function load(): Admin { return null!; }
    export class Repo { static create(): Admin { return null!; } }
}
"#;

#[test]
fn a_namespace_export_blocks_the_outer_binding_in_a_merged_namespace_block() {
    let source = format!(
        "{OUTER_AND_NAMESPACE_LOADERS}namespace A {{ export const nsMerged = load(); export const nsClass = Repo.create(); }}\n"
    );
    assert_eq!(inferred_type(&source, "nsMerged"), None);
    assert_eq!(inferred_type(&source, "nsClass"), None);
}

#[test]
fn a_namespace_export_blocks_the_outer_binding_in_a_nested_namespace_block() {
    let source = format!(
        "{OUTER_AND_NAMESPACE_LOADERS}namespace A.B {{ export const nestedNs = load(); }}\nnamespace A {{ namespace C {{ const deepNs = load(); }} }}\n"
    );
    assert_eq!(inferred_type(&source, "nestedNs"), None);
    assert_eq!(inferred_type(&source, "deepNs"), None);
}

#[test]
fn an_ambient_namespace_member_blocks_the_outer_binding_in_a_merged_block() {
    let source = r#"
function load(): User { return null!; }
declare namespace A { function load(): Admin; }
namespace A { const ambientMerged = load(); }
"#;
    assert_eq!(inferred_type(source, "ambientMerged"), None);
}

#[test]
fn a_namespace_export_types_calls_in_every_block_of_its_namespace() {
    let source = r#"
namespace A { export function load(): Admin { return null!; } }
namespace A { const merged = load(); }
namespace A.B { const nested = load(); }
const outside = load();
"#;
    assert_eq!(inferred_type(source, "merged"), inferred("Admin"));
    assert_eq!(inferred_type(source, "nested"), inferred("Admin"));
    assert_eq!(inferred_type(source, "outside"), None);
}

#[test]
fn a_namespace_member_without_export_stays_in_its_own_block() {
    let source = r#"
function load(): User { return null!; }
namespace A { function load(): Admin { return null!; } }
namespace A { const merged = load(); }
"#;
    assert_eq!(inferred_type(source, "merged"), inferred("User"));
}

#[test]
fn a_var_in_a_for_of_header_shadows_the_function_in_the_whole_function() {
    let source = r#"
function load(): User { return null!; }
export function f(fns: Array<() => Admin>) {
    for (var load of fns) {}
    const forOfVar = load();
}
"#;
    assert_eq!(inferred_type(source, "forOfVar"), None);
}

#[test]
fn a_const_in_a_for_of_header_shadows_only_the_loop() {
    let source = r#"
function load(): User { return null!; }
export function f(fns: Array<() => Admin>) {
    for (const load of fns) {}
    const afterLoop = load();
}
"#;
    assert_eq!(inferred_type(source, "afterLoop"), inferred("User"));
}

#[test]
fn a_this_array_return_type_records_the_enclosing_class_array() {
    assert_eq!(
        in_repo(
            "all(): this[] { return [this]; }\nm() { const thisArr = this.all(); }",
            "thisArr"
        ),
        inferred("Repo[]")
    );
}

#[test]
fn a_parenthesized_array_element_records_the_plain_array_type() {
    let source = r#"
function list(): (User)[] { return []; }
const users = list();
"#;
    assert_eq!(inferred_type(source, "users"), inferred("User[]"));
}

#[test]
fn a_satisfies_initializer_keeps_the_call_type() {
    let source = r#"
function load(): User { return null!; }
export const sat = load() satisfies User;
"#;
    assert_eq!(inferred_type(source, "sat"), inferred("User"));
}

const THIS_PARAMETER: &str = r#"
class User {}
class Admin {}
class Other { load(): Admin { return null!; } static create(): Admin { return null!; } }
class Repo {
    load(): User { return null!; }
    static create(): User { return null!; }
    BODY
}
"#;

fn with_this_parameter(body: &str, local: &str) -> Option<(String, bool)> {
    inferred_type(&THIS_PARAMETER.replace("BODY", body), local)
}

#[test]
fn a_method_with_a_this_parameter_records_nothing_for_this_calls() {
    assert_eq!(
        with_this_parameter("m(this: Other) { const user = this.load(); }", "user"),
        None
    );
}

#[test]
fn an_arrow_in_a_method_with_a_this_parameter_records_nothing_for_this_calls() {
    assert_eq!(
        with_this_parameter(
            "m(this: Other) { const f = () => { const user = this.load(); }; }",
            "user"
        ),
        None
    );
}

#[test]
fn a_static_method_with_a_this_parameter_records_nothing_for_this_calls() {
    assert_eq!(
        with_this_parameter(
            "static h(this: typeof Other) { const user = this.create(); }",
            "user"
        ),
        None
    );
}

#[test]
fn a_method_without_a_this_parameter_keeps_its_class() {
    assert_eq!(
        with_this_parameter("m(other: Other) { const user = this.load(); }", "user"),
        inferred("User")
    );
}

#[test]
fn a_this_call_in_a_member_decorator_records_nothing() {
    let source = r#"
class User {}
class Admin {}
class Other {
    load(): Admin { return null!; }
    m() {
        class Repo {
            load(): User { return null!; }
            @dec(() => { const user = this.load(); }) x() {}
        }
    }
}
"#;
    assert_eq!(inferred_type(source, "user"), None);
}

#[test]
fn a_this_call_in_a_field_decorator_records_nothing() {
    let source = r#"
class User {}
class Repo {
    load(): User { return null!; }
    @dec(() => { const user = this.load(); }) field = 1;
}
"#;
    assert_eq!(inferred_type(source, "user"), None);
}

#[test]
fn await_on_a_file_local_promise_class_records_nothing() {
    let source = r#"
class User { u = 1; }
class Promise<T> { constructor(public v: T) {} }
function load(): Promise<User> { return null!; }
async function f() { const user = await load(); }
"#;
    assert_eq!(inferred_type(source, "user"), None);
}

#[test]
fn await_with_a_promise_like_interface_in_the_file_records_nothing() {
    let source = r#"
class User {}
interface PromiseLike<T> { v: T; }
function load(): PromiseLike<User> { return null!; }
async function f() { const user = await load(); }
"#;
    assert_eq!(inferred_type(source, "user"), None);
}

#[test]
fn await_with_an_imported_promise_records_nothing() {
    let source = r#"
import { Promise } from "bluebird-like";
class User {}
function load(): Promise<User> { return null!; }
async function f() { const user = await load(); }
"#;
    assert_eq!(inferred_type(source, "user"), None);
}

#[test]
fn await_with_a_promise_type_alias_in_the_file_records_nothing() {
    let source = r#"
class User {}
type Promise<T> = { v: T };
function load(): Promise<User> { return null!; }
async function f() { const user = await load(); }
"#;
    assert_eq!(inferred_type(source, "user"), None);
}
