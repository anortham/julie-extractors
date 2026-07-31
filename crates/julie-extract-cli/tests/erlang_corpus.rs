#![cfg(feature = "test-real-world")]

//! Real-world Erlang corpus gate.
//!
//! Scans the three hex.pm packages vendored under `fixtures/real-world/erlang/`
//! (`telemetry` 1.3.0, `certifi` 2.15.0, `unicode_util_compat` 0.7.1) with the real
//! `julie-extract` CLI and asserts an exact committed baseline — no thresholds.
//!
//! Intentionally slow (it parses ~700 KB of third-party Erlang), so it stays behind the
//! `test-real-world` feature and never enters the default suite. Run it with:
//!
//! ```text
//! RUSTUP_TOOLCHAIN=1.97.1 cargo test -p julie-extract-cli --features test-real-world --test erlang_corpus -- --nocapture
//! ```

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::Instant;

use rusqlite::Connection;
use serde_json::Value;
use sha2::{Digest, Sha256};
use tempfile::TempDir;

/// Every `.erl`/`.hrl` file the corpus contributes, with the exact symbol and
/// parse-diagnostic row counts the pinned `tree-sitter-erlang` extractor produces.
///
/// The two nonzero diagnostic counts are both the same construct: `?WITH_STACKTRACE(...)`,
/// a macro whose body is a partial `catch` clause head (`Class:Reason:Stacktrace ->`).
/// tree-sitter has no preprocessor, so the fragment cannot parse before macro expansion.
/// `telemetry.hrl` reports one diagnostic per `-define` body (lines 7 and 9); `telemetry.erl`
/// reports 45 across the two call sites (lines 169 and 344) and the statements they swallow,
/// which costs the exports declared after line 184 (`list_handlers/1`, `execute/2`, `span/3`,
/// `report_cb/1`). Every other corpus file parses clean.
const BASELINE: &[FileBaseline] = &[
    FileBaseline::new("certifi-2.15.0/src/certifi.erl", 3, 0),
    FileBaseline::new("certifi-2.15.0/src/certifi_pt.erl", 5, 0),
    FileBaseline::new("telemetry-1.3.0/src/telemetry.erl", 16, 45),
    FileBaseline::new("telemetry-1.3.0/src/telemetry.hrl", 13, 2),
    FileBaseline::new("telemetry-1.3.0/src/telemetry_app.erl", 3, 0),
    FileBaseline::new("telemetry-1.3.0/src/telemetry_handler_table.erl", 15, 0),
    FileBaseline::new("telemetry-1.3.0/src/telemetry_sup.erl", 4, 0),
    FileBaseline::new("telemetry-1.3.0/src/telemetry_test.erl", 3, 0),
    FileBaseline::new("unicode_util_compat-0.7.1/src/string_compat.erl", 152, 0),
    FileBaseline::new(
        "unicode_util_compat-0.7.1/src/unicode_util_compat.erl",
        58,
        0,
    ),
];

/// `-behaviour` declarations in the corpus and the pending `implements` edge each one owes.
/// `telemetry.erl` itself declares no behaviour.
const BEHAVIOUR_EDGES: &[(&str, &str)] = &[
    ("telemetry-1.3.0/src/telemetry_app.erl", "application"),
    (
        "telemetry-1.3.0/src/telemetry_handler_table.erl",
        "gen_server",
    ),
    ("telemetry-1.3.0/src/telemetry_sup.erl", "supervisor"),
];

struct FileBaseline {
    path: &'static str,
    symbols: i64,
    parse_diagnostics: i64,
}

impl FileBaseline {
    const fn new(path: &'static str, symbols: i64, parse_diagnostics: i64) -> Self {
        Self {
            path,
            symbols,
            parse_diagnostics,
        }
    }
}

#[test]
fn erlang_corpus_scans_every_file_against_the_committed_baseline() {
    let scan = ErlangCorpusScan::run();

    let counts = &scan.report["counts"];
    assert_eq!(counts["files_scanned"], BASELINE.len() as i64);
    assert_eq!(counts["files_changed"], BASELINE.len() as i64);
    assert_eq!(counts["files_unsupported"], 0);
    assert_eq!(counts["files_failed"], 0);
    assert_eq!(counts["file_rows_truncated"], Value::Bool(false));

    let languages = scan.report["profile"]["languages"]
        .as_object()
        .expect("profile.languages must be an object");
    assert_eq!(
        languages.keys().collect::<Vec<_>>(),
        vec!["erlang"],
        "the corpus must scan as Erlang only"
    );
    assert_eq!(languages["erlang"]["files"], BASELINE.len() as i64);
    assert_eq!(languages["erlang"]["failed_files"], 0);

    let file_rows = counts["file_rows"]
        .as_array()
        .expect("counts.file_rows must be an array");
    let scanned = file_rows
        .iter()
        .map(|row| row["path"].as_str().expect("file row path").to_string())
        .collect::<BTreeSet<_>>();
    assert_eq!(
        scanned,
        BASELINE
            .iter()
            .map(|entry| entry.path.to_string())
            .collect::<BTreeSet<_>>()
    );

    for entry in BASELINE {
        let row = file_rows
            .iter()
            .find(|row| row["path"] == entry.path)
            .unwrap_or_else(|| panic!("{} missing from counts.file_rows", entry.path));
        assert_eq!(row["language"], "erlang", "{}", entry.path);
        assert_eq!(row["status"], "indexed", "{}", entry.path);
        assert_eq!(row["rows"]["symbols"], entry.symbols, "{}", entry.path);
        assert_eq!(
            row["rows"]["parse_diagnostics"], entry.parse_diagnostics,
            "{}",
            entry.path
        );
    }
}

#[test]
fn telemetry_module_exposes_its_module_exports_and_behaviour_edges() {
    let scan = ErlangCorpusScan::run();
    let connection = Connection::open(&scan.db).expect("open artifact");

    let telemetry = symbols_in(&connection, "telemetry-1.3.0/src/telemetry.erl");
    assert!(
        telemetry.contains(&(
            "telemetry".to_string(),
            "module".to_string(),
            "public".to_string()
        )),
        "telemetry.erl must expose its module symbol, got {telemetry:?}"
    );
    for exported in ["execute", "attach", "detach"] {
        assert!(
            telemetry.contains(&(
                exported.to_string(),
                "function".to_string(),
                "public".to_string()
            )),
            "exported function {exported} must be a public function symbol, got {telemetry:?}"
        );
    }

    for (path, behaviour) in BEHAVIOUR_EDGES {
        let edges = connection
            .prepare(
                "SELECT count(*) FROM pending_relationships \
                 WHERE path = ?1 AND kind = 'implements' AND target_display_name = ?2",
            )
            .expect("prepare behaviour query")
            .query_one((path, behaviour), |row| row.get::<_, i64>(0))
            .expect("behaviour edge count");
        assert_eq!(edges, 1, "{path} must own one -behaviour({behaviour}) edge");
    }
}

#[test]
fn vendored_corpus_matches_its_committed_checksums() {
    let corpus = corpus_root();
    let manifest = fs::read_to_string(corpus.join("CHECKSUMS.sha256"))
        .expect("read CHECKSUMS.sha256")
        .replace("\r\n", "\n");

    let mut recorded = BTreeSet::new();
    for line in manifest.lines().filter(|line| !line.trim().is_empty()) {
        let (expected, relative) = line
            .split_once("  ")
            .unwrap_or_else(|| panic!("malformed checksum line: {line}"));
        let bytes =
            fs::read(corpus.join(relative)).unwrap_or_else(|err| panic!("read {relative}: {err}"));
        let actual = format!("{:x}", Sha256::digest(&bytes));
        assert_eq!(actual, expected, "checksum mismatch for {relative}");
        recorded.insert(relative.to_string());
    }

    let mut on_disk = BTreeSet::new();
    collect_files(&corpus, &corpus, &mut on_disk);
    on_disk.remove("CHECKSUMS.sha256");
    on_disk.remove("README.md");
    assert_eq!(
        on_disk, recorded,
        "every vendored corpus file must be recorded in CHECKSUMS.sha256"
    );
}

struct ErlangCorpusScan {
    report: Value,
    db: PathBuf,
    _temp: TempDir,
}

impl ErlangCorpusScan {
    fn run() -> Self {
        let temp = TempDir::new().expect("create temp dir");
        let root = temp.path().join("repo");

        let corpus = corpus_root();
        let mut sources = BTreeSet::new();
        collect_files(&corpus, &corpus, &mut sources);
        let sources = sources
            .into_iter()
            .filter(|relative| relative.ends_with(".erl") || relative.ends_with(".hrl"))
            .collect::<Vec<_>>();
        assert_eq!(
            sources.len(),
            BASELINE.len(),
            "corpus source-file count drifted from the baseline"
        );
        for relative in &sources {
            let destination = root.join(relative);
            fs::create_dir_all(destination.parent().expect("source parent"))
                .expect("create scan dir");
            fs::copy(corpus.join(relative), &destination).expect("copy corpus source");
        }

        let db = temp.path().join("artifact.sqlite");
        let started = Instant::now();
        let output = julie_extract(&[
            "scan",
            "--root",
            path_str(&root),
            "--db",
            path_str(&db),
            "--json",
        ]);
        let elapsed = started.elapsed();
        println!(
            "erlang corpus scan: {} files, {} bytes, {:.2}s wall",
            sources.len(),
            sources
                .iter()
                .map(|relative| fs::metadata(corpus.join(relative))
                    .expect("corpus metadata")
                    .len())
                .sum::<u64>(),
            elapsed.as_secs_f64()
        );

        assert_eq!(
            output.status.code(),
            Some(0),
            "corpus scan must succeed\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        );

        let report = serde_json::from_slice(&output.stdout).unwrap_or_else(|err| {
            panic!(
                "stdout was not a JSON report: {err}\nstdout:\n{}\nstderr:\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            )
        });

        Self {
            report,
            db,
            _temp: temp,
        }
    }
}

fn symbols_in(connection: &Connection, path: &str) -> BTreeSet<(String, String, String)> {
    connection
        .prepare(
            "SELECT s.name, s.kind, s.visibility FROM symbols s \
             JOIN files f ON f.file_id = s.file_id WHERE f.path = ?1",
        )
        .expect("prepare symbol query")
        .query_map((path,), |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })
        .expect("query symbols")
        .collect::<Result<BTreeSet<_>, _>>()
        .expect("collect symbols")
}

fn collect_files(base: &Path, directory: &Path, into: &mut BTreeSet<String>) {
    for entry in fs::read_dir(directory).expect("read corpus dir") {
        let entry = entry.expect("corpus dir entry");
        let path = entry.path();
        if path.is_dir() {
            collect_files(base, &path, into);
        } else {
            into.insert(
                path.strip_prefix(base)
                    .expect("corpus-relative path")
                    .to_string_lossy()
                    .replace(std::path::MAIN_SEPARATOR, "/"),
            );
        }
    }
}

fn corpus_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/real-world/erlang")
        .canonicalize()
        .expect("fixtures/real-world/erlang must exist")
}

fn julie_extract(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_julie-extract"))
        .args(args)
        .output()
        .unwrap_or_else(|err| panic!("failed to run julie-extract {args:?}: {err}"))
}

fn path_str(path: &Path) -> &str {
    path.to_str().expect("path must be valid UTF-8")
}
