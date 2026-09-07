use julie_extractors::syntax::{SyntaxError, parse_source};
use std::path::Path;

#[test]
fn syntax_api_returns_tree_and_recovery_diagnostics() {
    let source = "fn compute() { let answer = 42; }\n";
    let parsed = parse_source(Path::new("unicode/λ.rs"), source).unwrap();
    assert_eq!(parsed.language, "rust");
    assert!(parsed.diagnostics.is_empty());
    let function = parsed.tree.root_node().named_child(0).unwrap();
    let body = function.child_by_field_name("body").unwrap();
    assert_eq!(&source[body.byte_range()], "{ let answer = 42; }");
    assert_eq!(
        &source[function.start_byte()..body.start_byte()],
        "fn compute() "
    );

    let malformed = "fn broken( {\n";
    let recovered = parse_source(Path::new("broken.rs"), malformed).unwrap();
    assert!(recovered.tree.root_node().has_error());
    assert!(!recovered.diagnostics.is_empty());
    assert!(
        recovered
            .diagnostics
            .iter()
            .all(|d| { d.start_byte <= d.end_byte && d.end_byte as usize <= malformed.len() })
    );
    assert!(matches!(
        parse_source(Path::new("x.unknown"), "x"),
        Err(SyntaxError::UnsupportedLanguage { .. })
    ));
    assert!(matches!(
        parse_source(Path::new("x.JSONL"), "{}\n{}\n"),
        Err(SyntaxError::UnsupportedContainer { .. })
    ));
    assert!(matches!(
        parse_source(Path::new("x.jsonl"), "{}\n{}\n"),
        Err(SyntaxError::UnsupportedContainer { .. })
    ));
    assert!(matches!(
        parse_source(Path::new(".jsonl"), "{}\n{}\n"),
        Err(SyntaxError::UnsupportedContainer { .. })
    ));
}

#[test]
fn syntax_api_preserves_unicode_crlf_byte_coordinates() {
    let source = "fn café() { let x = ; }\r\n";
    let parsed = parse_source(Path::new("test.rs"), source).unwrap();
    assert!(!parsed.diagnostics.is_empty());
    for d in &parsed.diagnostics {
        assert!(d.start_byte <= d.end_byte);
        assert!((d.end_byte as usize) <= source.len());
        assert!(source.is_char_boundary(d.start_byte as usize));
        assert!(source.is_char_boundary(d.end_byte as usize));

        let start_offset = d.start_byte as usize;
        let expected_start_line = source.as_bytes()[..start_offset]
            .iter()
            .filter(|&&b| b == b'\n')
            .count() as u32
            + 1;
        let last_nl_start = source.as_bytes()[..start_offset]
            .iter()
            .rposition(|&b| b == b'\n');
        let expected_start_col = match last_nl_start {
            Some(pos) => (start_offset - (pos + 1)) as u32,
            None => start_offset as u32,
        };
        assert_eq!(d.start_line, expected_start_line);
        assert_eq!(d.start_column, expected_start_col);

        let end_offset = d.end_byte as usize;
        let expected_end_line = source.as_bytes()[..end_offset]
            .iter()
            .filter(|&&b| b == b'\n')
            .count() as u32
            + 1;
        let last_nl_end = source.as_bytes()[..end_offset]
            .iter()
            .rposition(|&b| b == b'\n');
        let expected_end_col = match last_nl_end {
            Some(pos) => (end_offset - (pos + 1)) as u32,
            None => end_offset as u32,
        };
        assert_eq!(d.end_line, expected_end_line);
        assert_eq!(d.end_column, expected_end_col);
    }
}

#[test]
fn syntax_api_handles_missing_nodes() {
    let source = "fn f( {}";
    let parsed = parse_source(Path::new("missing.rs"), source).unwrap();
    assert!(parsed.tree.root_node().has_error());
    let missing = parsed
        .diagnostics
        .iter()
        .find(|d| d.kind == julie_extractors::ParseDiagnosticKind::Missing);
    assert!(missing.is_some());
    let d = missing.unwrap();
    assert_eq!(d.start_byte, d.end_byte);
    assert_eq!(d.start_line, d.end_line);
}

#[test]
fn syntax_api_selects_source_sensitive_grammars() {
    let cases = [
        ("math.h", "int add(int a, int b);\n", "c"),
        (
            "widget.h",
            "namespace app { class Widget { public: void run(); }; }\n",
            "cpp",
        ),
        ("empty.h", "", "c"),
        ("widget.H", "class C { public: int x; };\n", "cpp"),
        ("qmldir", "module MyModule\n", "qmldir"),
        ("code.fs", "module M\nlet x = 1\n", "fsharp"),
        ("script.fsx", "let y = 2\n", "fsharp"),
        (
            "api.fsi",
            "namespace Sample\nval compute : int -> int\n",
            "fsharp",
        ),
    ];
    for (filename, source, expected_lang) in cases {
        let parsed = parse_source(Path::new(filename), source).unwrap();
        assert_eq!(parsed.language, expected_lang, "{filename}");
        if filename == "api.fsi" {
            assert!(parsed.diagnostics.is_empty(), "api.fsi diagnostics");
        }
    }
}

#[test]
fn syntax_api_covers_every_supported_entry() {
    use julie_extractors::{capability_snapshot, supported_languages};
    use std::collections::BTreeSet;

    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let expected: BTreeSet<_> = supported_languages().into_iter().collect();
    let mut seen = BTreeSet::new();
    let snapshot = capability_snapshot();
    for row in snapshot.languages() {
        let fixture = row
            .fixtures
            .iter()
            .find(|f| f.name == "basic" && !f.source.ends_with(".jsonl"))
            .or_else(|| row.fixtures.iter().find(|f| !f.source.ends_with(".jsonl")))
            .expect("every language entry needs a non-JSONL host fixture");
        let path = root.join(&fixture.source);
        let source = std::fs::read_to_string(&path).unwrap();
        let parsed = parse_source(&path, &source).unwrap();
        assert_eq!(parsed.language, row.language, "{}", path.display());
        for diagnostic in &parsed.diagnostics {
            assert!(diagnostic.start_byte <= diagnostic.end_byte);
            assert!(diagnostic.end_byte as usize <= source.len());
        }
        seen.insert(row.language.as_str());
    }
    assert_eq!(seen, expected);
}

#[test]
fn syntax_api_reports_host_only_composition() {
    use julie_extractors::capability_snapshot;

    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let snapshot = capability_snapshot();
    for target_lang in ["vue", "markdown", "html", "razor"] {
        let row = snapshot
            .languages()
            .find(|r| r.language == target_lang)
            .expect("target language must exist in capabilities");
        let fixture = row
            .fixtures
            .iter()
            .find(|f| f.name == "basic" && !f.source.ends_with(".jsonl"))
            .or_else(|| row.fixtures.iter().find(|f| !f.source.ends_with(".jsonl")))
            .expect("target language needs a non-JSONL host fixture");
        let path = root.join(&fixture.source);
        let source = std::fs::read_to_string(&path).unwrap();
        let parsed = parse_source(&path, &source).unwrap();
        assert_eq!(parsed.language, target_lang);
        assert_eq!(parsed.tree.root_node().byte_range(), 0..source.len());
        for diagnostic in &parsed.diagnostics {
            assert!(diagnostic.start_byte <= diagnostic.end_byte);
            assert!(diagnostic.end_byte as usize <= source.len());
        }
    }
}

#[test]
fn syntax_api_allows_empty_supported_source() {
    let parsed = parse_source(Path::new("empty.rs"), "").unwrap();
    assert_eq!(parsed.language, "rust");
    assert!(parsed.diagnostics.is_empty());
    assert_eq!(parsed.tree.root_node().byte_range(), 0..0);
}

#[test]
fn syntax_api_rejects_cancelled_or_expired_requests() {
    use julie_extractors::syntax::{SyntaxOptions, parse_source_with_options};
    use std::sync::atomic::AtomicBool;
    use std::time::Instant;

    let cancelled = AtomicBool::new(true);
    let options = SyntaxOptions {
        cancelled: Some(&cancelled),
        deadline: None,
        max_source_bytes: 1024,
    };
    assert!(matches!(
        parse_source_with_options(Path::new("x.rs"), "", &options),
        Err(SyntaxError::Cancelled)
    ));
    let options = SyntaxOptions {
        cancelled: None,
        deadline: Some(Instant::now()),
        max_source_bytes: 1024,
    };
    assert!(matches!(
        parse_source_with_options(Path::new("x.rs"), "", &options),
        Err(SyntaxError::DeadlineExceeded)
    ));
    let both_options = SyntaxOptions {
        cancelled: Some(&cancelled),
        deadline: Some(Instant::now()),
        max_source_bytes: 1024,
    };
    assert!(matches!(
        parse_source_with_options(Path::new("x.rs"), "", &both_options),
        Err(SyntaxError::Cancelled)
    ));
    let zero_options = SyntaxOptions {
        max_source_bytes: 0,
        ..SyntaxOptions::default()
    };
    assert!(parse_source_with_options(Path::new("x.rs"), "", &zero_options).is_ok());
    assert!(matches!(
        parse_source_with_options(Path::new("x.rs"), "a", &zero_options),
        Err(SyntaxError::InputTooLarge { bytes: 1 })
    ));
    let options = SyntaxOptions {
        max_source_bytes: 2,
        ..SyntaxOptions::default()
    };
    assert!(matches!(
        parse_source_with_options(Path::new("x.rs"), "fn f() {}", &options),
        Err(SyntaxError::InputTooLarge { .. })
    ));
}
