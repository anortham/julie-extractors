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
        .find(|s| s.name == local)
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
    return load();
}
function run() { const user = load(); }
"#;
    assert_eq!(inferred_type(source, "user"), None);
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
