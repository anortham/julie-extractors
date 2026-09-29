//! CLI incremental scans must match a fresh scan of the same source tree.
//!
//! The comparator keeps all IDs and content facts. It excludes only the named
//! identity, history and timing fields below, and sorts JSON object keys while
//! preserving array order.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use rusqlite::Connection;
use rusqlite::types::Value;
use serde_json::Value as Json;
use tempfile::TempDir;

/// Artifact identity and timestamps vary between scans.
const RUN_VARIANT_METADATA_KEYS: &[&str] = &[
    "artifact_id",
    "created_at",
    "updated_at",
    "rebound_at",
    "rebound_from_root",
    "rebound_from_artifact_id",
];

/// Append-only records of how an artifact was reached.
const HISTORY_TABLES: &[&str] = &["extraction_revisions", "revision_file_changes"];

/// `last_revision_id` is contracted: it names a row in the excluded history.
///
/// An unchanged file keeps the stamp from the scan that last extracted it,
/// while a fresh scan stamps it again.
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

fn describe_difference(actual: &[String], fresh: &[String]) -> String {
    let missing = only_in(fresh, actual);
    let unexpected = only_in(actual, fresh);
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
        "actual has {} rows, fresh has {} rows{}{}",
        actual.len(),
        fresh.len(),
        sample("only in fresh", &missing),
        sample("only in actual", &unexpected),
    )
}

fn assert_equivalent(actual: &Path, fresh: &Path, what: &str) {
    assert_gate_is_not_vacuous(fresh, what);
    assert_eq!(
        data_tables(actual),
        data_tables(fresh),
        "{what}: the two artifacts must carry the same tables"
    );

    assert_eq!(
        comparable_metadata(actual),
        comparable_metadata(fresh),
        "{what}: every artifact_metadata key outside the contracted identity and \
         timing keys must match, root_path included"
    );

    for table in data_tables(fresh) {
        if table == "artifact_metadata" || HISTORY_TABLES.contains(&table.as_str()) {
            continue;
        }
        let actual_rows = table_rows(actual, &table);
        let fresh_rows = table_rows(fresh, &table);
        assert_eq!(
            actual_rows,
            fresh_rows,
            "{what}: {table} must match a fresh scan row for row — {}",
            describe_difference(&actual_rows, &fresh_rows)
        );
    }
}

fn file_identity(db: &Path, path: &str) -> (String, String) {
    let conn = Connection::open(db).expect("artifact opens");
    conn.query_row(
        "SELECT file_id, content_hash FROM files WHERE path = ?1",
        [path],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )
    .unwrap()
}

fn rows_for_path(db: &Path, table: &str, path: &str) -> i64 {
    let conn = Connection::open(db).expect("artifact opens");
    conn.query_row(
        &format!("SELECT COUNT(*) FROM \"{table}\" WHERE path = ?1"),
        [path],
        |row| row.get(0),
    )
    .unwrap()
}

fn type_arguments_for_path(db: &Path, path: &str) -> i64 {
    let conn = Connection::open(db).expect("artifact opens");
    conn.query_row(
        "SELECT COUNT(*)
         FROM type_arguments AS arguments
         JOIN type_argument_usages AS usages USING (usage_id)
         WHERE usages.path = ?1",
        [path],
        |row| row.get(0),
    )
    .unwrap()
}

fn assert_no_source_path_facts(db: &Path, path: &str) {
    for table in [
        "files",
        "symbols",
        "reference_sites",
        "identifiers",
        "relationships",
        "pending_relationships",
        "type_argument_usages",
        "literals",
        "source_regions",
        "structural_facts",
        "complexity_metrics",
        "parse_diagnostics",
    ] {
        assert_eq!(
            rows_for_path(db, table, path),
            0,
            "{table} retained facts for {path}"
        );
    }

    let conn = Connection::open(db).expect("artifact opens");
    for (table, query) in [
        (
            "symbol_annotations",
            "SELECT COUNT(*) FROM symbol_annotations
             JOIN symbols USING (symbol_id) WHERE symbols.path = ?1",
        ),
        (
            "type_facts",
            "SELECT COUNT(*) FROM type_facts
             JOIN symbols USING (symbol_id) WHERE symbols.path = ?1",
        ),
    ] {
        let count: i64 = conn.query_row(query, [path], |row| row.get(0)).unwrap();
        assert_eq!(count, 0, "{table} retained facts for {path}");
    }
    let type_argument_count: i64 = conn
        .query_row(
            "SELECT COUNT(*)
             FROM type_arguments AS arguments
             JOIN type_argument_usages AS usages USING (usage_id)
             WHERE usages.path = ?1",
            [path],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        type_argument_count, 0,
        "type_arguments retained facts for {path}"
    );
}

fn assert_incremental_report(report: &Json, min_changed: i64, min_deleted: i64) {
    assert_eq!(report["status"], "ok", "incremental scan report: {report}");
    assert!(
        report["counts"]["files_changed"]
            .as_i64()
            .unwrap_or_default()
            >= min_changed,
        "incremental scan changed too few files: {report}"
    );
    assert!(
        report["counts"]["files_deleted"]
            .as_i64()
            .unwrap_or_default()
            >= min_deleted,
        "incremental scan deleted too few files: {report}"
    );
    assert!(
        report["counts"]["files_unchanged"]
            .as_i64()
            .unwrap_or_default()
            > 0,
        "incremental scan did not skip any unchanged files: {report}"
    );
}

fn assert_noop_report(report: &Json) {
    assert_eq!(report["status"], "no_change", "no-op scan report: {report}");
    assert_eq!(
        report["counts"]["files_changed"], 0,
        "no-op scan report: {report}"
    );
    assert_eq!(
        report["counts"]["files_deleted"], 0,
        "no-op scan report: {report}"
    );
    assert!(
        report["counts"]["files_unchanged"]
            .as_i64()
            .unwrap_or_default()
            > 0,
        "no-op scan did not skip files: {report}"
    );
}

fn assert_matches_fresh(fixture: &Fixture, root: &Path, actual: &Path, label: &str) {
    let fresh = fixture.db(&format!("fresh-{label}.sqlite"));
    let report = scan(root, &fresh);
    assert_eq!(report["status"], "ok", "fresh scan report: {report}");
    assert_all_languages_extracted(&fresh, &["rust", "csharp", "typescript"]);
    assert_equivalent(actual, &fresh, label);
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
        julie_extract_cli::strip_verbatim_prefix(new_root.canonicalize().unwrap())
            .to_str()
            .unwrap(),
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

#[test]
fn incremental_scans_match_fresh_after_source_tree_changes() {
    let fixture = Fixture::new();
    let root = fixture.tree("incremental");
    let source = root.join("src");
    let incremental = fixture.db("incremental.sqlite");
    let initial_report = scan(&root, &incremental);
    assert_eq!(
        initial_report["status"], "ok",
        "initial scan report: {initial_report}"
    );
    assert_all_languages_extracted(&incremental, &["rust", "csharp", "typescript"]);
    assert_gate_is_not_vacuous(&incremental, "initial scan");
    let stable_identity = file_identity(&incremental, "src/core.rs");

    std::fs::write(
        source.join("retired.ts"),
        "export interface RetiredToken { id: string; }\n\
         export function retire(tokens: RetiredToken[]): string {\n  \
         const index = new Map<string, RetiredToken>();\n  \
         index.set(\"retired-only-literal\", tokens[0]);\n  \
         if (tokens.length > 0) {\n    \
         return fetch(\"https://retired.example.test/item\").then(response => response.url);\n  \
         }\n  \
         return \"empty\";\n}\n",
    )
    .unwrap();
    std::fs::write(
        source.join("recovery.go"),
        "package main\n\ntype Recovered struct {\n    value int\n\nfunc recoverValue() int { return 1 }\n",
    )
    .unwrap();

    let add_report = scan(&root, &incremental);
    assert_incremental_report(&add_report, 2, 0);
    assert!(
        rows_for_path(&incremental, "parse_diagnostics", "src/recovery.go") > 0,
        "the malformed Go source must produce parse diagnostics"
    );
    for table in [
        "symbols",
        "type_argument_usages",
        "literals",
        "structural_facts",
        "complexity_metrics",
    ] {
        assert!(
            rows_for_path(&incremental, table, "src/retired.ts") > 0,
            "the added source must exercise {table}"
        );
    }
    assert!(
        type_arguments_for_path(&incremental, "src/retired.ts") > 0,
        "the added source must exercise type_arguments"
    );
    assert_matches_fresh(&fixture, &root, &incremental, "after-add");
    assert_eq!(file_identity(&incremental, "src/core.rs"), stable_identity);

    apply_modify_delta(&root);
    std::fs::write(
        source.join("recovery.go"),
        "package main\n\nfunc recoverValue() int { return 2 }\n",
    )
    .unwrap();

    let edit_report = scan(&root, &incremental);
    assert_incremental_report(&edit_report, 4, 0);
    assert_eq!(
        rows_for_path(&incremental, "parse_diagnostics", "src/recovery.go"),
        0,
        "valid source must clear the old parse diagnostics"
    );
    assert_matches_fresh(&fixture, &root, &incremental, "after-edit");
    assert_eq!(file_identity(&incremental, "src/core.rs"), stable_identity);

    let old_retired_path = "src/retired.ts";
    let new_retired_path = "src/renamed_retired.ts";
    let renamed_fact_counts = [
        "symbols",
        "type_argument_usages",
        "literals",
        "structural_facts",
        "complexity_metrics",
    ]
    .map(|table| rows_for_path(&incremental, table, old_retired_path));
    let renamed_type_arguments = type_arguments_for_path(&incremental, old_retired_path);
    std::fs::rename(source.join("retired.ts"), source.join("renamed_retired.ts")).unwrap();

    let rename_report = scan(&root, &incremental);
    assert_incremental_report(&rename_report, 1, 1);
    assert_no_source_path_facts(&incremental, old_retired_path);
    assert_eq!(
        [
            "symbols",
            "type_argument_usages",
            "literals",
            "structural_facts",
            "complexity_metrics",
        ]
        .map(|table| rows_for_path(&incremental, table, new_retired_path)),
        renamed_fact_counts,
        "renaming the file must preserve its source facts"
    );
    assert_eq!(
        type_arguments_for_path(&incremental, new_retired_path),
        renamed_type_arguments
    );
    assert_matches_fresh(&fixture, &root, &incremental, "after-rename");
    assert_eq!(file_identity(&incremental, "src/core.rs"), stable_identity);

    std::fs::remove_file(source.join("renamed_retired.ts")).unwrap();
    std::fs::remove_file(source.join("Repository.cs")).unwrap();

    let delete_report = scan(&root, &incremental);
    assert_incremental_report(&delete_report, 0, 2);
    assert_no_source_path_facts(&incremental, new_retired_path);
    assert_no_source_path_facts(&incremental, "src/Repository.cs");
    assert_matches_fresh(&fixture, &root, &incremental, "after-delete");
    assert_eq!(file_identity(&incremental, "src/core.rs"), stable_identity);

    for label in ["first-noop", "second-noop"] {
        let report = scan(&root, &incremental);
        assert_noop_report(&report);
        assert_matches_fresh(&fixture, &root, &incremental, label);
        assert_eq!(file_identity(&incremental, "src/core.rs"), stable_identity);
    }
}
