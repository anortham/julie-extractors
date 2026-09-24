use super::parse_cpp;
use crate::base::SymbolKind;

fn inferred_fact(source: &str, local: &str) -> Option<(String, bool, Option<String>)> {
    let (mut extractor, tree) = parse_cpp(source);
    let symbols = extractor.extract_symbols(&tree);
    let local = symbols
        .iter()
        .find(|s| s.name == local && matches!(s.kind, SymbolKind::Variable | SymbolKind::Constant))
        .unwrap_or_else(|| panic!("missing local {local}"));
    extractor.base.type_info.get(&local.id).map(|fact| {
        let declared = fact
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.get("declared"))
            .and_then(|declared| declared.as_str())
            .map(str::to_string);
        (fact.resolved_type.clone(), fact.is_inferred, declared)
    })
}

fn inferred_type(source: &str, local: &str) -> Option<(String, bool)> {
    inferred_fact(source, local).map(|(resolved, inferred, _)| (resolved, inferred))
}

fn inferred(name: &str) -> Option<(String, bool)> {
    Some((name.to_string(), true))
}

fn in_run(prelude: &str, body: &str) -> String {
    format!("{prelude}\nvoid run() {{\n    {body}\n}}\n")
}

const LOADERS: &str = r#"
class Foo {};
Foo load();
Foo* load_ptr() { return nullptr; }
Foo& load_ref();
const Foo& load_cref();
auto load_trailing() -> Foo;
"#;

#[test]
fn auto_from_same_file_function_records_its_return_type() {
    assert_eq!(
        inferred_fact(&in_run(LOADERS, "auto x = load();"), "x"),
        Some(("Foo".to_string(), true, None))
    );
}

#[test]
fn pointer_and_reference_auto_forms_record_the_base_type() {
    for (body, declared) in [
        ("auto x = load_ptr();", Some("Foo*")),
        ("auto* x = load_ptr();", Some("Foo*")),
        ("auto& x = load_ref();", None),
        ("const auto& x = load_cref();", None),
        ("auto x = load_cref();", None),
        ("decltype(auto) x = load_ref();", Some("Foo&")),
        ("decltype(auto) x = load_cref();", Some("const Foo&")),
    ] {
        assert_eq!(
            inferred_fact(&in_run(LOADERS, body), "x"),
            Some(("Foo".to_string(), true, declared.map(str::to_string))),
            "{body}"
        );
    }
}

#[test]
fn trailing_return_type_is_the_inferred_type() {
    assert_eq!(
        inferred_type(&in_run(LOADERS, "auto x = load_trailing();"), "x"),
        inferred("Foo")
    );
}

#[test]
fn library_wrappers_are_not_unwrapped() {
    let source = in_run(
        r#"
class Foo {};
std::unique_ptr<Foo> make_owned();
std::optional<Foo> find();
"#,
        "auto owned = make_owned();\n    auto found = find();",
    );
    assert_eq!(inferred_type(&source, "owned"), inferred("std::unique_ptr"));
    assert_eq!(inferred_type(&source, "found"), inferred("std::optional"));
}

#[test]
fn this_arrow_call_records_the_member_return_type() {
    let source = r#"
class Foo {};
class Widget {
public:
    Foo load();
    void run() {
        auto x = this->load();
        auto y = (*this).load();
    }
};
"#;
    assert_eq!(inferred_type(source, "x"), inferred("Foo"));
    assert_eq!(inferred_type(source, "y"), inferred("Foo"));
}

#[test]
fn this_call_in_out_of_line_method_uses_the_qualified_class() {
    let source = r#"
class Foo {};
namespace ns {
class Widget {
public:
    Foo load();
    void run();
};
}
void ns::Widget::run() {
    auto x = this->load();
}
"#;
    assert_eq!(inferred_type(source, "x"), inferred("Foo"));
}

#[test]
fn out_of_line_member_definition_supplies_the_return_type() {
    let source = r#"
class Foo {};
class Widget {
public:
    void run();
};
Foo Widget::load() { return Foo(); }
void Widget::run() {
    auto x = this->load();
    auto y = Widget::load();
}
"#;
    assert_eq!(inferred_type(source, "x"), inferred("Foo"));
    assert_eq!(inferred_type(source, "y"), inferred("Foo"));
}

#[test]
fn static_member_call_on_same_file_type_records_its_return_type() {
    let source = in_run(
        r#"
class Foo {};
namespace ns {
class Factory {
public:
    static Foo create();
};
}
"#,
        "auto x = ns::Factory::create();\n    auto y = Factory::create();",
    );
    assert_eq!(inferred_type(&source, "x"), inferred("Foo"));
    assert_eq!(inferred_type(&source, "y"), inferred("Foo"));
}

#[test]
fn unqualified_call_inside_a_method_prefers_the_member() {
    let source = r#"
class Foo {};
class Bar {};
Bar load();
Bar fallback();
class Widget {
    Foo load();
    void run() {
        auto x = load();
        auto y = fallback();
    }
};
"#;
    assert_eq!(inferred_type(source, "x"), inferred("Foo"));
    assert_eq!(inferred_type(source, "y"), inferred("Bar"));
}

#[test]
fn template_function_with_concrete_return_records_it() {
    let source = in_run(
        "class Foo {};\ntemplate <typename T> Foo convert(T value);",
        "auto x = convert<int>(1);\n    auto y = convert(1);",
    );
    assert_eq!(inferred_type(&source, "x"), inferred("Foo"));
    assert_eq!(inferred_type(&source, "y"), inferred("Foo"));
}

#[test]
fn overloads_that_agree_on_the_base_type_record_it() {
    let source = r#"
class Foo {};
class Widget {
public:
    Foo& get();
    const Foo& get() const;
    void run() {
        auto x = get();
    }
};
"#;
    assert_eq!(inferred_type(source, "x"), inferred("Foo"));
}

#[test]
fn overloads_that_disagree_record_no_fact() {
    let source = in_run(
        "class Foo {};\nclass Bar {};\nFoo load(int id);\nBar load(const char* name);",
        "auto x = load(1);",
    );
    assert_eq!(inferred_type(&source, "x"), None);
}

#[test]
fn same_named_functions_in_different_namespaces_that_disagree_record_no_fact() {
    let source = in_run(
        "class Foo {};\nclass Bar {};\nnamespace a { Foo load(); }\nnamespace b { Bar load(); }",
        "auto x = load();",
    );
    assert_eq!(inferred_type(&source, "x"), None);
}

#[test]
fn template_parameter_returns_record_no_fact() {
    let source = r#"
template <typename T> T make();
template <typename T> const T& pick(const T& a);
template <typename T> typename T::value_type first(const T& c);
template <typename T> T::value_type last(const T& c);
template <typename T>
class Box {
public:
    T get();
    void run() {
        auto from_member = this->get();
    }
};
void run() {
    auto made = make<int>();
    auto picked = pick(1);
    auto head = first(v);
    auto tail = last(v);
}
"#;
    for local in ["made", "picked", "head", "tail", "from_member"] {
        assert_eq!(inferred_type(source, local), None, "{local}");
    }
}

#[test]
fn deduced_return_type_records_no_fact() {
    let source = in_run(
        "class Foo {};\nauto load() { return Foo(); }",
        "auto x = load();",
    );
    assert_eq!(inferred_type(&source, "x"), None);
}

#[test]
fn call_on_a_non_this_receiver_records_no_fact() {
    let source = r#"
class Foo {};
class Widget {
public:
    Foo load();
    Foo child();
    void run(Widget other, Widget* ptr) {
        auto x = other.load();
        auto y = ptr->load();
        auto z = this->load().child();
    }
};
"#;
    for local in ["x", "y", "z"] {
        assert_eq!(inferred_type(source, local), None, "{local}");
    }
}

#[test]
fn this_call_to_another_class_member_records_no_fact() {
    let source = r#"
class Foo {};
class Other {
public:
    Foo load();
};
class Widget {
    void run() {
        auto x = this->load();
    }
};
"#;
    assert_eq!(inferred_type(source, "x"), None);
}

#[test]
fn qualified_call_without_a_same_file_member_records_no_fact() {
    let source = in_run(
        "class Foo {};\nFoo load();\nclass Factory {};",
        "auto x = Factory::load();\n    auto y = other::load();",
    );
    assert_eq!(inferred_type(&source, "x"), None);
    assert_eq!(inferred_type(&source, "y"), None);
}

#[test]
fn free_call_does_not_use_a_member_of_the_same_name() {
    let source = in_run(
        "class Foo {};\nclass Widget {\npublic:\n    Foo load();\n};",
        "auto x = load();",
    );
    assert_eq!(inferred_type(&source, "x"), None);
}

#[test]
fn written_type_wins_over_the_call_return_type() {
    let source = in_run(
        "class Foo {};\nclass Bar {};\nFoo load();",
        "Bar x = load();",
    );
    assert_eq!(
        inferred_type(&source, "x"),
        Some(("Bar".to_string(), false))
    );
}

#[test]
fn same_file_constructor_call_still_records_the_class() {
    let source = in_run("class Foo {};", "auto x = Foo();\n    auto y = new Foo();");
    assert_eq!(inferred_type(&source, "x"), inferred("Foo"));
    assert_eq!(inferred_type(&source, "y"), inferred("Foo"));
}

#[test]
fn unqualified_call_in_a_derived_class_method_records_no_fact() {
    let source = r#"
class Foo {};
class Base {};
Foo load();
class Widget : public Base {
    void run() {
        auto x = load();
    }
};
"#;
    assert_eq!(inferred_type(source, "x"), None);
}

#[test]
fn unqualified_call_in_a_nested_class_method_records_no_fact() {
    let source = r#"
class Foo {};
Foo load();
class Outer {
    class Inner {
        void run() {
            auto x = load();
        }
    };
};
"#;
    assert_eq!(inferred_type(source, "x"), None);
}

#[test]
fn unqualified_call_in_out_of_line_method_of_a_class_defined_elsewhere_records_no_fact() {
    let source = "class Foo {};\nFoo load();\nvoid Widget::run() {\n    auto x = load();\n}\n";
    assert_eq!(inferred_type(source, "x"), None);
}

#[test]
fn unqualified_call_in_out_of_line_method_of_a_same_file_class_records_the_free_return_type() {
    let source = r#"
class Foo {};
Foo load();
class Widget {
    void run();
};
void Widget::run() {
    auto x = load();
}
"#;
    assert_eq!(inferred_type(source, "x"), inferred("Foo"));
}

#[test]
fn unqualified_call_in_a_specialization_method_records_no_fact() {
    let source = r#"
class Foo {};
class Base {};
Foo load();
template <typename T> struct Box {};
template <> struct Box<int> : Base {
    void run() {
        auto x = load();
    }
};
"#;
    assert_eq!(inferred_type(source, "x"), None);
}

#[test]
fn friend_declarations_are_not_members() {
    let source = r#"
class Foo {};
class Widget {
    friend Foo make();
    void run() {
        auto x = Widget::make();
        auto y = this->make();
    }
};
"#;
    assert_eq!(inferred_type(source, "x"), None);
    assert_eq!(inferred_type(source, "y"), None);
}

#[test]
fn static_call_on_a_template_parameter_records_no_fact() {
    let source = r#"
class Foo {};
class T {
public:
    static Foo create();
};
template <typename T>
void run() {
    auto x = T::create();
}
"#;
    assert_eq!(inferred_type(source, "x"), None);
}

#[test]
fn return_type_hidden_behind_an_unknown_macro_records_no_fact() {
    let source = r#"
class Json {};
Json load() { return Json(); }
MACRO_ATTR static Json diff(const Json& source, const Json& target) {
    return source;
}
void run() {
    auto x = diff(load(), load());
}
"#;
    let (mut extractor, tree) = parse_cpp(source);
    let symbols = extractor.extract_symbols(&tree);
    let diff = symbols.iter().find(|s| s.name == "diff").expect("diff");
    assert_eq!(
        extractor
            .base
            .type_info
            .get(&diff.id)
            .map(|fact| fact.resolved_type.clone()),
        None
    );
    assert_eq!(inferred_type(source, "x"), None);
}

const FOO_BAR_LOAD: &str = "struct Foo {};\nstruct Bar {};\nFoo load();\nFoo make();";

#[test]
fn class_member_that_is_not_a_function_hides_the_free_function() {
    for member in [
        "std::function<Bar()> load;",
        "Bar (*load)();",
        "using load = Bar;",
        "typedef Bar load;",
        "enum Kind { load };",
        "static constexpr auto load = [] { return Bar{}; };",
    ] {
        let source = format!(
            "{FOO_BAR_LOAD}\nstruct Service {{\n    {member}\n    void run() {{\n        auto w = load();\n    }}\n    void later();\n}};\nvoid Service::later() {{\n    auto v = load();\n}}\n"
        );
        assert_eq!(inferred_type(&source, "w"), None, "{member}");
        assert_eq!(inferred_type(&source, "v"), None, "{member}");
    }
}

#[test]
fn local_or_parameter_with_the_callee_name_records_no_fact() {
    for function in [
        "void f() { auto load = [] { return Bar{}; }; auto x = load(); }",
        "void f(Bar (*load)()) { auto x = load(); }",
        "void f(std::function<Bar()> load) { auto x = load(); }",
        "template <class F> void each(F load) { auto x = load(); }",
        "auto g = [](auto load) { auto x = load(); return 0; };",
        "void f() { auto h = [load = [] { return Bar{}; }] { auto x = load(); return 0; }; }",
        "void f() { if (auto load = pick(); true) { auto x = load(); } }",
        "void f() { for (auto& load : loaders) { auto x = load(); } }",
        "void f() { try {} catch (Bar (*load)()) { auto x = load(); } }",
        "void f() { using load = Bar; auto x = load(); }",
        "void f() { Bar load(); auto x = load(); }",
    ] {
        let source = format!("{FOO_BAR_LOAD}\n{function}\n");
        assert_eq!(inferred_type(&source, "x"), None, "{function}");
    }
}

#[test]
fn local_with_the_callee_name_in_an_out_of_line_method_records_no_fact() {
    let source = format!(
        "{FOO_BAR_LOAD}\nstruct Plain {{\n    void run();\n}};\nvoid Plain::run() {{\n    auto make = [] {{ return Bar{{}}; }};\n    auto x = make();\n    auto y = load();\n}}\n"
    );
    assert_eq!(inferred_type(&source, "x"), None);
    assert_eq!(inferred_type(&source, "y"), inferred("Foo"));
}

#[test]
fn a_local_in_another_function_does_not_hide_the_callee() {
    let source = format!(
        "{FOO_BAR_LOAD}\nvoid other(int load) {{}}\nvoid f() {{\n    {{ auto load = 1; }}\n    auto x = load();\n}}\n"
    );
    assert_eq!(inferred_type(&source, "x"), inferred("Foo"));
}

#[test]
fn free_function_in_a_namespace_that_does_not_enclose_the_call_records_no_fact() {
    for source in [
        "#include \"other.h\"\nstruct Foo {};\nnamespace detail { Foo helper(); }\nvoid use() { auto x = helper(); }\n",
        "struct Foo {};\nnamespace n1 { Foo helper(); }\nnamespace n2 { void f() { auto x = helper(); } }\n",
        "struct Foo {};\nstruct Bar {};\nFoo helper();\nnamespace n1 { struct S {}; Bar helper(S); }\nvoid f() { auto x = helper(n1::S{}); }\n",
    ] {
        assert_eq!(inferred_type(source, "x"), None, "{source}");
    }
}

#[test]
fn free_function_in_an_enclosing_namespace_records_its_return_type() {
    let source = r#"
struct Foo {};
struct Bar {};
Bar helper();
namespace a {
Foo load();
namespace b {
void f() {
    auto x = load();
}
}
namespace {
Foo tool();
}
inline namespace v1 {
Foo util();
}
void g() {
    auto y = tool();
    auto z = util();
}
}
"#;
    assert_eq!(inferred_type(source, "x"), inferred("Foo"));
    assert_eq!(inferred_type(source, "y"), inferred("Foo"));
    assert_eq!(inferred_type(source, "z"), inferred("Foo"));
}

#[test]
fn inner_namespace_name_hides_an_outer_free_function() {
    for inner in [
        "std::function<Bar()> load;",
        "Bar load();",
        "using other::load;",
    ] {
        let source = format!(
            "struct Foo {{}};\nstruct Bar {{}};\nFoo load();\nnamespace n {{\n{inner}\nvoid f() {{ auto x = load(); }}\n}}\n"
        );
        let expected = (inner == "Bar load();").then(|| ("Bar".to_string(), true));
        assert_eq!(inferred_type(&source, "x"), expected, "{inner}");
    }
}

#[test]
fn method_of_a_namespaced_class_uses_the_class_namespace() {
    let source = r#"
struct Foo {};
struct Bar {};
Bar load();
namespace ns {
Foo load();
struct Widget {
    void run();
};
}
void ns::Widget::run() {
    auto x = load();
}
"#;
    assert_eq!(inferred_type(source, "x"), inferred("Foo"));
}

#[test]
fn single_name_structured_binding_records_no_fact() {
    let source = in_run(
        "struct Bar {};\nstruct One { Bar b; };\nOne one();",
        "auto [x] = one();\n    auto [y] = One();\n    const auto& [z] = one();",
    );
    for local in ["x", "y", "z"] {
        assert_eq!(inferred_type(&source, local), None, "{local}");
    }
}

#[test]
fn function_misparsed_into_a_block_by_a_namespace_macro_is_not_a_local() {
    let source = r#"
MACRO_NAMESPACE_BEGIN
namespace detail
{
inline std::size_t combine(std::size_t seed, std::size_t h) noexcept { return seed ^ h; }
template<typename T>
std::size_t hash(const T& j)
{
    auto total = combine(1, 2);
    return total;
}
}
MACRO_NAMESPACE_END
"#;
    assert_eq!(inferred_type(source, "total"), inferred("std::size_t"));
}
