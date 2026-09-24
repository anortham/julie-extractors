use crate::base::{ExtractionResults, RelationshipKind, Symbol, SymbolKind, Visibility};
use crate::extract_canonical;
use std::path::Path;

fn extract(path: &str, source: &str) -> ExtractionResults {
    extract_canonical(path, source, Path::new("/tmp/test"))
        .expect("canonical C++ extraction must succeed")
}

fn rows<'a>(result: &'a ExtractionResults, name: &str) -> Vec<&'a Symbol> {
    result.symbols.iter().filter(|s| s.name == name).collect()
}

fn only<'a>(result: &'a ExtractionResults, name: &str) -> &'a Symbol {
    let found = rows(result, name);
    assert_eq!(
        found.len(),
        1,
        "expected one `{name}` row, got {found:#?}\nall: {:?}",
        result
            .symbols
            .iter()
            .map(|s| (&s.name, &s.kind))
            .collect::<Vec<_>>()
    );
    found[0]
}

fn parent_name(result: &ExtractionResults, symbol: &Symbol) -> Option<String> {
    let parent = symbol.parent_id.as_deref()?;
    result
        .symbols
        .iter()
        .find(|s| s.id == parent)
        .map(|s| s.name.clone())
}

fn metadata_str<'a>(symbol: &'a Symbol, key: &str) -> Option<&'a str> {
    symbol.metadata.as_ref()?.get(key)?.as_str()
}

fn resolved_type(result: &ExtractionResults, symbol: &Symbol) -> Option<String> {
    result
        .types
        .get(&symbol.id)
        .map(|fact| fact.resolved_type.clone())
}

#[test]
fn prototypes_with_qualified_return_types_are_named_by_their_declarator() {
    let result = extract(
        "qret.hpp",
        r#"class Repo {
public:
    std::optional<int> find(int id) const;
    virtual std::vector<std::string> names() const = 0;
    std::string label() const;
    static std::unique_ptr<Repo> make(double r);
};
std::optional<std::string> lookup(const std::string& key);
"#,
    );
    assert!(result.symbols.iter().all(|s| !s.name.starts_with("std::")));
    for name in ["find", "names", "label", "make"] {
        let method = only(&result, name);
        assert_eq!(method.kind, SymbolKind::Method, "{name}");
        assert_eq!(parent_name(&result, method).as_deref(), Some("Repo"));
    }
    assert_eq!(
        only(&result, "find").signature.as_deref(),
        Some("std::optional<int> find(int id) const")
    );
    let lookup = only(&result, "lookup");
    assert_eq!(lookup.kind, SymbolKind::Function);
    assert!(resolved_type(&result, lookup).is_some());
}

#[test]
fn wrapped_declarators_are_named_by_their_innermost_identifier() {
    let result = extract(
        "ptrvars.cpp",
        r#"Widget* g_default = nullptr;
Widget* g_current = g_default;
const char* kAppName = "inventory";
int g_table[10];
std::string g_first, g_second;
void (*g_handler)(int) = nullptr;
void run(Widget* other) {
    Widget* target = other;
    Widget& alias = *other;
    auto* p = new Widget();
    auto [key, value] = pair();
    const auto& [first, second] = pair();
    auto&& [left, right] = pair();
}
class Holder {
    Repo& repo_;
    const std::string& name_;
    int buffer_[16];
    void (*callback_)(int);
};
"#,
    );
    for name in [
        "g_default",
        "g_current",
        "kAppName",
        "g_table",
        "g_first",
        "g_second",
        "g_handler",
        "target",
        "alias",
        "p",
        "key",
        "value",
        "first",
        "second",
        "left",
        "right",
    ] {
        let symbol = only(&result, name);
        assert!(
            matches!(symbol.kind, SymbolKind::Variable | SymbolKind::Constant),
            "{name}: {:?}",
            symbol.kind
        );
    }
    assert_eq!(rows(&result, "other").len(), 1, "only the parameter");
    for name in [
        "target", "alias", "p", "key", "value", "first", "second", "left", "right",
    ] {
        assert_eq!(
            parent_name(&result, only(&result, name)).as_deref(),
            Some("run"),
            "{name}"
        );
    }
    for name in ["repo_", "name_", "buffer_", "callback_"] {
        let field = only(&result, name);
        assert!(
            matches!(field.kind, SymbolKind::Field | SymbolKind::Constant),
            "{name}"
        );
        assert_eq!(parent_name(&result, field).as_deref(), Some("Holder"));
    }
    assert_eq!(
        resolved_type(&result, only(&result, "g_current")).as_deref(),
        Some("Widget")
    );
    assert_eq!(
        resolved_type(&result, only(&result, "alias")).as_deref(),
        Some("Widget")
    );
    assert_eq!(
        resolved_type(&result, only(&result, "repo_")).as_deref(),
        Some("Repo")
    );
}

#[test]
fn block_scope_direct_initialization_declares_variables() {
    let result = extract(
        "directinit.cpp",
        r#"void Store::save(const std::string& path) {
    std::lock_guard<std::mutex> lock(mutex_);
    std::ofstream out(path);
    Writer writer(out, options_);
    writer.flush();
    Widget w(1, 2);
}
"#,
    );
    for name in ["lock", "out", "writer", "w"] {
        let symbol = only(&result, name);
        assert_eq!(symbol.kind, SymbolKind::Variable, "{name}");
        assert_eq!(
            parent_name(&result, symbol).as_deref(),
            Some("Store::save"),
            "{name}"
        );
    }
    assert!(
        result
            .symbols
            .iter()
            .all(|s| s.kind != SymbolKind::Function || s.name == "Store::save")
    );
    assert_eq!(
        resolved_type(&result, only(&result, "writer")).as_deref(),
        Some("Writer")
    );
    for argument in ["mutex_", "path", "out", "options_"] {
        assert!(
            result
                .structured_pending_relationships
                .iter()
                .all(|p| !(p.pending.kind == RelationshipKind::Uses
                    && p.target.terminal_name == argument)),
            "{argument} is not a type"
        );
    }
}

#[test]
fn alias_declarations_are_type_rows() {
    let result = extract(
        "alias.hpp",
        r#"namespace net {
/// Handler invoked per request.
using Handler = std::function<void(int)>;
template <typename T>
using List = std::vector<T>;
class Server { public: using Ptr = Server*; Handler h; };
}
"#,
    );
    let handler = only(&result, "Handler");
    assert_eq!(handler.kind, SymbolKind::Type);
    assert_eq!(parent_name(&result, handler).as_deref(), Some("net"));
    assert!(
        handler
            .doc_comment
            .as_deref()
            .is_some_and(|doc| doc.contains("Handler invoked"))
    );
    assert_eq!(
        handler.signature.as_deref(),
        Some("using Handler = std::function<void(int)>")
    );
    let list = only(&result, "List");
    assert_eq!(list.kind, SymbolKind::Type);
    assert!(
        list.signature
            .as_deref()
            .is_some_and(|s| s.starts_with("template"))
    );
    let ptr = only(&result, "Ptr");
    assert_eq!(ptr.kind, SymbolKind::Type);
    assert_eq!(parent_name(&result, ptr).as_deref(), Some("Server"));
    for alias in ["Handler", "List", "Ptr"] {
        assert!(
            result
                .structured_pending_relationships
                .iter()
                .all(|p| p.target.terminal_name != alias),
            "{alias}"
        );
    }
    assert!(
        result
            .relationships
            .iter()
            .any(|r| r.kind == RelationshipKind::Uses && r.to_symbol_id == handler.id),
        "the field h uses the local alias"
    );
}

#[test]
fn out_of_line_member_definitions_belong_to_their_class() {
    let result = extract(
        "ool.cpp",
        r#"class Widget {
public:
    Widget();
    ~Widget();
    void draw() const;
    bool operator==(const Widget& o) const;
    static int instances;
private:
    int compute() const;
};
Widget::Widget() {}
Widget::~Widget() { compute(); }
void Widget::draw() const { compute(); }
int Widget::compute() const { return 1; }
bool Widget::operator==(const Widget& o) const { return true; }
int Widget::instances = 0;
struct Counter { operator bool() const; };
Counter::operator bool() const { return true; }
"#,
    );
    let kind_of = |name: &str| only(&result, name).kind.clone();
    assert_eq!(kind_of("Widget::Widget"), SymbolKind::Constructor);
    assert_eq!(kind_of("Widget::~Widget"), SymbolKind::Destructor);
    assert_eq!(kind_of("Widget::operator=="), SymbolKind::Operator);
    assert_eq!(kind_of("Widget::draw"), SymbolKind::Method);
    assert_eq!(kind_of("Widget::compute"), SymbolKind::Method);
    for name in [
        "Widget::Widget",
        "Widget::~Widget",
        "Widget::draw",
        "Widget::compute",
        "Widget::operator==",
    ] {
        let symbol = only(&result, name);
        assert_eq!(
            parent_name(&result, symbol).as_deref(),
            Some("Widget"),
            "{name}"
        );
        assert_eq!(metadata_str(symbol, "scope"), Some("Widget"), "{name}");
    }
    assert_eq!(
        only(&result, "Widget::compute").visibility,
        Some(Visibility::Private)
    );
    assert_eq!(
        resolved_type(&result, only(&result, "Widget::operator==")).as_deref(),
        Some("bool")
    );
    assert_eq!(
        resolved_type(&result, only(&result, "Widget::compute")).as_deref(),
        Some("int")
    );
    let instances = only(&result, "Widget::instances");
    assert_eq!(parent_name(&result, instances).as_deref(), Some("Widget"));
    assert!(
        result
            .symbols
            .iter()
            .any(|s| s.name == "Counter::operator bool" && s.kind == SymbolKind::Operator)
    );
}

#[test]
fn out_of_line_test_role_uses_the_member_name() {
    let result = extract(
        "src/parser.cpp",
        "void TestParser::initTestCase() {}\nvoid Parser::parse() {}\n",
    );
    let init = only(&result, "TestParser::initTestCase");
    assert_eq!(metadata_str(init, "test_role"), None);
    assert_eq!(metadata_str(init, "scope"), Some("TestParser"));
}

#[test]
fn base_classes_produce_extends_edges() {
    let result = extract(
        "inherit.cpp",
        r#"#include "base.h"
class Local {};
namespace core { class Base {}; }
template <typename T> class Crtp {};
class Derived : public RemoteBase, private Local, public ns::Mixin, public Templ<int> {};
class A : public core::Base {};
class B : public Crtp<B> {};
"#,
    );
    let id = |name: &str| only(&result, name).id.clone();
    let extends = |from: &str, to: &str| {
        result.relationships.iter().any(|r| {
            r.kind == RelationshipKind::Extends
                && r.from_symbol_id == id(from)
                && r.to_symbol_id == id(to)
        })
    };
    assert!(extends("Derived", "Local"));
    assert!(extends("A", "Base"));
    assert!(extends("B", "Crtp"));
    assert!(
        !result
            .relationships
            .iter()
            .any(|r| r.kind == RelationshipKind::Uses
                && r.from_symbol_id == id("Derived")
                && r.to_symbol_id == id("Local"))
    );
    let pending_extends: Vec<_> = result
        .structured_pending_relationships
        .iter()
        .filter(|p| p.pending.kind == RelationshipKind::Extends)
        .map(|p| {
            (
                p.target.terminal_name.clone(),
                p.target.namespace_path.clone(),
            )
        })
        .collect();
    assert!(pending_extends.contains(&("RemoteBase".to_string(), vec![])));
    assert!(pending_extends.contains(&("Mixin".to_string(), vec!["ns".to_string()])));
    assert!(pending_extends.contains(&("Templ".to_string(), vec![])));
    assert!(
        result
            .structured_pending_relationships
            .iter()
            .all(|p| p.pending.kind != RelationshipKind::Uses
                || !["RemoteBase", "Mixin", "Templ"].contains(&p.target.terminal_name.as_str()))
    );
}

#[test]
fn visibility_macros_between_class_key_and_name_keep_the_class() {
    let result = extract(
        "api.h",
        r#"class ENGINE_API Renderer : public Base {
public:
    void draw();
    int frames_ = 0;
};
struct DLL_PUBLIC Point { int x; };
class IMGUI_API Context { public: void render(); int frame = 0; };
"#,
    );
    assert_eq!(only(&result, "Renderer").kind, SymbolKind::Class);
    assert_eq!(only(&result, "Point").kind, SymbolKind::Struct);
    assert_eq!(only(&result, "Context").kind, SymbolKind::Class);
    for (member, owner) in [
        ("draw", "Renderer"),
        ("frames_", "Renderer"),
        ("x", "Point"),
        ("render", "Context"),
        ("frame", "Context"),
    ] {
        assert_eq!(
            parent_name(&result, only(&result, member)).as_deref(),
            Some(owner),
            "{member}"
        );
    }
}

#[test]
fn qualified_definition_heads_emit_containers() {
    let result = extract(
        "ns17.cpp",
        r#"namespace company::product::detail {
struct Deep { int x; };
void deep_fn() {}
}
class Outer { class Inner; struct Impl; };
class Outer::Inner { public: void work(); };
struct Outer::Impl { int state; };
template <> struct hash<Point> { size_t operator()(const Point& p) const noexcept; };
template <typename T> struct Traits<T*> { static const bool is_ptr = true; };
"#,
    );
    let namespace = only(&result, "company::product::detail");
    assert_eq!(namespace.kind, SymbolKind::Namespace);
    assert_eq!(
        parent_name(&result, only(&result, "Deep")).as_deref(),
        Some("company::product::detail")
    );
    assert_eq!(
        parent_name(&result, only(&result, "deep_fn")).as_deref(),
        Some("company::product::detail")
    );
    let inner = only(&result, "Inner");
    assert_eq!(inner.kind, SymbolKind::Class);
    assert_eq!(parent_name(&result, inner).as_deref(), Some("Outer"));
    assert_eq!(
        parent_name(&result, only(&result, "work")).as_deref(),
        Some("Inner")
    );
    let impl_row = only(&result, "Impl");
    assert_eq!(impl_row.kind, SymbolKind::Struct);
    assert_eq!(
        parent_name(&result, only(&result, "state")).as_deref(),
        Some("Impl")
    );
    let hash = only(&result, "hash");
    assert_eq!(hash.kind, SymbolKind::Struct);
    assert!(
        hash.signature
            .as_deref()
            .is_some_and(|s| s.contains("hash<Point>"))
    );
    assert_eq!(
        parent_name(&result, only(&result, "operator()")).as_deref(),
        Some("hash")
    );
    let traits = only(&result, "Traits");
    assert!(
        traits
            .signature
            .as_deref()
            .is_some_and(|s| s.contains("Traits<T*>"))
    );
    assert_eq!(
        parent_name(&result, only(&result, "is_ptr")).as_deref(),
        Some("Traits")
    );
}

#[test]
fn catch2_test_symbols_own_their_detached_bodies() {
    let source = r#"TEST_CASE("adds numbers", "[math]") {
    Calculator calc;
    REQUIRE(calc.add(1, 2) == 3);
    SECTION("negative") {
        REQUIRE(subtract(1, 2) == -1);
    }
}
"#;
    let result = extract("tests/catch_test.cpp", source);
    let test = only(&result, "adds numbers");
    assert_eq!((test.start_line, test.end_line), (1, 7));
    let body = test.body_span.expect("test body");
    assert!(source[body.start_byte as usize..].starts_with('{'));
    let section = only(&result, "negative");
    assert_eq!(section.parent_id.as_deref(), Some(test.id.as_str()));
    assert_eq!((section.start_line, section.end_line), (4, 6));
    assert_eq!(
        only(&result, "calc").parent_id.as_deref(),
        Some(test.id.as_str())
    );
    let calls: Vec<_> = result
        .structured_pending_relationships
        .iter()
        .filter(|p| p.pending.kind == RelationshipKind::Calls)
        .map(|p| {
            (
                p.pending.from_symbol_id.clone(),
                p.target.terminal_name.clone(),
            )
        })
        .collect();
    assert!(
        calls.contains(&(test.id.clone(), "add".to_string())),
        "{calls:?}"
    );
    assert!(
        calls.contains(&(section.id.clone(), "subtract".to_string())),
        "{calls:?}"
    );
    assert!(
        calls
            .iter()
            .all(|(_, target)| target != "TEST_CASE" && target != "SECTION")
    );
}
