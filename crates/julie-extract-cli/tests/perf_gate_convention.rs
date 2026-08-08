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

/// Same convention for the heavy contract suites: they spawn the real CLI
/// against large generated fixtures, so they must stay behind
/// `test-heavy-contracts` and only run from the xtask contract tier.
#[test]
fn heavy_contract_gates_are_feature_gated_out_of_default_suite() {
    let crate_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));

    let manifest = read(&crate_root.join("Cargo.toml"));
    assert!(
        manifest.contains("test-heavy-contracts = []"),
        "Cargo feature `test-heavy-contracts` must exist so the heavy contract suites are \
         selectable by name"
    );

    for harness in ["deep_recursion_contract.rs", "reference_site_identity.rs"] {
        let source = read(&crate_root.join("tests").join(harness));
        assert!(
            source.contains("#![cfg(feature = \"test-heavy-contracts\")]"),
            "tests/{harness} must start with `#![cfg(feature = \"test-heavy-contracts\")]` so \
             the heavy contract suite never leaks into the default suite"
        );
    }
}

#[test]
fn store_process_contracts_and_hooks_are_feature_gated_out_of_default_suite() {
    let crate_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let manifest = read(&crate_root.join("Cargo.toml"));
    assert!(
        manifest.contains("test-store-contract = [\"julie-extract-artifact/test-store-crash\"]")
    );
    for harness in ["store_equivalence.rs", "store_mixed_version.rs"] {
        let source = read(&crate_root.join("tests").join(harness));
        assert!(
            source.starts_with("#![cfg(feature = \"test-store-contract\")]"),
            "tests/{harness} must stay outside the default process tier"
        );
    }
    let executor = read(&crate_root.join("src/store/executor.rs"));
    assert!(executor.contains("#[cfg(feature = \"test-store-contract\")]\nfn wait_for_test_hook("));
    assert!(!executor.contains("#[cfg(debug_assertions)]\nfn wait_for_test_hook("));
}

fn read(path: &PathBuf) -> String {
    fs::read_to_string(path)
        .unwrap_or_else(|err| panic!("failed to read {}: {}", path.display(), err))
        .replace("\r\n", "\n")
}
