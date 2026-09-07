use std::path::Path;

#[test]
fn strict_header_probe_propagates_failure() {
    let result = crate::language_spec::source_detection::detect_with_probe(
        Path::new("sample.h"),
        "int answer(void);",
        |_| Err(anyhow::anyhow!("probe sentinel")),
    );
    assert!(result.unwrap_err().to_string().contains("probe sentinel"));
}

#[test]
fn syntax_parser_none_is_parse_failed() {
    let result = crate::syntax::complete_parsed_tree(None, None);
    assert!(matches!(
        result,
        Err(crate::syntax::SyntaxError::ParseFailed { .. })
    ));
}

#[test]
fn syntax_parser_setup_error_preserves_cause() {
    use crate::syntax::{SyntaxError, parse_source, set_test_parser_setup_fail};
    use std::error::Error;

    set_test_parser_setup_fail(true);
    let result = parse_source(Path::new("test.rs"), "fn foo() {}\n");
    set_test_parser_setup_fail(false);

    match result {
        Err(err @ SyntaxError::ParseFailed { .. }) => {
            let source = err.source().expect("source must be present");
            assert!(source.to_string().contains("custom parser setup failed"));
        }
        other => panic!("expected ParseFailed error, got {:?}", other),
    }
}

#[test]
fn syntax_source_length_guard_does_not_truncate() {
    let max = (u32::MAX - 1) as usize;
    assert!(crate::syntax::validate_source_len(max, usize::MAX).is_ok());
    assert!(matches!(
        crate::syntax::validate_source_len(u32::MAX as usize, usize::MAX),
        Err(crate::syntax::SyntaxError::InputTooLarge { bytes }) if bytes == u32::MAX as usize
    ));
    assert!(crate::syntax::validate_source_len(0, 1000).is_ok());
    assert!(matches!(
        crate::syntax::validate_source_len(1001, 1000),
        Err(crate::syntax::SyntaxError::InputTooLarge { bytes: 1001 })
    ));
    assert!(crate::syntax::validate_source_len(0, 0).is_ok());
    assert!(matches!(
        crate::syntax::validate_source_len(1, 0),
        Err(crate::syntax::SyntaxError::InputTooLarge { bytes: 1 })
    ));
}

#[test]
fn syntax_header_reuses_winning_tree() {
    use crate::language_spec::source_detection::{
        header_probe_parse_count, reset_header_probe_parse_count,
    };
    use crate::syntax::{parse_source, reset_syntax_direct_parse_count, syntax_direct_parse_count};

    reset_header_probe_parse_count();
    reset_syntax_direct_parse_count();
    let parsed_h = parse_source(
        Path::new("widget.h"),
        "namespace app { class Widget { public: void run(); }; }\n",
    )
    .unwrap();
    assert_eq!(parsed_h.language, "cpp");
    assert_eq!(header_probe_parse_count(), 2);
    assert_eq!(syntax_direct_parse_count(), 0);

    reset_header_probe_parse_count();
    reset_syntax_direct_parse_count();
    let parsed_rs = parse_source(Path::new("main.rs"), "fn main() {}\n").unwrap();
    assert_eq!(parsed_rs.language, "rust");
    assert_eq!(header_probe_parse_count(), 0);
    assert_eq!(syntax_direct_parse_count(), 1);

    reset_header_probe_parse_count();
    reset_syntax_direct_parse_count();
    let parsed_empty_h = parse_source(Path::new("empty.h"), "").unwrap();
    assert_eq!(parsed_empty_h.language, "c");
    assert_eq!(header_probe_parse_count(), 0);
    assert_eq!(syntax_direct_parse_count(), 1);
}

#[test]
fn syntax_diagnostics_include_deep_recovery() {
    let mut deep = String::new();
    for _ in 0..1200 {
        deep.push_str("if true { ");
    }
    let error_start = deep.len();
    deep.push_str("let x = ; ");
    let error_end = deep.len();
    for _ in 0..1200 {
        deep.push_str("} ");
    }

    let handle = std::thread::Builder::new()
        .stack_size(256 * 1024)
        .spawn(move || {
            let parsed = crate::syntax::parse_source(Path::new("deep.rs"), &deep).unwrap();
            assert!(parsed.tree.root_node().has_error());
            assert!(!parsed.diagnostics.is_empty());
            let deep_diagnostic = parsed
                .diagnostics
                .iter()
                .find(|d| d.start_byte >= error_start as u32 && d.end_byte <= error_end as u32);
            assert!(
                deep_diagnostic.is_some(),
                "expected error diagnostic at depth 1200 within byte range {}..{}",
                error_start,
                error_end
            );
            for d in &parsed.diagnostics {
                assert!(d.start_byte <= d.end_byte);
                assert!((d.end_byte as usize) <= deep.len());
            }
        })
        .unwrap();
    handle.join().unwrap();
}

#[test]
fn syntax_path_keeps_non_utf8_parents() {
    #[cfg(unix)]
    {
        use std::ffi::OsStr;
        use std::os::unix::ffi::OsStrExt;
        let non_utf8 = OsStr::from_bytes(b"/tmp/\xff\xfe/test.rs");
        let path = Path::new(non_utf8);
        let parsed = crate::syntax::parse_source(path, "fn main() {}\n").unwrap();
        assert_eq!(parsed.language, "rust");
    }
    #[cfg(windows)]
    {
        let path = Path::new(r"C:\test_unicode\λ_dir\test.rs");
        let parsed = crate::syntax::parse_source(path, "fn main() {}\n").unwrap();
        assert_eq!(parsed.language, "rust");
    }
}

#[test]
fn syntax_cancellation_reaches_parser_progress() {
    use crate::syntax::{
        SyntaxError, SyntaxOptions, parse_source_with_options, set_test_progress_hook,
    };
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    let cancelled = Arc::new(AtomicBool::new(false));
    let cancelled_clone = cancelled.clone();
    let progress_count = Arc::new(AtomicUsize::new(0));
    let progress_count_clone = progress_count.clone();

    let source = "fn compute() { let mut x = 1; x += 2; }\n".repeat(80);
    assert!(source.len() >= 2000);

    set_test_progress_hook(Some(Box::new(move |_| {
        progress_count_clone.fetch_add(1, Ordering::SeqCst);
        cancelled_clone.store(true, Ordering::Release);
    })));

    let options = SyntaxOptions {
        cancelled: Some(cancelled.as_ref()),
        deadline: None,
        max_source_bytes: source.len() * 2,
    };

    let result = parse_source_with_options(Path::new("test.rs"), &source, &options);
    set_test_progress_hook(None);

    assert!(progress_count.load(Ordering::SeqCst) > 0);
    assert!(matches!(result, Err(SyntaxError::Cancelled)));
}

#[test]
fn syntax_deadline_reaches_parser_progress() {
    use crate::syntax::{
        SyntaxError, SyntaxOptions, parse_source_with_options, set_test_clock,
        set_test_progress_hook,
    };
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::{Duration, Instant};

    let progress_count = std::sync::Arc::new(AtomicUsize::new(0));
    let progress_count_clone = progress_count.clone();

    let start = Instant::now();
    let deadline = start + Duration::from_secs(10);
    set_test_clock(Some(start));

    let source = "fn compute() { let mut x = 1; x += 2; }\n".repeat(80);
    assert!(source.len() >= 2000);

    set_test_progress_hook(Some(Box::new(move |_| {
        progress_count_clone.fetch_add(1, Ordering::SeqCst);
        set_test_clock(Some(start + Duration::from_secs(20)));
    })));

    let options = SyntaxOptions {
        cancelled: None,
        deadline: Some(deadline),
        max_source_bytes: source.len() * 2,
    };

    let result = parse_source_with_options(Path::new("test.rs"), &source, &options);
    set_test_progress_hook(None);
    set_test_clock(None);

    assert!(progress_count.load(Ordering::SeqCst) > 0);
    assert!(matches!(result, Err(SyntaxError::DeadlineExceeded)));
}

#[test]
fn syntax_cancellation_stops_header_probes() {
    use crate::language_spec::source_detection::{
        header_probe_parse_count, reset_header_probe_parse_count,
        set_test_header_between_probes_hook, set_test_header_scoring_hook,
    };
    use crate::syntax::{
        SyntaxError, SyntaxOptions, parse_source_with_options, set_test_progress_hook,
    };
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

    let cancelled = Arc::new(AtomicBool::new(false));
    let cancelled_clone = cancelled.clone();
    let source = "int add(int a, int b) { return a + b; }\n".repeat(80);

    reset_header_probe_parse_count();
    set_test_progress_hook(Some(Box::new(move |_| {
        cancelled_clone.store(true, Ordering::Release);
    })));

    let options = SyntaxOptions {
        cancelled: Some(cancelled.as_ref()),
        deadline: None,
        max_source_bytes: source.len() * 2,
    };

    let result = parse_source_with_options(Path::new("test.h"), &source, &options);
    set_test_progress_hook(None);

    assert!(matches!(result, Err(SyntaxError::Cancelled)));
    assert_eq!(header_probe_parse_count(), 1);

    reset_header_probe_parse_count();
    let cancelled_between = Arc::new(AtomicBool::new(false));
    let cancelled_between_clone = cancelled_between.clone();
    set_test_header_between_probes_hook(Some(Box::new(move || {
        cancelled_between_clone.store(true, Ordering::Release);
    })));

    let options_between = SyntaxOptions {
        cancelled: Some(cancelled_between.as_ref()),
        deadline: None,
        max_source_bytes: 1024,
    };

    let result_between =
        parse_source_with_options(Path::new("test.h"), "int simple(void);\n", &options_between);
    set_test_header_between_probes_hook(None);

    assert!(matches!(result_between, Err(SyntaxError::Cancelled)));
    assert_eq!(header_probe_parse_count(), 1);

    reset_header_probe_parse_count();
    let cancelled_scoring = Arc::new(AtomicBool::new(false));
    let cancelled_scoring_clone = cancelled_scoring.clone();
    set_test_header_scoring_hook(Some(Box::new(move || {
        cancelled_scoring_clone.store(true, Ordering::Release);
    })));

    let options_scoring = SyntaxOptions {
        cancelled: Some(cancelled_scoring.as_ref()),
        deadline: None,
        max_source_bytes: 1024,
    };

    let result_scoring =
        parse_source_with_options(Path::new("test.h"), "int simple(void);\n", &options_scoring);
    set_test_header_scoring_hook(None);

    assert!(matches!(result_scoring, Err(SyntaxError::Cancelled)));
    assert_eq!(header_probe_parse_count(), 2);
}

#[test]
fn syntax_cancellation_stops_header_whitespace_scan() {
    use crate::syntax::{SyntaxError, SyntaxOptions, parse_source_with_options};
    use std::sync::atomic::AtomicBool;

    let cancelled = AtomicBool::new(true);
    let whitespace_source = " ".repeat(2048);
    let options = SyntaxOptions {
        cancelled: Some(&cancelled),
        deadline: None,
        max_source_bytes: 4096,
    };

    let result = parse_source_with_options(Path::new("test.h"), &whitespace_source, &options);
    assert!(matches!(result, Err(SyntaxError::Cancelled)));
}

#[test]
fn syntax_cancellation_stops_diagnostic_walk() {
    use crate::syntax::{
        SyntaxError, SyntaxOptions, parse_source_with_options, set_test_diagnostic_node_hook,
    };
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

    let cancelled = Arc::new(AtomicBool::new(false));
    let cancelled_clone = cancelled.clone();
    let visits = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let visits_clone = visits.clone();

    set_test_diagnostic_node_hook(Some(Box::new(move |_| {
        let count = visits_clone.fetch_add(1, Ordering::SeqCst);
        if count >= 2 {
            cancelled_clone.store(true, Ordering::Release);
        }
    })));

    let source = "fn broken( {\n let y = ; }\n";
    let options = SyntaxOptions {
        cancelled: Some(cancelled.as_ref()),
        deadline: None,
        max_source_bytes: 1024,
    };

    let result = parse_source_with_options(Path::new("broken.rs"), source, &options);
    set_test_diagnostic_node_hook(None);

    assert!(visits.load(Ordering::SeqCst) >= 2);
    assert!(matches!(result, Err(SyntaxError::Cancelled)));
}

#[test]
fn syntax_deadline_after_parse_rejects_result() {
    use crate::syntax::{
        SyntaxError, SyntaxOptions, parse_source_with_options, set_test_clock,
        set_test_diagnostic_node_hook,
    };
    use std::time::{Duration, Instant};

    let start = Instant::now();
    let deadline = start + Duration::from_secs(10);
    set_test_clock(Some(start));

    set_test_diagnostic_node_hook(Some(Box::new(move |_| {
        set_test_clock(Some(start + Duration::from_secs(20)));
    })));

    let source = "fn broken( {\n";
    let options = SyntaxOptions {
        cancelled: None,
        deadline: Some(deadline),
        max_source_bytes: 1024,
    };

    let result = parse_source_with_options(Path::new("broken.rs"), source, &options);
    set_test_diagnostic_node_hook(None);
    set_test_clock(None);

    assert!(matches!(result, Err(SyntaxError::DeadlineExceeded)));
}

#[test]
fn syntax_cancellation_precedence_over_deadline_in_parser_progress() {
    use crate::syntax::{
        SyntaxError, SyntaxOptions, parse_source_with_options, set_test_clock,
        set_test_progress_hook,
    };
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::time::{Duration, Instant};

    let cancelled = Arc::new(AtomicBool::new(false));
    let cancelled_clone = cancelled.clone();
    let progress_count = Arc::new(AtomicUsize::new(0));
    let progress_count_clone = progress_count.clone();

    let start = Instant::now();
    let deadline = start + Duration::from_secs(10);
    set_test_clock(Some(start));

    let source = "fn compute() { let mut x = 1; x += 2; }\n".repeat(80);
    assert!(source.len() >= 2000);

    set_test_progress_hook(Some(Box::new(move |_| {
        progress_count_clone.fetch_add(1, Ordering::SeqCst);
        cancelled_clone.store(true, Ordering::Release);
        set_test_clock(Some(start + Duration::from_secs(20)));
    })));

    let options = SyntaxOptions {
        cancelled: Some(cancelled.as_ref()),
        deadline: Some(deadline),
        max_source_bytes: source.len() * 2,
    };

    let result = parse_source_with_options(Path::new("test.rs"), &source, &options);
    set_test_progress_hook(None);
    set_test_clock(None);

    assert!(progress_count.load(Ordering::SeqCst) > 0);
    assert!(matches!(result, Err(SyntaxError::Cancelled)));
}
