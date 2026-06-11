//! Python ordered/nested generic type-argument capture (Miller bridge Phase 2).
//!
//! Python uses subscript syntax for generics: `List[int]`, `Dict[str, List[int]]`.
//! These appear as use sites in variable annotations, typed parameters, and
//! return type hints — all paths that pass through a `type` node, which is the
//! boundary that `is_python_type_usage_node` checks for.
//!
//! Heritage/construction generics (`class Repo(Mapping[str, int])`) are
//! captured by allowing `argument_list` nodes whose parent is `class_definition`
//! to count as type-usage positions in `is_python_type_usage_node`.

use crate::base::TypeArgumentUsage;
use crate::python::PythonExtractor;
use std::path::Path;
use std::path::PathBuf;
use tree_sitter::Parser;

const FIXTURE_SOURCE: &str =
    include_str!("../../../../../fixtures/extraction/python/basic/source.py");

fn capture(code: &str) -> Vec<TypeArgumentUsage> {
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_python::LANGUAGE.into())
        .expect("load Python grammar");
    let tree = parser.parse(code, None).expect("parse Python");
    let mut ext = PythonExtractor::new(
        "test.py".to_string(),
        code.to_string(),
        &PathBuf::from("/test/workspace"),
    );
    let symbols = ext.extract_symbols(&tree);
    ext.extract_identifiers(&tree, &symbols);
    ext.base().type_argument_usages.clone()
}

/// Flatten a usage's top-level arguments to `(ordinal, type_name)` pairs.
fn top_level(usage: &TypeArgumentUsage) -> Vec<(u32, &str)> {
    usage
        .arguments
        .iter()
        .map(|arg| (arg.ordinal, arg.type_name.as_str()))
        .collect()
}

#[test]
fn annotation_single_arg_records_one_argument() {
    // `Optional[User]` is a single-arg subscript in a variable annotation.
    // The outermost generic is `Optional`; `User` rides along as ordinal 0.
    let code = r#"
class User: pass
x: Optional[User] = None
"#;
    let usages = capture(code);
    assert_eq!(
        usages.len(),
        1,
        "exactly one generic use site (Optional[User]), got {usages:?}"
    );
    assert_eq!(top_level(&usages[0]), vec![(0, "User")]);
    assert!(usages[0].arguments[0].children.is_empty());
}

#[test]
fn annotation_nested_generic_preserves_order_and_nesting() {
    // `Dict[str, List[User]]` — Dict is the outermost; List[User] is nested.
    // Top-level args: (0, "str"), (1, "List"). List carries child (0, "User").
    // `List` inside Dict's args must NOT create a second TypeArgumentUsage row.
    let code = r#"
class User: pass
x: Dict[str, List[User]] = {}
"#;
    let usages = capture(code);
    assert_eq!(
        usages.len(),
        1,
        "Dict is the only outermost generic use site (List[User] is nested), got {usages:?}"
    );
    let args = &usages[0].arguments;
    assert_eq!(top_level(&usages[0]), vec![(0, "str"), (1, "List")]);
    assert!(args[0].children.is_empty(), "str has no nested args");
    assert_eq!(
        args[1]
            .children
            .iter()
            .map(|c| (c.ordinal, c.type_name.as_str()))
            .collect::<Vec<_>>(),
        vec![(0, "User")],
        "List[User] nested argument preserved under ordinal 1"
    );
}

#[test]
fn typed_parameter_single_arg_records_argument() {
    // `List[Item]` in a function parameter — same hook, different AST parent.
    let code = r#"
class Item: pass
def process(items: List[Item]) -> None:
    pass
"#;
    let usages = capture(code);
    assert_eq!(
        usages.len(),
        1,
        "one generic parameter annotation, got {usages:?}"
    );
    assert_eq!(top_level(&usages[0]), vec![(0, "Item")]);
    assert!(usages[0].arguments[0].children.is_empty());
}

#[test]
fn class_heritage_generic_base_records_arguments() {
    // `class Repo(Mapping[str, int])` — class inherits a generic base.
    // Heritage subscript is in an `argument_list` whose parent is `class_definition`.
    // Bug: `argument_list` was a stopping node in `is_python_type_usage_node`,
    // so `Mapping` never got a TypeUsage identifier. Fix: allow class-heritage position.
    let code = r#"
class Repo(Mapping[str, int]):
    pass
"#;
    let usages = capture(code);
    assert_eq!(
        usages.len(),
        1,
        "exactly one generic use site (Mapping[str,int] in class base), got {usages:?}"
    );
    assert_eq!(
        top_level(&usages[0]),
        vec![(0, "str"), (1, "int")],
        "Mapping base ordinals must be (0,str) (1,int)"
    );
}

#[test]
fn call_arg_generic_does_not_record() {
    // `foo(Bar[int])` — generic subscript passed as a call argument, not a heritage base.
    // Must NOT produce any TypeArgumentUsage rows (regression guard for the heritage fix).
    let code = r#"
foo(Bar[int])
"#;
    let usages = capture(code);
    assert!(
        usages.is_empty(),
        "call-argument generic must not record type args, got {usages:?}"
    );
}

#[test]
fn non_generic_annotation_records_no_arguments() {
    // Plain `User` annotation — no subscript, no type arguments recorded.
    let code = r#"
class User: pass
x: User = None
"#;
    let usages = capture(code);
    assert!(
        usages.is_empty(),
        "non-generic annotation must record no type arguments, got {usages:?}"
    );
}

fn extract_fixture(source: &str) -> crate::ExtractionResults {
    crate::pipeline::extract_canonical(
        "fixtures/extraction/python/basic/source.py",
        source,
        Path::new("/repo"),
    )
    .expect("canonical Python extraction should succeed")
}

#[test]
fn basic_fixture_emits_nested_type_arguments_via_canonical_pipeline() {
    let results = extract_fixture(FIXTURE_SOURCE);
    assert_eq!(
        results.type_argument_usages.len(),
        1,
        "fixture should emit one Dict[str, List[int]] usage, got {:?}",
        results.type_argument_usages
    );
    let usage = &results.type_argument_usages[0];
    assert_eq!(top_level(usage), vec![(0, "str"), (1, "List")]);
    assert!(usage.arguments[0].children.is_empty());
    assert_eq!(
        usage.arguments[1]
            .children
            .iter()
            .map(|c| (c.ordinal, c.type_name.as_str()))
            .collect::<Vec<_>>(),
        vec![(0, "int")],
        "List[int] nested argument preserved under ordinal 1"
    );
}

#[test]
fn basic_fixture_non_generic_scalar_emits_no_type_arguments() {
    let results = extract_fixture(FIXTURE_SOURCE);
    assert!(
        !results
            .type_argument_usages
            .iter()
            .any(|usage| usage.identifier_id.contains(":Worker:")),
        "plain Worker class must not emit type_argument_usages"
    );
}
