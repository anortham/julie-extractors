use julie_extractors::{
    ExtractionLevel, ExtractionResults, Identifier, IdentifierKind,
    extract_canonical_for_language_at,
};
use std::path::Path;

#[test]
fn r_qualified_calls_anchor_the_terminal_name() {
    let source = "service$run(); service@slot(); pkg::exported(); pkg:::internal(); service$run()";
    let results = extract_canonical_for_language_at(
        "r",
        "source.r",
        source,
        Path::new("."),
        ExtractionLevel::Full,
    )
    .unwrap();
    let calls: Vec<_> = results
        .identifiers
        .iter()
        .filter(|identifier| identifier.kind == IdentifierKind::Call)
        .collect();
    assert_eq!(calls.len(), 5);
    for call in calls {
        assert_eq!(
            &source[call.start_byte as usize..call.end_byte as usize],
            call.name,
            "{} must select its target token",
            call.name
        );
    }
    let runs: Vec<_> = results
        .identifiers
        .iter()
        .filter(|identifier| identifier.kind == IdentifierKind::Call && identifier.name == "run")
        .collect();
    assert_eq!(runs.len(), 2);
    assert_ne!(runs[0].id, runs[1].id);
    assert_ne!(runs[0].start_byte, runs[1].start_byte);
}

#[test]
fn r_member_spans_skip_comments_and_preserve_unicode_coordinates() {
    let source = "café$ # receiver\n  méthode(); café@ # slot\n  donné";
    let results = extract_canonical_for_language_at(
        "r",
        "source.r",
        source,
        Path::new("."),
        ExtractionLevel::Full,
    )
    .unwrap();
    for (name, kind, line) in [
        ("méthode", IdentifierKind::Call, 2),
        ("donné", IdentifierKind::MemberAccess, 3),
    ] {
        let identifier = results
            .identifiers
            .iter()
            .find(|identifier| identifier.name == name && identifier.kind == kind)
            .unwrap_or_else(|| panic!("missing {name}"));
        let start = source.find(name).unwrap();
        assert_eq!(identifier.start_byte as usize, start);
        assert_eq!(identifier.end_byte as usize, start + name.len());
        assert_eq!(identifier.start_line, line);
        assert_eq!(identifier.end_line, line);
        assert_eq!(identifier.start_column, 2);
        assert_eq!(identifier.end_column as usize, 2 + name.len());
    }
}

#[test]
fn html_declared_ids_and_classes_use_each_value_token_span() {
    let source = "<div id=\"worker\" class=\"  worker\tworker café \"></div><span id=unquoted></span><p class=bare></p>";
    let results = extract_canonical_for_language_at(
        "html",
        "source.html",
        source,
        Path::new("."),
        ExtractionLevel::Full,
    )
    .unwrap();

    assert_named_token_spans(&results, source, "worker", &["worker", "worker", "worker"]);
    assert_named_token_spans(&results, source, "café", &["café"]);
    assert_named_token_spans(&results, source, "unquoted", &["unquoted"]);
    assert_named_token_spans(&results, source, "bare", &["bare"]);
}

#[test]
fn regex_named_backreferences_use_the_name_token_span() {
    let source = "é(?P<word>x)\n(?P=word)\n\\k<word>";
    let results = extract_canonical_for_language_at(
        "regex",
        "source.regex",
        source,
        Path::new("."),
        ExtractionLevel::Full,
    )
    .unwrap();
    let capture_start = source.find("<word>").unwrap() + 1;
    let python_reference_start = source.find("(?P=word)").unwrap() + 4;
    let escaped_reference_start = source.find("\\k<word>").unwrap() + 3;

    assert_token_span(
        results
            .identifiers
            .iter()
            .find(|identifier| {
                identifier.name == "word" && identifier.kind == IdentifierKind::MemberAccess
            })
            .unwrap(),
        source,
        "word",
        "word",
        capture_start,
    );
    let mut references: Vec<_> = results
        .identifiers
        .iter()
        .filter(|identifier| identifier.name == "word" && identifier.kind == IdentifierKind::Call)
        .collect();
    references.sort_by_key(|identifier| identifier.start_byte);
    assert_eq!(references.len(), 2);
    assert_token_span(
        references[0],
        source,
        "word",
        "word",
        python_reference_start,
    );
    assert_token_span(
        references[1],
        source,
        "word",
        "word",
        escaped_reference_start,
    );
}

#[test]
fn csharp_target_typed_new_uses_the_new_keyword_span() {
    let source = "class Example { Order Build() { Order order = new(); Order explicitOrder = new Order(); return new(); } } class Order {}";
    let results = extract_canonical_for_language_at(
        "csharp",
        "source.cs",
        source,
        Path::new("."),
        ExtractionLevel::Full,
    )
    .unwrap();
    let calls: Vec<_> = results
        .identifiers
        .iter()
        .filter(|identifier| identifier.name == "Order" && identifier.kind == IdentifierKind::Call)
        .collect();
    let new_starts: Vec<_> = source
        .match_indices("new()")
        .map(|(start, _)| start)
        .collect();
    assert_eq!(calls.len(), new_starts.len() + 1);
    let mut target_typed_calls: Vec<_> = calls
        .iter()
        .copied()
        .filter(|identifier| {
            &source[identifier.start_byte as usize..identifier.end_byte as usize] == "new"
        })
        .collect();
    target_typed_calls.sort_by_key(|identifier| identifier.start_byte);
    let explicit_calls: Vec<_> = calls
        .iter()
        .copied()
        .filter(|identifier| {
            &source[identifier.start_byte as usize..identifier.end_byte as usize] == "Order"
        })
        .collect();
    assert_eq!(target_typed_calls.len(), new_starts.len());
    assert_eq!(explicit_calls.len(), 1);
    let build_symbol_id = results
        .symbols
        .iter()
        .find(|symbol| symbol.name == "Build")
        .unwrap()
        .id
        .clone();
    for (identifier, start) in target_typed_calls.into_iter().zip(new_starts) {
        assert_token_span(identifier, source, "Order", "new", start);
        assert_eq!(
            identifier.containing_symbol_id.as_deref(),
            Some(build_symbol_id.as_str())
        );
    }
    assert_token_span(
        explicit_calls[0],
        source,
        "Order",
        "Order",
        source.find("new Order").unwrap() + "new ".len(),
    );
    assert_eq!(
        explicit_calls[0].containing_symbol_id.as_deref(),
        Some(build_symbol_id.as_str())
    );
}

fn assert_named_token_spans(
    results: &ExtractionResults,
    source: &str,
    name: &str,
    expected_tokens: &[&str],
) {
    let mut identifiers: Vec<_> = results
        .identifiers
        .iter()
        .filter(|identifier| {
            identifier.name == name && identifier.kind == IdentifierKind::MemberAccess
        })
        .collect();
    identifiers.sort_by_key(|identifier| identifier.start_byte);
    assert_eq!(identifiers.len(), expected_tokens.len());
    let mut search_start = 0;
    for (identifier, token) in identifiers.into_iter().zip(expected_tokens) {
        let relative_start = source[search_start..].find(token).unwrap();
        let expected_start = search_start + relative_start;
        assert_token_span(identifier, source, name, token, expected_start);
        search_start = expected_start + token.len();
    }
}

fn assert_token_span(
    identifier: &Identifier,
    source: &str,
    expected_name: &str,
    token: &str,
    expected_start: usize,
) {
    let expected_end = expected_start + token.len();
    let before = &source[..expected_start];
    let expected_line = before.bytes().filter(|byte| *byte == b'\n').count() as u32 + 1;
    let expected_column = before.rsplit('\n').next().unwrap().len() as u32;
    assert_eq!(identifier.name, expected_name);
    assert_eq!(identifier.start_byte as usize, expected_start);
    assert_eq!(identifier.end_byte as usize, expected_end);
    assert_eq!(&source[expected_start..expected_end], token);
    assert_eq!(identifier.start_line, expected_line);
    assert_eq!(identifier.end_line, expected_line);
    assert_eq!(identifier.start_column, expected_column);
    assert_eq!(identifier.end_column, expected_column + token.len() as u32);
}
