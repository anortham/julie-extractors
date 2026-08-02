//! Regression gate for the generated-source stack overflow.
//!
//! dotnet/runtime ships JIT regression tests whose single statement chains tens
//! of thousands of `+` operators (`src/tests/JIT/Regression/JitBlue/GitHub_10215.cs`
//! has 17,602). tree-sitter parses the resulting left-leaning spine iteratively,
//! but a tree walker that recurses one Rust frame per CST node does not survive
//! it on a default worker stack — and a stack overflow aborts the process rather
//! than failing the file, so no per-file recovery path can catch it.
//!
//! This test runs the real binary at default stacks (no `RUST_MIN_STACK`) and
//! requires a green scan with facts for the file.

use std::path::Path;
use std::process::{Command, Output};

use rusqlite::Connection;
use serde_json::Value;
use tempfile::TempDir;

/// Trimmed from the real GitHub_10215.cs statement's 17,602 operators: past the
/// 1,024-node traversal budget, which is what the guards must survive, without
/// paying for the C# extractor's superlinear cost on long operator spines (a
/// 2,048-term chain costs ~37 s in a debug build).
const CHAIN_OPERATORS: usize = 1_200;

fn julie_extract(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_julie-extract"))
        .args(args)
        .env_remove("RUST_MIN_STACK")
        .output()
        .unwrap_or_else(|err| panic!("failed to run julie-extract {args:?}: {err}"))
}

fn deep_expression_chain_source() -> String {
    let mut source = String::from(
        "// Trimmed shape of dotnet/runtime GitHub_10215.cs: one statement, thousands of operators.\npublic class GitHubDeep\n{\n    public static int Sum(int b)\n    {\n        return b",
    );
    for _ in 0..CHAIN_OPERATORS {
        source.push_str(" + b");
    }
    source.push_str(";\n    }\n\n    public static int Main() => Sum(1);\n}\n");
    source
}

fn table_count(db: &Path, table: &str) -> i64 {
    let connection = Connection::open(db).expect("artifact should open");
    connection
        .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
            row.get(0)
        })
        .unwrap_or_else(|err| panic!("failed to count {table}: {err}"))
}

#[test]
fn scan_survives_a_generated_deep_expression_chain_at_default_stacks() {
    let fixture = TempDir::new().expect("temp fixture root should be created");
    let root = fixture.path();
    std::fs::create_dir_all(root.join("src")).expect("fixture src should be created");
    std::fs::write(
        root.join("src/GitHubDeep.cs"),
        deep_expression_chain_source(),
    )
    .expect("deep fixture should be written");
    let db = root.join("artifact.sqlite");

    let output = julie_extract(&[
        "scan",
        "--root",
        root.to_str().expect("fixture root should be utf-8"),
        "--db",
        db.to_str().expect("db path should be utf-8"),
        "--json",
    ]);

    assert_eq!(
        output.status.code(),
        Some(0),
        "deep generated source must not abort the scan\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Value = serde_json::from_slice(&output.stdout).unwrap_or_else(|err| {
        panic!(
            "stdout was not a JSON report: {err}\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    });
    assert_eq!(report["status"], "ok");
    assert_eq!(report["counts"]["files_failed"], 0);
    assert_eq!(report["counts"]["files_changed"], 1);

    assert!(
        table_count(&db, "complexity_metrics") > 0,
        "the deep file must still produce complexity metrics"
    );

    let connection = Connection::open(&db).expect("artifact should open");
    let truncation_diagnostics: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM parse_diagnostics WHERE kind = 'depth_truncated'",
            [],
            |row| row.get(0),
        )
        .expect("parse_diagnostics should be queryable");
    assert_eq!(
        truncation_diagnostics, 1,
        "depth-capped extraction must be visible in the artifact, not silent"
    );
}
