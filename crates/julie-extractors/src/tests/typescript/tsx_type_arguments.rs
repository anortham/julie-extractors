//! TSX ordered/nested generic type-argument capture via the canonical pipeline.
//!
//! TSX files route through the TypeScript extractor with the TSX grammar.
//! These tests prove the basic TSX fixture through `extract_canonical(...)`,
//! not the plain `.ts` parser helper in `type_arguments.rs`.

use crate::base::TypeArgumentUsage;
use std::path::Path;

const FIXTURE_SOURCE: &str =
    include_str!("../../../../../fixtures/extraction/tsx/basic/source.tsx");

fn top_level(usage: &TypeArgumentUsage) -> Vec<(u32, &str)> {
    usage
        .arguments
        .iter()
        .map(|arg| (arg.ordinal, arg.type_name.as_str()))
        .collect()
}

fn extract_fixture(source: &str) -> crate::ExtractionResults {
    crate::pipeline::extract_canonical(
        "fixtures/extraction/tsx/basic/source.tsx",
        source,
        Path::new("/repo"),
    )
    .expect("canonical TSX extraction should succeed")
}

#[test]
fn basic_fixture_emits_nested_type_arguments_via_canonical_pipeline() {
    let results = extract_fixture(FIXTURE_SOURCE);
    assert_eq!(
        results.type_argument_usages.len(),
        1,
        "fixture should emit one Map<string, Array<number>> usage, got {:?}",
        results.type_argument_usages
    );
    let usage = &results.type_argument_usages[0];
    assert_eq!(top_level(usage), vec![(0, "string"), (1, "Array")]);
    assert!(usage.arguments[0].children.is_empty());
    assert_eq!(
        usage.arguments[1]
            .children
            .iter()
            .map(|c| (c.ordinal, c.type_name.as_str()))
            .collect::<Vec<_>>(),
        vec![(0, "number")],
        "Array<number> nested argument preserved under ordinal 1"
    );
}

#[test]
fn basic_fixture_non_generic_function_emits_no_type_arguments() {
    let results = extract_fixture(FIXTURE_SOURCE);
    assert!(
        !results
            .type_argument_usages
            .iter()
            .any(|usage| usage.identifier_id.contains(":format:")),
        "plain format() must not emit type_argument_usages"
    );
}
