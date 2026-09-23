use std::path::Path;

use crate::ExtractionResults;
use crate::base::{IdentifierKind, RelationshipKind, SourceRegionKind, StructuralFact, Symbol};
use crate::tests::helpers::{facts_with_pattern, metadata_str};

fn extract(file_path: &str, source: &str) -> ExtractionResults {
    crate::pipeline::extract_canonical(file_path, source, Path::new("/repo"))
        .expect("canonical regex extraction should succeed")
}

fn symbol<'a>(results: &'a ExtractionResults, name: &str) -> &'a Symbol {
    results
        .symbols
        .iter()
        .find(|symbol| symbol.name == name)
        .unwrap_or_else(|| panic!("missing symbol {name}: {:#?}", names(results)))
}

fn names(results: &ExtractionResults) -> Vec<&str> {
    results.symbols.iter().map(|s| s.name.as_str()).collect()
}

fn meta<'a>(symbol: &'a Symbol, key: &str) -> Option<&'a serde_json::Value> {
    symbol.metadata.as_ref().and_then(|m| m.get(key))
}

fn parent_name<'a>(results: &'a ExtractionResults, child: &Symbol) -> Option<&'a str> {
    let parent_id = child.parent_id.as_ref()?;
    results
        .symbols
        .iter()
        .find(|symbol| &symbol.id == parent_id)
        .map(|symbol| symbol.name.as_str())
}

fn fact_u64(fact: &StructuralFact, key: &str) -> Option<u64> {
    fact.metadata.as_ref()?.get(key)?.as_u64()
}

fn fact_bool(fact: &StructuralFact, key: &str) -> Option<bool> {
    fact.metadata.as_ref()?.get(key)?.as_bool()
}

#[test]
fn lookarounds_and_unicode_properties_nest_under_their_enclosing_symbols() {
    let source = "^(?=.*[A-Z])(?<pw>.{8,64})$\n(?<price>(?<=\\$)\\d+)\n^(?<name>\\p{Lu}+)[\\p{Script=Greek}\\d]$\n";
    let results = extract("rules.regex", source);

    let lookahead = symbol(&results, "(?=.*[A-Z])");
    assert_eq!(
        parent_name(&results, lookahead),
        Some("^(?=.*[A-Z])(?<pw>.{8,64})$")
    );
    assert_eq!(
        parent_name(&results, symbol(&results, "[A-Z]")),
        Some("(?=.*[A-Z])")
    );
    assert_eq!(
        parent_name(&results, symbol(&results, "(?<=\\$)")),
        Some("price")
    );
    assert_eq!(
        parent_name(&results, symbol(&results, "\\p{Lu}")),
        Some("name")
    );
    assert_eq!(
        parent_name(&results, symbol(&results, "\\p{Script=Greek}")),
        Some("[\\p{Script=Greek}\\d]")
    );
    assert_eq!(
        results
            .symbols
            .iter()
            .filter(|s| s.name == "(?=.*[A-Z])")
            .count(),
        1
    );
}

#[test]
fn nested_lookaround_takes_direction_and_polarity_from_its_opening_token() {
    let results = extract("file.regex", "^(?!.*(?<=\\.)$)[\\w.-]+$\n");

    let outer = symbol(&results, "(?!.*(?<=\\.)$)");
    assert_eq!(meta(outer, "direction").unwrap(), "lookahead");
    assert_eq!(meta(outer, "positive").unwrap(), "false");
    let inner = symbol(&results, "(?<=\\.)");
    assert_eq!(parent_name(&results, inner), Some("(?!.*(?<=\\.)$)"));
    assert_eq!(meta(inner, "direction").unwrap(), "lookbehind");
    assert_eq!(meta(inner, "positive").unwrap(), "true");

    let facts = facts_with_pattern(&results, "regex.lookaround.v1");
    let summary: Vec<_> = facts
        .iter()
        .map(|f| {
            (
                metadata_str(f, "direction").unwrap(),
                metadata_str(f, "polarity").unwrap(),
            )
        })
        .collect();
    assert_eq!(
        summary,
        vec![("lookahead", "negative"), ("lookbehind", "positive")]
    );
}

#[test]
fn lookarounds_count_toward_nesting_depth() {
    let results = extract("pw.regex", "^(?=.*[A-Z])x$\n");
    let lookahead = symbol(&results, "(?=.*[A-Z])");
    let metric = results
        .complexity_metrics
        .iter()
        .find(|m| m.symbol_id.as_deref() == Some(lookahead.id.as_str()))
        .expect("lookaround metric");
    assert_eq!(metric.max_nesting_depth, 1);
}

#[test]
fn python_named_backreference_links_to_its_group() {
    let results = extract("quote.regex", "(?P<quote>[\"'])(?P<body>.*?)(?P=quote)\n");
    let group = symbol(&results, "quote");
    let reference = results
        .relationships
        .iter()
        .find(|r| r.kind == RelationshipKind::References && r.to_symbol_id == group.id)
        .expect("named backreference edge");
    assert_eq!(
        reference.metadata.as_ref().unwrap()["referenceType"],
        "named-backreference"
    );
    assert!(
        results
            .identifiers
            .iter()
            .any(|i| i.kind == IdentifierKind::Call && i.name == "quote")
    );
}

#[test]
fn named_groups_are_capturing() {
    let results = extract("date.regex", "^(?<year>\\d{4})-(?P<month>\\d{2})$\n");
    assert_eq!(meta(symbol(&results, "year"), "capturing").unwrap(), "true");
    assert_eq!(
        meta(symbol(&results, "month"), "capturing").unwrap(),
        "true"
    );
}

#[test]
fn boundary_and_string_anchors_emit_anchor_facts() {
    let results = extract("anch.regex", "\\bcolou?r\\B\n\\A[a-z]{3}\\z\\Z\n");
    let kinds: Vec<_> = facts_with_pattern(&results, "regex.anchor.v1")
        .iter()
        .map(|f| metadata_str(f, "anchor_kind").unwrap())
        .collect();
    assert_eq!(
        kinds,
        vec![
            "word_boundary",
            "non_word_boundary",
            "string_start",
            "absolute_end",
            "string_end"
        ]
    );
}

#[test]
fn alternation_branch_count_counts_only_direct_branches() {
    let results = extract("alt.regex", "^(?:GET|POST|PUT)\\s/\\S*$|^[|!]$\n");
    let mut counts: Vec<_> = facts_with_pattern(&results, "regex.alternation.v1")
        .iter()
        .map(|f| fact_u64(f, "branch_count").unwrap())
        .collect();
    counts.sort();
    assert_eq!(counts, vec![2, 3]);
}

#[test]
fn pattern_literal_runs_are_recorded() {
    let results = extract(
        "route.regex",
        "^/api/v1/users/(?<id>\\d+)/(orders|invoices)$\ncolou?r\n(foo)\\1\n",
    );
    let texts: Vec<_> = results
        .literals
        .iter()
        .map(|l| l.literal_text.as_str())
        .collect();
    assert_eq!(
        texts,
        vec!["api", "v1", "users", "orders", "invoices", "colo", "foo"]
    );
    let foo = results
        .literals
        .iter()
        .find(|l| l.literal_text == "foo")
        .unwrap();
    assert_eq!(foo.end_byte - foo.start_byte, 3);
    assert!(
        results
            .literals
            .iter()
            .all(|l| l.carrier.as_deref() == Some("pattern"))
    );
    let mut artifact_literals = results.literals.clone();
    crate::classify_literals_by_carrier(&mut artifact_literals);
    assert_eq!(artifact_literals.len(), results.literals.len());
}

#[test]
fn regexp_extension_is_indexed_as_regex() {
    let results = extract("email.regexp", "^(?<user>\\w+)@(?<domain>[\\w.-]+)$\n");
    assert!(results.symbols.iter().any(|s| s.name == "user"));
    assert_eq!(
        crate::language::detect_language_from_extension("regexp"),
        Some("regex")
    );
}

#[test]
fn inline_flag_groups_emit_inline_flags_facts() {
    let results = extract("flags.regex", "(?i)^select\\s+(?s:.*)(?-m)x(?i-s:y)$\n");
    let facts = facts_with_pattern(&results, "regex.inline_flags.v1");
    let summary: Vec<_> = facts
        .iter()
        .map(|f| {
            (
                metadata_str(f, "enabled_flags").unwrap(),
                metadata_str(f, "disabled_flags").unwrap(),
                fact_bool(f, "scoped").unwrap(),
            )
        })
        .collect();
    assert_eq!(
        summary,
        vec![
            ("i", "", false),
            ("s", "", true),
            ("", "m", false),
            ("i", "s", true)
        ]
    );
}

#[test]
fn verbose_mode_comments_are_not_parsed_as_pattern() {
    let source = "(?x)  # ISO date\n(?<year>\\d{4})   # year (\\1 is unused)\n-\n(?<month>\\d{2})  # month\n";
    let results = extract("date.regex", source);

    assert_eq!(meta(symbol(&results, "month"), "captureIndex").unwrap(), 2);
    assert!(
        results.relationships.is_empty(),
        "{:#?}",
        results.relationships
    );
    assert!(!names(&results).iter().any(|name| name.contains("unused")));

    let root = results
        .symbols
        .iter()
        .find(|s| s.parent_id.is_none())
        .unwrap();
    assert_eq!(root.name, "(?x)(?<year>\\d{4})-(?<month>\\d{2})");

    let comments: Vec<_> = results
        .source_regions
        .iter()
        .filter(|r| r.kind == SourceRegionKind::Comment)
        .map(|r| &source[r.start_byte as usize..r.end_byte as usize])
        .collect();
    assert_eq!(
        comments,
        vec!["# ISO date", "# year (\\1 is unused)", "# month"]
    );
}

#[test]
fn root_pattern_span_excludes_the_line_terminator() {
    for source in ["^a|b$\n", "^a|b$\r\n"] {
        let results = extract("email.regex", source);
        let root = results
            .symbols
            .iter()
            .find(|s| s.parent_id.is_none())
            .unwrap();
        assert_eq!(root.name, "^a|b$");
        assert_eq!(meta(root, "pattern").unwrap(), "^a|b$");
        assert_eq!((root.start_line, root.end_line), (1, 1));
        assert_eq!(root.end_byte, 5);
    }
}

#[test]
fn backreferences_emit_backreference_facts() {
    let results = extract(
        "tags.regex",
        "<([a-z]+)>(.*?)</\\1>\\2\\3\n(?<w>\\w+)\\k<w>\\k<nope>\n(?P<q>')(?P=q)\n",
    );
    let facts = facts_with_pattern(&results, "regex.backreference.v1");
    let summary: Vec<_> = facts
        .iter()
        .map(|f| {
            (
                metadata_str(f, "form").unwrap().to_string(),
                fact_u64(f, "capture_index")
                    .map(|i| i.to_string())
                    .or_else(|| metadata_str(f, "capture_name").map(str::to_string))
                    .unwrap(),
                fact_bool(f, "resolved").unwrap(),
            )
        })
        .collect();
    assert_eq!(
        summary,
        vec![
            ("numeric".to_string(), "1".to_string(), true),
            ("numeric".to_string(), "2".to_string(), true),
            ("numeric".to_string(), "3".to_string(), false),
            ("named".to_string(), "w".to_string(), true),
            ("named".to_string(), "nope".to_string(), false),
            ("python_named".to_string(), "q".to_string(), true),
        ]
    );
}

#[test]
fn possessive_quantifiers_and_quoted_literals_emit_facts() {
    let results = extract("adv.regex", "a++b\\Qa.b\\E\n");
    let possessive = facts_with_pattern(&results, "regex.quantifier.v1")
        .into_iter()
        .find(|f| fact_bool(f, "possessive") == Some(true))
        .expect("possessive quantifier fact");
    assert_eq!(metadata_str(possessive, "quantifier"), Some("++"));

    let quoted = facts_with_pattern(&results, "regex.quoted_literal.v1");
    assert_eq!(quoted.len(), 1);
    assert_eq!(metadata_str(quoted[0], "literal_text"), Some("a.b"));
}
