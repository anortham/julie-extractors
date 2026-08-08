#![cfg(feature = "test-store-contract")]

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use rusqlite::Connection;

use julie_extract_cli::store::test_support::write_v3_extraction_oracle;

const FAMILY_INCREMENTAL: &str = "c095f60c-5655-47a4-8af6-c24e85b15001";
const FAMILY_FRESH: &str = "c095f60c-5655-47a4-8af6-c24e85b15002";
const CHILD_TABLES: [&str; 14] = [
    "symbols",
    "symbol_annotations",
    "reference_sites",
    "identifiers",
    "relationships",
    "pending_relationships",
    "type_facts",
    "type_argument_usages",
    "type_arguments",
    "literals",
    "source_regions",
    "structural_facts",
    "complexity_metrics",
    "parse_diagnostics",
];
const GLOBAL_TABLES: [&str; 4] = [
    "parser_inventory",
    "language_capabilities",
    "language_capability_fixtures",
    "language_capability_gaps",
];

#[test]
fn incremental_update_delete_and_path_reuse_equal_a_fresh_full_import() {
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path().join("root");
    let incremental_store = fixture.path().join("incremental-store");
    let fresh_store = fixture.path().join("fresh-store");
    fs::create_dir(&root).unwrap();
    write_multilanguage_fixture(&root);

    run_store(&[
        "import",
        "--store",
        incremental_store.to_str().unwrap(),
        "--family",
        FAMILY_INCREMENTAL,
        "--root",
        root.to_str().unwrap(),
        "--view",
        "view-main",
        "--level",
        "full",
        "--request-id",
        "request-incremental-import",
        "--idempotency-key",
        "idem-incremental-import",
    ]);

    fs::write(
        root.join("src/lib.rs"),
        "pub fn rust_value() -> i32 { 2 }\n",
    )
    .unwrap();
    run_store(&[
        "update",
        "--store",
        incremental_store.to_str().unwrap(),
        "--root",
        root.to_str().unwrap(),
        "--view",
        "view-main",
        "--file",
        "src/lib.rs",
        "--level",
        "full",
        "--request-id",
        "request-rust-update",
        "--idempotency-key",
        "idem-rust-update",
    ]);

    fs::remove_file(root.join("data/config.json")).unwrap();
    fs::remove_file(root.join("data/config.yaml")).unwrap();
    run_store(&[
        "delete",
        "--store",
        incremental_store.to_str().unwrap(),
        "--root",
        root.to_str().unwrap(),
        "--view",
        "view-main",
        "--file",
        "data/config.json",
        "--file",
        "data/config.yaml",
        "--request-id",
        "request-multi-delete",
        "--idempotency-key",
        "idem-multi-delete",
    ]);

    fs::write(
        root.join("data/config.json"),
        r#"{"name":"reused","items":[3]}"#,
    )
    .unwrap();
    fs::write(
        root.join("data/config.yaml"),
        "name: reused\nitems:\n  - three\n",
    )
    .unwrap();
    for (index, path) in ["data/config.json", "data/config.yaml"]
        .into_iter()
        .enumerate()
    {
        let request = format!("request-path-reuse-{index}");
        let idempotency = format!("idem-path-reuse-{index}");
        run_store(&[
            "update",
            "--store",
            incremental_store.to_str().unwrap(),
            "--root",
            root.to_str().unwrap(),
            "--view",
            "view-main",
            "--file",
            path,
            "--level",
            "full",
            "--request-id",
            &request,
            "--idempotency-key",
            &idempotency,
        ]);
    }

    run_store(&[
        "import",
        "--store",
        fresh_store.to_str().unwrap(),
        "--family",
        FAMILY_FRESH,
        "--root",
        root.to_str().unwrap(),
        "--view",
        "view-main",
        "--level",
        "full",
        "--request-id",
        "request-fresh-import",
        "--idempotency-key",
        "idem-fresh-import",
    ]);

    let incremental = normalized_visible_rows(&incremental_store.join("gen-001/store.db"));
    let fresh = normalized_visible_rows(&fresh_store.join("gen-001/store.db"));
    assert_required_languages(&incremental_store.join("gen-001/store.db"));
    assert_required_languages(&fresh_store.join("gen-001/store.db"));
    assert!(!incremental.is_empty(), "normalizer must cover Ph2b tables");
    assert_eq!(incremental, fresh);
}

#[test]
fn full_store_rows_equal_the_v3_extraction_only_writer_oracle() {
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path().join("root");
    let store = fixture.path().join("store");
    let oracle = fixture.path().join("oracle.db");
    fs::create_dir(&root).unwrap();
    write_multilanguage_fixture(&root);
    run_store(&[
        "import",
        "--store",
        store.to_str().unwrap(),
        "--family",
        FAMILY_FRESH,
        "--root",
        root.to_str().unwrap(),
        "--view",
        "view-main",
        "--level",
        "full",
        "--request-id",
        "request-oracle-import",
        "--idempotency-key",
        "idem-oracle-import",
    ]);
    write_v3_extraction_oracle(&root, &oracle).unwrap();

    let store_rows = normalized_store_rows_for_v3(&store.join("gen-001/store.db"), &oracle);
    let oracle_rows = normalized_v3_rows(&oracle, &store.join("gen-001/store.db"));
    assert_v3_has_mixed_reference_site_levels(&oracle);
    assert!(
        !store_rows.is_empty(),
        "oracle normalizer must cover extraction tables"
    );
    assert_eq!(store_rows, oracle_rows);
}

#[test]
fn public_import_killed_after_manifest_flip_reopens_and_reconciles_once() {
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path().join("root");
    let store = fixture.path().join("store");
    fs::create_dir(&root).unwrap();
    for index in 0..101 {
        fs::write(
            root.join(format!("file_{index:03}.rs")),
            format!("pub fn value_{index}() -> usize {{ {index} }}\n"),
        )
        .unwrap();
    }
    let arguments = [
        "store",
        "import",
        "--store",
        store.to_str().unwrap(),
        "--family",
        FAMILY_INCREMENTAL,
        "--root",
        root.to_str().unwrap(),
        "--view",
        "view-main",
        "--level",
        "full",
        "--jobs",
        "1",
        "--request-id",
        "request-public-crash",
        "--idempotency-key",
        "idem-public-crash",
        "--json",
    ];
    let mut child = Command::new(env!("CARGO_BIN_EXE_julie-extract"))
        .args(arguments)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let database = store.join("gen-001/store.db");
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        assert!(Instant::now() < deadline, "manifest flip was not observed");
        assert!(
            child.try_wait().unwrap().is_none(),
            "import exited before the externally killed boundary"
        );
        let flipped = database.exists()
            && Connection::open(&database)
                .and_then(|connection| {
                    connection.query_row(
                        "SELECT EXISTS(SELECT 1 FROM store_log WHERE request_id = 'request-public-crash' AND event_kind = 'manifest_flipped')",
                        [],
                        |row| row.get::<_, bool>(0),
                    )
                })
                .unwrap_or(false);
        if flipped {
            break;
        }
        thread::sleep(Duration::from_millis(2));
    }
    child.kill().unwrap();
    child.wait().unwrap();
    assert_database_valid(&database);
    assert_database_valid(&store.join("coord.db"));

    let retry = Command::new(env!("CARGO_BIN_EXE_julie-extract"))
        .args(arguments)
        .output()
        .unwrap();
    assert!(
        retry.status.success(),
        "stdout: {} stderr: {}",
        String::from_utf8_lossy(&retry.stdout),
        String::from_utf8_lossy(&retry.stderr)
    );
    assert_database_valid(&database);
    assert_database_valid(&store.join("coord.db"));
    let connection = Connection::open(&database).unwrap();
    let effects: (i64, i64) = connection
        .query_row(
            "SELECT
               (SELECT COUNT(*) FROM store_log WHERE request_id = 'request-public-crash' AND event_kind = 'manifest_flipped'),
               (SELECT COUNT(*) FROM store_log WHERE request_id = 'request-public-crash' AND terminal = 1)",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(effects, (1, 1));
}

#[test]
fn public_import_killed_after_a_nonfinal_deep_chunk_resumes_once() {
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path().join("root");
    let store = fixture.path().join("store");
    fs::create_dir(&root).unwrap();
    for index in 0..24 {
        fs::write(
            root.join(format!("file_{index:02}.rs")),
            format!("pub fn value_{index}() -> usize {{ {index} }}\n"),
        )
        .unwrap();
    }
    let arguments = [
        "store",
        "import",
        "--store",
        store.to_str().unwrap(),
        "--family",
        FAMILY_INCREMENTAL,
        "--root",
        root.to_str().unwrap(),
        "--view",
        "view-main",
        "--level",
        "full",
        "--jobs",
        "1",
        "--request-id",
        "request-deep-crash",
        "--idempotency-key",
        "idem-deep-crash",
        "--json",
    ];
    let mut child = Command::new(env!("CARGO_BIN_EXE_julie-extract"))
        .env("MILLER_STORE_CHUNK_VERSIONS", "0")
        .args(arguments)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let database = store.join("gen-001/store.db");
    let deadline = Instant::now() + Duration::from_secs(30);
    let committed_deep_chunks = loop {
        assert!(
            Instant::now() < deadline,
            "nonfinal deep chunk was not observed"
        );
        assert!(
            child.try_wait().unwrap().is_none(),
            "import exited before the nonfinal deep-chunk boundary"
        );
        let state = database.exists().then(|| {
            Connection::open(&database)
                .and_then(|connection| {
                    connection.query_row(
                        "SELECT
                           (SELECT COUNT(*) FROM request_chunks WHERE request_id = 'request-deep-crash' AND level = 3),
                           (SELECT COUNT(*) FROM store_log WHERE request_id = 'request-deep-crash' AND terminal = 1)",
                        [],
                        |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)),
                    )
                })
                .unwrap_or((0, 0))
        });
        if let Some((deep_chunks, 0)) = state
            && deep_chunks > 0
        {
            break deep_chunks;
        }
        thread::sleep(Duration::from_millis(1));
    };
    child.kill().unwrap();
    child.wait().unwrap();
    assert_database_valid(&database);
    assert_database_valid(&store.join("coord.db"));
    assert!(committed_deep_chunks < 23);

    let retry = Command::new(env!("CARGO_BIN_EXE_julie-extract"))
        .env("MILLER_STORE_CHUNK_VERSIONS", "0")
        .args(arguments)
        .output()
        .unwrap();
    assert!(
        retry.status.success(),
        "stdout: {} stderr: {}",
        String::from_utf8_lossy(&retry.stdout),
        String::from_utf8_lossy(&retry.stderr)
    );
    assert_database_valid(&database);
    assert_database_valid(&store.join("coord.db"));
    let connection = Connection::open(&database).unwrap();
    let state: (i64, i64, i64, i64) = connection
        .query_row(
            "SELECT
               (SELECT COUNT(*) FROM request_chunks WHERE request_id = 'request-deep-crash' AND level = 3),
               (SELECT COUNT(*) FROM store_log WHERE request_id = 'request-deep-crash' AND event_kind = 'manifest_flipped'),
               (SELECT COUNT(*) FROM store_log WHERE request_id = 'request-deep-crash' AND terminal = 1),
               (SELECT COUNT(*) FROM file_versions WHERE complete_l3 IS NOT NULL)",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .unwrap();
    assert_eq!(state, (23, 1, 1, 24));
}

fn run_store(arguments: &[&str]) {
    let mut command = Command::new(env!("CARGO_BIN_EXE_julie-extract"));
    command.arg("store").args(arguments);
    if arguments
        .first()
        .is_some_and(|operation| *operation != "delete")
    {
        command.args(["--jobs", "1"]);
    }
    let output = command.arg("--json").output().unwrap();
    assert_eq!(
        output.status.code(),
        Some(0),
        "args: {arguments:?}\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn write_multilanguage_fixture(root: &Path) {
    for directory in ["src", "data", "docs", "views"] {
        fs::create_dir_all(root.join(directory)).unwrap();
    }
    let files = [
        (
            "src/lib.rs",
            "pub fn helper() -> i32 { 1 }\npub fn rust_value() -> i32 { helper() }\n",
        ),
        ("src/App.cs", "class App { int Value() => 1; }\n"),
        (
            "src/app.ts",
            "export function value(input: number): number { return input + 1; }\n",
        ),
        ("src/app.py", "def value(input):\n    return input + 1\n"),
        (
            "data/schema.sql",
            "CREATE TABLE items (id INTEGER PRIMARY KEY, name TEXT);\nSELECT name FROM items;\n",
        ),
        ("data/config.json", r#"{"name":"initial","items":[1,2]}"#),
        ("data/config.yaml", "name: initial\nitems:\n  - one\n"),
        ("docs/readme.md", "# Fixture\n\nMulti-language contract.\n"),
        (
            "views/Index.razor",
            "@page \"/fixture\"\n<h1>@Title</h1>\n@code { string Title => \"Fixture\"; }\n",
        ),
    ];
    for (path, content) in files {
        fs::write(root.join(path), content).unwrap();
    }
}

fn normalized_visible_rows(_database: &Path) -> BTreeMap<String, Vec<String>> {
    let connection = Connection::open(_database).unwrap();
    let mut result = BTreeMap::new();
    let file_columns = table_columns(&connection, "file_versions")
        .into_iter()
        .filter(|column| {
            !matches!(
                column.as_str(),
                "version_id" | "complete_l1" | "complete_l2" | "complete_l3"
            )
        })
        .collect::<Vec<_>>();
    let file_projection = file_columns
        .iter()
        .map(|column| format!("fv.{column}"))
        .collect::<Vec<_>>()
        .join(", ");
    result.insert(
        "file_versions".to_string(),
        query_rows(
            &connection,
            &format!(
                "SELECT {file_projection}
                 FROM file_versions fv
                 JOIN manifest_entries me ON me.version_id = fv.version_id
                 JOIN views v ON v.view_id = me.view_id AND v.current_generation = me.generation
                 WHERE v.view_id = 'view-main' AND me.status = 'indexed'"
            ),
            file_columns.len(),
        ),
    );

    for table in CHILD_TABLES {
        let columns = table_columns(&connection, table)
            .into_iter()
            .filter(|column| column != "version_id")
            .collect::<Vec<_>>();
        let projection = columns
            .iter()
            .map(|column| format!("t.{column}"))
            .collect::<Vec<_>>()
            .join(", ");
        result.insert(
            table.to_string(),
            query_rows(
                &connection,
                &format!(
                    "SELECT fv.path, {projection}
                     FROM {table} t
                     JOIN file_versions fv ON fv.version_id = t.version_id
                     JOIN manifest_entries me ON me.version_id = fv.version_id
                     JOIN views v ON v.view_id = me.view_id AND v.current_generation = me.generation
                     WHERE v.view_id = 'view-main' AND me.status = 'indexed'"
                ),
                columns.len() + 1,
            ),
        );
    }

    for table in GLOBAL_TABLES {
        let columns = table_columns(&connection, table);
        let projection = columns.join(", ");
        result.insert(
            table.to_string(),
            query_rows(
                &connection,
                &format!("SELECT {projection} FROM {table}"),
                columns.len(),
            ),
        );
    }
    result
}

fn table_columns(connection: &Connection, table: &str) -> Vec<String> {
    connection
        .prepare("SELECT name FROM pragma_table_info(?1) ORDER BY cid")
        .unwrap()
        .query_map([table], |row| row.get(0))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap()
}

fn query_rows(connection: &Connection, sql: &str, width: usize) -> Vec<String> {
    let mut rows = connection
        .prepare(sql)
        .unwrap()
        .query_map([], |row| {
            (0..width)
                .map(|index| row.get::<_, rusqlite::types::Value>(index))
                .collect::<Result<Vec<_>, _>>()
        })
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap()
        .into_iter()
        .map(|row| format!("{row:?}"))
        .collect::<Vec<_>>();
    rows.sort();
    rows
}

fn assert_database_valid(database: &Path) {
    let connection = Connection::open(database).unwrap();
    let quick: String = connection
        .query_row("PRAGMA quick_check", [], |row| row.get(0))
        .unwrap();
    assert_eq!(quick, "ok");
    let foreign_keys: i64 = connection
        .query_row("SELECT COUNT(*) FROM pragma_foreign_key_check", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(foreign_keys, 0);
}

fn assert_required_languages(database: &Path) {
    let connection = Connection::open(database).unwrap();
    let languages = connection
        .prepare(
            "SELECT DISTINCT fv.language
             FROM file_versions fv
             JOIN manifest_entries me ON me.version_id = fv.version_id
             JOIN views v ON v.view_id = me.view_id AND v.current_generation = me.generation
             WHERE v.view_id = 'view-main' AND me.status = 'indexed'
             ORDER BY fv.language",
        )
        .unwrap()
        .query_map([], |row| row.get::<_, String>(0))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    for required in [
        "csharp",
        "json",
        "markdown",
        "python",
        "razor",
        "rust",
        "sql",
        "typescript",
        "yaml",
    ] {
        assert!(
            languages.iter().any(|language| language == required),
            "{required}: {languages:?}"
        );
    }
}

fn normalized_store_rows_for_v3(
    store_database: &Path,
    oracle_database: &Path,
) -> BTreeMap<String, Vec<String>> {
    let store = Connection::open(store_database).unwrap();
    let oracle = Connection::open(oracle_database).unwrap();
    let mut result = BTreeMap::new();
    let file_columns = common_columns(&store, "file_versions", &oracle, "files");
    let projection = file_columns
        .iter()
        .map(|column| format!("fv.{column}"))
        .collect::<Vec<_>>()
        .join(", ");
    result.insert(
        "files".to_string(),
        query_rows(
            &store,
            &format!(
                "SELECT {projection}
                 FROM file_versions fv
                 JOIN manifest_entries me ON me.version_id = fv.version_id
                 JOIN views v ON v.view_id = me.view_id AND v.current_generation = me.generation
                 WHERE v.view_id = 'view-main' AND me.status = 'indexed'"
            ),
            file_columns.len(),
        ),
    );
    for table in CHILD_TABLES {
        let columns = common_columns(&store, table, &oracle, table);
        let mut projection = columns
            .iter()
            .map(|column| format!("t.{column}"))
            .collect::<Vec<_>>()
            .join(", ");
        let width = if table == "reference_sites" {
            projection.push_str(", t.level");
            columns.len() + 2
        } else {
            columns.len() + 1
        };
        result.insert(
            table.to_string(),
            query_rows(
                &store,
                &format!(
                    "SELECT fv.path, {projection}
                     FROM {table} t
                     JOIN file_versions fv ON fv.version_id = t.version_id
                     JOIN manifest_entries me ON me.version_id = fv.version_id
                     JOIN views v ON v.view_id = me.view_id AND v.current_generation = me.generation
                     WHERE v.view_id = 'view-main' AND me.status = 'indexed'"
                ),
                width,
            ),
        );
    }
    for table in GLOBAL_TABLES {
        let columns = common_columns(&store, table, &oracle, table);
        result.insert(
            table.to_string(),
            query_rows(
                &store,
                &format!("SELECT {} FROM {table}", columns.join(", ")),
                columns.len(),
            ),
        );
    }
    result
}

fn normalized_v3_rows(
    oracle_database: &Path,
    store_database: &Path,
) -> BTreeMap<String, Vec<String>> {
    let oracle = Connection::open(oracle_database).unwrap();
    let store = Connection::open(store_database).unwrap();
    let mut result = BTreeMap::new();
    let file_columns = common_columns(&store, "file_versions", &oracle, "files");
    result.insert(
        "files".to_string(),
        query_rows(
            &oracle,
            &format!("SELECT {} FROM files", file_columns.join(", ")),
            file_columns.len(),
        ),
    );
    for table in CHILD_TABLES {
        let columns = common_columns(&store, table, &oracle, table);
        let mut projection = columns
            .iter()
            .map(|column| format!("t.{column}"))
            .collect::<Vec<_>>()
            .join(", ");
        let width = if table == "reference_sites" {
            projection.push_str(
                ", CASE WHEN EXISTS (
                     SELECT 1 FROM relationships r WHERE r.reference_site_id = t.reference_site_id
                     UNION ALL
                     SELECT 1 FROM pending_relationships p WHERE p.reference_site_id = t.reference_site_id
                   ) THEN 1 ELSE 2 END",
            );
            columns.len() + 2
        } else {
            columns.len() + 1
        };
        result.insert(
            table.to_string(),
            query_rows(
                &oracle,
                &format!(
                    "SELECT f.path, {projection}
                     FROM {table} t {}",
                    v3_path_join(table)
                ),
                width,
            ),
        );
    }
    for table in GLOBAL_TABLES {
        let columns = common_columns(&store, table, &oracle, table);
        result.insert(
            table.to_string(),
            query_rows(
                &oracle,
                &format!("SELECT {} FROM {table}", columns.join(", ")),
                columns.len(),
            ),
        );
    }
    result
}

fn assert_v3_has_mixed_reference_site_levels(oracle: &Path) {
    let connection = Connection::open(oracle).unwrap();
    let levels: (i64, i64) = connection
        .query_row(
            "SELECT
               SUM(CASE WHEN EXISTS (
                 SELECT 1 FROM relationships r WHERE r.reference_site_id = rs.reference_site_id
                 UNION ALL
                 SELECT 1 FROM pending_relationships p WHERE p.reference_site_id = rs.reference_site_id
               ) THEN 1 ELSE 0 END),
               SUM(CASE WHEN NOT EXISTS (
                 SELECT 1 FROM relationships r WHERE r.reference_site_id = rs.reference_site_id
                 UNION ALL
                 SELECT 1 FROM pending_relationships p WHERE p.reference_site_id = rs.reference_site_id
               ) THEN 1 ELSE 0 END)
             FROM reference_sites rs",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert!(
        levels.0 > 0 && levels.1 > 0,
        "derived L1/L2 evidence: {levels:?}"
    );
}

fn v3_path_join(table: &str) -> &'static str {
    match table {
        "symbol_annotations" => {
            "JOIN symbols owner ON owner.symbol_id = t.symbol_id \
             JOIN files f ON f.file_id = owner.file_id"
        }
        "type_facts" => {
            "JOIN symbols owner ON owner.symbol_id = t.symbol_id \
             JOIN files f ON f.file_id = owner.file_id"
        }
        "type_arguments" => {
            "JOIN type_argument_usages owner ON owner.usage_id = t.usage_id \
             JOIN files f ON f.file_id = owner.file_id"
        }
        _ => "JOIN files f ON f.file_id = t.file_id",
    }
}

fn common_columns(
    left: &Connection,
    left_table: &str,
    right: &Connection,
    right_table: &str,
) -> Vec<String> {
    let right_columns = table_columns(right, right_table);
    table_columns(left, left_table)
        .into_iter()
        .filter(|column| right_columns.contains(column))
        .collect()
}
