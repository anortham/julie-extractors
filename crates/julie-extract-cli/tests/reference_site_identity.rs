//! Reference-site identity contract.
//!
//! One source token owns ONE reference site, shared by the identifier,
//! relationship, and pending-relationship passes (site id =
//! `blake3(file_id, start_byte, end_byte)`). The passes compute the site's
//! denormalized `containing_symbol_id` through different code paths, so they can
//! disagree — and a disagreement used to abort the single import transaction,
//! zeroing the whole scan.
//!
//! Two enforcement surfaces:
//!
//! 1. **Root fixes** — the PowerShell one-line-function and C multi-declarator
//!    roundtrips assert the passes now AGREE (span containment decides for
//!    PowerShell; a total containment tie-break makes the shared helper immune to
//!    input iteration order for C).
//! 2. **Cross-language sweep** (parity gate) — a scan of the whole extraction
//!    fixture corpus must report no `reference_site_payload_conflict` warning for
//!    any language.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use rusqlite::Connection;
use serde_json::Value;
use tempfile::TempDir;

const CONFLICT_CODE: &str = "reference_site_payload_conflict";

struct Scan {
    _temp: TempDir,
    db: PathBuf,
    report: Value,
}

impl Scan {
    fn conflict_warnings(&self) -> Vec<&Value> {
        self.report["warnings"]
            .as_array()
            .map(|warnings| {
                warnings
                    .iter()
                    .filter(|warning| warning["code"] == CONFLICT_CODE)
                    .collect()
            })
            .unwrap_or_default()
    }

    fn connection(&self) -> Connection {
        Connection::open(&self.db).expect("artifact opens")
    }

    fn symbol_id(&self, name: &str) -> String {
        self.connection()
            .query_row(
                "SELECT symbol_id FROM symbols WHERE name = ?1",
                [name],
                |row| row.get(0),
            )
            .unwrap_or_else(|err| panic!("symbol {name} exists: {err}"))
    }

    /// `(reference_site_id, identifier containing, site containing)` for the sole
    /// identifier named `name`.
    fn call_site(&self, name: &str) -> (String, Option<String>, Option<String>) {
        self.connection()
            .query_row(
                "SELECT i.reference_site_id, i.containing_symbol_id, s.containing_symbol_id \
                 FROM identifiers i \
                 JOIN reference_sites s ON s.reference_site_id = i.reference_site_id \
                 WHERE i.name = ?1 AND i.kind = 'call'",
                [name],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap_or_else(|err| panic!("call identifier {name} exists: {err}"))
    }

    fn relationship_from(&self, reference_site_id: &str) -> Option<String> {
        self.connection()
            .query_row(
                "SELECT from_symbol_id FROM relationships WHERE reference_site_id = ?1",
                [reference_site_id],
                |row| row.get(0),
            )
            .ok()
    }

    fn pending_caller_scope(&self, reference_site_id: &str) -> Option<String> {
        self.connection()
            .query_row(
                "SELECT COALESCE(caller_scope_symbol_id, from_symbol_id) \
                 FROM pending_relationships WHERE reference_site_id = ?1",
                [reference_site_id],
                |row| row.get(0),
            )
            .ok()
    }
}

fn scan_sources(files: &[(&str, &str)]) -> Scan {
    let temp = TempDir::new().unwrap();
    let root = temp.path().join("repo");
    std::fs::create_dir_all(&root).unwrap();
    for (name, contents) in files {
        std::fs::write(root.join(name), contents).unwrap();
    }
    let db = temp.path().join("artifact.sqlite");
    let report = scan_root(&root, &db);
    Scan {
        _temp: temp,
        db,
        report,
    }
}

fn scan_root(root: &Path, db: &Path) -> Value {
    let output = julie_extract(&[
        "scan",
        "--root",
        path_str(root),
        "--db",
        path_str(db),
        "--json",
    ]);
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    assert_eq!(
        output.status.code(),
        Some(0),
        "scan of {} must succeed\nstdout:\n{stdout}\nstderr:\n{}",
        root.display(),
        String::from_utf8_lossy(&output.stderr),
    );
    serde_json::from_str(&stdout).unwrap_or_else(|err| panic!("report is JSON: {err}\n{stdout}"))
}

fn julie_extract(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_julie-extract"))
        .args(args)
        .output()
        .unwrap_or_else(|err| panic!("failed to run julie-extract {args:?}: {err}"))
}

fn path_str(path: &Path) -> &str {
    path.to_str().unwrap()
}

/// A ONE-LINE `function F { G }` used to yield `containing_symbol_id = NULL` from
/// the identifier pass (it filtered containment candidates to multi-line symbols)
/// while the relationship pass attached `F` at the same site id — the payload
/// disagreement that aborted every scan of a repo holding a one-line function.
#[test]
fn powershell_one_line_function_agrees_on_the_shared_call_site() {
    let scan = scan_sources(&[(
        "build.ps1",
        "function Get-Thing { 'thing' }\nfunction Invoke-All { Get-Thing }\n",
    )]);

    assert!(
        scan.conflict_warnings().is_empty(),
        "no payload conflict expected: {:?}",
        scan.conflict_warnings()
    );

    let caller = scan.symbol_id("Invoke-All");
    let (site_id, identifier_containing, site_containing) = scan.call_site("Get-Thing");

    assert_eq!(
        identifier_containing.as_deref(),
        Some(caller.as_str()),
        "identifier pass must place the call inside the one-line function"
    );
    assert_eq!(
        scan.relationship_from(&site_id).as_deref(),
        Some(caller.as_str()),
        "relationship pass must share the site and agree on the caller"
    );
    assert_eq!(
        site_containing.as_deref(),
        Some(caller.as_str()),
        "the shared site row must carry the agreed containing symbol"
    );
}

/// A C multi-declarator statement emits one variable symbol PER declarator, all
/// with the IDENTICAL statement span — a perfect containment tie. The identifier
/// pass feeds the shared helper from a `HashMap` (per-process random order) and
/// the relationship pass from a `Vec`, so before the total tie-break the two
/// passes picked different declarators and the import aborted. Repeated runs
/// defeat the per-process hash seed.
#[test]
fn c_multi_declarator_agrees_on_the_shared_call_site_across_runs() {
    const SOURCE: &str = "static long ticks(void) { return 42; }\n\
                          long warmup_iterations = 1, warmup_duration = 2, deadline = ticks() + 3;\n";

    let mut containers = std::collections::BTreeSet::new();
    for _ in 0..8 {
        let scan = scan_sources(&[("benchmark.c", SOURCE)]);
        assert!(
            scan.conflict_warnings().is_empty(),
            "no payload conflict expected: {:?}",
            scan.conflict_warnings()
        );

        let (site_id, identifier_containing, site_containing) = scan.call_site("ticks");
        let relationship_containing = scan
            .relationship_from(&site_id)
            .or_else(|| scan.pending_caller_scope(&site_id));

        assert_eq!(
            identifier_containing, site_containing,
            "the shared site row must carry the identifier pass's containing symbol"
        );
        assert_eq!(
            relationship_containing, identifier_containing,
            "identifier and relationship passes must pick the same equal-span declarator"
        );

        let containing = identifier_containing.expect("the call sits inside a declarator");
        containers.insert(
            scan.connection()
                .query_row(
                    "SELECT name FROM symbols WHERE symbol_id = ?1",
                    [&containing],
                    |row| row.get::<_, String>(0),
                )
                .unwrap(),
        );
    }

    assert_eq!(
        containers.len(),
        1,
        "the containment winner must not vary between processes: {containers:?}"
    );
}

/// Language-parity gate. The shared containment helper covers every language that
/// routes through it, but each language's relationship/pending pass computes the
/// caller through its own path. A scan of the whole extraction fixture corpus is
/// the sweep that catches any remaining pass-disagreement shape.
#[test]
fn extraction_fixture_corpus_reports_no_reference_site_payload_conflicts() {
    let corpus = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/extraction")
        .canonicalize()
        .expect("extraction fixture corpus exists");
    let temp = TempDir::new().unwrap();
    let db = temp.path().join("corpus.sqlite");

    let report = scan_root(&corpus, &db);
    let conflicts: Vec<&Value> = report["warnings"]
        .as_array()
        .map(|warnings| {
            warnings
                .iter()
                .filter(|warning| warning["code"] == CONFLICT_CODE)
                .collect()
        })
        .unwrap_or_default();

    assert!(
        conflicts.is_empty(),
        "extraction passes disagree on a reference-site payload in {} file(s): {}",
        conflicts.len(),
        serde_json::to_string_pretty(&conflicts).unwrap()
    );
}
