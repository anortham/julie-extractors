use std::fs;
use std::path::PathBuf;

/// Mirrors the `julie-extractors` test-tier convention for the
/// `julie-extract-artifact` perf gate. The perf harness is intentionally slow
/// and informational, so it must stay behind the `test-perf` feature and never
/// leak into the default suite. This fails loudly if the gate is removed.
#[test]
fn perf_gate_is_feature_gated_out_of_default_suite() {
    let crate_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));

    let manifest = read(&crate_root.join("Cargo.toml"));
    assert!(
        manifest.contains("test-perf = []"),
        "Cargo feature `test-perf` must exist so the perf gate is selectable by name"
    );

    let perf_harness = read(&crate_root.join("tests/writer_perf.rs"));
    assert!(
        perf_harness.contains("#![cfg(feature = \"test-perf\")]"),
        "tests/writer_perf.rs must start with `#![cfg(feature = \"test-perf\")]` so the \
         slow perf gate never leaks into the default suite"
    );
}

fn read(path: &PathBuf) -> String {
    fs::read_to_string(path)
        .unwrap_or_else(|err| panic!("failed to read {}: {}", path.display(), err))
        .replace("\r\n", "\n")
}
