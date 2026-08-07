//! Shadow mode's own gate: `JULIE_RESOLUTION_SHADOW=1` must prove the row-scoped
//! delta writes the overlay a legacy file-scoped pass would have written, without
//! changing a single durable row and without silently passing when the two
//! disagree.
//!
//! Both properties are load-bearing for the release gate. A shadow that perturbs
//! the artifact makes the dogfood evidence worthless; a shadow that cannot fail
//! makes "zero mismatches over 40 saves" a tautology. So one case pins the
//! side-effect-free half against a shadow-off control run, and the other injects a
//! divergence and demands the structured report plus the non-zero exit.
//!
//! Every environment variable is set on the SPAWNED process, never through
//! `std::env::set_var`: these tests run in parallel with the rest of the suite in
//! one process, and an in-process write would leak into unrelated passes.

use std::path::Path;
use std::process::{Command, Output};

use julie_extract_cli::resolution::SHADOW_MISMATCH_EXIT_CODE;
use rusqlite::Connection;
use serde_json::Value;
use tempfile::TempDir;

const SHADOW_ENV: &str = "JULIE_RESOLUTION_SHADOW";
const SHADOW_INJECT_ENV: &str = "JULIE_RESOLUTION_SHADOW_INJECT";
const INJECTED_IDENTIFIER_ID: &str = "shadow-injection-probe";

fn path_str(path: &Path) -> &str {
    path.to_str().expect("fixture paths are UTF-8")
}

fn julie_extract(args: &[&str], env: &[(&str, &str)]) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_julie-extract"));
    command.args(args);
    for (key, value) in env {
        command.env(key, value);
    }
    command
        .output()
        .unwrap_or_else(|err| panic!("failed to run julie-extract {args:?}: {err}"))
}

fn assert_success(output: &Output, what: &str) {
    assert_eq!(
        output.status.code(),
        Some(0),
        "{what}\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn scan(root: &Path, db: &Path) {
    let output = julie_extract(
        &[
            "scan",
            "--root",
            path_str(root),
            "--db",
            path_str(db),
            "--json",
        ],
        &[],
    );
    assert_success(&output, "the corpus scan must succeed");
}

fn update(root: &Path, db: &Path, file: &str, env: &[(&str, &str)]) -> Output {
    julie_extract(
        &[
            "update",
            "--root",
            path_str(root),
            "--db",
            path_str(db),
            "--file",
            file,
            "--json",
        ],
        env,
    )
}

/// The overlay as ordered comparable rows, in the equivalence oracle's
/// serialization (`tests/resolution_scope_equivalence.rs`). `resolved_at_revision`
/// is excluded for the same reason it is there: it records WHEN a row was written.
fn overlay(db: &Path) -> Vec<String> {
    let conn = Connection::open(db).expect("artifact opens");
    let mut rows = Vec::new();

    let mut pending = conn
        .prepare(
            "SELECT pending_relationship_id, target_symbol_id, tier, confidence, method \
             FROM pending_resolutions ORDER BY pending_relationship_id",
        )
        .unwrap();
    rows.extend(
        pending
            .query_map([], |row| {
                Ok(format!(
                    "pending|{}|{}|{}|{}|{}",
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, f64>(3)?,
                    row.get::<_, String>(4)?,
                ))
            })
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap(),
    );

    let mut identifiers = conn
        .prepare(
            "SELECT identifier_id, target_symbol_id, tier, confidence, method, outcome, candidates \
             FROM identifier_resolutions ORDER BY identifier_id",
        )
        .unwrap();
    rows.extend(
        identifiers
            .query_map([], |row| {
                Ok(format!(
                    "identifier|{}|{:?}|{:?}|{:?}|{:?}|{}|{:?}",
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, Option<i64>>(2)?,
                    row.get::<_, Option<f64>>(3)?,
                    row.get::<_, Option<String>>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, Option<i64>>(6)?,
                ))
            })
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap(),
    );

    let mut targets = conn
        .prepare(
            "SELECT identifier_id, target_symbol_id FROM identifier_resolutions \
             WHERE target_symbol_id IS NOT NULL ORDER BY identifier_id",
        )
        .unwrap();
    rows.extend(
        targets
            .query_map([], |row| {
                Ok(format!(
                    "resolved_target|{}|{}",
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                ))
            })
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap(),
    );

    rows
}

/// The overlay rows the pass reports having WRITTEN. The final artifact cannot
/// distinguish a rolled-back legacy leg from a leaked one when the two paths agree
/// — the rows are the same rows — but the counts can: a leaked leg has already
/// written them, so the real pass reports none.
fn resolution_counts(output: &Output) -> Value {
    let report: Value = serde_json::from_slice(&output.stdout).unwrap_or_else(|err| {
        panic!(
            "the update report must be JSON: {err}\nstdout:\n{}",
            String::from_utf8_lossy(&output.stdout)
        )
    });
    report["languages"]["reference_resolution"]["counts"].clone()
}

fn shadow_report(output: &Output) -> Option<Value> {
    let stderr = String::from_utf8_lossy(&output.stderr);
    stderr
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .find(|value| value.get("julie_resolution_shadow").is_some())
}

struct Fixture {
    _temp: TempDir,
    root: std::path::PathBuf,
}

impl Fixture {
    /// A corpus carrying both keying relations the row-scoped path replaced a file
    /// union with: an aliased TypeScript import (tier 2, keyed by `imported_name`)
    /// and a C# member reached through its receiver's resolved type (tier 3).
    fn new() -> Self {
        let temp = TempDir::new().unwrap();
        let root = temp.path().join("repo");
        std::fs::create_dir_all(root.join("src")).unwrap();
        let fixture = Self { _temp: temp, root };
        fixture.write("src/b.ts", "export function placeholder(): void {}\n");
        fixture.write(
            "src/a.ts",
            "import { realName as localName } from './b';\nexport function caller(): void { localName(); }\n",
        );
        fixture.write(
            "src/widget.cs",
            "namespace App { public class Widget { public int Render() { return 1; } } }\n",
        );
        fixture.write(
            "src/consumer.cs",
            "namespace App { public class Consumer { public int Run() { Widget w = new Widget(); return w.Render(); } } }\n",
        );
        fixture.write(
            "src/rival.cs",
            "namespace Other { public class Placeholder { } }\n",
        );
        fixture
    }

    fn write(&self, relative: &str, contents: &str) {
        let path = self.root.join(relative);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, contents).unwrap();
    }

    fn db(&self, name: &str) -> std::path::PathBuf {
        self.root.join(name)
    }

    /// Fill the aliased import's missing export — the tier-2 keyed relation.
    fn fill_aliased_export(&self) {
        self.write(
            "src/b.ts",
            "export function placeholder(): void {}\nexport function realName(): void {}\n",
        );
    }

    /// Make the receiver's type ambiguous from an unchanged-name file — the tier-3
    /// keyed relation.
    fn duplicate_receiver_type(&self) {
        self.write(
            "src/rival.cs",
            "namespace Other { public class Widget { } }\n",
        );
    }
}

#[test]
fn shadow_mode_agrees_with_the_row_scoped_path_and_writes_the_same_overlay() {
    let fixture = Fixture::new();
    let control = fixture.db("control.sqlite");
    scan(&fixture.root, &control);
    let shadowed = fixture.db("shadowed.sqlite");
    std::fs::copy(&control, &shadowed).expect("the scanned artifact copies");

    fixture.fill_aliased_export();
    let control_aliased = update(&fixture.root, &control, "src/b.ts", &[]);
    assert_success(&control_aliased, "the control update must succeed");
    let shadow_env = [(SHADOW_ENV, "1")];
    let aliased = update(&fixture.root, &shadowed, "src/b.ts", &shadow_env);
    assert_success(
        &aliased,
        "a shadowed delta whose two paths agree must exit zero",
    );
    assert!(
        shadow_report(&aliased).is_none(),
        "the aliased-import delta must produce no mismatch report; stderr:\n{}",
        String::from_utf8_lossy(&aliased.stderr)
    );
    assert_eq!(
        resolution_counts(&aliased),
        resolution_counts(&control_aliased),
        "the real pass must write every overlay row itself, so the legacy leg left \
         none behind for it to find already written"
    );

    fixture.duplicate_receiver_type();
    let control_receiver = update(&fixture.root, &control, "src/rival.cs", &[]);
    assert_success(&control_receiver, "the control update must succeed");
    let receiver = update(&fixture.root, &shadowed, "src/rival.cs", &shadow_env);
    assert_success(
        &receiver,
        "a shadowed delta whose two paths agree must exit zero",
    );
    assert!(
        shadow_report(&receiver).is_none(),
        "the receiver-type delta must produce no mismatch report; stderr:\n{}",
        String::from_utf8_lossy(&receiver.stderr)
    );
    assert_eq!(
        resolution_counts(&receiver),
        resolution_counts(&control_receiver),
        "the real pass must write every overlay row itself, so the legacy leg left \
         none behind for it to find already written"
    );

    assert_eq!(
        overlay(&shadowed),
        overlay(&control),
        "the legacy leg runs inside a rolled-back savepoint, so a shadowed artifact \
         must be byte-identical to the same deltas run with shadow mode off"
    );
}

#[test]
fn an_injected_divergence_reports_the_mismatch_and_fails_the_process() {
    let fixture = Fixture::new();
    let control = fixture.db("control.sqlite");
    scan(&fixture.root, &control);
    let injected = fixture.db("injected.sqlite");
    std::fs::copy(&control, &injected).expect("the scanned artifact copies");

    fixture.fill_aliased_export();
    assert_success(
        &update(&fixture.root, &control, "src/b.ts", &[(SHADOW_ENV, "1")]),
        "the un-injected control update must succeed",
    );
    let output = update(
        &fixture.root,
        &injected,
        "src/b.ts",
        &[
            (SHADOW_ENV, "1"),
            (SHADOW_INJECT_ENV, INJECTED_IDENTIFIER_ID),
        ],
    );

    assert_eq!(
        output.status.code(),
        Some(i32::from(SHADOW_MISMATCH_EXIT_CODE)),
        "an injected divergence must fail the process with the dedicated shadow code; \
         stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let report = shadow_report(&output).unwrap_or_else(|| {
        panic!(
            "an injected divergence must write a JSON mismatch report to stderr; stderr:\n{}",
            String::from_utf8_lossy(&output.stderr)
        )
    });
    assert_eq!(report["julie_resolution_shadow"], "mismatch");
    assert!(
        report["legacy_row_count"].as_u64().unwrap_or(0) > 0
            && report["scoped_row_count"].as_u64().unwrap_or(0) > 0,
        "both legs must have captured a real overlay, or the diff compares nothing; \
         report:\n{report}"
    );
    let differences = report["differences"]
        .as_array()
        .expect("the report carries a differences array");
    let injected_difference = differences
        .iter()
        .find(|difference| difference["key"] == INJECTED_IDENTIFIER_ID)
        .unwrap_or_else(|| panic!("the report must name the injected key; report:\n{report}"));
    assert_eq!(
        injected_difference["table"], "identifier_resolutions",
        "the report must name the table the divergent row belongs to"
    );
    assert!(
        !injected_difference["legacy"].is_null(),
        "the report must carry the legacy value; report:\n{report}"
    );

    assert_eq!(
        overlay(&injected),
        overlay(&control),
        "the mismatch must fail the process only AFTER the write completes, so the \
         artifact carries the same overlay an un-injected shadowed delta wrote"
    );
}
