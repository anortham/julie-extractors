use crate::ExtractionResults;
use crate::base::RelationshipKind;
use crate::extract_canonical;
use crate::tests::helpers::{
    call_owners, owner_rows, supertype_edges, symbol_metadata, symbol_named, test_flags,
};
use std::path::Path;

fn extract(path: &str, source: &str) -> ExtractionResults {
    extract_canonical(path, source, Path::new("/tmp/test")).expect("kotlin extraction")
}

#[test]
fn object_expression_owns_its_members_under_the_function_or_property() {
    let result = extract(
        "src/Make.kt",
        "interface Task {\n\
         \x20   fun run(): Int\n\
         }\n\
         fun make(): Task {\n\
         \x20   return object : Task {\n\
         \x20       override fun run() = helper()\n\
         \x20       fun extra() = 2\n\
         \x20   }\n\
         }\n\
         fun helper() = 1\n\
         val holder = object : Task {\n\
         \x20   override fun run() = 3\n\
         }\n",
    );

    assert_eq!(
        owner_rows(&result),
        [
            "interface task < -",
            "method run < task",
            "function make < -",
            "class anonymous_class_l5 < make",
            "method run < anonymous_class_l5",
            "method extra < anonymous_class_l5",
            "function helper < -",
            "property holder < -",
            "class anonymous_class_l11 < holder",
            "method run < anonymous_class_l11",
        ]
    );
    let anonymous = symbol_named(&result, "anonymous_class_L5");
    assert_eq!(symbol_metadata(anonymous, "type"), Some("anonymous_class"));
    assert_eq!(anonymous.signature.as_deref(), Some("object : Task"));
    assert_eq!(
        anonymous.metadata.as_ref().unwrap()["base_types"],
        serde_json::json!(["Task"])
    );
    assert_eq!(
        supertype_edges(&result),
        [
            "implements anonymous_class_l11 -> task",
            "implements anonymous_class_l5 -> task",
        ]
    );
    assert_eq!(call_owners(&result), ["helper in run"]);
}

#[test]
fn object_expression_records_superclass_and_interfaces_like_a_named_object() {
    let result = extract(
        "src/Listener.kt",
        "fun listen(button: Button) {\n\
         \x20   button.add(object : MouseAdapter(), Closeable {\n\
         \x20       override fun click() = close()\n\
         \x20   })\n\
         \x20   val bare = object {\n\
         \x20       fun x() = 1\n\
         \x20   }\n\
         }\n",
    );

    assert_eq!(
        supertype_edges(&result),
        [
            "extends anonymous_class_l2 -> ?mouseadapter",
            "implements anonymous_class_l2 -> ?closeable",
        ]
    );
    assert_eq!(
        owner_rows(&result),
        [
            "function listen < -",
            "class anonymous_class_l2 < listen",
            "method click < anonymous_class_l2",
            "class anonymous_class_l5 < bare",
            "method x < anonymous_class_l5",
        ]
    );
    assert!(
        symbol_named(&result, "anonymous_class_L5")
            .metadata
            .as_ref()
            .is_some_and(|metadata| !metadata.contains_key("base_types"))
    );
    assert_eq!(call_owners(&result), ["add in listen", "close in click"]);
}

#[test]
fn named_objects_companions_and_local_functions_keep_their_rows() {
    let result = extract(
        "src/Named.kt",
        "interface Task {\n\
         \x20   fun run(): Int\n\
         }\n\
         object Named : Task {\n\
         \x20   override fun run() = 4\n\
         }\n\
         class Host {\n\
         \x20   companion object {\n\
         \x20       fun create() = Host()\n\
         \x20   }\n\
         }\n\
         fun outer(): Int {\n\
         \x20   fun inner() = 1\n\
         \x20   return inner()\n\
         }\n",
    );

    assert_eq!(
        owner_rows(&result),
        [
            "interface task < -",
            "method run < task",
            "class named < -",
            "method run < named",
            "class host < -",
            "class companion < host",
            "method create < companion",
            "function outer < -",
            "function inner < outer",
        ]
    );
}

#[test]
fn object_expressions_in_tests_keep_test_detection() {
    let result = extract(
        "src/test/kotlin/SpecTest.kt",
        "import io.kotest.core.spec.style.FunSpec\n\
         import org.junit.jupiter.api.Test\n\
         \n\
         class WorkerTest {\n\
         \x20   @Test\n\
         \x20   fun runsTask() {\n\
         \x20       val r = object : Runnable {\n\
         \x20           override fun run() {}\n\
         \x20           fun testHelper() {}\n\
         \x20       }\n\
         \x20   }\n\
         }\n\
         \n\
         val factory = object : FunSpec({\n\
         \x20   test(\"inside\") {}\n\
         }) {}\n",
    );

    assert_eq!(
        test_flags(&result),
        [
            "WorkerTest: test_container,test_role",
            "factory: test_container,test_role",
            "inside: is_test,test_role",
            "runsTask: is_test,test_role",
            "testHelper: is_test,test_role",
        ]
    );
    assert_eq!(
        symbol_named(&result, "inside").parent_id.as_deref(),
        Some(symbol_named(&result, "factory").id.as_str())
    );
}

#[test]
fn this_inside_an_object_expression_names_the_object_not_the_outer_class() {
    let result = extract(
        "src/Host.kt",
        "interface Task {\n\
         \x20   fun run(): Int\n\
         }\n\
         class Host {\n\
         \x20   fun local() = 1\n\
         \x20   fun make(): Task {\n\
         \x20       return object : Task {\n\
         \x20           override fun run() = this.local()\n\
         \x20           fun local() = 2\n\
         \x20       }\n\
         \x20   }\n\
         }\n",
    );

    let symbol_at = |name: &str, line: u32| {
        result
            .symbols
            .iter()
            .find(|symbol| symbol.name == name && symbol.start_line == line)
            .unwrap_or_else(|| panic!("no {name} on line {line}"))
    };
    let (run, local_in_object) = (symbol_at("run", 8), symbol_at("local", 9));
    assert!(result.relationships.iter().any(|relationship| {
        relationship.kind == RelationshipKind::Calls
            && relationship.from_symbol_id == run.id
            && relationship.to_symbol_id == local_in_object.id
    }));
    assert!(
        !result
            .structured_pending_relationships
            .iter()
            .any(|pending| pending.target.terminal_name == "local")
    );
}
