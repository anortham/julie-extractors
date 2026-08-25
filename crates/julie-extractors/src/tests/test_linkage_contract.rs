//! Source-scan guard for the `test_linkage` / `test_coverage` metadata keys.
//!
//! Miller reads both keys from `symbols.metadata_json` and turns them into
//! graph edges, but only after a `LIMIT 1` probe proves at least one test
//! symbol carries one. Writing either key on any symbol flips that probe true
//! and restores a whole-index metadata scan that Miller measured at 2,978 ms
//! per graph load against 206 ms for the probe.
//!
//! `docs/decisions/2026-08-25-test-linkage-metadata-contract.md` records why
//! this repo does not write them yet, and the exact shape to write when it
//! does.

use std::fs;
use std::path::{Path, PathBuf};

const LINKAGE_KEYS: [&str; 2] = ["test_linkage", "test_coverage"];

#[test]
fn no_production_source_writes_a_miller_linkage_metadata_key() {
    let sources = production_sources(&PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src"));
    assert!(
        sources.len() > 100,
        "source scan should see the whole production crate, found only {} files",
        sources.len()
    );

    let mut violations = Vec::new();
    for path in &sources {
        let source = fs::read_to_string(path)
            .unwrap_or_else(|err| panic!("failed to read {}: {err}", path.display()));
        for (index, line) in source.lines().enumerate() {
            for key in LINKAGE_KEYS {
                if line.contains(key) {
                    violations.push(format!("{}:{} {}", path.display(), index + 1, line.trim()));
                }
            }
        }
    }

    assert!(
        violations.is_empty(),
        "production source mentions a Miller test-linkage metadata key. Writing one \
         costs every Miller graph load a whole-index metadata scan. Read \
         docs/decisions/2026-08-25-test-linkage-metadata-contract.md before \
         opening this:\n{}",
        violations.join("\n")
    );
}

fn production_sources(root: &Path) -> Vec<PathBuf> {
    let mut sources = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(directory) = stack.pop() {
        let entries = fs::read_dir(&directory)
            .unwrap_or_else(|err| panic!("failed to read {}: {err}", directory.display()));
        for entry in entries {
            let path = entry
                .expect("source directory entry should be readable")
                .path();
            if path.is_dir() {
                if path.file_name().and_then(|name| name.to_str()) != Some("tests") {
                    stack.push(path);
                }
            } else if path.extension().and_then(|extension| extension.to_str()) == Some("rs") {
                sources.push(path);
            }
        }
    }
    sources.sort();
    sources
}
