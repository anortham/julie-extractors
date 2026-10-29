use crate::ExtractionResults;
use crate::base::RelationshipKind;
use crate::extract_canonical;
use crate::tests::helpers::{
    call_owners, owner_rows, supertype_edges, symbol_metadata, symbol_named, test_flags,
};
use std::path::Path;

fn extract(path: &str, source: &str) -> ExtractionResults {
    extract_canonical(path, source, Path::new("/tmp/test")).expect("scala extraction")
}

#[test]
fn anonymous_class_owns_its_members_and_local_defs_are_functions() {
    let result = extract(
        "src/N.scala",
        "trait Task { def run(): Int }\n\
         object N {\n\
         \x20 def outer(): Int = { def inner(): Int = 1; inner() }\n\
         \x20 def make(): Task = new Task { def run() = helper() }\n\
         \x20 val v = { def local(): Int = 3; local() }\n\
         \x20 def helper(): Int = 2\n\
         }\n",
    );

    assert_eq!(
        owner_rows(&result),
        [
            "trait task < -",
            "method run < task",
            "class n < -",
            "method outer < n",
            "function inner < outer",
            "method make < n",
            "class anonymous_class_l4 < make",
            "method run < anonymous_class_l4",
            "property v < n",
            "function local < v",
            "method helper < n",
        ]
    );
    let anonymous = symbol_named(&result, "anonymous_class_L4");
    assert_eq!(symbol_metadata(anonymous, "type"), Some("anonymous_class"));
    assert_eq!(anonymous.signature.as_deref(), Some("new Task"));
    assert_eq!(
        anonymous.metadata.as_ref().unwrap()["base_types"],
        serde_json::json!(["Task"])
    );
    assert_eq!(
        symbol_metadata(symbol_named(&result, "inner"), "type"),
        Some("function")
    );
    assert_eq!(
        supertype_edges(&result),
        ["implements anonymous_class_l4 -> task"]
    );
    assert_eq!(
        call_owners(&result),
        ["inner in outer", "helper in run", "local in N"]
    );
}

#[test]
fn anonymous_class_records_superclass_mixins_and_leaves_arguments_outside() {
    let result = extract(
        "src/W.scala",
        "abstract class Base(n: Int) { def go(): Unit }\n\
         object W {\n\
         \x20 def start(): Base = new Base(seed()) {\n\
         \x20   def go(): Unit = log()\n\
         \x20 }\n\
         \x20 def mixed() = new Logging with Closeable { def close(): Unit = () }\n\
         \x20 def seed(): Int = 1\n\
         \x20 def plain(): Base = new Concrete(2)\n\
         }\n",
    );

    assert_eq!(
        supertype_edges(&result),
        [
            "extends anonymous_class_l3 -> base",
            "extends anonymous_class_l6 -> ?logging",
            "implements anonymous_class_l6 -> ?closeable",
        ]
    );
    assert_eq!(
        symbol_named(&result, "anonymous_class_L6")
            .metadata
            .as_ref()
            .unwrap()["base_types"],
        serde_json::json!(["Logging", "Closeable"])
    );
    assert_eq!(
        symbol_named(&result, "anonymous_class_L3")
            .signature
            .as_deref(),
        Some("new Base(seed())")
    );
    assert_eq!(
        call_owners(&result),
        ["seed in start", "log in go", "Concrete in plain"]
    );
    assert_eq!(
        owner_rows(&result),
        [
            "class base < -",
            "property n < base",
            "method go < base",
            "class w < -",
            "method start < w",
            "class anonymous_class_l3 < start",
            "method go < anonymous_class_l3",
            "method mixed < w",
            "class anonymous_class_l6 < mixed",
            "method close < anonymous_class_l6",
            "method seed < w",
            "method plain < w",
        ]
    );
}

#[test]
fn members_of_named_types_inside_a_def_stay_methods() {
    let result = extract(
        "src/Local.scala",
        "object Local {\n\
         \x20 def build(): Int = {\n\
         \x20   class Helper { def twice(n: Int): Int = n * 2 }\n\
         \x20   object Cache { def size(): Int = 0 }\n\
         \x20   new Helper().twice(Cache.size())\n\
         \x20 }\n\
         }\n",
    );

    assert_eq!(
        owner_rows(&result),
        [
            "class local < -",
            "method build < local",
            "class helper < build",
            "method twice < helper",
            "class cache < build",
            "method size < cache",
        ]
    );
}

#[test]
fn anonymous_classes_and_local_defs_in_tests_keep_test_detection() {
    let result = extract(
        "src/test/scala/SuiteTest.scala",
        "import org.scalatest.funsuite.AnyFunSuite\n\
         \n\
         class SuiteTest extends AnyFunSuite {\n\
         \x20 test(\"works\") {\n\
         \x20   def helper(): Int = 1\n\
         \x20   val r = new Runnable { def run(): Unit = () }\n\
         \x20   assert(helper() == 1)\n\
         \x20 }\n\
         \x20 def testLocal(): Unit = {\n\
         \x20   def testInner(): Int = 1\n\
         \x20 }\n\
         }\n\
         \n\
         object Other {\n\
         \x20 def testThing(): Unit = {\n\
         \x20   def testNested(): Int = 1\n\
         \x20   val s = new AnyFunSuite { test(\"x\") {} }\n\
         \x20 }\n\
         }\n",
    );

    assert_eq!(
        test_flags(&result),
        [
            "SuiteTest: test_container,test_role",
            "testInner: is_test,test_role",
            "testLocal: is_test,test_role",
            "works: is_test,test_role",
        ]
    );
}

#[test]
fn this_and_super_inside_an_anonymous_class_name_the_anonymous_class() {
    let result = extract(
        "src/This.scala",
        "trait Task { def run(): Int }\n\
         object N {\n\
         \x20 def local(): Int = 1\n\
         \x20 def make(): Task = new Task {\n\
         \x20   def run() = this.local() + super.hashCode()\n\
         \x20   def local() = 2\n\
         \x20 }\n\
         }\n",
    );

    let local_in_class = result
        .symbols
        .iter()
        .find(|symbol| symbol.name == "local" && symbol.start_line == 6)
        .expect("local in the anonymous class");
    assert!(result.relationships.iter().any(|relationship| {
        relationship.kind == RelationshipKind::Calls
            && relationship.to_symbol_id == local_in_class.id
    }));
    let hash_code = result
        .structured_pending_relationships
        .iter()
        .find(|pending| pending.target.terminal_name == "hashCode")
        .expect("super.hashCode call");
    assert_eq!(hash_code.receiver_type.as_deref(), Some("Task"));
}
