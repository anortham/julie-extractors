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

/// Gating the perf harness is not enough on its own: a wall-clock budget added
/// to an ungated file is the same leak wearing a different name. `elapsed <
/// Duration` in the default suite passes on a fast laptop and fails on a shared
/// runner, which is how `child_row_batch_*` blocked a release while the code was
/// fine. Timing belongs in `writer_perf.rs`; the default suite asserts structure.
///
/// The guard names the patterns it forbids, so it must exempt its own source.
#[test]
fn default_suite_tests_assert_no_wall_clock_budget() {
    let tests_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests");
    let guard_file = PathBuf::from(file!())
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .expect("guard file has a name");
    let mut offenders = Vec::new();

    for entry in fs::read_dir(&tests_dir).expect("tests directory is readable") {
        let path = entry.expect("readable dir entry").path();
        if path.extension().is_none_or(|extension| extension != "rs") {
            continue;
        }
        if path
            .file_name()
            .is_some_and(|name| name == guard_file.as_str())
        {
            continue;
        }
        let source = read(&path);
        if source.contains("#![cfg(feature = \"test-perf\")]") {
            continue;
        }
        if source.contains("Instant::now()") || source.contains("std::time::Instant") {
            offenders.push(path.file_name().unwrap().to_string_lossy().into_owned());
        }
    }

    assert!(
        offenders.is_empty(),
        "these ungated default-suite tests time themselves; move the budget into \
         tests/writer_perf.rs behind `test-perf`: {offenders:?}"
    );
}

#[test]
fn store_schema_contract_is_part_of_the_default_suite() {
    let crate_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let source = read(&crate_root.join("tests/store_schema_contract.rs"));

    assert!(!source.contains("#![cfg("));
    assert!(source.contains("store_and_coordinator_catalogs_match_the_checked_in_authority"));
}

fn read(path: &PathBuf) -> String {
    fs::read_to_string(path)
        .unwrap_or_else(|err| panic!("failed to read {}: {}", path.display(), err))
        .replace("\r\n", "\n")
}
