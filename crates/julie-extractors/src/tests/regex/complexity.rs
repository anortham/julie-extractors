use std::path::Path;

fn extract(file_path: &str, source: &str) -> crate::ExtractionResults {
    crate::pipeline::extract_canonical(file_path, source, Path::new("/repo"))
        .expect("canonical regex extraction should succeed")
}

const FIXTURE_SOURCE: &str =
    include_str!("../../../../../fixtures/extraction/regex/basic/source.regex");

#[test]
fn regex_complexity_metrics_emit_file_and_symbol_scopes() {
    // Hand-tallied expectations for fixtures/extraction/regex/basic/source.regex:
    //   decisions (0): no alternation/conditional nodes
    //   loops (2): quantifiers on [A-Za-z]+ and \d+
    //   max nesting depth (1): named capture groups, no nested groups
    let results = extract(
        "fixtures/extraction/regex/basic/source.regex",
        FIXTURE_SOURCE,
    );

    let file_metric = results
        .complexity_metrics
        .iter()
        .find(|metric| metric.scope == "file")
        .expect("expected file complexity metric");
    assert_eq!(file_metric.algorithm_id, "julie-regex-complexity-v1");
    assert_eq!(file_metric.language, "regex");
    assert_eq!(file_metric.symbol_id, None);
    assert_eq!(file_metric.decision_count, 0);
    assert_eq!(file_metric.loop_count, 2);
    assert_eq!(file_metric.max_nesting_depth, 1);
    assert_eq!(file_metric.parameter_count, None);

    let name_group = results
        .symbols
        .iter()
        .find(|symbol| symbol.name == "(?<name>[A-Za-z]+)")
        .expect("expected name capture symbol");
    let body_span = name_group
        .body_span
        .expect("named capture should expose a body span");
    let name_metric = results
        .complexity_metrics
        .iter()
        .find(|metric| {
            metric.scope == "symbol" && metric.symbol_id.as_deref() == Some(&name_group.id)
        })
        .expect("expected name group complexity metric");
    assert_eq!(name_metric.algorithm_id, "julie-regex-complexity-v1");
    assert_eq!(name_metric.start_byte, body_span.start_byte);
    assert_eq!(name_metric.end_byte, body_span.end_byte);
    assert_eq!(name_metric.decision_count, 0);
    assert_eq!(name_metric.loop_count, 1);
    assert_eq!(name_metric.max_nesting_depth, 1);
}

#[test]
fn regex_symbol_complexity_does_not_inherit_surrounding_alternation_or_quantifiers() {
    let source = r"(a|b)(?<named>[a-z]+)(?<simple>bar)";

    let results = extract("source.regex", source);

    let file_metric = results
        .complexity_metrics
        .iter()
        .find(|metric| metric.scope == "file")
        .expect("expected file complexity metric");
    assert_eq!(file_metric.decision_count, 1);
    assert_eq!(file_metric.loop_count, 1);

    let named = results
        .symbols
        .iter()
        .find(|symbol| symbol.name == "(?<named>[a-z]+)")
        .expect("expected named capture symbol");
    let named_metric = results
        .complexity_metrics
        .iter()
        .find(|metric| metric.scope == "symbol" && metric.symbol_id.as_deref() == Some(&named.id))
        .expect("expected named capture complexity metric");
    assert_eq!(named_metric.decision_count, 0);
    assert_eq!(named_metric.loop_count, 1);

    let simple = results
        .symbols
        .iter()
        .find(|symbol| symbol.name == "(?<simple>bar)")
        .expect("expected simple capture symbol");
    let simple_metric = results
        .complexity_metrics
        .iter()
        .find(|metric| metric.scope == "symbol" && metric.symbol_id.as_deref() == Some(&simple.id))
        .expect("expected simple capture complexity metric");
    assert_eq!(simple_metric.decision_count, 0);
    assert_eq!(simple_metric.loop_count, 0);
}

#[test]
fn regex_named_group_metric_prefers_body_span_after_leading_comment() {
    let source = r"(?# leading comment)(?<part>[a-z]+)";

    let results = extract("source.regex", source);

    let part = results
        .symbols
        .iter()
        .find(|symbol| symbol.name == "(?<part>[a-z]+)")
        .expect("expected part capture symbol");
    assert!(
        part.start_byte > 0,
        "named capture declaration should begin after the leading inline comment"
    );
    let body_span = part
        .body_span
        .expect("named capture should expose a body span");
    assert!(
        body_span.start_byte >= part.start_byte && body_span.end_byte <= part.end_byte,
        "body span should stay inside the named capture declaration"
    );

    let part_metric = results
        .complexity_metrics
        .iter()
        .find(|metric| metric.scope == "symbol" && metric.symbol_id.as_deref() == Some(&part.id))
        .expect("expected part capture complexity metric");
    assert_eq!(part_metric.start_byte, body_span.start_byte);
    assert_eq!(part_metric.end_byte, body_span.end_byte);
    assert_eq!(part_metric.loop_count, 1);
}
