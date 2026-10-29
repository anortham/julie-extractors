use crate::ExtractionResults;
use crate::extract_canonical;
use crate::tests::helpers::{
    call_owners, owner_rows, supertype_edges, symbol_metadata, symbol_named, test_flags,
};
use std::path::Path;

fn extract(path: &str, source: &str) -> ExtractionResults {
    extract_canonical(path, source, Path::new("/tmp/test")).expect("java extraction")
}

#[test]
fn anonymous_class_owns_its_members_under_the_enclosing_method_or_field() {
    let result = extract(
        "src/J.java",
        "interface Task { int run(); }\n\
         class J {\n\
           Task make() {\n\
             return new Task() {\n\
               public int run() { return helper(); }\n\
               int helper() { return 1; }\n\
             };\n\
           }\n\
           Runnable field = new Runnable() { public void run() { make(); } };\n\
         }\n",
    );

    assert_eq!(
        owner_rows(&result),
        [
            "interface task < -",
            "method run < task",
            "class j < -",
            "method make < j",
            "class anonymous_class_l4 < make",
            "method run < anonymous_class_l4",
            "method helper < anonymous_class_l4",
            "property field < j",
            "class anonymous_class_l9 < field",
            "method run < anonymous_class_l9",
        ]
    );
    let anonymous = symbol_named(&result, "anonymous_class_L4");
    assert_eq!(symbol_metadata(anonymous, "type"), Some("anonymous_class"));
    assert_eq!(anonymous.signature.as_deref(), Some("new Task()"));
    assert_eq!(
        anonymous.metadata.as_ref().unwrap()["base_types"],
        serde_json::json!(["Task"])
    );
    assert_eq!(
        supertype_edges(&result),
        [
            "extends anonymous_class_l9 -> ?runnable",
            "implements anonymous_class_l4 -> task",
        ]
    );
    assert_eq!(call_owners(&result), ["helper in run", "make in run"]);
}

#[test]
fn creation_arguments_stay_with_the_enclosing_method() {
    let result = extract(
        "src/T.java",
        "class Base { Base(Runnable r) {} void go() {} }\n\
         class T {\n\
           void start() {\n\
             Base b = new Base(() -> work()) {\n\
               void go() { stop(); }\n\
             };\n\
           }\n\
           void work() {}\n\
           void stop() {}\n\
         }\n",
    );

    assert_eq!(call_owners(&result), ["work in start", "stop in go"]);
    assert_eq!(
        supertype_edges(&result),
        ["extends anonymous_class_l4 -> base"]
    );
    let go = result
        .symbols
        .iter()
        .find(|symbol| symbol.name == "go" && symbol.start_line == 5)
        .expect("go in the anonymous class");
    assert_eq!(
        go.parent_id.as_deref(),
        Some(symbol_named(&result, "anonymous_class_L4").id.as_str())
    );
}

#[test]
fn anonymous_class_row_spans_the_body_so_the_creation_head_keeps_its_owner() {
    let result = extract(
        "src/F.java",
        "class Base { Base(int n) {} void go() {} }\n\
         class F {\n\
           static int seed() { return 1; }\n\
           Base field = new Base(seed()) {\n\
             int x = seed();\n\
             void go() { stop(); }\n\
           };\n\
           void stop() {}\n\
         }\n",
    );

    assert_eq!(
        call_owners(&result),
        ["seed in F", "seed in anonymous_class_L4", "stop in go"]
    );
    let anonymous = symbol_named(&result, "anonymous_class_L4");
    assert_eq!(anonymous.signature.as_deref(), Some("new Base(seed())"));
    assert_eq!((anonymous.start_line, anonymous.start_column), (4, 30));
    assert_eq!(
        supertype_edges(&result),
        ["extends anonymous_class_l4 -> base"]
    );
}

#[test]
fn named_types_enum_constant_bodies_and_plain_creations_write_no_anonymous_class() {
    let result = extract(
        "src/Plain.java",
        "enum Op { ADD { int apply() { return 1; } }; abstract int apply(); }\n\
         class Plain {\n\
           Object make() {\n\
             class Local { void m() {} }\n\
             return new Plain();\n\
           }\n\
         }\n",
    );

    assert_eq!(
        owner_rows(&result),
        [
            "enum op < -",
            "enummember add < op",
            "method apply < add",
            "method apply < op",
            "class plain < -",
            "method make < plain",
            "class local < make",
            "method m < local",
        ]
    );
}

#[test]
fn anonymous_classes_in_tests_keep_test_detection() {
    let result = extract(
        "src/test/java/com/example/WorkerTest.java",
        "import org.junit.jupiter.api.Test;\n\
         import junit.framework.TestCase;\n\
         class Worker { Runnable task() { return new Runnable() { public void run() {} }; } }\n\
         class WorkerTest {\n\
           @Test void runsTask() {\n\
             Runnable r = new Runnable() {\n\
               @Test public void run() {}\n\
               public void testHelper() {}\n\
             };\n\
             TestCase legacy = new TestCase() { public void testInner() {} };\n\
           }\n\
         }\n",
    );

    assert_eq!(
        test_flags(&result),
        [
            "WorkerTest: test_container,test_role",
            "run: is_test,test_role",
            "runsTask: is_test,test_role",
            "testHelper: is_test,test_role",
            "testInner: is_test,test_role",
        ]
    );
}
