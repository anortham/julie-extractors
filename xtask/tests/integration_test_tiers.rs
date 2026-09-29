//! Convention test: every test target must be classified into a tier.
//!
//! `cargo test -p <crate>` auto-discovers every `tests/*.rs` target, so a new
//! integration test silently joins the default suite unless it carries a
//! file-level `#![cfg(feature = ...)]` gate. This test fails when a test target
//! is neither feature-gated nor named in the explicit allowlists below, which
//! makes "route slow tests out of the default suite from the start" a reviewed
//! decision instead of an accident.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

/// Auto-discovered `tests/*.rs` targets that intentionally run in the default
/// tier. Adding a path here is the review claim that the target is fast,
/// deterministic, and free of real-world corpora, parser certification, and
/// wall-clock floors.
const FAST_INTEGRATION_TESTS: &[&str] = &[
    // julie-extract-artifact
    "crates/julie-extract-artifact/tests/report_contract.rs",
    "crates/julie-extract-artifact/tests/schema_contract.rs",
    "crates/julie-extract-artifact/tests/spool_contract.rs",
    "crates/julie-extract-artifact/tests/test_tiers.rs",
    "crates/julie-extract-artifact/tests/writer_batching_contract.rs",
    "crates/julie-extract-artifact/tests/writer_contract.rs",
    // julie-extract-cli
    "crates/julie-extract-cli/tests/cli_contract.rs",
    "crates/julie-extract-cli/tests/csharp_interpolated_verbatim.rs",
    "crates/julie-extract-cli/tests/determinism_contract.rs",
    "crates/julie-extract-cli/tests/flask_route_contract.rs",
    "crates/julie-extract-cli/tests/human_report_contract.rs",
    "crates/julie-extract-cli/tests/operations_contract.rs",
    "crates/julie-extract-cli/tests/path_policy.rs",
    "crates/julie-extract-cli/tests/perf_gate_convention.rs",
    "crates/julie-extract-cli/tests/producer_freshness.rs",
    "crates/julie-extract-cli/tests/producer_freshness_contract.rs",
    "crates/julie-extract-cli/tests/python_inferred_return_type.rs",
    "crates/julie-extract-cli/tests/qml_javascript_directives.rs",
    "crates/julie-extract-cli/tests/rebind_contract.rs",
    "crates/julie-extract-cli/tests/rebind_equivalence.rs",
    "crates/julie-extract-cli/tests/receiver_contract.rs",
    "crates/julie-extract-cli/tests/receiver_type_contract.rs",
    "crates/julie-extract-cli/tests/reference_occurrences.rs",
    "crates/julie-extract-cli/tests/sqlite_engine_contract.rs",
    "crates/julie-extract-cli/tests/test_tiers.rs",
    // julie-extractors
    "crates/julie-extractors/tests/call_site_containers.rs",
    "crates/julie-extractors/tests/ecmascript_receiver_scope.rs",
    "crates/julie-extractors/tests/elixir_call_arity.rs",
    "crates/julie-extractors/tests/elixir_framework_gaps.rs",
    "crates/julie-extractors/tests/framework_context_boundaries.rs",
    "crates/julie-extractors/tests/kotlin_relationship_targets.rs",
    "crates/julie-extractors/tests/nonshared_scope_targets.rs",
    "crates/julie-extractors/tests/receiver_metadata.rs",
    "crates/julie-extractors/tests/receiver_rust_macros.rs",
    "crates/julie-extractors/tests/receiver_spans.rs",
    "crates/julie-extractors/tests/receiver_sql.rs",
    "crates/julie-extractors/tests/reference_targets.rs",
    "crates/julie-extractors/tests/regex_branch_reset.rs",
    "crates/julie-extractors/tests/regex_conditionals.rs",
    "crates/julie-extractors/tests/rust_framework_gaps.rs",
    "crates/julie-extractors/tests/testing_trait_benchmark_gaps.rs",
];

/// Explicit `[[test]]` targets with a `path` outside `tests/` that
/// intentionally run in the default tier. A declared target without
/// `required-features` joins the default tier just like an auto-discovered one.
const FAST_DECLARED_TESTS: &[&str] =
    &["crates/julie-extractors/src/tests/public_relationship_contract.rs"];

#[test]
fn every_integration_test_file_is_classified() {
    let root = repo_root();
    let allowed = FAST_INTEGRATION_TESTS
        .iter()
        .copied()
        .collect::<BTreeSet<_>>();
    let mut seen = BTreeSet::new();
    let mut unclassified = Vec::new();

    for crate_dir in crate_dirs(&root) {
        let tests_dir = crate_dir.join("tests");
        if !tests_dir.is_dir() {
            continue;
        }
        for entry in fs::read_dir(&tests_dir).expect("read crate tests directory") {
            let path = entry.expect("read tests dir entry").path();
            if path.extension().and_then(|extension| extension.to_str()) != Some("rs") {
                continue;
            }
            let relative = relative_path(&root, &path);
            seen.insert(relative.clone());
            if is_file_feature_gated(&path) || allowed.contains(relative.as_str()) {
                continue;
            }
            unclassified.push(relative);
        }
    }

    assert!(
        unclassified.is_empty(),
        "these integration tests are neither feature-gated nor listed in \
         FAST_INTEGRATION_TESTS, so they join the default tier by accident:\n{}\n\
         Add a file-level `#![cfg(feature = \"...\")]` gate for a slow gate, or \
         add the path to FAST_INTEGRATION_TESTS to claim it is default-tier fast.",
        unclassified.join("\n")
    );

    let stale = allowed
        .iter()
        .filter(|path| !seen.contains(**path))
        .collect::<Vec<_>>();
    assert!(
        stale.is_empty(),
        "FAST_INTEGRATION_TESTS names files that no longer exist; remove them:\n{}",
        stale
            .iter()
            .map(|path| path.to_string())
            .collect::<Vec<_>>()
            .join("\n")
    );
}

#[test]
fn every_declared_test_target_is_classified() {
    let root = repo_root();
    let allowed = FAST_DECLARED_TESTS.iter().copied().collect::<BTreeSet<_>>();
    let mut seen = BTreeSet::new();
    let mut unclassified = Vec::new();

    for crate_dir in crate_dirs(&root) {
        let manifest = fs::read_to_string(crate_dir.join("Cargo.toml")).expect("read Cargo.toml");
        for target in declared_test_targets(&manifest) {
            let path = crate_dir.join(&target.path);
            let relative = relative_path(&root, &path);
            seen.insert(relative.clone());
            if target.has_required_features || allowed.contains(relative.as_str()) {
                continue;
            }
            unclassified.push(relative);
        }
    }

    assert!(
        unclassified.is_empty(),
        "these declared `[[test]]` targets have no `required-features` and are \
         not listed in FAST_DECLARED_TESTS, so they join the default tier \
         silently:\n{}",
        unclassified.join("\n")
    );

    let stale = allowed
        .iter()
        .filter(|path| !seen.contains(**path))
        .collect::<Vec<_>>();
    assert!(
        stale.is_empty(),
        "FAST_DECLARED_TESTS names targets that no longer exist; remove them:\n{}",
        stale
            .iter()
            .map(|path| path.to_string())
            .collect::<Vec<_>>()
            .join("\n")
    );
}

struct DeclaredTestTarget {
    path: String,
    has_required_features: bool,
}

/// Minimal `[[test]]` block reader: `path` and whether the block declares
/// `required-features`. Avoids a TOML dependency for a two-field check.
fn declared_test_targets(manifest: &str) -> Vec<DeclaredTestTarget> {
    let mut targets = Vec::new();
    let mut in_test_block = false;
    let mut path: Option<String> = None;
    let mut has_required_features = false;

    let mut flush = |path: &mut Option<String>, has_required_features: &mut bool| {
        if let Some(path) = path.take() {
            targets.push(DeclaredTestTarget {
                path,
                has_required_features: *has_required_features,
            });
        }
        *has_required_features = false;
    };

    for line in manifest.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            if in_test_block {
                flush(&mut path, &mut has_required_features);
            }
            in_test_block = trimmed == "[[test]]";
            continue;
        }
        if !in_test_block {
            continue;
        }
        if let Some(value) = trimmed.strip_prefix("path") {
            if let Some(value) = toml_string(value) {
                path = Some(value);
            }
        } else if trimmed.starts_with("required-features") {
            has_required_features = true;
        }
    }
    if in_test_block {
        flush(&mut path, &mut has_required_features);
    }

    targets
}

fn toml_string(value: &str) -> Option<String> {
    let value = value.trim_start().strip_prefix('=')?.trim();
    let value = value.strip_prefix('"')?.split('"').next()?;
    Some(value.to_string())
}

fn is_file_feature_gated(path: &Path) -> bool {
    let Ok(contents) = fs::read_to_string(path) else {
        return false;
    };
    contents
        .lines()
        .take(40)
        .any(|line| line.trim_start().starts_with("#![cfg(feature = \"test-"))
}

fn crate_dirs(root: &Path) -> Vec<PathBuf> {
    let mut dirs = fs::read_dir(root.join("crates"))
        .expect("read crates directory")
        .map(|entry| entry.expect("read crate entry").path())
        .filter(|path| path.join("Cargo.toml").is_file())
        .collect::<Vec<_>>();
    dirs.sort();
    dirs
}

fn relative_path(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .expect("path should live under the repository root")
        .to_string_lossy()
        .replace('\\', "/")
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask crate should live under the repository root")
        .to_path_buf()
}
