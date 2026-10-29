use crate::ExtractionResults;
use crate::extract_canonical;
use crate::tests::helpers::{
    call_owners, owner_rows, supertype_edges, symbol_metadata, symbol_named, test_flags,
};
use std::path::Path;

fn extract(path: &str, source: &str) -> ExtractionResults {
    extract_canonical(path, source, Path::new("/tmp/test")).expect("fsharp extraction")
}

#[test]
fn object_expression_owns_its_members_and_records_its_supertypes() {
    let result = extract(
        "src/Make.fs",
        "module M\n\
         type ITask =\n\
         \x20   abstract Run : unit -> int\n\
         type Base(n: int) =\n\
         \x20   abstract M : unit -> int\n\
         \x20   default _.M() = n\n\
         let helper () = 1\n\
         let make () =\n\
         \x20   { new ITask with\n\
         \x20       member _.Run() = helper () }\n\
         let other () =\n\
         \x20   { new Base(1) with\n\
         \x20       member x.M() = helper ()\n\
         \x20     interface System.IDisposable with\n\
         \x20       member _.Dispose() = () }\n\
         let value = { new ITask with member _.Run() = 2 }\n",
    );

    let rows = owner_rows(&result);
    let anonymous_rows: Vec<&str> = rows
        .iter()
        .map(String::as_str)
        .filter(|row| row.contains("anonymous_class"))
        .collect();
    assert_eq!(
        anonymous_rows,
        [
            "class anonymous_class_l9 < make",
            "method run < anonymous_class_l9",
            "class anonymous_class_l12 < other",
            "method m < anonymous_class_l12",
            "method dispose < anonymous_class_l12",
            "class anonymous_class_l16 < value",
            "method run < anonymous_class_l16",
        ]
    );
    let anonymous = symbol_named(&result, "anonymous_class_L12");
    assert_eq!(symbol_metadata(anonymous, "type"), Some("anonymous_class"));
    assert_eq!(anonymous.signature.as_deref(), Some("new Base(1)"));
    assert_eq!(
        anonymous.metadata.as_ref().unwrap()["base_types"],
        serde_json::json!(["Base", "System.IDisposable"])
    );
    assert_eq!(
        symbol_named(&result, "anonymous_class_L16")
            .parent_id
            .as_deref(),
        Some(symbol_named(&result, "value").id.as_str())
    );
    assert_eq!(
        supertype_edges(&result),
        [
            "extends anonymous_class_l12 -> base",
            "implements anonymous_class_l12 -> ?idisposable",
            "implements anonymous_class_l16 -> itask",
            "implements anonymous_class_l9 -> itask",
        ]
    );
    assert_eq!(
        call_owners(&result),
        ["helper in Run", "Base in other", "helper in M"]
    );
}

#[test]
fn object_expressions_in_tests_keep_test_detection() {
    let result = extract(
        "tests/WorkerTests.fs",
        "module WorkerTests\n\
         open Xunit\n\
         \n\
         type WorkerTests() =\n\
         \x20   [<Fact>]\n\
         \x20   member _.RunsTask() =\n\
         \x20       let d = { new System.IDisposable with member _.Dispose() = () }\n\
         \x20       d.Dispose()\n",
    );

    assert_eq!(
        test_flags(&result),
        [
            "RunsTask: is_test,test_role",
            "WorkerTests: test_container,test_role",
        ]
    );
}
