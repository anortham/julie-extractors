use crate::base::{ExtractionResults, IdentifierKind, RelationshipKind, SourceRegionKind, Symbol};
use crate::extract_canonical;
use std::path::Path;

fn extract(path: &str, source: &str) -> ExtractionResults {
    extract_canonical(path, source, Path::new("/tmp/test")).expect("C extraction succeeds")
}

fn symbol<'a>(result: &'a ExtractionResults, name: &str) -> &'a Symbol {
    result
        .symbols
        .iter()
        .find(|symbol| symbol.name == name)
        .unwrap_or_else(|| panic!("missing symbol {name}"))
}

fn parent_name<'a>(result: &'a ExtractionResults, symbol: &Symbol) -> Option<&'a str> {
    let parent_id = symbol.parent_id.as_deref()?;
    result
        .symbols
        .iter()
        .find(|candidate| candidate.id == parent_id)
        .map(|parent| parent.name.as_str())
}

fn test_role(symbol: &Symbol) -> Option<&str> {
    symbol
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get("test_role"))
        .and_then(|role| role.as_str())
}

fn call_names(result: &ExtractionResults) -> Vec<&str> {
    result
        .identifiers
        .iter()
        .filter(|identifier| identifier.kind == IdentifierKind::Call)
        .map(|identifier| identifier.name.as_str())
        .collect()
}

fn pending_call_targets(result: &ExtractionResults) -> Vec<&str> {
    result
        .structured_pending_relationships
        .iter()
        .filter(|pending| pending.pending.kind == RelationshipKind::Calls)
        .map(|pending| pending.target.terminal_name.as_str())
        .collect()
}

#[test]
fn calls_through_struct_fields_name_the_field() {
    let result = extract(
        "membercall.c",
        "struct Ops { void (*run)(int); };\n\
         struct Server { struct Ops *ops; void (*on_request)(int code); };\n\
         void serve(struct Server *srv) {\n\
         \x20   srv->on_request(200);\n\
         \x20   srv->ops->run(1);\n\
         \x20   (*srv->on_request)(201);\n\
         }\n",
    );

    assert_eq!(call_names(&result), ["on_request", "run", "on_request"]);
    assert_eq!(
        pending_call_targets(&result),
        ["on_request", "run", "on_request"]
    );
    let dereferenced = &result.structured_pending_relationships[2];
    assert_eq!(dereferenced.target.receiver.as_deref(), Some("srv"));
    assert!(
        !result.identifiers.iter().any(|identifier| {
            identifier.kind == IdentifierKind::MemberAccess && identifier.name == "on_request"
        }),
        "a called field is a call site, not also a member access"
    );
}

#[test]
fn function_return_types_record_type_facts() {
    let result = extract(
        "rettypes.c",
        "struct Point { int x; };\n\
         union Value { int i; };\n\
         enum Level { LOW };\n\
         struct Point make_point(void) { struct Point p; return p; }\n\
         static int local_helper(void) { return 1; }\n\
         enum Level current_level(void) { return LOW; }\n\
         union Value make_value(void) { union Value v; return v; }\n\
         static struct Point *find_point(int id) { return 0; }\n\
         enum Level level_of(int x);\n\
         int (*handler_for(int code))(int);\n",
    );

    let fact = |name: &str| {
        let fact = result.types.get(&symbol(&result, name).id);
        fact.map(|fact| {
            let declared = fact
                .metadata
                .as_ref()
                .and_then(|metadata| metadata.get("declared"))
                .and_then(|declared| declared.as_str())
                .map(str::to_string);
            (fact.resolved_type.clone(), declared)
        })
    };
    assert_eq!(
        fact("make_point"),
        Some(("Point".into(), Some("struct Point".into())))
    );
    assert_eq!(fact("local_helper"), Some(("int".into(), None)));
    assert_eq!(
        fact("current_level"),
        Some(("Level".into(), Some("enum Level".into())))
    );
    assert_eq!(
        fact("make_value"),
        Some(("Value".into(), Some("union Value".into())))
    );
    assert_eq!(
        fact("find_point"),
        Some(("Point".into(), Some("struct Point *".into())))
    );
    assert_eq!(
        fact("level_of"),
        Some(("Level".into(), Some("enum Level".into())))
    );
    assert_eq!(fact("handler_for"), None);

    let local = result
        .symbols
        .iter()
        .find(|symbol| symbol.name == "v" && symbol.signature.as_deref() == Some("union Value v"));
    assert!(
        local.is_some(),
        "a union-typed local keeps its type in the signature"
    );
}

#[test]
fn bodiless_declarations_have_no_body_span() {
    let result = extract(
        "protobody.c",
        "[[deprecated(\"use v2\")]] int old_api(void);\n\
         int fmt_log(const char *fmt, ...) __attribute__((format(printf, 1, 2)));\n\
         int api_version(void);\n\
         typedef int (*compare_fn)(const void *a, const void *b);\n\
         struct Ops { int (*open)(const char *path); };\n\
         int (*cmp)(int, int);\n\
         typedef struct { int id; } Worker;\n\
         int run(void) { return 0; }\n",
    );

    for name in [
        "old_api",
        "fmt_log",
        "api_version",
        "compare_fn",
        "open",
        "cmp",
    ] {
        let symbol = symbol(&result, name);
        assert_eq!(symbol.body_span, None, "{name} has no body");
        assert_eq!(symbol.body_hash, None, "{name} has no body hash");
    }
    for name in ["Worker", "Ops", "run"] {
        assert!(
            symbol(&result, name).body_span.is_some(),
            "{name} keeps its body"
        );
    }
}

#[test]
fn anonymous_record_members_are_fields() {
    let result = extract(
        "nested.c",
        "struct Value {\n\
         \x20   int tag;\n\
         \x20   union {\n\
         \x20       long as_int;\n\
         \x20       double as_float;\n\
         \x20   };\n\
         \x20   struct {\n\
         \x20       int line, col;\n\
         \x20   } pos;\n\
         };\n",
    );

    for (field, owner) in [
        ("as_int", "Value"),
        ("as_float", "Value"),
        ("pos", "Value"),
        ("line", "pos"),
        ("col", "pos"),
    ] {
        assert_eq!(
            parent_name(&result, symbol(&result, field)),
            Some(owner),
            "{field} belongs to {owner}"
        );
    }
}

#[test]
fn unity_hooks_in_test_files_are_fixture_lifecycle() {
    let source = "void setUp(void) { init_math(); }\n\
                  void tearDown(void) { cleanup_math(); }\n\
                  void suiteSetUp(void) { }\n\
                  int suiteTearDown(int failures) { return failures; }\n\
                  void test_add_works(void) { }\n";
    let result = extract("tests/test_math.c", source);

    assert_eq!(test_role(symbol(&result, "setUp")), Some("fixture_setup"));
    assert_eq!(
        test_role(symbol(&result, "suiteSetUp")),
        Some("fixture_setup")
    );
    assert_eq!(
        test_role(symbol(&result, "tearDown")),
        Some("fixture_teardown")
    );
    assert_eq!(
        test_role(symbol(&result, "suiteTearDown")),
        Some("fixture_teardown")
    );
    assert_eq!(
        test_role(symbol(&result, "test_add_works")),
        Some("test_case")
    );

    let production = extract("src/math.c", source);
    assert_eq!(test_role(symbol(&production, "setUp")), None);
}

#[test]
fn cmocka_registrations_give_test_roles() {
    let result = extract(
        "tests/cmocka_list.c",
        "static int group_setup(void **state) { return 0; }\n\
         static int group_teardown(void **state) { return 0; }\n\
         static int each_setup(void **state) { return 0; }\n\
         static void list_push_increments_length(void **state) { }\n\
         static void list_pop(void **state) { }\n\
         int main(void) {\n\
         \x20   const struct CMUnitTest tests[] = {\n\
         \x20       cmocka_unit_test(list_push_increments_length),\n\
         \x20       cmocka_unit_test_setup_teardown(list_pop, each_setup, NULL),\n\
         \x20   };\n\
         \x20   return cmocka_run_group_tests(tests, group_setup, group_teardown);\n\
         }\n",
    );

    assert_eq!(
        test_role(symbol(&result, "list_push_increments_length")),
        Some("test_case")
    );
    assert_eq!(test_role(symbol(&result, "list_pop")), Some("test_case"));
    assert_eq!(
        test_role(symbol(&result, "each_setup")),
        Some("fixture_setup")
    );
    assert_eq!(
        test_role(symbol(&result, "group_setup")),
        Some("fixture_setup")
    );
    assert_eq!(
        test_role(symbol(&result, "group_teardown")),
        Some("fixture_teardown")
    );
    assert_eq!(test_role(symbol(&result, "main")), None);
}

#[test]
fn doxygen_trailing_and_bang_docs_attach_to_the_right_symbol() {
    let result = extract(
        "docs.c",
        "enum State {\n\
         \x20   STATE_IDLE,\n\
         \x20   STATE_OPEN, /**< Connected and ready. */\n\
         };\n\
         struct Conn {\n\
         \x20   int port; /**< TCP port. */\n\
         \x20   /// Buffer length.\n\
         \x20   int len;\n\
         };\n\
         /*! Qt style doc. */\n\
         int qt_doc(void) { return 0; }\n\
         //! Line bang doc.\n\
         int bang_doc(void) { return 0; }\n\
         ////////////////////\n\
         int banner(void) { return 0; }\n",
    );

    let doc = |name: &str| symbol(&result, name).doc_comment.clone();
    assert_eq!(
        doc("STATE_OPEN").as_deref(),
        Some("/**< Connected and ready. */")
    );
    assert_eq!(doc("STATE_IDLE"), None);
    assert_eq!(doc("port").as_deref(), Some("/**< TCP port. */"));
    assert_eq!(doc("len").as_deref(), Some("/// Buffer length."));
    assert_eq!(doc("qt_doc").as_deref(), Some("/*! Qt style doc. */"));
    assert_eq!(doc("bang_doc").as_deref(), Some("//! Line bang doc."));
    assert_eq!(doc("banner"), None);

    let port = symbol(&result, "port");
    let trailing_region = result
        .source_regions
        .iter()
        .find(|region| region.kind == SourceRegionKind::DocComment && region.start_line == 6)
        .expect("trailing doc region");
    assert_eq!(
        trailing_region.containing_symbol_id.as_deref(),
        Some(port.id.as_str())
    );
}

#[test]
fn attributes_on_records_fields_and_variables_are_annotations_not_calls() {
    let result = extract(
        "attrs.c",
        "struct __attribute__((packed)) Header {\n\
         \x20   unsigned char kind;\n\
         \x20   unsigned short len __attribute__((aligned(2)));\n\
         };\n\
         [[maybe_unused]] static int debug_level = 0;\n\
         __attribute__((section(\".data.hot\"))) int hot_counter;\n\
         int fmt_log(const char *fmt, ...) __attribute__((format(printf, 1, 2)));\n",
    );

    let annotation_keys = |name: &str| -> Vec<String> {
        symbol(&result, name)
            .annotations
            .iter()
            .map(|annotation| annotation.annotation_key.clone())
            .collect()
    };
    assert_eq!(annotation_keys("Header"), ["packed"]);
    assert_eq!(annotation_keys("len"), ["aligned"]);
    assert_eq!(annotation_keys("kind"), Vec::<String>::new());
    assert_eq!(annotation_keys("debug_level"), ["maybe_unused"]);
    assert_eq!(annotation_keys("hot_counter"), ["section"]);
    assert_eq!(annotation_keys("fmt_log"), ["format"]);
    assert!(call_names(&result).is_empty());
    assert!(pending_call_targets(&result).is_empty());
    assert!(result.identifiers.is_empty());
}

#[test]
fn bare_identifier_statements_are_not_structs() {
    let result = extract(
        "exprstmt.c",
        "typedef int myint;\n\
         MODULE_MARKER;\n\
         void f(void) {\n\
         \x20   typedef long L;\n\
         \x20   L value = 0;\n\
         \x20   value;\n\
         }\n",
    );

    assert!(
        !result
            .symbols
            .iter()
            .any(|symbol| symbol.name == "MODULE_MARKER")
    );
    assert_eq!(
        result
            .symbols
            .iter()
            .filter(|symbol| symbol.name == "value")
            .count(),
        1
    );
}

#[test]
fn compile_time_keywords_are_not_calls_and_directives_contain_no_code() {
    let result = extract(
        "static_assert.c",
        "#include <assert.h>\n\
         _Static_assert(sizeof(long) >= 4, \"long too small\");\n\
         #include <linux/module.h>\n\
         MODULE_LICENSE(\"GPL\");\n\
         int pick(int x) {\n\
         \x20   static_assert(sizeof(int) == 4, \"int size\");\n\
         \x20   return _Generic(x, int: 1, default: 0);\n\
         }\n",
    );

    assert_eq!(call_names(&result), ["MODULE_LICENSE"]);
    let module_license = result
        .identifiers
        .iter()
        .find(|identifier| identifier.name == "MODULE_LICENSE")
        .expect("MODULE_LICENSE call");
    assert_eq!(module_license.containing_symbol_id, None);
    assert!(
        !result
            .identifiers
            .iter()
            .any(|identifier| identifier.name == "default")
    );
    assert!(
        !result
            .structured_pending_relationships
            .iter()
            .any(|pending| pending.target.terminal_name == "default")
    );
    assert!(pending_call_targets(&result).is_empty());
}
