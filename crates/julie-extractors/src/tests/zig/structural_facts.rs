use std::collections::BTreeSet;
use std::path::Path;

use crate::tests::helpers::metadata_str;

const FIXTURE_SOURCE: &str =
    include_str!("../../../../../fixtures/extraction/zig/basic/source.zig");

fn extract(source: &str) -> crate::ExtractionResults {
    crate::pipeline::extract_canonical(
        "fixtures/extraction/zig/basic/source.zig",
        source,
        Path::new("/repo"),
    )
    .expect("canonical Zig extraction should succeed")
}


#[test]
fn zig_emits_expected_structural_fact_patterns() {
    let results = extract(FIXTURE_SOURCE);
    let pattern_ids = results
        .structural_facts
        .iter()
        .map(|fact| fact.pattern_id.as_str())
        .collect::<BTreeSet<_>>();

    for pattern_id in [
        "zig.builtin_call.v1",
        "zig.threadlocal_variable.v1",
        "zig.inline_function.v1",
        "zig.exported_function.v1",
        "zig.comptime_parameter.v1",
    ] {
        assert!(
            pattern_ids.contains(pattern_id),
            "missing structural fact pattern `{pattern_id}`"
        );
    }

    for name in ["import", "This", "sqrt"] {
        let count = results
            .structural_facts
            .iter()
            .filter(|fact| {
                fact.pattern_id == "zig.builtin_call.v1"
                    && metadata_str(fact, "builtin_name") == Some(name)
            })
            .count();
        assert_eq!(
            count, 1,
            "expected exactly one zig.builtin_call.v1 fact for `{name}`"
        );
    }

    let import_call = results
        .structural_facts
        .iter()
        .find(|fact| {
            fact.pattern_id == "zig.builtin_call.v1"
                && metadata_str(fact, "builtin_name") == Some("import")
        })
        .expect("expected @import builtin call fact");
    // The top-level `const std = @import("std");` has no enclosing scope-bearing
    // symbol: its only byte/line container is the `std` constant, which the
    // shared binder now excludes as a value-holder. Structural facts bind to
    // scopes, not value declarations, so a module-scope import correctly leaves
    // containing_symbol_id = None (the in-struct `@This()` fact still binds to
    // the `Worker` struct).
    assert!(
        import_call.containing_symbol_id.is_none(),
        "a module-scope @import must not bind to the excluded `std` constant"
    );

    let threadlocal = results
        .structural_facts
        .iter()
        .find(|fact| fact.pattern_id == "zig.threadlocal_variable.v1")
        .expect("expected threadlocal variable fact");
    assert_eq!(
        metadata_str(threadlocal, "variable_name"),
        Some("worker_tls")
    );

    let inline_fn = results
        .structural_facts
        .iter()
        .find(|fact| fact.pattern_id == "zig.inline_function.v1")
        .expect("expected inline function fact");
    assert_eq!(metadata_str(inline_fn, "function_name"), Some("fast_path"));

    let export_fn = results
        .structural_facts
        .iter()
        .find(|fact| fact.pattern_id == "zig.exported_function.v1")
        .expect("expected exported function fact");
    assert_eq!(metadata_str(export_fn, "function_name"), Some("ffi_entry"));
    assert_ne!(
        metadata_str(inline_fn, "function_name"),
        metadata_str(export_fn, "function_name"),
        "inline and export functions must remain distinct"
    );

    let comptime_param = results
        .structural_facts
        .iter()
        .find(|fact| fact.pattern_id == "zig.comptime_parameter.v1")
        .expect("expected comptime parameter fact");
    assert_eq!(metadata_str(comptime_param, "parameter_name"), Some("T"));
}

#[test]
fn zig_builtin_call_does_not_match_ordinary_function_calls() {
    let source = r#"
fn helper() void {}

pub fn run() void {
    helper();
}
"#;
    let results = extract(source);
    assert!(
        results
            .structural_facts
            .iter()
            .all(|fact| fact.pattern_id != "zig.builtin_call.v1"),
        "ordinary function calls must not emit zig.builtin_call.v1"
    );
}
