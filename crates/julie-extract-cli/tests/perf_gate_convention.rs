use std::fs;
use std::path::PathBuf;

/// Mirrors the `julie-extract-artifact` test-tier convention guard
/// (`tests/test_tiers.rs`) for the CLI's resolution performance gate. The perf
/// harness is intentionally slow and synthetic-scale, so it must stay behind the
/// `test-perf` feature and never leak into the default suite. This test runs in
/// the DEFAULT build (no `test-perf`) and fails loudly if the gate is removed or
/// un-gated.
#[test]
fn resolution_perf_gate_is_feature_gated_out_of_default_suite() {
    let crate_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));

    let manifest = read(&crate_root.join("Cargo.toml"));
    assert!(
        manifest.contains("test-perf = []"),
        "Cargo feature `test-perf` must exist so the resolution perf gate is selectable by name"
    );

    let perf_harness = read(&crate_root.join("tests/resolution_perf.rs"));
    assert!(
        perf_harness.contains("#![cfg(feature = \"test-perf\")]"),
        "tests/resolution_perf.rs must start with `#![cfg(feature = \"test-perf\")]` so the \
         slow perf gate never leaks into the default suite"
    );
}

/// Same convention for the real-world Erlang corpus gate: it scans the vendored
/// hex.pm packages with the real CLI, so it must stay behind `test-real-world`
/// and only run from the xtask real-world tier.
#[test]
fn erlang_corpus_gate_is_feature_gated_out_of_default_suite() {
    let crate_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));

    let manifest = read(&crate_root.join("Cargo.toml"));
    assert!(
        manifest.contains("test-real-world = []"),
        "Cargo feature `test-real-world` must exist so the corpus gate is selectable by name"
    );

    let corpus_harness = read(&crate_root.join("tests/erlang_corpus.rs"));
    assert!(
        corpus_harness.contains("#![cfg(feature = \"test-real-world\")]"),
        "tests/erlang_corpus.rs must start with `#![cfg(feature = \"test-real-world\")]` so the \
         slow corpus gate never leaks into the default suite"
    );
}

fn read(path: &PathBuf) -> String {
    fs::read_to_string(path)
        .unwrap_or_else(|err| panic!("failed to read {}: {}", path.display(), err))
        .replace("\r\n", "\n")
}
