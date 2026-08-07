//! Cross-process determinism gate for `metadata_json`.
//!
//! Every artifact table that carries a `metadata_json` column must serialize
//! byte-identical text for the same source tree, no matter which process
//! produced it. Downstream stores prove row equivalence and produce binding
//! diffs by comparing that text directly, so a key-order wobble reads as a
//! content change on every row.
//!
//! The two scans run as two SEPARATE spawned processes on purpose: `RandomState`
//! reseeds per process, so `HashMap` iteration order — and therefore the key
//! order of any `HashMap`-backed metadata map — only diverges across a process
//! boundary. A same-process double scan passes while the defect is live.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use rusqlite::Connection;
use serde_json::Value;
use tempfile::TempDir;

const METADATA_TABLES: &[(&str, &str)] = &[
    ("parser_inventory", "language || char(31) || parser_package"),
    ("files", "file_id"),
    ("symbols", "symbol_id"),
    ("symbol_annotations", "annotation_id"),
    ("identifiers", "identifier_id"),
    ("relationships", "relationship_id"),
    ("pending_relationships", "pending_relationship_id"),
    ("type_facts", "type_fact_id"),
    ("type_argument_usages", "usage_id"),
    ("literals", "literal_id"),
    ("source_regions", "source_region_id"),
    ("structural_facts", "structural_fact_id"),
    ("complexity_metrics", "complexity_metric_id"),
    ("parse_diagnostics", "diagnostic_id"),
];

/// Written into the scanned root beside the `resolution_contract` languages so the
/// gate covers the carrying tables that tree alone leaves empty: structural facts
/// (documents), annotations, type-argument usages, carrier literals, and a parse
/// diagnostic.
const SUPPLEMENT: &[(&str, &str)] = &[
    (
        "supplement/notes.md",
        "# Title\n\nIntro text.\n\n## Section\n\n- item one\n- item two\n",
    ),
    (
        "supplement/config.json",
        "{\"name\":\"demo\",\"version\":\"1.0.0\",\"nested\":{\"a\":1,\"b\":[1,2,3]}}\n",
    ),
    (
        "supplement/app.yaml",
        "service:\n  name: demo\n  port: 8080\n",
    ),
    (
        "supplement/decorated.py",
        "import functools\n\n\n@functools.cache\ndef compute(name: str) -> int:\n    return len(name)\n",
    ),
    (
        "supplement/client.py",
        "import requests\nimport sqlite3\n\n\ndef fetch(conn: sqlite3.Connection) -> int:\n    \
         response = requests.get(\"https://api.example.com/v1/widgets\")\n    \
         cursor = conn.execute(\"SELECT id FROM widgets WHERE name = ?\", (\"demo\",))\n    \
         return len(cursor.fetchall()) + response.status_code\n",
    ),
    (
        "supplement/Attributed.cs",
        "using System;\nusing System.Collections.Generic;\n\n[Serializable]\npublic class Widget\n{\n    \
         [Obsolete(\"use Make instead\")]\n    public static Widget Create(string name)\n    {\n        \
         var list = new List<string>();\n        list.Add(name);\n        return new Widget();\n    }\n}\n",
    ),
    ("supplement/unparsable.rs", "fn broken( {\n"),
];

type MetadataRowset = BTreeMap<String, Option<String>>;
type ArtifactMetadata = BTreeMap<String, MetadataRowset>;

#[test]
fn two_scan_processes_write_byte_identical_metadata_json() {
    let fixture = ScannedTwice::over_resolution_contract_tree();

    let first = read_metadata(&fixture.first_db);
    let second = read_metadata(&fixture.second_db);

    assert_fixture_exercises_carrying_tables(&first);
    assert_metadata_is_not_vacuous(&first);

    let report = diff_report(&first, &second);
    assert!(
        report.is_empty(),
        "metadata_json diverged between two scan processes over the same tree:\n{}",
        report.join("\n")
    );
}

#[test]
fn metadata_json_is_written_in_canonical_form() {
    let fixture = ScannedTwice::over_resolution_contract_tree();

    let deviations = read_metadata(&fixture.first_db)
        .into_iter()
        .flat_map(|(table, rows)| {
            rows.into_iter().filter_map(move |(pk, json)| {
                let json = json?;
                let canonical = canonicalize(&json)?;
                (canonical != json).then(|| {
                    format!("  {table} pk={pk}\n    stored   : {json}\n    canonical: {canonical}")
                })
            })
        })
        .take(5)
        .collect::<Vec<_>>();

    assert!(
        deviations.is_empty(),
        "metadata_json must already be its own canonical (sorted-key) serialization:\n{}",
        deviations.join("\n")
    );
}

struct ScannedTwice {
    _temp: TempDir,
    first_db: PathBuf,
    second_db: PathBuf,
}

impl ScannedTwice {
    fn over_resolution_contract_tree() -> Self {
        let temp = TempDir::new().unwrap();
        let root = temp.path().join("repo");
        std::fs::create_dir_all(&root).unwrap();
        copy_dir(&fixture_base(), &root);
        write_supplement(&root);

        let first_db = temp.path().join("first.sqlite");
        let second_db = temp.path().join("second.sqlite");
        scan(&root, &first_db);
        scan(&root, &second_db);

        Self {
            _temp: temp,
            first_db,
            second_db,
        }
    }
}

fn fixture_base() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/extraction/resolution_contract")
        .canonicalize()
        .expect("resolution_contract fixture tree exists")
}

fn write_supplement(root: &Path) {
    for (relative, contents) in SUPPLEMENT {
        let target = root.join(relative);
        std::fs::create_dir_all(target.parent().unwrap()).unwrap();
        std::fs::write(&target, contents).unwrap();
    }
}

fn copy_dir(src: &Path, dst: &Path) {
    for entry in std::fs::read_dir(src).unwrap() {
        let entry = entry.unwrap();
        let target = dst.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            std::fs::create_dir_all(&target).unwrap();
            copy_dir(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), &target).unwrap();
        }
    }
}

fn scan(root: &Path, db: &Path) {
    let output = Command::new(env!("CARGO_BIN_EXE_julie-extract"))
        .args([
            "scan",
            "--root",
            path_str(root),
            "--db",
            path_str(db),
            "--json",
        ])
        .output()
        .expect("julie-extract runs");
    assert_eq!(
        output.status.code(),
        Some(0),
        "scan into {} must succeed\nstdout:\n{}\nstderr:\n{}",
        db.display(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}

fn path_str(path: &Path) -> &str {
    path.to_str().expect("fixture paths are UTF-8")
}

fn read_metadata(db: &Path) -> ArtifactMetadata {
    let conn = Connection::open(db).unwrap();
    METADATA_TABLES
        .iter()
        .map(|(table, primary_key)| {
            let sql = format!("SELECT {primary_key}, metadata_json FROM {table}");
            let mut statement = conn
                .prepare(&sql)
                .unwrap_or_else(|error| panic!("{table} is queryable: {error}"));
            let rows = statement
                .query_map([], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?))
                })
                .unwrap()
                .collect::<Result<MetadataRowset, _>>()
                .unwrap_or_else(|error| panic!("{table} rows are readable: {error}"));
            ((*table).to_string(), rows)
        })
        .collect()
}

fn assert_fixture_exercises_carrying_tables(metadata: &ArtifactMetadata) {
    let empty = METADATA_TABLES
        .iter()
        .map(|(table, _)| *table)
        .filter(|table| metadata.get(*table).is_none_or(BTreeMap::is_empty))
        .collect::<Vec<_>>();
    assert!(
        empty.is_empty(),
        "the scanned tree must populate every metadata_json-carrying table, otherwise the \
         determinism assertion silently stops covering it; empty: {empty:?}"
    );
}

fn assert_metadata_is_not_vacuous(metadata: &ArtifactMetadata) {
    let carries_a_multi_key_object = metadata.values().flat_map(BTreeMap::values).any(|json| {
        json.as_deref()
            .and_then(object_key_count)
            .is_some_and(|keys| keys >= 2)
    });
    assert!(
        carries_a_multi_key_object,
        "no table carried a metadata_json object with 2+ keys, so an equality assertion over \
         metadata_json would pass vacuously"
    );
}

fn diff_report(first: &ArtifactMetadata, second: &ArtifactMetadata) -> Vec<String> {
    let mut report = Vec::new();
    for (table, first_rows) in first {
        let second_rows = second
            .get(table)
            .unwrap_or_else(|| panic!("{table} was read from both artifacts"));

        let only_first = first_rows
            .keys()
            .filter(|pk| !second_rows.contains_key(*pk));
        let only_second = second_rows
            .keys()
            .filter(|pk| !first_rows.contains_key(*pk));
        let differing = first_rows
            .iter()
            .filter_map(|(pk, value)| {
                let other = second_rows.get(pk)?;
                (other != value).then_some((pk, value, other))
            })
            .collect::<Vec<_>>();

        let missing = only_first.count() + only_second.count();
        if missing == 0 && differing.is_empty() {
            continue;
        }
        report.push(format!(
            "  {table}: {} row(s) present in only one artifact, {} row(s) with differing \
             metadata_json (of {} rows)",
            missing,
            differing.len(),
            first_rows.len()
        ));
        for (pk, left, right) in differing.iter().take(3) {
            report.push(format!(
                "    pk={pk}\n      first : {left:?}\n      second: {right:?}"
            ));
        }
    }
    report
}

fn object_key_count(json: &str) -> Option<usize> {
    match serde_json::from_str::<Value>(json).ok()? {
        Value::Object(map) => Some(map.len()),
        _ => None,
    }
}

/// Re-serialize through `serde_json::Value`, whose `Map` is BTreeMap-backed while
/// the `preserve_order` feature stays off, so the result has sorted keys.
fn canonicalize(json: &str) -> Option<String> {
    serde_json::to_string(&serde_json::from_str::<Value>(json).ok()?).ok()
}
