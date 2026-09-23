use crate::base::{
    ExtractionResults, IdentifierKind, RelationshipKind, Symbol, SymbolKind, Visibility,
};
use crate::extract_canonical;
use std::path::Path;

fn extract(path: &str, source: &str) -> ExtractionResults {
    extract_canonical(path, source, Path::new("/tmp/test")).expect("C++ extraction succeeds")
}

fn symbol<'a>(result: &'a ExtractionResults, name: &str) -> &'a Symbol {
    result
        .symbols
        .iter()
        .find(|symbol| symbol.name == name)
        .unwrap_or_else(|| panic!("missing symbol {name}"))
}

fn symbol_name<'a>(result: &'a ExtractionResults, id: Option<&str>) -> Option<&'a str> {
    let id = id?;
    result
        .symbols
        .iter()
        .find(|symbol| symbol.id == id)
        .map(|symbol| symbol.name.as_str())
}

fn test_role(symbol: &Symbol) -> Option<&str> {
    symbol
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get("test_role"))
        .and_then(|role| role.as_str())
}

fn return_fact(result: &ExtractionResults, name: &str) -> Option<(String, Option<String>)> {
    result.types.get(&symbol(result, name).id).map(|fact| {
        let declared = fact
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.get("declared"))
            .and_then(|declared| declared.as_str())
            .map(str::to_string);
        (fact.resolved_type.clone(), declared)
    })
}

#[test]
fn includes_and_macros_are_import_and_constant_rows() {
    let result = extract(
        "pre.cpp",
        "#include <vector>\n\
         #include \"engine/renderer.h\"\n\
         #define APP_VERSION \"1.2.0\"\n\
         #define CLAMP(v, lo, hi) ((v) < (lo) ? (lo) : (v))\n\
         int limit(int v) { return CLAMP(v, 0, 10); }\n",
    );

    assert_eq!(symbol(&result, "vector").kind, SymbolKind::Import);
    assert_eq!(
        symbol(&result, "engine/renderer.h").kind,
        SymbolKind::Import
    );
    assert_eq!(symbol(&result, "APP_VERSION").kind, SymbolKind::Constant);
    assert_eq!(symbol(&result, "CLAMP").kind, SymbolKind::Constant);
    let macro_names: Vec<_> = result
        .structural_facts
        .iter()
        .filter_map(|fact| fact.metadata.as_ref()?.get("name")?.as_str())
        .collect();
    assert_eq!(macro_names, ["APP_VERSION", "CLAMP"]);
}

#[test]
fn destructor_operator_and_struct_bodies_own_their_references() {
    let result = extract(
        "contain.cpp",
        "namespace app {\n\
         struct Point {\n\
         \x20   std::string label;\n\
         \x20   bool operator==(const Point& o) const { return compare(label, o.label); }\n\
         };\n\
         class Conn {\n\
         public:\n\
         \x20   ~Conn() { close_handle(fd); }\n\
         \x20   Conn& operator<<(const std::string& s) { write_all(fd, s); return *this; }\n\
         \x20   int fd;\n\
         };\n\
         }\n",
    );

    let container_of = |name: &str, kind: IdentifierKind| {
        let identifier = result
            .identifiers
            .iter()
            .find(|identifier| identifier.name == name && identifier.kind == kind)
            .unwrap_or_else(|| panic!("missing identifier {name}"));
        symbol_name(&result, identifier.containing_symbol_id.as_deref())
    };
    assert_eq!(
        container_of("string", IdentifierKind::TypeUsage),
        Some("Point")
    );
    assert_eq!(
        container_of("compare", IdentifierKind::Call),
        Some("operator==")
    );
    assert_eq!(
        container_of("close_handle", IdentifierKind::Call),
        Some("~Conn")
    );
    assert_eq!(
        container_of("write_all", IdentifierKind::Call),
        Some("operator<<")
    );

    let callers: Vec<_> = result
        .structured_pending_relationships
        .iter()
        .filter(|pending| pending.pending.kind == RelationshipKind::Calls)
        .map(|pending| {
            (
                symbol_name(&result, Some(&pending.pending.from_symbol_id)),
                pending.target.terminal_name.as_str(),
            )
        })
        .collect();
    assert!(callers.contains(&(Some("~Conn"), "close_handle")));
}

#[test]
fn conversion_operators_member_templates_and_friends_have_one_row_each() {
    let result = extract(
        "ops.cpp",
        "class Circle {\n\
         public:\n\
         \x20   explicit operator bool() const { return true; }\n\
         \x20   template <typename U> void push(U&& value) {}\n\
         \x20   template <typename U> U* get() const;\n\
         \x20   friend std::ostream& operator<<(std::ostream& os, const Circle& c);\n\
         \x20   friend void audit(Circle&);\n\
         };\n",
    );

    let circle = symbol(&result, "Circle");
    let conversion = symbol(&result, "operator bool");
    assert_eq!(conversion.kind, SymbolKind::Operator);
    assert_eq!(conversion.parent_id.as_deref(), Some(circle.id.as_str()));
    assert_eq!(
        conversion.signature.as_deref(),
        Some("explicit operator bool() const")
    );
    assert_eq!(symbol(&result, "push").kind, SymbolKind::Method);
    let get = symbol(&result, "get");
    assert_eq!(get.kind, SymbolKind::Method);
    assert_eq!(
        get.signature.as_deref(),
        Some("template<typename U>\nU *get() const")
    );
    for friend in ["operator<<", "audit"] {
        let rows: Vec<_> = result
            .symbols
            .iter()
            .filter(|symbol| symbol.name == friend)
            .collect();
        assert_eq!(rows.len(), 1, "{friend} has one row");
        assert_eq!(rows[0].parent_id.as_deref(), Some(circle.id.as_str()));
    }
}

#[test]
fn pointer_reference_and_trailing_returns_record_type_facts() {
    let result = extract(
        "ret.cpp",
        "class Order { public: void ship(); };\n\
         class Repo {\n\
         public:\n\
         \x20   Order* raw() { return nullptr; }\n\
         \x20   const Order& ref() const;\n\
         };\n\
         Order* Repo::raw_out() { return nullptr; }\n\
         auto trailing() -> Order { return {}; }\n\
         auto deduced() { return 1; }\n\
         std::vector<std::shared_ptr<Order>> all() { return {}; }\n",
    );

    assert_eq!(
        return_fact(&result, "raw"),
        Some(("Order".into(), Some("Order*".into())))
    );
    assert_eq!(
        return_fact(&result, "ref"),
        Some(("Order".into(), Some("const Order&".into())))
    );
    assert_eq!(
        return_fact(&result, "Repo::raw_out"),
        Some(("Order".into(), Some("Order*".into())))
    );
    assert_eq!(
        return_fact(&result, "trailing"),
        Some(("Order".into(), None))
    );
    assert_eq!(return_fact(&result, "deduced"), None);
    assert_eq!(
        return_fact(&result, "all"),
        Some((
            "std::vector".into(),
            Some("std::vector<std::shared_ptr<Order>>".into())
        ))
    );
}

#[test]
fn bodiless_declarations_have_no_body_span() {
    let result = extract(
        "body_decl.h",
        "class Api {\n\
         public:\n\
         \x20   virtual void start() = 0;\n\
         \x20   void stop();\n\
         \x20   Api() = default;\n\
         \x20   int count(int limit) const;\n\
         \x20   int retries_ = default_retries();\n\
         \x20   void run() { stop(); }\n\
         };\n\
         void free_fn();\n\
         std::string g_path = build_path();\n",
    );

    for name in [
        "start", "stop", "Api", "count", "retries_", "free_fn", "g_path",
    ] {
        let symbol = result
            .symbols
            .iter()
            .find(|symbol| symbol.name == name && symbol.kind != SymbolKind::Class)
            .unwrap_or_else(|| panic!("missing {name}"));
        assert_eq!(symbol.body_span, None, "{name} has no body");
        assert_eq!(symbol.body_hash, None, "{name} has no body hash");
    }
    assert!(symbol(&result, "run").body_span.is_some());
}

#[test]
fn doxygen_bang_docs_count_and_trailing_docs_document_the_member_before() {
    let result = extract(
        "docs.cpp",
        "//! Qt-style line doc for Parser.\n\
         class Parser {\n\
         public:\n\
         \x20   /*! Parses the input. */\n\
         \x20   void parse();\n\
         \x20   int depth; ///< Current nesting depth.\n\
         \x20   /// Resets the parser.\n\
         \x20   void reset();\n\
         };\n\
         enum class Mode { Fast, ///< Skip validation.\n\
         \x20   Safe };\n\
         ///////////////////////////////////////\n\
         class Banner {};\n",
    );

    let doc = |name: &str| symbol(&result, name).doc_comment.clone();
    assert_eq!(
        doc("Parser").as_deref(),
        Some("//! Qt-style line doc for Parser.")
    );
    assert_eq!(doc("parse").as_deref(), Some("/*! Parses the input. */"));
    assert_eq!(doc("depth").as_deref(), Some("///< Current nesting depth."));
    assert_eq!(doc("reset").as_deref(), Some("/// Resets the parser."));
    assert_eq!(doc("Fast").as_deref(), Some("///< Skip validation."));
    assert_eq!(doc("Safe"), None);
    assert_eq!(doc("Banner"), None);
}

#[test]
fn boost_doctest_and_qttest_declarations_get_test_roles() {
    let boost = extract(
        "boost_test.cpp",
        "BOOST_AUTO_TEST_SUITE(math_suite)\n\
         BOOST_AUTO_TEST_CASE(test_add) { BOOST_CHECK_EQUAL(add(1, 2), 3); }\n\
         BOOST_FIXTURE_TEST_CASE(test_fix, Fixture) { BOOST_CHECK(true); }\n\
         BOOST_AUTO_TEST_SUITE_END()\n",
    );
    assert_eq!(
        test_role(symbol(&boost, "math_suite")),
        Some("test_container")
    );
    assert_eq!(test_role(symbol(&boost, "test_add")), Some("test_case"));
    assert_eq!(test_role(symbol(&boost, "test_fix")), Some("test_case"));
    assert!(
        !boost
            .structured_pending_relationships
            .iter()
            .any(|pending| pending.target.terminal_name.starts_with("test_")
                || pending
                    .target
                    .terminal_name
                    .starts_with("BOOST_AUTO_TEST_SUITE"))
    );

    let doctest = extract(
        "doctest_test.cpp",
        "TEST_SUITE(\"math\") { TEST_CASE(\"mult\") { SUBCASE(\"zero\") { CHECK(true); } } }\n\
         TEST_CASE_FIXTURE(Fixture, \"fixture case\") { CHECK(true); }\n",
    );
    assert_eq!(test_role(symbol(&doctest, "math")), Some("test_container"));
    assert_eq!(test_role(symbol(&doctest, "mult")), Some("test_case"));
    assert_eq!(test_role(symbol(&doctest, "zero")), Some("test_container"));
    assert_eq!(
        test_role(symbol(&doctest, "fixture case")),
        Some("test_case")
    );

    let qttest = extract(
        "tst_qstring.cpp",
        "class tst_QString : public QObject\n\
         {\n\
         \x20   Q_OBJECT\n\
         private slots:\n\
         \x20   void initTestCase();\n\
         \x20   void append();\n\
         \x20   void append_data();\n\
         \x20   void chop();\n\
         \x20   void cleanup();\n\
         public slots:\n\
         \x20   void helper();\n\
         };\n\
         void tst_QString::chop() {}\n\
         QTEST_MAIN(tst_QString)\n",
    );
    assert_eq!(
        test_role(symbol(&qttest, "tst_QString")),
        Some("test_container")
    );
    assert_eq!(
        test_role(symbol(&qttest, "initTestCase")),
        Some("fixture_setup")
    );
    assert_eq!(
        test_role(symbol(&qttest, "cleanup")),
        Some("fixture_teardown")
    );
    assert_eq!(
        test_role(symbol(&qttest, "append")),
        Some("parameterized_test")
    );
    assert_eq!(test_role(symbol(&qttest, "append_data")), None);
    assert_eq!(test_role(symbol(&qttest, "chop")), Some("test_case"));
    assert_eq!(
        test_role(symbol(&qttest, "tst_QString::chop")),
        Some("test_case")
    );
    assert_eq!(test_role(symbol(&qttest, "helper")), None);
}

#[test]
fn template_parameters_are_not_type_uses_and_qualified_uses_keep_their_scope() {
    let result = extract(
        "tparams.cpp",
        "class Allocator { public: void* allocate(int n); };\n\
         template <typename Allocator, typename Iterator>\n\
         void fill(Iterator first, Iterator last, Allocator& alloc) { alloc.allocate(1); }\n\
         template <typename T = int, typename... Rest> struct Holder { T value; Rest* rest; };\n\
         void label(ns::Widget w);\n",
    );

    for name in ["Allocator", "Iterator", "Rest"] {
        assert!(
            !result.identifiers.iter().any(|identifier| {
                identifier.kind == IdentifierKind::TypeUsage && identifier.name == name
            }),
            "{name} is a template parameter"
        );
    }
    assert!(
        !result
            .relationships
            .iter()
            .any(|relationship| relationship.kind == RelationshipKind::Uses)
    );
    let widget = result
        .structured_pending_relationships
        .iter()
        .find(|pending| pending.target.terminal_name == "Widget")
        .expect("Widget use");
    assert_eq!(widget.target.namespace_path, ["ns"]);
    assert!(
        !result
            .structured_pending_relationships
            .iter()
            .any(|pending| pending.target.terminal_name == "Iterator")
    );
}

#[test]
fn internal_linkage_symbols_are_private() {
    let result = extract(
        "vis.cpp",
        "namespace {\n\
         void anon_fn() {}\n\
         int anon_var = 1;\n\
         }\n\
         static void file_static() {}\n\
         static int g_count = 0;\n\
         void exported() {}\n",
    );

    for name in ["anon_fn", "anon_var", "file_static", "g_count"] {
        assert_eq!(
            symbol(&result, name).visibility,
            Some(Visibility::Private),
            "{name}"
        );
    }
    assert_eq!(
        symbol(&result, "exported").visibility,
        Some(Visibility::Public)
    );
}

#[test]
fn standard_attributes_on_declarations_and_classes_are_annotations() {
    let result = extract(
        "attr.h",
        "class [[deprecated(\"use NewApi\")]] OldApi {\n\
         public:\n\
         \x20   [[nodiscard]] int value() const;\n\
         \x20   [[deprecated]] void legacy();\n\
         };\n\
         [[nodiscard]] int compute(int x);\n",
    );

    let keys = |name: &str| -> Vec<String> {
        symbol(&result, name)
            .annotations
            .iter()
            .map(|annotation| annotation.annotation_key.clone())
            .collect()
    };
    assert_eq!(keys("OldApi"), ["deprecated"]);
    assert_eq!(keys("value"), ["nodiscard"]);
    assert_eq!(keys("legacy"), ["deprecated"]);
    assert_eq!(keys("compute"), ["nodiscard"]);
}

#[test]
fn complexity_counts_range_for_catch_and_variadic_parameters() {
    let result = extract(
        "loops.cpp",
        "int range_for(const std::vector<int>& v) {\n\
         \x20   int s = 0;\n\
         \x20   for (int x : v) { s += x; }\n\
         \x20   return s;\n\
         }\n\
         int trying(int n) {\n\
         \x20   try { if (n > 0) return 1; } catch (const std::exception& e) { return 2; } catch (...) { return 3; }\n\
         \x20   return 0;\n\
         }\n\
         template <typename... Args> void log_all(const char* fmt, Args&&... args) {}\n",
    );

    let metric = |name: &str| {
        let id = &symbol(&result, name).id;
        result
            .complexity_metrics
            .iter()
            .find(|metric| metric.symbol_id.as_ref() == Some(id))
            .unwrap_or_else(|| panic!("missing metric for {name}"))
    };
    assert_eq!(metric("range_for").loop_count, 1);
    assert_eq!(metric("range_for").max_nesting_depth, 1);
    assert_eq!(metric("trying").decision_count, 3);
    assert_eq!(metric("log_all").parameter_count, Some(2));
    assert!(result.symbols.iter().any(|symbol| symbol.name == "args"));
}
