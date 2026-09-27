use std::path::Path;

use julie_extractors::{ExtractionResults, RelationshipKind, StructuralFact, extract_canonical};

const SOURCE: &str = include_str!("../../../fixtures/extraction/regex/branch_reset/source.regex");

fn extract(source: &str) -> ExtractionResults {
    extract_canonical("branch_reset.regex", source, Path::new("/repo"))
        .expect("extraction should succeed")
}

fn fact_number(fact: &StructuralFact, key: &str) -> Option<u64> {
    fact.metadata.as_ref()?.get(key)?.as_u64()
}

fn fact_text(fact: &StructuralFact) -> &'static str {
    &SOURCE[fact.start_byte as usize..fact.end_byte as usize]
}

fn capture_numbers(results: &ExtractionResults, line: u32) -> Vec<(&'static str, u64)> {
    let mut captures: Vec<_> = results
        .structural_facts
        .iter()
        .filter(|fact| {
            fact.start_line == line
                && matches!(
                    fact.pattern_id.as_str(),
                    "regex.capture_group.v1" | "regex.named_capture.v1"
                )
        })
        .map(|fact| {
            (
                fact.start_byte,
                fact_text(fact),
                fact_number(fact, "capture_index").unwrap(),
            )
        })
        .collect();
    captures.sort();
    captures
        .into_iter()
        .map(|(_, text, index)| (text, index))
        .collect()
}

fn reference_targets(
    results: &ExtractionResults,
    line: u32,
    reference_type: &str,
) -> Vec<(&'static str, u64)> {
    let mut targets: Vec<_> = results
        .relationships
        .iter()
        .filter(|relationship| {
            relationship.line_number == line
                && relationship.kind == RelationshipKind::References
                && relationship
                    .metadata
                    .as_ref()
                    .and_then(|metadata| metadata.get("referenceType"))
                    .and_then(|value| value.as_str())
                    == Some(reference_type)
        })
        .map(|relationship| {
            let target = results
                .symbols
                .iter()
                .find(|symbol| symbol.id == relationship.to_symbol_id)
                .expect("reference target should be an extracted symbol");
            let span = target.body_span.expect("capture symbol should have a span");
            let index = target
                .metadata
                .as_ref()
                .and_then(|metadata| metadata.get("captureIndex"))
                .and_then(|value| value.as_u64())
                .expect("capture symbol should carry its capture number");
            (
                span.start_byte,
                &SOURCE[span.start_byte as usize..span.end_byte as usize],
                index,
            )
        })
        .collect();
    targets.sort();
    targets
        .into_iter()
        .map(|(_, text, index)| (text, index))
        .collect()
}

#[test]
fn branch_reset_groups_emit_facts_with_direct_branch_counts() {
    let results = extract(SOURCE);
    let facts: Vec<_> = results
        .structural_facts
        .iter()
        .filter(|fact| fact.pattern_id == "regex.branch_reset.v1")
        .map(|fact| {
            assert_eq!(fact.capture_name, "branch_reset");
            assert_eq!(fact.node_kind, "branch_reset_group");
            (
                fact.start_line,
                fact_text(fact),
                fact_number(fact, "branch_count").unwrap(),
            )
        })
        .collect();

    assert_eq!(
        facts,
        vec![
            (1, "(?|(a)|(b))", 2),
            (2, "(?|(a)(b)|(c))", 2),
            (3, r"(?|(?<year>\d{4})|(?<year>\d{2}))", 2),
            (4, "(?|(a)(b)|(c))", 2),
            (5, "(?|(a)|(b)(?(-1)c))", 2),
            (6, "(?|(a)(?|(b)|(c))|(d))", 2),
            (6, "(?|(b)|(c))", 2),
            (7, "(?|abc|def)", 2),
            (8, "(?|(a))", 1),
        ]
    );
    assert!(results.parse_diagnostics.is_empty());
}

#[test]
fn each_branch_restarts_capture_numbers_and_later_groups_follow_the_widest_branch() {
    let results = extract(SOURCE);

    assert_eq!(
        capture_numbers(&results, 2),
        vec![("(a)", 1), ("(b)", 2), ("(c)", 1), ("(d)", 3)]
    );
    assert_eq!(
        capture_numbers(&results, 3),
        vec![(r"(?<year>\d{4})", 1), (r"(?<year>\d{2})", 1)]
    );
    assert_eq!(
        capture_numbers(&results, 5),
        vec![("(x)", 1), ("(a)", 2), ("(b)", 2), ("(e)", 3)]
    );
    assert_eq!(
        capture_numbers(&results, 6),
        vec![("(a)", 1), ("(b)", 2), ("(c)", 2), ("(d)", 1)]
    );
}

#[test]
fn references_to_a_shared_capture_number_target_every_group_holding_it() {
    let results = extract(SOURCE);

    assert_eq!(
        reference_targets(&results, 1, "numeric-backreference"),
        vec![("(a)", 1), ("(b)", 1)]
    );
    assert_eq!(
        reference_targets(&results, 2, "numeric-backreference"),
        vec![("(d)", 3)]
    );
    assert_eq!(
        reference_targets(&results, 3, "named-backreference"),
        vec![(r"(?<year>\d{4})", 1), (r"(?<year>\d{2})", 1)]
    );
    assert_eq!(
        reference_targets(&results, 6, "numeric-backreference"),
        vec![("(b)", 2), ("(c)", 2)]
    );
    assert_eq!(
        reference_targets(&results, 8, "numeric-backreference"),
        vec![("(a)", 1)]
    );
}

#[test]
fn relative_conditions_count_from_the_capture_numbers_opened_so_far() {
    let results = extract(SOURCE);

    assert_eq!(
        reference_targets(&results, 4, "relative-condition"),
        vec![("(b)", 2)]
    );
    assert_eq!(
        reference_targets(&results, 5, "relative-condition"),
        vec![("(a)", 2), ("(b)", 2), ("(e)", 3)]
    );
}

#[test]
fn backreferences_past_the_widest_branch_are_unresolved() {
    let source = r"(?|(a)(b)|(c))\3";
    let results = extract_canonical("unresolved.regex", source, Path::new("/repo")).unwrap();
    let backreference = results
        .structural_facts
        .iter()
        .find(|fact| fact.pattern_id == "regex.backreference.v1")
        .expect("backreference fact should be present");

    assert_eq!(
        backreference
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.get("resolved"))
            .and_then(|value| value.as_bool()),
        Some(false)
    );
    assert!(
        !results
            .relationships
            .iter()
            .any(|relationship| relationship.kind == RelationshipKind::References)
    );
}
