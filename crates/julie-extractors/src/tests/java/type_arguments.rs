//! Java ordered/nested generic type-argument capture (Miller bridge Phase 2).
//!
//! Every generic *use site* (`List<String>` field, `new ArrayList<String>()`,
//! `extends ArrayList<String>`, `Map<String, List<Integer>>` nesting) must emit
//! its applied type arguments in order, with nesting preserved, attached to the
//! use-site identifier. Order is the whole point — assert exact ordinals, not
//! set membership. Nested generics are captured as children of their enclosing
//! usage (one usage per outermost generic), never double-counted.

use crate::base::TypeArgumentUsage;
use crate::java::JavaExtractor;
use std::path::Path;
use std::path::PathBuf;
use tree_sitter::Parser;

const FIXTURE_SOURCE: &str =
    include_str!("../../../../../fixtures/extraction/java/basic/source.java");

fn capture(code: &str) -> Vec<TypeArgumentUsage> {
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_java::LANGUAGE.into())
        .expect("load Java grammar");
    let tree = parser.parse(code, None).expect("parse Java");
    let mut ext = JavaExtractor::new(
        "java".to_string(),
        "test.java".to_string(),
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
fn field_single_generic_records_one_argument() {
    // `List<String>` field declaration — single outermost generic use site.
    let code = r#"
public class Repo {
    public List<String> items;
}
"#;
    let usages = capture(code);
    assert_eq!(
        usages.len(),
        1,
        "exactly one generic use site (List<String>), got {usages:?}"
    );
    assert_eq!(top_level(&usages[0]), vec![(0, "String")]);
    assert!(usages[0].arguments[0].children.is_empty());
}

#[test]
fn map_two_args_records_ordered_pair() {
    // `Map<String, Integer>` — two ordered type arguments.
    let code = r#"
public class Repo {
    public Map<String, Integer> counts;
}
"#;
    let usages = capture(code);
    assert_eq!(
        usages.len(),
        1,
        "exactly one generic use site (Map<String,Integer>), got {usages:?}"
    );
    assert_eq!(
        top_level(&usages[0]),
        vec![(0, "String"), (1, "Integer")],
        "Map<String,Integer> ordinal order must be preserved"
    );
}

#[test]
fn object_creation_generic_records_argument() {
    // `new ArrayList<String>()` — generic on the constructor call.
    let code = r#"
public class Repo {
    public void init() {
        new ArrayList<String>();
    }
}
"#;
    let usages = capture(code);
    assert_eq!(
        usages.len(),
        1,
        "exactly one generic use site (ArrayList<String>), got {usages:?}"
    );
    assert_eq!(top_level(&usages[0]), vec![(0, "String")]);
    assert!(usages[0].arguments[0].children.is_empty());
}

#[test]
fn inheritance_generic_records_type_arg() {
    // `extends ArrayList<String>` — generic in superclass clause.
    let code = r#"
public class MyList extends ArrayList<String> {
}
"#;
    let usages = capture(code);
    assert_eq!(
        usages.len(),
        1,
        "one generic use site in extends clause (ArrayList<String>), got {usages:?}"
    );
    assert_eq!(top_level(&usages[0]), vec![(0, "String")]);
    assert!(usages[0].arguments[0].children.is_empty());
}

#[test]
fn nested_generic_preserves_order_and_nesting() {
    // `Map<String, List<Integer>>` — Map is the outermost; List<Integer> is nested.
    // The List usage must NOT be double-counted as a separate top-level usage.
    let code = r#"
public class Repo {
    public Map<String, List<Integer>> index;
}
"#;
    let usages = capture(code);
    assert_eq!(
        usages.len(),
        1,
        "Map is the only outermost generic use site (List<Integer> is nested), got {usages:?}"
    );
    let args = &usages[0].arguments;
    assert_eq!(
        top_level(&usages[0]),
        vec![(0, "String"), (1, "List")],
        "outer arguments: String at 0, List at 1"
    );
    assert!(args[0].children.is_empty(), "String has no nested args");
    assert_eq!(
        args[1]
            .children
            .iter()
            .map(|c| (c.ordinal, c.type_name.as_str()))
            .collect::<Vec<_>>(),
        vec![(0, "Integer")],
        "List<Integer> nested argument preserved under ordinal 1"
    );
}

#[test]
fn non_generic_type_records_no_arguments() {
    // Plain `List` with no `<...>` — zero rows.
    let code = r#"
public class Repo {
    public List items;
}
"#;
    let usages = capture(code);
    assert!(
        usages.is_empty(),
        "non-generic use must record no type arguments, got {usages:?}"
    );
}

fn extract_fixture(source: &str) -> crate::ExtractionResults {
    crate::pipeline::extract_canonical(
        "fixtures/extraction/java/basic/source.java",
        source,
        Path::new("/repo"),
    )
    .expect("canonical Java extraction should succeed")
}

#[test]
fn basic_fixture_emits_nested_type_arguments_via_canonical_pipeline() {
    let results = extract_fixture(FIXTURE_SOURCE);
    assert_eq!(
        results.type_argument_usages.len(),
        3,
        "fixture should emit one Map<String, List<Integer>> usage and two BinaryOperator<Integer> usages, got {:?}",
        results.type_argument_usages
    );
    let operator_usages: Vec<_> = results
        .type_argument_usages
        .iter()
        .filter(|usage| top_level(usage) == vec![(0, "Integer")])
        .collect();
    assert_eq!(operator_usages.len(), 2);
    assert!(
        operator_usages
            .iter()
            .all(|usage| usage.arguments[0].children.is_empty())
    );
    let usage = results
        .type_argument_usages
        .iter()
        .find(|usage| top_level(usage) == vec![(0, "String"), (1, "List")])
        .expect("Map<String, List<Integer>> usage");
    assert!(usage.arguments[0].children.is_empty());
    assert_eq!(
        usage.arguments[1]
            .children
            .iter()
            .map(|c| (c.ordinal, c.type_name.as_str()))
            .collect::<Vec<_>>(),
        vec![(0, "Integer")],
        "List<Integer> nested argument preserved under ordinal 1"
    );
}

#[test]
fn basic_fixture_non_generic_scalar_emits_no_type_arguments() {
    let results = extract_fixture(FIXTURE_SOURCE);
    assert!(
        !results
            .type_argument_usages
            .iter()
            .any(|usage| usage.identifier_id.contains(":Job:")),
        "plain Job interface must not emit type_argument_usages"
    );
}
