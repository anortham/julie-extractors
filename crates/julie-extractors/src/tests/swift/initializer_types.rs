use crate::swift::SwiftExtractor;
use std::path::PathBuf;

#[derive(Debug, PartialEq, Eq)]
struct Inferred {
    resolved: String,
    is_inferred: bool,
    declared: Option<String>,
}

fn inferred(resolved: &str, declared: Option<&str>) -> Option<Inferred> {
    Some(Inferred {
        resolved: resolved.to_string(),
        is_inferred: true,
        declared: declared.map(str::to_string),
    })
}

fn type_of(source: &str, local: &str) -> Option<Inferred> {
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_swift::LANGUAGE.into())
        .unwrap();
    let tree = parser.parse(source, None).unwrap();
    let mut extractor = SwiftExtractor::new(
        "swift".to_string(),
        "initializer_types.swift".to_string(),
        source.to_string(),
        &PathBuf::from("/tmp/test"),
    );
    let symbols = extractor.extract_symbols(&tree);
    let matching: Vec<_> = symbols.iter().filter(|s| s.name == local).collect();
    assert_eq!(matching.len(), 1, "expected one symbol named {local}");
    extractor
        .base
        .type_info
        .get(&matching[0].id)
        .map(|fact| Inferred {
            resolved: fact.resolved_type.clone(),
            is_inferred: fact.is_inferred,
            declared: fact
                .metadata
                .as_ref()
                .and_then(|m| m.get("declared"))
                .and_then(|v| v.as_str())
                .map(str::to_string),
        })
}

const LOADERS: &str = r#"
class Workspace {}
func loadWorkspace() -> Workspace { fatalError() }
func findWorkspace() -> Workspace? { fatalError() }
func openWorkspace() throws -> Workspace { fatalError() }
func fetchWorkspace() async throws -> Workspace { fatalError() }
func qualifiedWorkspace() -> Store.Workspace { fatalError() }
func noResult() {}
"#;

fn in_function(body: &str) -> String {
    format!("{LOADERS}\nfunc run() async throws {{\n    {body}\n}}\n")
}

#[test]
fn same_file_function_call_records_return_type_as_inferred() {
    assert_eq!(
        type_of(&in_function("let ws = loadWorkspace()"), "ws"),
        inferred("Workspace", None)
    );
}

#[test]
fn optional_return_records_base_name_and_optional_declared_text() {
    assert_eq!(
        type_of(&in_function("let ws = findWorkspace()"), "ws"),
        inferred("Workspace", Some("Workspace?"))
    );
}

#[test]
fn qualified_return_records_qualified_name() {
    assert_eq!(
        type_of(&in_function("let ws = qualifiedWorkspace()"), "ws"),
        inferred("Store.Workspace", None)
    );
}

#[test]
fn try_and_await_pass_the_call_type_through() {
    for value in [
        "try openWorkspace()",
        "try! openWorkspace()",
        "await fetchWorkspace()",
        "try await fetchWorkspace()",
    ] {
        assert_eq!(
            type_of(&in_function(&format!("let ws = {value}")), "ws"),
            inferred("Workspace", None),
            "{value}"
        );
    }
}

#[test]
fn optional_try_records_optional_declared_text() {
    assert_eq!(
        type_of(&in_function("let ws = try? openWorkspace()"), "ws"),
        inferred("Workspace", Some("Workspace?"))
    );
    assert_eq!(
        type_of(&in_function("let ws = try? findWorkspace()"), "ws"),
        inferred("Workspace", Some("Workspace?"))
    );
}

#[test]
fn force_unwrap_removes_the_optional_from_declared_text() {
    assert_eq!(
        type_of(&in_function("let ws = findWorkspace()!"), "ws"),
        inferred("Workspace", None)
    );
}

#[test]
fn constructor_through_try_still_records_the_type() {
    assert_eq!(
        type_of(&in_function("let ws = try Workspace()"), "ws"),
        inferred("Workspace", None)
    );
}

#[test]
fn top_level_binding_records_return_type() {
    let source = format!("{LOADERS}\nlet shared = loadWorkspace()\n");
    assert_eq!(type_of(&source, "shared"), inferred("Workspace", None));
}

#[test]
fn trailing_closure_call_records_return_type() {
    let source = r#"
class Workspace {}
func withWorkspace(_ body: () -> Void) -> Workspace { fatalError() }
func run() {
    let ws = withWorkspace { }
}
"#;
    assert_eq!(type_of(source, "ws"), inferred("Workspace", None));
}

const SESSION: &str = r#"
class Request {}
final class Session {
    func request() -> Request { fatalError() }
    static func make() -> Self { fatalError() }
    class func shared() -> Session { fatalError() }
    func run() {
        BODY
    }
}
extension Session {
    func extraRequest() throws -> Request { fatalError() }
}
"#;

fn in_session(body: &str) -> String {
    SESSION.replace("BODY", body)
}

#[test]
fn self_method_call_records_member_return_type() {
    assert_eq!(
        type_of(&in_session("let req = self.request()"), "req"),
        inferred("Request", None)
    );
}

#[test]
fn implicit_self_call_records_member_return_type() {
    assert_eq!(
        type_of(&in_session("let req = request()"), "req"),
        inferred("Request", None)
    );
}

#[test]
fn same_file_extension_member_counts_as_member() {
    assert_eq!(
        type_of(&in_session("let req = try self.extraRequest()"), "req"),
        inferred("Request", None)
    );
}

#[test]
fn static_calls_on_type_name_and_self_type_record_return_type() {
    assert_eq!(
        type_of(&in_session("let made = Session.make()"), "made"),
        inferred("Session", Some("Self"))
    );
    assert_eq!(
        type_of(&in_session("let made = Self.make()"), "made"),
        inferred("Session", Some("Self"))
    );
    assert_eq!(
        type_of(&in_session("let made = Session.shared()"), "made"),
        inferred("Session", None)
    );
}

#[test]
fn implicit_call_in_type_without_inheritance_falls_back_to_free_function() {
    let source = r#"
class Workspace {}
func loadWorkspace() -> Workspace { fatalError() }
struct Loader {
    func run() {
        let ws = loadWorkspace()
    }
}
"#;
    assert_eq!(type_of(source, "ws"), inferred("Workspace", None));
}

#[test]
fn in_scope_local_function_shadows_free_function() {
    let source = r#"
class Workspace {}
class Draft {}
func load() -> Workspace { fatalError() }
func run() {
    func load() -> Draft { fatalError() }
    let item = load()
}
"#;
    assert_eq!(type_of(source, "item"), inferred("Draft", None));
}

#[test]
fn written_type_wins_over_call_inference() {
    let source = in_function("let ws: Store.Workspace = loadWorkspace()");
    let fact = type_of(&source, "ws").unwrap();
    assert_eq!(fact.resolved, "Store.Workspace");
    assert!(!fact.is_inferred);
}

#[test]
fn unknown_or_void_callee_records_nothing() {
    assert_eq!(type_of(&in_function("let ws = elsewhere()"), "ws"), None);
    assert_eq!(type_of(&in_function("let ws = noResult()"), "ws"), None);
}

#[test]
fn disagreeing_overloads_that_fit_the_labels_record_nothing() {
    let source = r#"
class Workspace {}
class Draft {}
func load(id: Int) -> Workspace { fatalError() }
func load(id: String) -> Draft { fatalError() }
func run() {
    let item = load(id: 1)
}
"#;
    assert_eq!(type_of(source, "item"), None);
}

#[test]
fn argument_labels_pick_among_disagreeing_overloads() {
    let source = r#"
class Workspace {}
class Draft {}
func load(id: Int) -> Workspace { fatalError() }
func load(name: String) -> Draft { fatalError() }
func run() {
    let byId = load(id: 1)
    let byName = load(name: "n")
}
"#;
    assert_eq!(type_of(source, "byId"), inferred("Workspace", None));
    assert_eq!(type_of(source, "byName"), inferred("Draft", None));
}

#[test]
fn agreeing_overloads_record_the_shared_type() {
    let source = r#"
class Workspace {}
func load(id: Int) -> Workspace { fatalError() }
func load(name: String) -> Workspace { fatalError() }
func run() {
    let item = load(id: 1)
}
"#;
    assert_eq!(type_of(source, "item"), inferred("Workspace", None));
}

#[test]
fn generic_returns_record_nothing() {
    let source = r#"
func decode<T>() -> T { fatalError() }
class Box<Element> {
    func first() -> Element? { fatalError() }
    func run() {
        let decoded = decode()
        let head = self.first()
    }
}
"#;
    assert_eq!(type_of(source, "decoded"), None);
    assert_eq!(type_of(source, "head"), None);
}

#[test]
fn chain_ending_in_unknown_method_records_nothing() {
    assert_eq!(
        type_of(&in_session("let req = self.request().retry()"), "req"),
        None
    );
}

#[test]
fn receivers_other_than_self_or_a_same_file_type_record_nothing() {
    assert_eq!(
        type_of(&in_session("let req = other.request()"), "req"),
        None
    );
    assert_eq!(
        type_of(&in_session("let req = super.request()"), "req"),
        None
    );
    assert_eq!(
        type_of(&in_session("let req = Remote.request()"), "req"),
        None
    );
}

#[test]
fn type_name_call_to_instance_method_records_nothing() {
    assert_eq!(
        type_of(&in_session("let req = Session.request()"), "req"),
        None
    );
}

#[test]
fn implicit_call_in_inheriting_type_records_nothing() {
    let source = r#"
class Workspace {}
func loadWorkspace() -> Workspace { fatalError() }
class Loader: Base {
    func run() {
        let ws = loadWorkspace()
    }
}
"#;
    assert_eq!(type_of(source, "ws"), None);
}

#[test]
fn implicit_call_in_extension_of_other_file_type_records_nothing() {
    let source = r#"
class Workspace {}
func loadWorkspace() -> Workspace { fatalError() }
extension Remote {
    func loadWorkspace() -> Workspace { fatalError() }
    func run() {
        let ws = loadWorkspace()
        let other = self.loadWorkspace()
    }
}
"#;
    assert_eq!(type_of(source, "ws"), None);
    assert_eq!(type_of(source, "other"), None);
}

#[test]
fn protocol_extension_members_record_nothing() {
    let source = r#"
protocol Store {
    associatedtype Item
}
extension Store {
    func first() -> Item { fatalError() }
    func run() {
        let head = self.first()
    }
}
"#;
    assert_eq!(type_of(source, "head"), None);
}

#[test]
fn callee_name_bound_as_a_value_records_nothing() {
    let source = r#"
class Workspace {}
class Draft {}
func load() -> Workspace { fatalError() }
func run(load: () -> Draft) {
    let item = load()
}
"#;
    assert_eq!(type_of(source, "item"), None);
}

#[test]
fn callee_name_bound_by_if_let_or_if_var_records_nothing() {
    let source = r#"
func load() -> Int { 0 }
func g(handler: (() -> String)?, other: (() -> String)?) {
    if let load = handler { let e01 = load() }
    if var load = other { let e02 = load() }
}
"#;
    assert_eq!(type_of(source, "e01"), None);
    assert_eq!(type_of(source, "e02"), None);
}

#[test]
fn member_call_colliding_with_member_property_records_nothing() {
    let source = r#"
class Workspace {}
class Draft {}
class Session {
    var load: () -> Draft = { fatalError() }
    func load(id: Int) -> Workspace { fatalError() }
    func run() {
        let item = self.load()
    }
}
"#;
    assert_eq!(type_of(source, "item"), None);
}

#[test]
fn local_function_in_another_scope_records_nothing() {
    let source = r#"
class Workspace {}
func setup() {
    func helper() -> Workspace { fatalError() }
}
func run() {
    let item = helper()
}
"#;
    assert_eq!(type_of(source, "item"), None);
}

#[test]
fn destructured_bindings_record_nothing() {
    let source = r#"
class Workspace {}
func pair() -> Workspace { fatalError() }
func run() {
    let (first, second) = pair()
    let (left, right) = Workspace()
}
"#;
    assert_eq!(type_of(source, "first"), None);
    assert_eq!(type_of(source, "second"), None);
    assert_eq!(type_of(source, "left"), None);
    assert_eq!(type_of(source, "right"), None);
}

#[test]
fn optional_try_on_implicitly_unwrapped_return_records_optional_declared_text() {
    let source = r#"
class Workspace {}
func lenientWorkspace() throws -> Workspace! { fatalError() }
func run() {
    let ws = try? lenientWorkspace()
}
"#;
    assert_eq!(
        type_of(source, "ws"),
        inferred("Workspace", Some("Workspace?"))
    );
}

#[test]
fn stored_property_initializer_records_return_type() {
    let source = r#"
class Workspace {}
func loadWorkspace() -> Workspace { fatalError() }
struct Holder {
    let ws = loadWorkspace()
}
"#;
    assert_eq!(type_of(source, "ws"), inferred("Workspace", None));
}

#[test]
fn local_function_returning_a_generic_of_an_enclosing_init_or_subscript_records_nothing() {
    let source = r#"
struct Holder {
    init<Seed>(seed: Seed) {
        func echo() -> Seed { seed }
        let fromInit = echo()
    }
    subscript<Key>(key: Key) -> Int {
        func echo() -> Key { key }
        let fromSubscript = echo()
        return 0
    }
}
"#;
    assert_eq!(type_of(source, "fromInit"), None);
    assert_eq!(type_of(source, "fromSubscript"), None);
}

#[test]
fn local_function_in_extension_of_other_file_type_records_nothing() {
    let source = r#"
extension Remote {
    func run() {
        func head() -> Element { fatalError() }
        let first = head()
    }
}
"#;
    assert_eq!(type_of(source, "first"), None);
}

#[test]
fn member_call_in_inheriting_type_records_nothing() {
    let source = r#"
class Base { func load() -> String { "" } }
class Sub: Base {
    func load(x: Int) -> Int { 0 }
    func f() {
        let c03a = self.load()
        let c03b = load()
    }
}
"#;
    assert_eq!(type_of(source, "c03a"), None);
    assert_eq!(type_of(source, "c03b"), None);
}

#[test]
fn member_call_in_type_conforming_to_protocol_records_nothing() {
    let source = r#"
protocol P {}
extension P { func load() -> String { "" } }
struct Foo: P {
    func load(x: Int) -> Int { 0 }
    func f() { let c04 = self.load() }
}
"#;
    assert_eq!(type_of(source, "c04"), None);
}

#[test]
fn static_call_on_inheriting_type_records_nothing() {
    let source = r#"
class Base { class func make() -> String { "" } }
class Sub: Base { static func make(x: Int) -> Int { 0 } }
let c05 = Sub.make()
"#;
    assert_eq!(type_of(source, "c05"), None);
}

#[test]
fn nested_types_sharing_a_name_record_nothing() {
    let source = r#"
func load() -> String { "" }
struct A { struct Node {
    func load() -> Int { 0 }
    static func make() -> Int { 0 }
} }
struct B { struct Node {
    func f() { let c01 = load() }
} }
let c06 = Node.make()
"#;
    assert_eq!(type_of(source, "c01"), None);
    assert_eq!(type_of(source, "c06"), None);
}

#[test]
fn unqualified_call_in_nested_type_finds_static_member_of_outer_type() {
    let source = r#"
func helper() -> String { "" }
struct Outer {
    static func helper() -> Int { 0 }
    func other() -> Int { 0 }
    struct Inner {
        func f() {
            let c02 = helper()
            let c07 = other()
        }
    }
}
"#;
    assert_eq!(type_of(source, "c02"), inferred("Int", None));
    assert_eq!(type_of(source, "c07"), None);
}

#[test]
fn unqualified_call_in_type_nested_in_inheriting_type_records_nothing() {
    let source = r#"
func helper() -> String { "" }
class Outer: Base {
    struct Inner {
        func f() { let c08 = helper() }
    }
}
"#;
    assert_eq!(type_of(source, "c08"), None);
}

#[test]
fn static_subscript_on_same_file_type_records_nothing() {
    let source = r#"
struct Foo { static subscript(i: Int) -> Int { i } }
let c10 = Foo[0]
let c11 = try Foo[0]
"#;
    assert_eq!(type_of(source, "c10"), None);
    assert_eq!(type_of(source, "c11"), None);
}

#[test]
fn call_whose_labels_or_argument_count_fit_no_same_file_function_records_nothing() {
    let source = r#"
struct Workspace {}
func load() -> Workspace { Workspace() }
final class Store {
    func fetch() -> Workspace { Workspace() }
    static func make() -> Workspace { Workspace() }
    func run() {
        let memberLabel = self.fetch(path: "p")
        let implicitCount = fetch(1)
        let staticLabel = Store.make(x: 1)
    }
}
func run() {
    let labelMismatch = load(path: "p")
    let argMismatch = load(42)
}
"#;
    for local in [
        "labelMismatch",
        "argMismatch",
        "memberLabel",
        "implicitCount",
        "staticLabel",
    ] {
        assert_eq!(type_of(source, local), None, "{local}");
    }
}

#[test]
fn call_that_fits_labels_defaults_variadics_and_trailing_closure_records_return_type() {
    let source = r#"
struct Workspace {}
func open(path p: String, mode: Int = 0, _ tags: String..., done: () -> Void) -> Workspace { Workspace() }
func watch(done: @escaping () -> Void) -> Workspace { Workspace() }
func run() {
    let withTags = open(path: "p", "a", "b", done: {})
    let withMode = open(path: "p", mode: 1, done: {})
    let trailing = watch { }
    let parenthesesAndTrailing = open(path: "p") { }
}
"#;
    for local in ["withTags", "withMode", "trailing"] {
        assert_eq!(
            type_of(source, local),
            inferred("Workspace", None),
            "{local}"
        );
    }
    assert_eq!(type_of(source, "parenthesesAndTrailing"), None);
}

#[test]
fn trailing_closure_that_may_skip_a_defaulted_parameter_records_nothing() {
    let source = r#"
struct Workspace {}
func load(retries: Int = 0, done: () -> Void) -> Workspace { Workspace() }
func run() {
    let unsure = load { }
}
"#;
    assert_eq!(type_of(source, "unsure"), None);
}

#[test]
fn nested_type_name_resolves_only_where_the_nested_type_is_visible() {
    let source = r#"
struct Parser {
    enum Options { static func defaults() -> Int { 1 } }
    func parse() {
        let inParser = Options.defaults()
    }
}
func run() {
    let nestedAtFile = Options.defaults()
    enum Local { static func make() -> Int { 1 } }
    let localReceiver = Local.make()
}
"#;
    assert_eq!(type_of(source, "inParser"), inferred("Int", None));
    assert_eq!(type_of(source, "nestedAtFile"), None);
    assert_eq!(type_of(source, "localReceiver"), None);
}

#[test]
fn extension_named_like_a_nested_type_extends_another_type() {
    let source = r#"
struct Parser {
    enum Options {}
}
extension Options {
    static func defaults() -> Int { 1 }
    func run() {
        let viaExtension = defaults()
        let viaSelf = Self.defaults()
    }
}
"#;
    assert_eq!(type_of(source, "viaExtension"), None);
    assert_eq!(type_of(source, "viaSelf"), None);
}

#[test]
fn member_of_local_type_shadows_outer_local_function() {
    let source = r#"
struct A {}
struct B {}
func outer() {
    func load() -> A { A() }
    final class C {
        func load() -> B { B() }
        func g() { let memberWins = load() }
    }
    let localWins = load()
}
"#;
    assert_eq!(type_of(source, "memberWins"), inferred("B", None));
    assert_eq!(type_of(source, "localWins"), inferred("A", None));
}

#[test]
fn optional_generic_spelling_records_the_wrapped_type() {
    let source = r#"
struct Workspace {}
func maybe() -> Optional<Workspace> { nil }
func qualified() -> Swift.Optional<Workspace> { nil }
func lenient() -> Workspace! { nil }
func wrap<T>() -> Optional<T> { nil }
let forcedOptional = maybe()!
let plainOptional = maybe()
let qualifiedOptional = qualified()
let lenientResult = lenient()
let wrapped = wrap()
"#;
    assert_eq!(
        type_of(source, "forcedOptional"),
        inferred("Workspace", None)
    );
    for local in ["plainOptional", "qualifiedOptional", "lenientResult"] {
        assert_eq!(
            type_of(source, local),
            inferred("Workspace", Some("Workspace?")),
            "{local}"
        );
    }
    assert_eq!(type_of(source, "wrapped"), None);
}

#[test]
fn typealias_for_a_generic_parameter_counts_as_generic() {
    let source = r#"
struct Box<T> {
    typealias Item = T
    typealias Items = [Item]
    typealias Count = Int
    func get() -> Item { fatalError() }
    func all() -> Items { fatalError() }
    func count() -> Count { 0 }
    func use() {
        let aliasGeneric = get()
        let aliasOfAlias = all()
        let concreteAlias = count()
    }
}
"#;
    assert_eq!(type_of(source, "aliasGeneric"), None);
    assert_eq!(type_of(source, "aliasOfAlias"), None);
    assert_eq!(type_of(source, "concreteAlias"), inferred("Count", None));
}

#[test]
fn type_name_receiver_inside_inheriting_or_unknown_type_records_nothing() {
    let source = r#"
enum Helper { static func make() -> Int { 1 } }
class Sub: Base {
    func f() { let inSub = Helper.make() }
}
extension Remote {
    func f() { let inRemote = Helper.make() }
}
struct Plain {
    func f() { let inPlain = Helper.make() }
}
"#;
    assert_eq!(type_of(source, "inSub"), None);
    assert_eq!(type_of(source, "inRemote"), None);
    assert_eq!(type_of(source, "inPlain"), inferred("Int", None));
}
