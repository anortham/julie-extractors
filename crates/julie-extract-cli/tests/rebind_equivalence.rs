//! The rebind acceptance gate: an artifact copied, retargeted at a second
//! checkout, and reconciled by an ordinary incremental scan must be
//! indistinguishable from a from-scratch scan of that checkout.
//!
//! ## What "indistinguishable" means here
//!
//! Per-table row-multiset equality over every data table, with these
//! normalizations applied first:
//!
//! 1. Identity and timing metadata — `artifact_id`, `created_at`, `updated_at`,
//!    and the three `rebound_*` provenance keys. Every REMAINING
//!    `artifact_metadata` key must match, `root_path` included: that key is the
//!    whole point of the verb.
//! 2. Revision-history bookkeeping — `extraction_revisions` and
//!    `revision_file_changes` are append-only histories of HOW an artifact was
//!    reached, and a rebound artifact necessarily reached the same content by a
//!    different route. They are excluded whole; every other table is compared
//!    whole minus its revision-id columns, and the one revision id that lives in
//!    metadata goes with them (see [`RUN_VARIANT_METADATA_KEYS`]).
//! 3. `files.indexed_at` — see [`RUN_VARIANT_FILE_COLUMNS`]. NOT one of the
//!    design's contracted exclusions; excluded here as a reported finding.
//! 4. `*_json` columns are compared as JSON with keys sorted, not as bytes — see
//!    [`canonical_json`]. Also a reported finding, and also not a rebind
//!    difference: two plain scans of the same tree disagree the same way.
//!
//! A failure here is a defect in the rebind path, never a test to relax: it
//! means a consumer served by a rebound artifact would get a different answer
//! than one served by a fresh scan of the same tree.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use rusqlite::Connection;
use rusqlite::types::Value;
use serde_json::Value as Json;
use tempfile::TempDir;

/// Excluded because they record the artifact's identity and when it was
/// touched, which a retarget deliberately rewrites.
///
const RUN_VARIANT_METADATA_KEYS: &[&str] = &[
    "artifact_id",
    "created_at",
    "updated_at",
    "rebound_at",
    "rebound_from_root",
    "rebound_from_artifact_id",
];

/// Append-only records of how an artifact was reached rather than what it
/// contains. A rebound artifact carries the base scan's revision plus the
/// reconciling one; a fresh scan carries a single revision.
const HISTORY_TABLES: &[&str] = &["extraction_revisions", "revision_file_changes"];

/// `last_revision_id` is contracted: it names a row in the excluded history.
///
/// `indexed_at` is NOT, and is excluded here as a reported finding — an
/// unchanged file keeps the wall-clock stamp of the scan that last extracted it,
/// so every file the reconciling scan skipped carries the BASE scan's stamp
/// while a fresh scan stamps them all with its own. The column is a scan-time
/// audit fact, not content, so no consumer answer depends on it.
const RUN_VARIANT_FILE_COLUMNS: &[&str] = &["last_revision_id", "indexed_at"];

fn excluded_columns(table: &str) -> &'static [&'static str] {
    match table {
        "files" => RUN_VARIANT_FILE_COLUMNS,
        _ => &[],
    }
}

// ---------------------------------------------------------------------------
// Binary invocation
// ---------------------------------------------------------------------------

fn julie_extract(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_julie-extract"))
        .args(args)
        .output()
        .unwrap_or_else(|err| panic!("failed to run julie-extract {args:?}: {err}"))
}

fn json_report(output: &Output) -> Json {
    serde_json::from_slice(&output.stdout).unwrap_or_else(|err| {
        panic!(
            "stdout was not a JSON report: {err}\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    })
}

fn path_str(path: &Path) -> &str {
    path.to_str().expect("fixture paths are UTF-8")
}

fn assert_success(output: &Output, what: &str) {
    assert_eq!(
        output.status.code(),
        Some(0),
        "{what} failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn scan(root: &Path, db: &Path) -> Json {
    let output = julie_extract(&[
        "scan",
        "--root",
        path_str(root),
        "--db",
        path_str(db),
        "--json",
    ]);
    assert_success(&output, "scan");
    json_report(&output)
}

fn rebind(db: &Path, root: &Path) -> Json {
    let output = julie_extract(&[
        "rebind",
        "--db",
        path_str(db),
        "--root",
        path_str(root),
        "--json",
    ]);
    assert_success(&output, "rebind");
    json_report(&output)
}

// ---------------------------------------------------------------------------
// Fixture trees
// ---------------------------------------------------------------------------

/// The base tree, written identically wherever it is planted.
///
/// Multi-language by design: a rebind that worked for one grammar and silently
/// dropped rows for another would pass a single-language gate. Each language
/// carries a cross-file reference so pending relationships stay non-empty, plus
/// an attribute, a generic instantiation, and
/// a URL-carrier call so `symbol_annotations`, `type_argument_usages`,
/// `type_arguments`, `literals`, and `structural_facts` are non-empty too — a
/// table that is empty on both sides proves nothing.
fn write_base_tree(root: &Path) {
    let src = root.join("src");
    std::fs::create_dir_all(&src).unwrap();

    std::fs::write(
        src.join("core.rs"),
        "/// The engine every caller boots.\n\
         #[derive(Debug, Clone)]\n\
         pub struct Engine {\n    pub name: String,\n}\n\n\
         impl Engine {\n    \
         pub fn new(name: &str) -> Engine {\n        Engine { name: name.to_string() }\n    }\n\n    \
         pub fn run(&self) -> usize {\n        self.name.len()\n    }\n\n    \
         pub fn labels(&self) -> Vec<String> {\n        \
         let mut labels: Vec<String> = Vec::new();\n        \
         labels.push(self.name.clone());\n        \
         labels\n    }\n}\n\n\
         pub fn boot() -> usize {\n    let engine = Engine::new(\"engine\");\n    engine.run()\n}\n",
    )
    .unwrap();

    std::fs::write(
        src.join("util.rs"),
        "use crate::core::Engine;\n\n\
         pub fn describe(engine: &Engine) -> String {\n    \
         format!(\"engine:{}\", engine.name)\n}\n\n\
         pub fn tally(values: &[usize]) -> usize {\n    \
         let mut total = 0;\n    \
         for value in values {\n        if *value > 2 {\n            total += *value;\n        }\n    }\n    \
         total\n}\n",
    )
    .unwrap();

    std::fs::write(
        src.join("Service.cs"),
        "using System;\nusing System.Collections.Generic;\nusing System.Net.Http;\n\n\
         namespace Fixture;\n\n\
         /// <summary>Totals orders for a caller.</summary>\n\
         public sealed class OrderService\n{\n    \
         private readonly OrderRepository repository;\n    \
         private readonly List<int> cache = new List<int>();\n\n    \
         public OrderService(OrderRepository repository)\n    {\n        \
         this.repository = repository;\n    }\n\n    \
         [Obsolete(\"prefer Total\")]\n    \
         public int Sum(int[] amounts)\n    {\n        return this.Total(amounts);\n    }\n\n    \
         public int Total(int[] amounts)\n    {\n        \
         var total = 0;\n        \
         foreach (var amount in amounts)\n        {\n            \
         if (amount > 0)\n            {\n                total += amount;\n            }\n        }\n\n        \
         return total + this.repository.Count();\n    }\n\n    \
         public void Warm(HttpClient client)\n    {\n        \
         client.GetAsync(\"https://api.example.com/orders\");\n    }\n}\n",
    )
    .unwrap();

    std::fs::write(
        src.join("Repository.cs"),
        "namespace Fixture;\n\n\
         public sealed class OrderRepository\n{\n    \
         public int Count()\n    {\n        return 3;\n    }\n}\n",
    )
    .unwrap();

    std::fs::write(
        src.join("app.ts"),
        "import { formatLabel } from \"./helpers\";\n\n\
         export interface Order {\n  id: string;\n  amount: number;\n}\n\n\
         export function summarize(orders: Order[]): string {\n  \
         let total = 0;\n  \
         for (const order of orders) {\n    \
         if (order.amount > 0) {\n      total += order.amount;\n    }\n  }\n\n  \
         return formatLabel(\"total\", total);\n}\n\n\
         export async function load(): Promise<Order[]> {\n  \
         const response = await fetch(\"https://api.example.com/orders\");\n  \
         const index = new Map<string, number>();\n  \
         index.set(\"count\", 0);\n  \
         return response.json();\n}\n",
    )
    .unwrap();

    std::fs::write(
        src.join("helpers.ts"),
        "export function formatLabel(name: string, value: number): string {\n  \
         return `${name}=${value}`;\n}\n",
    )
    .unwrap();

    std::fs::write(
        root.join("config.json"),
        "{\n  \"name\": \"fixture\",\n  \"limits\": {\n    \"orders\": 25\n  }\n}\n",
    )
    .unwrap();
}

/// Body-only edits in all three languages: same files, same symbol set,
/// different content hashes and body spans.
fn apply_modify_delta(root: &Path) {
    let src = root.join("src");

    std::fs::write(
        src.join("util.rs"),
        "use crate::core::Engine;\n\n\
         pub fn describe(engine: &Engine) -> String {\n    \
         format!(\"engine<{}>\", engine.name)\n}\n\n\
         pub fn tally(values: &[usize]) -> usize {\n    \
         let mut total = 0;\n    \
         for value in values {\n        \
         if *value > 5 {\n            total += *value * 2;\n        } else {\n            total += 1;\n        }\n    }\n    \
         total\n}\n",
    )
    .unwrap();

    std::fs::write(
        src.join("Service.cs"),
        "using System;\nusing System.Collections.Generic;\nusing System.Net.Http;\n\n\
         namespace Fixture;\n\n\
         /// <summary>Totals orders for a caller.</summary>\n\
         public sealed class OrderService\n{\n    \
         private readonly OrderRepository repository;\n    \
         private readonly List<int> cache = new List<int>();\n\n    \
         public OrderService(OrderRepository repository)\n    {\n        \
         this.repository = repository;\n    }\n\n    \
         [Obsolete(\"prefer Total\")]\n    \
         public int Sum(int[] amounts)\n    {\n        return this.Total(amounts) * 2;\n    }\n\n    \
         public int Total(int[] amounts)\n    {\n        \
         var total = this.repository.Count();\n        \
         foreach (var amount in amounts)\n        {\n            \
         if (amount > 10)\n            {\n                total += amount * 2;\n            }\n        }\n\n        \
         return total;\n    }\n\n    \
         public void Warm(HttpClient client)\n    {\n        \
         client.GetAsync(\"https://api.example.com/orders?warm=1\");\n    }\n}\n",
    )
    .unwrap();

    std::fs::write(
        src.join("app.ts"),
        "import { formatLabel } from \"./helpers\";\n\n\
         export interface Order {\n  id: string;\n  amount: number;\n}\n\n\
         export function summarize(orders: Order[]): string {\n  \
         let total = 0;\n  \
         for (const order of orders) {\n    \
         if (order.amount > 10) {\n      total += order.amount * 2;\n    }\n  }\n\n  \
         return formatLabel(\"grand total\", total);\n}\n\n\
         export async function load(): Promise<Order[]> {\n  \
         const response = await fetch(\"https://api.example.com/orders?page=2\");\n  \
         const index = new Map<string, number>();\n  \
         index.set(\"count\", 1);\n  \
         return response.json();\n}\n",
    )
    .unwrap();
}

/// One added file and one deleted file, where the deleted file's type is
/// referenced from a file that does NOT change. That is the case worth gating:
/// the reconciling scan must re-resolve the untouched referrer rather than leave
/// it pointing at a symbol that no longer exists.
fn apply_structure_delta(root: &Path) {
    let src = root.join("src");

    std::fs::remove_file(src.join("Repository.cs")).unwrap();

    std::fs::write(
        src.join("report.ts"),
        "import { formatLabel } from \"./helpers\";\n\n\
         export function renderReport(rows: number[]): string[] {\n  \
         return rows.map((row, index) => formatLabel(`row${index}`, row));\n}\n",
    )
    .unwrap();
}

struct Fixture {
    _temp: TempDir,
    temp: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let temp = TempDir::new().unwrap();
        let path = temp.path().to_path_buf();
        Self {
            _temp: temp,
            temp: path,
        }
    }

    /// Trees live in siblings of the artifacts so no scan ever walks a `.sqlite`.
    fn tree(&self, name: &str) -> PathBuf {
        let root = self.temp.join("trees").join(name);
        write_base_tree(&root);
        root
    }

    fn db(&self, name: &str) -> PathBuf {
        let dir = self.temp.join("artifacts");
        std::fs::create_dir_all(&dir).unwrap();
        dir.join(name)
    }
}

// ---------------------------------------------------------------------------
// Artifact comparison
// ---------------------------------------------------------------------------

fn data_tables(db: &Path) -> Vec<String> {
    let conn = Connection::open(db).expect("artifact opens");
    let mut statement = conn
        .prepare(
            "SELECT name FROM sqlite_master \
             WHERE type = 'table' AND name NOT LIKE 'sqlite_%' ORDER BY name",
        )
        .unwrap();
    statement
        .query_map([], |row| row.get(0))
        .unwrap()
        .collect::<Result<Vec<String>, _>>()
        .unwrap()
}

fn compared_columns(conn: &Connection, table: &str) -> Vec<String> {
    let excluded = excluded_columns(table);
    let mut statement = conn
        .prepare(&format!("PRAGMA table_info(\"{table}\")"))
        .unwrap();
    let columns = statement
        .query_map([], |row| row.get::<_, String>(1))
        .unwrap()
        .collect::<Result<Vec<String>, _>>()
        .unwrap();
    assert!(
        !columns.is_empty(),
        "{table} reported no columns; the schema probe is broken"
    );
    columns
        .into_iter()
        .filter(|column| !excluded.contains(&column.as_str()))
        .collect()
}

/// JSON-valued columns compared by content rather than by bytes.
///
/// The extractor serializes them from hash maps, so two runs over IDENTICAL
/// bytes emit the same keys in different orders — `{"role":…,"variableType":…}`
/// one run, `{"variableType":…,"role":…}` the next. That is run-to-run
/// nondeterminism in extraction, not a rebind difference (it shows up between
/// two plain scans of the same tree), so the value is parsed and re-emitted with
/// keys sorted at every depth before comparison. A malformed value is compared
/// verbatim rather than silently passing.
fn canonical_json(text: &str) -> String {
    serde_json::from_str::<Json>(text)
        .map(|value| sort_json_keys(&value).to_string())
        .unwrap_or_else(|_| text.to_string())
}

fn sort_json_keys(value: &Json) -> Json {
    match value {
        Json::Object(entries) => Json::Object(
            entries
                .iter()
                .map(|(key, nested)| (key.clone(), sort_json_keys(nested)))
                .collect::<BTreeMap<_, _>>()
                .into_iter()
                .collect(),
        ),
        Json::Array(items) => Json::Array(items.iter().map(sort_json_keys).collect()),
        scalar => scalar.clone(),
    }
}

fn render_value(column: &str, value: &Value) -> String {
    match value {
        Value::Text(text) if column.ends_with("_json") => {
            format!("{column}={:?}", canonical_json(text))
        }
        other => format!("{column}={other:?}"),
    }
}

/// Every row of `table` as a sorted multiset of column-tagged strings.
///
/// Sorted rather than ordered by primary key so the comparison is insensitive to
/// insertion order — an incremental scan writes the files it touched last, and
/// that ordering carries no meaning a consumer can observe.
fn table_rows(db: &Path, table: &str) -> Vec<String> {
    let conn = Connection::open(db).expect("artifact opens");
    let columns = compared_columns(&conn, table);
    let projection = columns
        .iter()
        .map(|column| format!("\"{column}\""))
        .collect::<Vec<_>>()
        .join(", ");

    let mut statement = conn
        .prepare(&format!("SELECT {projection} FROM \"{table}\""))
        .unwrap();
    let mut rows = statement
        .query_map([], |row| {
            let mut rendered = String::new();
            for (index, column) in columns.iter().enumerate() {
                if index > 0 {
                    rendered.push('\u{1f}');
                }
                let value: Value = row.get(index)?;
                rendered.push_str(&render_value(column, &value));
            }
            Ok(rendered)
        })
        .unwrap()
        .collect::<Result<Vec<String>, _>>()
        .unwrap();
    rows.sort();
    rows
}

fn comparable_metadata(db: &Path) -> BTreeMap<String, String> {
    let conn = Connection::open(db).expect("artifact opens");
    let mut statement = conn
        .prepare("SELECT key, value FROM artifact_metadata ORDER BY key")
        .unwrap();
    statement
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .unwrap()
        .collect::<Result<BTreeMap<String, String>, _>>()
        .unwrap()
        .into_iter()
        .filter(|(key, _)| !RUN_VARIANT_METADATA_KEYS.contains(&key.as_str()))
        .collect()
}

fn only_in<'a>(left: &'a [String], right: &[String]) -> Vec<&'a String> {
    let mut remaining = right.to_vec();
    let mut extra = Vec::new();
    for row in left {
        match remaining.binary_search(row) {
            Ok(index) => {
                remaining.remove(index);
            }
            Err(_) => extra.push(row),
        }
    }
    extra
}

fn describe_difference(rebound: &[String], fresh: &[String]) -> String {
    let missing = only_in(fresh, rebound);
    let unexpected = only_in(rebound, fresh);
    let sample = |label: &str, rows: &[&String]| {
        if rows.is_empty() {
            return String::new();
        }
        let shown = rows
            .iter()
            .take(5)
            .map(|row| format!("    {row}"))
            .collect::<Vec<_>>()
            .join("\n");
        format!("\n  {label} ({}):\n{shown}", rows.len())
    };
    format!(
        "rebound has {} rows, fresh has {} rows{}{}",
        rebound.len(),
        fresh.len(),
        sample("only in fresh", &missing),
        sample("only in rebound", &unexpected),
    )
}

/// The gate itself: a rebound-and-reconciled artifact against a fresh scan.
fn assert_equivalent(rebound: &Path, fresh: &Path, what: &str) {
    assert_gate_is_not_vacuous(fresh, what);
    assert_eq!(
        data_tables(rebound),
        data_tables(fresh),
        "{what}: the two artifacts must carry the same tables"
    );

    assert_eq!(
        comparable_metadata(rebound),
        comparable_metadata(fresh),
        "{what}: every artifact_metadata key outside the contracted identity and \
         timing keys must match, root_path included"
    );

    for table in data_tables(fresh) {
        if table == "artifact_metadata" || HISTORY_TABLES.contains(&table.as_str()) {
            continue;
        }
        let rebound_rows = table_rows(rebound, &table);
        let fresh_rows = table_rows(fresh, &table);
        assert_eq!(
            rebound_rows,
            fresh_rows,
            "{what}: {table} must match a fresh scan row for row — {}",
            describe_difference(&rebound_rows, &fresh_rows)
        );
    }
}

/// Guards the gate against passing vacuously on an artifact that extracted
/// nothing, and against a fixture that quietly stops covering a language.
fn assert_all_languages_extracted(db: &Path, expected: &[&str]) {
    let conn = Connection::open(db).expect("artifact opens");
    let mut statement = conn
        .prepare("SELECT language, COUNT(*) FROM symbols GROUP BY language")
        .unwrap();
    let counts = statement
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })
        .unwrap()
        .collect::<Result<BTreeMap<String, i64>, _>>()
        .unwrap();

    for language in expected {
        let count = counts.get(*language).copied().unwrap_or_default();
        assert!(
            count > 0,
            "{language} extracted no symbols; the equivalence gate would be vacuous \
             for it (languages present: {counts:?})"
        );
    }
}

/// Tables the fixture is built to populate. A table that is empty on BOTH sides
/// compares equal for free, so an edit that quietly stops exercising one would
/// hollow the gate out without failing it.
///
/// `parse_diagnostics` and `language_capability_*` are deliberately absent: the
/// first needs a deliberately unparsable file, and the latter are populated by
/// the binary's own capability snapshot rather than by the fixture.
const TABLES_THE_FIXTURE_MUST_EXERCISE: &[&str] = &[
    "files",
    "symbols",
    "symbol_annotations",
    "reference_sites",
    "identifiers",
    "pending_relationships",
    "type_facts",
    "type_argument_usages",
    "type_arguments",
    "literals",
    "source_regions",
    "structural_facts",
    "complexity_metrics",
];

fn assert_gate_is_not_vacuous(db: &Path, what: &str) {
    for table in TABLES_THE_FIXTURE_MUST_EXERCISE {
        assert!(
            !table_rows(db, table).is_empty(),
            "{what}: the fixture no longer produces {table} rows, so comparing that \
             table proves nothing"
        );
    }
}

fn assert_rebound(report: &Json, previous_root: &Path, new_root: &Path) {
    assert_eq!(report["status"], "ok", "rebind report: {report}");
    let rebind = &report["rebind"];
    assert_eq!(rebind["changed"], true, "rebind report: {report}");
    assert!(
        rebind["previous_root"]
            .as_str()
            .expect("previous_root is a string")
            .ends_with(
                previous_root
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap()
            ),
        "rebind report: {report}"
    );
    assert_eq!(
        rebind["new_root"].as_str().expect("new_root is a string"),
        new_root.canonicalize().unwrap().to_str().unwrap(),
        "rebind report: {report}"
    );
    assert_ne!(
        rebind["previous_artifact_id"], rebind["new_artifact_id"],
        "a retarget must mint a new identity: {report}"
    );
}

/// Stage the shared arms: scan tree A, copy the artifact, retarget the copy at
/// tree B, and scan tree B through it. Returns the reconciling scan's report.
fn rebind_and_reconcile(fixture: &Fixture, tree_a: &Path, tree_b: &Path) -> (PathBuf, Json) {
    let base = fixture.db("base.sqlite");
    scan(tree_a, &base);
    assert_all_languages_extracted(&base, &["rust", "csharp", "typescript"]);

    let rebound = fixture.db("rebound.sqlite");
    std::fs::copy(&base, &rebound).expect("the artifact copies");

    assert_rebound(&rebind(&rebound, tree_b), tree_a, tree_b);
    let report = scan(tree_b, &rebound);
    (rebound, report)
}

// ---------------------------------------------------------------------------
// Arms
// ---------------------------------------------------------------------------

/// Invariant: retargeting at a byte-identical checkout costs nothing and changes
/// nothing — the reconciling scan finds no work, and the artifact still answers
/// exactly as a fresh scan of that checkout would.
#[test]
fn rebound_artifact_matches_a_fresh_scan_of_an_identical_tree() {
    let fixture = Fixture::new();
    let tree_a = fixture.tree("a");
    let tree_b = fixture.tree("b");

    let (rebound, report) = rebind_and_reconcile(&fixture, &tree_a, &tree_b);
    assert_eq!(
        report["status"], "no_change",
        "an identical checkout must cost the reconciling scan nothing: {report}"
    );

    let fresh = fixture.db("fresh.sqlite");
    scan(&tree_b, &fresh);

    assert_equivalent(&rebound, &fresh, "identical tree");
}

/// Invariant: a rebound artifact reconciled over body-only edits in every
/// fixture language carries the same rows as a fresh scan — the incremental
/// re-extraction of a changed file leaves nothing of the old content behind.
#[test]
fn rebound_artifact_matches_a_fresh_scan_after_a_modify_only_delta() {
    let fixture = Fixture::new();
    let tree_a = fixture.tree("a");
    let tree_b = fixture.tree("b");
    apply_modify_delta(&tree_b);

    let (rebound, report) = rebind_and_reconcile(&fixture, &tree_a, &tree_b);
    assert_eq!(
        report["status"], "ok",
        "a modified checkout must give the reconciling scan work: {report}"
    );

    let fresh = fixture.db("fresh.sqlite");
    scan(&tree_b, &fresh);
    assert_all_languages_extracted(&fresh, &["rust", "csharp", "typescript"]);

    assert_equivalent(&rebound, &fresh, "modify-only delta");
}

/// Invariant: a rebound artifact reconciled over an added file and a deleted one
/// carries the same rows as a fresh scan — including the resolution overlay of
/// an UNCHANGED file that referenced the deleted file's type, which only the
/// structure-changed full-resolution path can restate correctly.
#[test]
fn rebound_artifact_matches_a_fresh_scan_after_an_add_and_delete_delta() {
    let fixture = Fixture::new();
    let tree_a = fixture.tree("a");
    let tree_b = fixture.tree("b");
    apply_structure_delta(&tree_b);

    let (rebound, report) = rebind_and_reconcile(&fixture, &tree_a, &tree_b);
    assert_eq!(
        report["status"], "ok",
        "an added and a deleted file must give the reconciling scan work: {report}"
    );

    let fresh = fixture.db("fresh.sqlite");
    scan(&tree_b, &fresh);
    assert_all_languages_extracted(&fresh, &["rust", "csharp", "typescript"]);

    assert_equivalent(&rebound, &fresh, "add-and-delete delta");
}
