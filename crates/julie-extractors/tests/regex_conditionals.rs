use std::path::Path;

use julie_extractors::{
    ExtractionResults, IdentifierKind, StructuralFact, Symbol, extract_canonical,
};

const SOURCE: &str = include_str!("../../../fixtures/extraction/regex/conditionals/source.regex");
const CONDITIONAL_PATTERN: &str = "regex.conditional.v1";
const NAMED_CAPTURE_PATTERN: &str = "regex.named_capture.v1";
const CAPTURE_GROUP_PATTERN: &str = "regex.capture_group.v1";

fn extract(path: &str, source: &str) -> ExtractionResults {
    extract_canonical(path, source, Path::new("/repo")).expect("extraction should succeed")
}

fn conditional_facts(results: &ExtractionResults) -> Vec<&StructuralFact> {
    let mut facts: Vec<_> = results
        .structural_facts
        .iter()
        .filter(|fact| fact.pattern_id == CONDITIONAL_PATTERN)
        .collect();
    facts.sort_by_key(|fact| fact.start_byte);
    facts
}

fn fact_string<'a>(fact: &'a StructuralFact, key: &str) -> Option<&'a str> {
    fact.metadata.as_ref()?.get(key)?.as_str()
}

fn fact_number(fact: &StructuralFact, key: &str) -> Option<u64> {
    fact.metadata.as_ref()?.get(key)?.as_u64()
}

fn symbol_number(symbol: &Symbol, key: &str) -> Option<u64> {
    symbol.metadata.as_ref()?.get(key)?.as_u64()
}

fn has_reference_to(
    results: &ExtractionResults,
    line: u32,
    target: &Symbol,
    reference_type: &str,
) -> bool {
    results.relationships.iter().any(|relationship| {
        relationship.line_number == line
            && relationship.to_symbol_id == target.id
            && relationship.kind == julie_extractors::RelationshipKind::References
            && relationship.metadata.as_ref().is_some_and(|metadata| {
                metadata
                    .get("referenceType")
                    .and_then(|value| value.as_str())
                    == Some(reference_type)
            })
    })
}

#[test]
fn conditionals_preserve_capture_indexes_and_emit_source_spans() {
    let results = extract("conditionals.regex", SOURCE);
    let facts = conditional_facts(&results);

    assert_eq!(
        facts
            .iter()
            .map(|fact| (
                fact_string(fact, "condition").unwrap().to_owned(),
                fact_number(fact, "branch_count").unwrap(),
            ))
            .collect::<Vec<_>>(),
        vec![
            ("1".to_owned(), 2),
            ("<named>".to_owned(), 1),
            ("bare".to_owned(), 2),
            ("'quoted'".to_owned(), 2),
            ("(?=a)".to_owned(), 2),
            ("(?!a)".to_owned(), 1),
            ("(?<=a)".to_owned(), 2),
            ("(?<!z)".to_owned(), 1),
            ("outer".to_owned(), 2),
            ("1".to_owned(), 2),
            ("1".to_owned(), 2),
            ("1".to_owned(), 2),
            ("(?=(a))".to_owned(), 1),
            ("R".to_owned(), 2),
            ("R1".to_owned(), 2),
            ("R&name".to_owned(), 2),
            ("DEFINE".to_owned(), 1),
            ("VERSION>=10.4".to_owned(), 2),
            ("VERSION=10".to_owned(), 2),
            ("R".to_owned(), 2),
            ("-1".to_owned(), 2),
            ("-1".to_owned(), 2),
            ("+1".to_owned(), 2),
            ("-2".to_owned(), 2),
            ("+1".to_owned(), 2),
            ("+2".to_owned(), 2),
            ("R1".to_owned(), 2),
            ("DEFINE".to_owned(), 2),
            ("VERSION>=10".to_owned(), 2),
            ("-1".to_owned(), 2),
        ]
    );
    assert_eq!(
        facts
            .iter()
            .map(|fact| { &SOURCE[fact.start_byte as usize..fact.end_byte as usize] })
            .collect::<Vec<_>>(),
        vec![
            "(?(1)b|c)",
            "(?(<named>)b)",
            "(?(bare)b|c)",
            "(?('quoted')b|c)",
            "(?(?=a)b|c)",
            "(?(?!a)b)",
            "(?(?<=a)b|c)",
            "(?(?<!z)d)",
            "(?(outer)(?(1)b|c)|d)",
            "(?(1)b|c)",
            "(?(1)(a|b)|c)",
            "(?(1)\\(|\\))",
            "(?(?=(a))b)",
            "(?(R)a|b)",
            "(?(R1)a|b)",
            "(?(R&name)a|b)",
            "(?(DEFINE)(?<helper>a))",
            "(?(VERSION>=10.4)a|b)",
            "(?(VERSION=10)a|b)",
            "(?(R)b|c)",
            "(?(-1)c|d)",
            "(?(-1)a|b)",
            "(?(+1)a|b)",
            "(?(-2)c|d)",
            "(?(+1)b|c)",
            "(?(+2)a|b)",
            "(?(R1)b|c)",
            "(?(DEFINE)b|c)",
            "(?(VERSION>=10)b|c)",
            "(?(-1)a|b)",
        ]
    );
    assert!(facts.iter().all(|fact| {
        fact.capture_name == "conditional"
            && fact.node_kind == "conditional_group"
            && fact_string(fact, "query_family") == Some("pattern_structure")
    }));

    let named_captures = results
        .structural_facts
        .iter()
        .filter(|fact| fact.pattern_id == NAMED_CAPTURE_PATTERN)
        .map(|fact| {
            (
                fact.start_line,
                fact_string(fact, "capture_name").unwrap().to_owned(),
                fact_number(fact, "capture_index").unwrap(),
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        named_captures,
        vec![
            (1, "before".to_owned(), 1),
            (1, "after".to_owned(), 2),
            (2, "named".to_owned(), 1),
            (2, "named_after".to_owned(), 2),
            (3, "bare".to_owned(), 1),
            (3, "bare_after".to_owned(), 2),
            (4, "quoted".to_owned(), 1),
            (4, "quoted_after".to_owned(), 2),
            (8, "outer".to_owned(), 1),
            (8, "outer_after".to_owned(), 2),
            (11, "assertion_after".to_owned(), 2),
            (15, "helper".to_owned(), 1),
            (18, "R".to_owned(), 1),
            (19, "preceding".to_owned(), 1),
            (19, "latest".to_owned(), 2),
            (19, "relative_after".to_owned(), 3),
            (21, "next_capture".to_owned(), 1),
            (22, "older".to_owned(), 1),
            (22, "first".to_owned(), 2),
            (22, "last".to_owned(), 3),
            (23, "first".to_owned(), 1),
            (23, "next".to_owned(), 2),
            (24, "only".to_owned(), 1),
            (25, "R1".to_owned(), 1),
            (26, "DEFINE".to_owned(), 1),
            (27, "VERSION".to_owned(), 1),
            (28, "enclosing".to_owned(), 1),
        ]
    );
    let escaped_and_class_captures: Vec<_> = results
        .structural_facts
        .iter()
        .filter(|fact| fact.pattern_id == CAPTURE_GROUP_PATTERN && fact.start_line == 10)
        .filter_map(|fact| fact_number(fact, "capture_index"))
        .collect();
    assert_eq!(escaped_and_class_captures, vec![1]);
}

#[test]
fn conditional_capture_references_are_usage_sites() {
    let results = extract("conditionals.regex", SOURCE);
    let named_capture = results
        .symbols
        .iter()
        .find(|symbol| symbol.name == "bare" && symbol.start_line == 3)
        .expect("named capture should be a symbol");
    let anonymous_capture = results
        .symbols
        .iter()
        .find(|symbol| symbol.start_line == 10 && symbol_number(symbol, "captureIndex") == Some(1))
        .expect("numbered condition should retain the anonymous capture symbol");

    assert!(has_reference_to(
        &results,
        3,
        named_capture,
        "named-condition"
    ));
    assert!(has_reference_to(
        &results,
        10,
        anonymous_capture,
        "numeric-condition"
    ));
    assert!(results.identifiers.iter().any(|identifier| {
        identifier.name == "bare"
            && identifier.start_line == 3
            && identifier.kind == IdentifierKind::Call
    }));
    assert!(!results.identifiers.iter().any(|identifier| {
        matches!(identifier.start_line, 12..=17 | 26..=27)
            && identifier.kind == IdentifierKind::Call
    }));
    assert!(!results.relationships.iter().any(|relationship| {
        matches!(relationship.line_number, 12..=17 | 26..=27)
            && relationship.kind == julie_extractors::RelationshipKind::References
    }));
    let named_recursion_group = results
        .symbols
        .iter()
        .find(|symbol| symbol.name == "R" && symbol.start_line == 18)
        .expect("named R capture should be a symbol");
    assert!(results.identifiers.iter().any(|identifier| {
        identifier.name == "R"
            && identifier.start_line == 18
            && identifier.kind == IdentifierKind::Call
    }));
    assert!(has_reference_to(
        &results,
        18,
        named_recursion_group,
        "named-condition"
    ));
    let named_r1_group = results
        .symbols
        .iter()
        .find(|symbol| symbol.name == "R1" && symbol.start_line == 25)
        .expect("named R1 capture should be a symbol");
    assert!(results.identifiers.iter().any(|identifier| {
        identifier.name == "R1"
            && identifier.start_line == 25
            && identifier.kind == IdentifierKind::Call
    }));
    assert!(has_reference_to(
        &results,
        25,
        named_r1_group,
        "named-condition"
    ));
}

#[test]
fn relative_condition_references_resolve_from_capture_open_order() {
    let results = extract("conditionals.regex", SOURCE);
    let expected = [
        (19, "latest", 2),
        (21, "next_capture", 1),
        (22, "older", 1),
        (23, "next", 2),
        (28, "enclosing", 1),
    ];

    for (line, target_name, capture_index) in expected {
        let target = results
            .symbols
            .iter()
            .find(|symbol| symbol.name == target_name && symbol.start_line == line)
            .expect("relative condition target should be a capture symbol");
        assert!(results.relationships.iter().any(|relationship| {
            relationship.line_number == line
                && relationship.to_symbol_id == target.id
                && relationship.kind == julie_extractors::RelationshipKind::References
                && relationship.metadata.as_ref().is_some_and(|metadata| {
                    metadata
                        .get("referenceType")
                        .and_then(|value| value.as_str())
                        == Some("relative-condition")
                        && metadata
                            .get("captureIndex")
                            .and_then(|value| value.as_u64())
                            == Some(capture_index)
                })
        }));
    }

    assert!(!results.relationships.iter().any(|relationship| {
        matches!(relationship.line_number, 20 | 24)
            && relationship.metadata.as_ref().is_some_and(|metadata| {
                metadata
                    .get("referenceType")
                    .and_then(|value| value.as_str())
                    == Some("relative-condition")
            })
    }));
}

#[test]
fn condition_decisions_do_not_double_count_branch_separators() {
    let source = "(?(1)(a|b)|c)";
    let results = extract("complexity.regex", source);
    let file_metric = results
        .complexity_metrics
        .iter()
        .find(|metric| metric.scope == "file")
        .expect("file complexity metric should be present");

    assert_eq!(file_metric.decision_count, 2);
    let alternations = results
        .structural_facts
        .iter()
        .filter(|fact| fact.pattern_id == "regex.alternation.v1")
        .map(|fact| &source[fact.start_byte as usize..fact.end_byte as usize])
        .collect::<Vec<_>>();
    assert_eq!(alternations, vec!["a|b"]);
}

#[test]
fn captures_inside_atomic_groups_keep_indexes_and_complexity_spans() {
    let source = r"(?>(?<inside>a|b)+)(?(inside)c|d)";
    let results = extract("atomic-condition.regex", source);

    let capture_fact = results
        .structural_facts
        .iter()
        .find(|fact| {
            fact.pattern_id == NAMED_CAPTURE_PATTERN
                && fact_number(fact, "capture_index") == Some(1)
        })
        .expect("capture inside the atomic group should retain index one");
    assert_eq!(
        &source[capture_fact.start_byte as usize..capture_fact.end_byte as usize],
        "(?<inside>a|b)"
    );

    let capture = results
        .symbols
        .iter()
        .find(|symbol| symbol.name == "inside")
        .expect("capture inside the atomic group should be a symbol");
    assert!(has_reference_to(&results, 1, capture, "named-condition"));

    let conditional = conditional_facts(&results)
        .into_iter()
        .next()
        .expect("conditional following the atomic group should be extracted");
    assert_eq!(
        &source[conditional.start_byte as usize..conditional.end_byte as usize],
        "(?(inside)c|d)"
    );

    let file_metric = results
        .complexity_metrics
        .iter()
        .find(|metric| metric.scope == "file")
        .expect("file complexity metric should be present");
    assert_eq!(
        (file_metric.start_byte, file_metric.end_byte),
        (0, source.len() as u32)
    );
    assert_eq!(file_metric.decision_count, 2);
    assert_eq!(file_metric.loop_count, 1);
    assert_eq!(file_metric.max_nesting_depth, 2);

    let body_span = capture
        .body_span
        .expect("capture symbol should expose its source span");
    let capture_metric = results
        .complexity_metrics
        .iter()
        .find(|metric| metric.scope == "symbol" && metric.symbol_id.as_deref() == Some(&capture.id))
        .expect("capture complexity metric should be present");
    assert_eq!(capture_metric.start_byte, body_span.start_byte);
    assert_eq!(capture_metric.end_byte, body_span.end_byte);
    assert_eq!(
        &source[body_span.start_byte as usize..body_span.end_byte as usize],
        "(?<inside>a|b)"
    );
}

#[test]
fn verbose_conditionals_ignore_comments_and_keep_capture_order() {
    let source =
        "(?x)\n(?<before>a) # (?(99)ignored)\n(?(before)\n  (b|c)\n  | d\n)\n(?<after>e)\n";
    let results = extract("verbose.regex", source);
    let facts = conditional_facts(&results);

    assert_eq!(facts.len(), 1);
    assert_eq!(fact_string(facts[0], "condition"), Some("before"));
    let named_captures = results
        .structural_facts
        .iter()
        .filter(|fact| fact.pattern_id == NAMED_CAPTURE_PATTERN)
        .map(|fact| fact_number(fact, "capture_index").unwrap())
        .collect::<Vec<_>>();
    assert_eq!(named_captures, vec![1, 3]);
    assert_eq!(
        &source[facts[0].start_byte as usize..facts[0].end_byte as usize],
        "(?(before)\n  (b|c)\n  | d\n)"
    );
}

#[test]
fn a_single_conditional_pattern_has_no_parser_diagnostics() {
    let results = extract("condition.regex", "(?<capture>a)(?(capture)b|c)");

    assert!(
        results.parse_diagnostics.is_empty(),
        "{:#?}",
        results.parse_diagnostics
    );
    assert_eq!(conditional_facts(&results).len(), 1);
}
