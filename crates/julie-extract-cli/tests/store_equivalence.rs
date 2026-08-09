#![cfg(feature = "test-store-contract")]

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use rusqlite::Connection;

use julie_extract_cli::store::test_support::{
    write_all_language_fixture, write_v3_extraction_oracle,
};

const FAMILY_INCREMENTAL: &str = "c095f60c-5655-47a4-8af6-c24e85b15001";
const FAMILY_FRESH: &str = "c095f60c-5655-47a4-8af6-c24e85b15002";
const ORACLE_TIME: &str = "2026-08-08T00:00:00Z";
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
        "#[derive(Debug)]\npub struct Wrapper<T> { pub value: T }\n\npub fn helper() -> i32 { 2 }\npub fn rust_value() -> String {\n    let values = Vec::<i32>::from([helper()]);\n    reqwest::get(\"https://api.example.com\");\n    format!(\"value={}\", values[0])\n}\npub fn unresolved() { external_call(); }\n",
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

    let generation_after_rust = current_generation(&incremental_store);
    let rust_versions_after_change = version_count(&incremental_store, "src/lib.rs");
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
        "request-rust-same-hash",
        "--idempotency-key",
        "idem-rust-same-hash",
    ]);
    assert_eq!(
        current_generation(&incremental_store),
        generation_after_rust
    );
    assert_eq!(
        version_count(&incremental_store, "src/lib.rs"),
        rust_versions_after_change
    );

    fs::write(
        root.join("src/App.cs"),
        "class App { int Value(int input) => input + 2; }\n",
    )
    .unwrap();
    fs::write(
        root.join("src/app.ts"),
        "export function value(input: number): number { return input + 2; }\n",
    )
    .unwrap();
    for (index, path) in ["src/App.cs", "src/app.ts"].into_iter().enumerate() {
        let request = format!("request-language-update-{index}");
        let idempotency = format!("idem-language-update-{index}");
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

    let original_json_version = current_version_id(&incremental_store, "data/config.json");
    let original_yaml_version = current_version_id(&incremental_store, "data/config.yaml");

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
    let generation_after_delete = current_generation(&incremental_store);
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
        "request-repeat-multi-delete",
        "--idempotency-key",
        "idem-repeat-multi-delete",
    ]);
    assert_eq!(
        current_generation(&incremental_store),
        generation_after_delete
    );

    fs::write(
        root.join("data/config.json"),
        r#"{"name":"reused","items":[3]}"#,
    )
    .unwrap();
    fs::write(
        root.join("data/config.yaml"),
        "name: initial\nitems:\n  - one\n",
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
    assert_ne!(
        current_version_id(&incremental_store, "data/config.json"),
        original_json_version
    );
    assert_eq!(
        current_version_id(&incremental_store, "data/config.yaml"),
        original_yaml_version
    );

    let original_markdown_version = current_version_id(&incremental_store, "docs/readme.md");
    fs::rename(root.join("docs/readme.md"), root.join("docs/guide.md")).unwrap();
    run_store(&[
        "delete",
        "--store",
        incremental_store.to_str().unwrap(),
        "--root",
        root.to_str().unwrap(),
        "--view",
        "view-main",
        "--file",
        "docs/readme.md",
        "--request-id",
        "request-rename-delete",
        "--idempotency-key",
        "idem-rename-delete",
    ]);
    run_store(&[
        "update",
        "--store",
        incremental_store.to_str().unwrap(),
        "--root",
        root.to_str().unwrap(),
        "--view",
        "view-main",
        "--file",
        "docs/guide.md",
        "--level",
        "full",
        "--request-id",
        "request-rename-update",
        "--idempotency-key",
        "idem-rename-update",
    ]);
    assert_ne!(
        current_version_id(&incremental_store, "docs/guide.md"),
        original_markdown_version
    );
    assert!(!current_manifest_contains(
        &incremental_store,
        "docs/readme.md"
    ));

    fs::remove_file(root.join("src/app.py")).unwrap();
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
        "request-import-after-disappearance",
        "--idempotency-key",
        "idem-import-after-disappearance",
    ]);
    assert!(!current_manifest_contains(&incremental_store, "src/app.py"));
    fs::write(
        root.join("src/app.py"),
        "def value(input):\n    return input + 1\n",
    )
    .unwrap();
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
        "request-import-after-return",
        "--idempotency-key",
        "idem-import-after-return",
    ]);

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

    let incremental_database = incremental_store.join("gen-001/store.db");
    let fresh_database = fresh_store.join("gen-001/store.db");
    let incremental_connection = Connection::open(&incremental_database).unwrap();
    let original_manifest_hash: String = incremental_connection
        .query_row(
            "SELECT m.manifest_hash
             FROM manifests m
             JOIN views v ON v.view_id = m.view_id AND v.current_generation = m.generation
             WHERE v.view_id = 'view-main'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    incremental_connection
        .execute(
            "UPDATE manifests
             SET manifest_hash = 'corrupt-manifest-hash'
             WHERE view_id = 'view-main'
               AND generation = (SELECT current_generation FROM views WHERE view_id = 'view-main')",
            [],
        )
        .unwrap();
    let fresh = normalized_visible_rows(&fresh_database);
    assert_ne!(
        normalized_visible_rows(&incremental_database),
        fresh,
        "equivalence must compare the current manifest hash"
    );
    incremental_connection
        .execute(
            "UPDATE manifests
             SET manifest_hash = ?1
             WHERE view_id = 'view-main'
               AND generation = (SELECT current_generation FROM views WHERE view_id = 'view-main')",
            [&original_manifest_hash],
        )
        .unwrap();

    let incremental = normalized_visible_rows(&incremental_database);
    assert!(
        incremental.contains_key("manifests"),
        "equivalence must include the current manifest hash"
    );
    assert!(
        incremental.contains_key("manifest_entries"),
        "equivalence must include current manifest-entry status, hash, and error payload"
    );
    assert_required_languages(&incremental_database);
    assert_required_languages(&fresh_database);
    assert_every_normalized_table_nonempty(&incremental);
    assert_every_normalized_table_nonempty(&fresh);
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

    assert_v3_schema_differences_are_explicit(&store.join("gen-001/store.db"), &oracle);

    let store_rows = normalized_store_rows_for_v3(&store.join("gen-001/store.db"), &oracle);
    let oracle_rows = normalized_v3_rows(&oracle, &store.join("gen-001/store.db"));
    assert_v3_has_mixed_reference_site_levels(&oracle);
    assert_v3_completion_and_manifest_bridge(&store.join("gen-001/store.db"), &oracle);
    assert_every_v3_table_nonempty(&store_rows);
    assert_every_v3_table_nonempty(&oracle_rows);
    assert!(
        !store_rows.is_empty(),
        "oracle normalizer must cover extraction tables"
    );
    assert_eq!(store_rows, oracle_rows);
}

#[test]
fn public_claim_before_effect_crash_retries_once() {
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path().join("root");
    let store = fixture.path().join("store");
    fs::create_dir(&root).unwrap();
    fs::write(
        root.join("lib.rs"),
        "pub fn value(input: i32) -> i32 { input + 1 }\n",
    )
    .unwrap();
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
        "request-claim-crash",
        "--idempotency-key",
        "idem-claim-crash",
        "--json",
    ];

    let crashed = Command::new(env!("CARGO_BIN_EXE_julie-extract"))
        .args(arguments)
        .env("JULIE_EXTRACT_STORE_TEST_CRASH_AT", "claim_before_effect")
        .output()
        .unwrap();
    assert!(!crashed.status.success());

    let database = store.join("gen-001/store.db");
    assert_database_valid(&database);
    assert_database_valid(&store.join("coord.db"));
    let store_effects: i64 = Connection::open(&database)
        .unwrap()
        .query_row("SELECT COUNT(*) FROM store_log", [], |row| row.get(0))
        .unwrap();
    assert_eq!(store_effects, 0);
    let crashed_claim: (String, String, i64) = Connection::open(store.join("coord.db"))
        .unwrap()
        .query_row(
            "SELECT r.state, r.claim_owner, wl.fencing_token
             FROM requests r JOIN writer_lease wl ON wl.resource = 'store-writer'
             WHERE r.request_id = 'request-claim-crash'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert_eq!(crashed_claim.0, "claimed");
    let takeover = Command::new(env!("CARGO_BIN_EXE_julie-extract"))
        .args(arguments)
        .env("JULIE_EXTRACT_STORE_TEST_CRASH_AT", "claim_before_effect")
        .output()
        .unwrap();
    assert!(!takeover.status.success());
    assert_database_valid(&database);
    assert_database_valid(&store.join("coord.db"));
    let takeover_claim: (String, String, i64) = Connection::open(store.join("coord.db"))
        .unwrap()
        .query_row(
            "SELECT r.state, r.claim_owner, wl.fencing_token
             FROM requests r JOIN writer_lease wl ON wl.resource = 'store-writer'
             WHERE r.request_id = 'request-claim-crash'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert_eq!(takeover_claim.0, "claimed");
    assert_ne!(takeover_claim.1, crashed_claim.1);
    assert!(takeover_claim.2 > crashed_claim.2);

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
    let effects: (i64, i64) = Connection::open(&database)
        .unwrap()
        .query_row(
            "SELECT
               (SELECT COUNT(*) FROM store_log WHERE request_id = 'request-claim-crash' AND event_kind = 'manifest_flipped'),
               (SELECT COUNT(*) FROM store_log WHERE request_id = 'request-claim-crash' AND terminal = 1)",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(effects, (1, 1));
}

#[test]
fn public_transaction_boundaries_reopen_and_retry_without_duplicates() {
    for case in [
        CrashCase::new("child_rows_before_level_stamp", CrashOperation::ImportL1),
        CrashCase::new("level_stamp_before_store_commit", CrashOperation::ImportL1),
        CrashCase::new("progress_before_store_commit", CrashOperation::ImportFull),
        CrashCase::new("progress_after_store_commit", CrashOperation::ImportFull),
        CrashCase::new("manifest_before_publish", CrashOperation::ImportL1),
        CrashCase::new(
            "manifest_after_publish_before_commit",
            CrashOperation::ImportL1,
        ),
        CrashCase::new("manifest_after_store_commit", CrashOperation::ImportFull),
        CrashCase::new("l1_only_final_before_terminal", CrashOperation::ImportL1),
        CrashCase::new("deep_after_l2_before_l3", CrashOperation::ImportFull),
        CrashCase::new("deep_after_l3_before_commit", CrashOperation::ImportFull),
        CrashCase::new(
            "nonfinal_deep_after_store_commit",
            CrashOperation::ImportFull,
        ),
        CrashCase::new("terminal_before_store_commit", CrashOperation::Delete),
        CrashCase::new("terminal_after_store_commit", CrashOperation::Delete),
        CrashCase::new("post_store_pre_coord_reconcile", CrashOperation::UpdateFull),
    ] {
        assert_public_crash_case(case);
    }
}

#[test]
fn public_import_killed_after_manifest_flip_reopens_and_reconciles_once() {
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path().join("root");
    let store = fixture.path().join("store");
    fs::create_dir(&root).unwrap();
    for index in 0..101 {
        let source = if index == 0 {
            "pub fn value_0() -> String {\n    let values = Vec::<i32>::from([0]);\n    reqwest::get(\"https://api.example.com\");\n    format!(\"{}\", values[0])\n}\n".to_string()
        } else {
            format!("pub fn value_{index}() -> usize {{ {index} }}\n")
        };
        fs::write(root.join(format!("file_{index:03}.rs")), source).unwrap();
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
    let crash_facts = request_store_facts(&database, "request-public-crash");
    assert_eq!(crash_facts.0, 0);
    assert_eq!(crash_facts.1, 1);
    assert!((2..=3).contains(&crash_facts.2));
    assert_eq!(crash_facts.3, 0);
    assert_eq!(crash_facts.4, 0);

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
    assert_request_terminal_once(
        &database,
        &store.join("coord.db"),
        "request-public-crash",
        "external_manifest_kill",
        14,
    );
    assert_real_rows(&database, "external_manifest_kill", true);
}

#[test]
fn public_import_killed_after_a_nonfinal_deep_chunk_resumes_once() {
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path().join("root");
    let store = fixture.path().join("store");
    fs::create_dir(&root).unwrap();
    for index in 0..24 {
        let source = if index == 0 {
            "pub fn value_0() -> String {\n    let values = Vec::<i32>::from([0]);\n    reqwest::get(\"https://api.example.com\");\n    format!(\"{}\", values[0])\n}\n".to_string()
        } else {
            format!("pub fn value_{index}() -> usize {{ {index} }}\n")
        };
        fs::write(root.join(format!("file_{index:02}.rs")), source).unwrap();
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
    let crash_facts = request_store_facts(&database, "request-deep-crash");
    assert_eq!(crash_facts.0, 0);
    assert_eq!(crash_facts.1, 1);
    assert!((25..47).contains(&crash_facts.2));
    assert_eq!(crash_facts.3, 0);
    assert_eq!(crash_facts.4, 0);

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
    assert_request_terminal_once(
        &database,
        &store.join("coord.db"),
        "request-deep-crash",
        "external_nonfinal_deep_kill",
        47,
    );
    assert_real_rows(&database, "external_nonfinal_deep_kill", true);
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

#[derive(Clone, Copy)]
enum CrashOperation {
    ImportL1,
    ImportFull,
    UpdateFull,
    Delete,
}

#[derive(Clone, Copy)]
struct CrashCase {
    boundary: &'static str,
    operation: CrashOperation,
}

impl CrashCase {
    const fn new(boundary: &'static str, operation: CrashOperation) -> Self {
        Self {
            boundary,
            operation,
        }
    }
}

fn assert_public_crash_case(case: CrashCase) {
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path().join("root");
    let store = fixture.path().join("store");
    fs::create_dir(&root).unwrap();
    let source = "#[derive(Debug)]\npub struct Wrapper<T> { pub value: T }\n\npub fn compute(input: i32) -> String {\n    let values = Vec::<i32>::from([input]);\n    format!(\"value={}\", values[0])\n}\n";
    fs::write(root.join("lib.rs"), source).unwrap();
    fs::write(
        root.join("other.rs"),
        "pub fn other(input: i32) -> i32 { input + 1 }\n",
    )
    .unwrap();

    if matches!(
        case.operation,
        CrashOperation::UpdateFull | CrashOperation::Delete
    ) {
        run_store(&[
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
            "--request-id",
            "request-crash-seed",
            "--idempotency-key",
            "idem-crash-seed",
        ]);
    }
    match case.operation {
        CrashOperation::UpdateFull => {
            fs::write(
                root.join("lib.rs"),
                source.replace("values[0]", "values[0] + 1"),
            )
            .unwrap();
        }
        CrashOperation::Delete => fs::remove_file(root.join("other.rs")).unwrap(),
        CrashOperation::ImportL1 | CrashOperation::ImportFull => {}
    }

    let operation = match case.operation {
        CrashOperation::ImportL1 | CrashOperation::ImportFull => "import",
        CrashOperation::UpdateFull => "update",
        CrashOperation::Delete => "delete",
    };
    let level = match case.operation {
        CrashOperation::ImportL1 => "l1",
        CrashOperation::ImportFull | CrashOperation::UpdateFull => "full",
        CrashOperation::Delete => "",
    };
    let request_id = format!("request-{}", case.boundary);
    let idempotency_key = format!("idem-{}", case.boundary);
    let mut arguments = vec![
        "store".to_string(),
        operation.to_string(),
        "--store".to_string(),
        store.to_string_lossy().into_owned(),
        "--root".to_string(),
        root.to_string_lossy().into_owned(),
        "--view".to_string(),
        "view-main".to_string(),
    ];
    if matches!(
        case.operation,
        CrashOperation::ImportL1 | CrashOperation::ImportFull
    ) {
        arguments.extend(["--family".to_string(), FAMILY_INCREMENTAL.to_string()]);
    }
    match case.operation {
        CrashOperation::ImportL1 | CrashOperation::ImportFull => {
            arguments.extend(["--level".to_string(), level.to_string()]);
        }
        CrashOperation::UpdateFull => arguments.extend([
            "--file".to_string(),
            "lib.rs".to_string(),
            "--level".to_string(),
            level.to_string(),
        ]),
        CrashOperation::Delete => {
            arguments.extend(["--file".to_string(), "other.rs".to_string()]);
        }
    }
    arguments.extend([
        "--request-id".to_string(),
        request_id.clone(),
        "--idempotency-key".to_string(),
        idempotency_key,
    ]);
    if !matches!(case.operation, CrashOperation::Delete) {
        arguments.extend(["--jobs".to_string(), "1".to_string()]);
    }
    arguments.push("--json".to_string());

    let crashed = Command::new(env!("CARGO_BIN_EXE_julie-extract"))
        .args(&arguments)
        .env("MILLER_STORE_CHUNK_VERSIONS", "0")
        .env("JULIE_EXTRACT_STORE_TEST_CRASH_AT", case.boundary)
        .output()
        .unwrap();
    assert!(
        !crashed.status.success(),
        "{} did not crash\nstdout: {}\nstderr: {}",
        case.boundary,
        String::from_utf8_lossy(&crashed.stdout),
        String::from_utf8_lossy(&crashed.stderr)
    );

    let database = store.join("gen-001/store.db");
    let coordinator = store.join("coord.db");
    assert_database_valid(&database);
    assert_database_valid(&coordinator);
    let expected_chunks = match case.operation {
        CrashOperation::ImportL1 => 1,
        CrashOperation::ImportFull => 3,
        CrashOperation::UpdateFull => 1,
        CrashOperation::Delete => 0,
    };
    assert_request_consistent_after_crash(
        &database,
        &request_id,
        case.boundary,
        expected_chunks,
        expected_crash_facts(case.boundary),
    );

    let retry = Command::new(env!("CARGO_BIN_EXE_julie-extract"))
        .args(&arguments)
        .env("MILLER_STORE_CHUNK_VERSIONS", "0")
        .output()
        .unwrap();
    assert!(
        retry.status.success(),
        "{} retry failed\nstdout: {}\nstderr: {}",
        case.boundary,
        String::from_utf8_lossy(&retry.stdout),
        String::from_utf8_lossy(&retry.stderr)
    );
    assert_database_valid(&database);
    assert_database_valid(&coordinator);
    assert_request_terminal_once(
        &database,
        &coordinator,
        &request_id,
        case.boundary,
        expected_chunks,
    );
    assert_real_rows(
        &database,
        case.boundary,
        !matches!(case.operation, CrashOperation::ImportL1),
    );
}

fn request_store_facts(database: &Path, request_id: &str) -> (i64, i64, i64, i64, i64) {
    Connection::open(database)
        .unwrap()
        .query_row(
            "SELECT
               (SELECT COUNT(*) FROM store_log WHERE request_id = ?1 AND terminal = 1),
               (SELECT COUNT(*) FROM store_log WHERE request_id = ?1 AND event_kind = 'manifest_flipped'),
               (SELECT COUNT(*) FROM request_chunks WHERE request_id = ?1),
               (SELECT COUNT(*) FROM (
                  SELECT chunk_index FROM request_chunks WHERE request_id = ?1
                  GROUP BY chunk_index HAVING COUNT(*) > 1
                )),
               (SELECT COUNT(*) FROM file_versions
                WHERE complete_l3 IS NOT NULL AND complete_l2 IS NULL
                   OR complete_l2 IS NOT NULL AND complete_l1 IS NULL)",
            [request_id],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            },
        )
        .unwrap()
}

fn assert_request_consistent_after_crash(
    database: &Path,
    request_id: &str,
    boundary: &str,
    expected_chunks: i64,
    expected: (i64, i64, i64),
) {
    let (terminal, manifest, chunks, duplicate_chunks, invalid_stamps) =
        request_store_facts(database, request_id);
    assert_eq!(terminal, expected.0, "{boundary}: terminal");
    assert_eq!(manifest, expected.1, "{boundary}: manifest");
    assert_eq!(chunks, expected.2, "{boundary}: progress");
    assert!(
        chunks <= expected_chunks,
        "{boundary}: chunks={chunks}, expected={expected_chunks}"
    );
    assert_eq!(duplicate_chunks, 0, "{boundary}");
    assert_eq!(invalid_stamps, 0, "{boundary}");
}

fn expected_crash_facts(boundary: &str) -> (i64, i64, i64) {
    match boundary {
        "child_rows_before_level_stamp"
        | "level_stamp_before_store_commit"
        | "progress_before_store_commit"
        | "terminal_before_store_commit" => (0, 0, 0),
        "progress_after_store_commit" => (0, 0, 1),
        "manifest_before_publish"
        | "manifest_after_publish_before_commit"
        | "l1_only_final_before_terminal" => (0, 0, 1),
        "manifest_after_store_commit"
        | "deep_after_l2_before_l3"
        | "deep_after_l3_before_commit" => (0, 1, 2),
        "nonfinal_deep_after_store_commit" => (0, 1, 3),
        "terminal_after_store_commit" => (1, 1, 0),
        "post_store_pre_coord_reconcile" => (1, 1, 1),
        other => panic!("missing crash fact expectation for {other}"),
    }
}

fn assert_request_terminal_once(
    database: &Path,
    coordinator: &Path,
    request_id: &str,
    boundary: &str,
    expected_chunks: i64,
) {
    let (terminal, manifest, chunks, duplicate_chunks, invalid_stamps) =
        request_store_facts(database, request_id);
    assert_eq!(terminal, 1, "{boundary}");
    assert_eq!(manifest, 1, "{boundary}");
    assert_eq!(chunks, expected_chunks, "{boundary}");
    assert_eq!(duplicate_chunks, 0, "{boundary}");
    assert_eq!(invalid_stamps, 0, "{boundary}");
    let state: String = Connection::open(coordinator)
        .unwrap()
        .query_row(
            "SELECT state FROM requests WHERE request_id = ?1",
            [request_id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(state, "committed", "{boundary}");
}

fn assert_real_rows(database: &Path, boundary: &str, expect_full: bool) {
    let counts: (i64, i64, i64) = Connection::open(database)
        .unwrap()
        .query_row(
            "SELECT
               (SELECT COUNT(*) FROM symbols)
                 + (SELECT COUNT(*) FROM relationships)
                 + (SELECT COUNT(*) FROM reference_sites WHERE level = 1),
               (SELECT COUNT(*) FROM identifiers)
                 + (SELECT COUNT(*) FROM reference_sites WHERE level = 2),
               (SELECT COUNT(*) FROM type_argument_usages)
                 + (SELECT COUNT(*) FROM type_arguments)
                 + (SELECT COUNT(*) FROM literals)
                 + (SELECT COUNT(*) FROM source_regions)
                 + (SELECT COUNT(*) FROM structural_facts)",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert!(counts.0 > 0, "{boundary}: real L1 rows {counts:?}");
    if expect_full {
        assert!(
            counts.1 > 0 && counts.2 > 0,
            "{boundary}: real deep rows {counts:?}"
        );
    }
}

fn write_multilanguage_fixture(root: &Path) {
    for directory in ["src", "data", "docs", "views"] {
        fs::create_dir_all(root.join(directory)).unwrap();
    }
    let files = [
        (
            "src/lib.rs",
            "#[derive(Debug)]\npub struct Wrapper<T> { pub value: T }\n\npub fn helper() -> i32 { 1 }\npub fn rust_value() -> String {\n    let values = Vec::<i32>::from([helper()]);\n    reqwest::get(\"https://api.example.com\");\n    format!(\"value={}\", values[0])\n}\npub fn unresolved() { external_call(); }\n",
        ),
        (
            "src/App.cs",
            "[System.Obsolete]\nclass App { int Value() => 1; }\n",
        ),
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
        ("src/broken.rs", "pub fn broken( {\n"),
    ];
    for (path, content) in files {
        fs::write(root.join(path), content).unwrap();
    }
    write_all_language_fixture(root).unwrap();
}

fn normalized_visible_rows(_database: &Path) -> BTreeMap<String, Vec<String>> {
    let connection = Connection::open(_database).unwrap();
    let mut result = BTreeMap::new();
    result.insert(
        "manifests".to_string(),
        query_rows(
            &connection,
            "SELECT m.manifest_hash
             FROM manifests m
             JOIN views v ON v.view_id = m.view_id AND v.current_generation = m.generation
             WHERE v.view_id = 'view-main'",
            1,
        ),
    );
    result.insert(
        "manifest_entries".to_string(),
        query_rows(
            &connection,
            "SELECT me.path, me.status, me.observed_content_hash,
                    me.error_class, me.error_json
             FROM manifest_entries me
             JOIN views v ON v.view_id = me.view_id AND v.current_generation = me.generation
             WHERE v.view_id = 'view-main'",
            5,
        ),
    );
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
                        , fv.complete_l1 IS NOT NULL
                        , fv.complete_l2 IS NOT NULL
                        , fv.complete_l3 IS NOT NULL
                 FROM file_versions fv
                 JOIN manifest_entries me ON me.version_id = fv.version_id
                 JOIN views v ON v.view_id = me.view_id AND v.current_generation = me.generation
                 WHERE v.view_id = 'view-main' AND me.status = 'indexed'"
            ),
            file_columns.len() + 3,
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

fn assert_v3_schema_differences_are_explicit(store_database: &Path, oracle_database: &Path) {
    let store = Connection::open(store_database).unwrap();
    let oracle = Connection::open(oracle_database).unwrap();
    for (store_table, oracle_table) in std::iter::once(("file_versions", "files"))
        .chain(CHILD_TABLES.into_iter().map(|table| (table, table)))
        .chain(GLOBAL_TABLES.into_iter().map(|table| (table, table)))
    {
        v3_normalized_columns(&store, store_table, &oracle, oracle_table);
    }
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

fn assert_every_normalized_table_nonempty(rows: &BTreeMap<String, Vec<String>>) {
    let empty = ["manifests", "manifest_entries", "file_versions"]
        .into_iter()
        .chain(CHILD_TABLES)
        .chain(GLOBAL_TABLES)
        .filter(|table| rows.get(*table).is_none_or(Vec::is_empty))
        .collect::<Vec<_>>();
    assert!(empty.is_empty(), "empty normalized tables: {empty:?}");
}

fn assert_every_v3_table_nonempty(rows: &BTreeMap<String, Vec<String>>) {
    let empty = std::iter::once("files")
        .chain(CHILD_TABLES)
        .chain(GLOBAL_TABLES)
        .filter(|table| rows.get(*table).is_none_or(Vec::is_empty))
        .collect::<Vec<_>>();
    assert!(empty.is_empty(), "empty v3-normalized tables: {empty:?}");
}

fn assert_v3_completion_and_manifest_bridge(store_database: &Path, oracle_database: &Path) {
    let store = Connection::open(store_database).unwrap();
    let oracle = Connection::open(oracle_database).unwrap();
    let invalid_store_versions: i64 = store
        .query_row(
            "SELECT COUNT(*)
             FROM file_versions fv
             JOIN manifest_entries me ON me.version_id = fv.version_id
             JOIN views v ON v.view_id = me.view_id AND v.current_generation = me.generation
             WHERE v.view_id = 'view-main'
               AND (fv.extraction_epoch != 1 OR fv.complete_l1 IS NULL
                    OR fv.complete_l2 IS NULL OR fv.complete_l3 IS NULL
                    OR me.status != 'indexed'
                    OR me.observed_content_hash != fv.content_hash
                    OR me.error_class IS NOT NULL OR me.error_json IS NOT NULL)",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(invalid_store_versions, 0);
    let oracle_revision: (i64, String, String, String) = oracle
        .query_row(
            "SELECT COUNT(*), MIN(operation), MIN(mode), MIN(completed_at)
             FROM extraction_revisions",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .unwrap();
    assert_eq!(
        oracle_revision,
        (
            1,
            "scan".to_string(),
            "force".to_string(),
            ORACLE_TIME.to_string()
        )
    );
    let store_bridge = query_rows(
        &store,
        "SELECT me.path, me.status, me.observed_content_hash
         FROM manifest_entries me
         JOIN views v ON v.view_id = me.view_id AND v.current_generation = me.generation
         WHERE v.view_id = 'view-main'",
        3,
    );
    let oracle_bridge = query_rows(
        &oracle,
        "SELECT path, status, content_hash FROM files WHERE last_revision_id = 1",
        3,
    );
    assert_eq!(store_bridge, oracle_bridge);
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

fn current_generation(store: &Path) -> i64 {
    Connection::open(store.join("gen-001/store.db"))
        .unwrap()
        .query_row(
            "SELECT current_generation FROM views WHERE view_id = 'view-main'",
            [],
            |row| row.get(0),
        )
        .unwrap()
}

fn current_version_id(store: &Path, path: &str) -> i64 {
    Connection::open(store.join("gen-001/store.db"))
        .unwrap()
        .query_row(
            "SELECT me.version_id
             FROM manifest_entries me
             JOIN views v ON v.view_id = me.view_id AND v.current_generation = me.generation
             WHERE v.view_id = 'view-main' AND me.path = ?1 AND me.status = 'indexed'",
            [path],
            |row| row.get(0),
        )
        .unwrap()
}

fn version_count(store: &Path, path: &str) -> i64 {
    Connection::open(store.join("gen-001/store.db"))
        .unwrap()
        .query_row(
            "SELECT COUNT(*) FROM file_versions WHERE path = ?1",
            [path],
            |row| row.get(0),
        )
        .unwrap()
}

fn current_manifest_contains(store: &Path, path: &str) -> bool {
    Connection::open(store.join("gen-001/store.db"))
        .unwrap()
        .query_row(
            "SELECT EXISTS(
               SELECT 1 FROM manifest_entries me
               JOIN views v ON v.view_id = me.view_id AND v.current_generation = me.generation
               WHERE v.view_id = 'view-main' AND me.path = ?1
             )",
            [path],
            |row| row.get(0),
        )
        .unwrap()
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
    let mut expected = julie_extractors::language::supported_languages()
        .iter()
        .map(|language| (*language).to_string())
        .collect::<Vec<_>>();
    expected.sort();
    assert_eq!(languages, expected);
}

fn normalized_store_rows_for_v3(
    store_database: &Path,
    oracle_database: &Path,
) -> BTreeMap<String, Vec<String>> {
    let store = Connection::open(store_database).unwrap();
    let oracle = Connection::open(oracle_database).unwrap();
    let mut result = BTreeMap::new();
    let file_columns = v3_normalized_columns(&store, "file_versions", &oracle, "files");
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
        let columns = v3_normalized_columns(&store, table, &oracle, table);
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
        let columns = v3_normalized_columns(&store, table, &oracle, table);
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
    let file_columns = v3_normalized_columns(&store, "file_versions", &oracle, "files");
    result.insert(
        "files".to_string(),
        query_rows(
            &oracle,
            &format!("SELECT {} FROM files", file_columns.join(", ")),
            file_columns.len(),
        ),
    );
    for table in CHILD_TABLES {
        let columns = v3_normalized_columns(&store, table, &oracle, table);
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
        let columns = v3_normalized_columns(&store, table, &oracle, table);
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

fn v3_normalized_columns(
    left: &Connection,
    left_table: &str,
    right: &Connection,
    right_table: &str,
) -> Vec<String> {
    let (shared_count, expected_left_only, expected_right_only) = v3_schema_spec(left_table);
    let left_columns = table_columns(left, left_table);
    let right_columns = table_columns(right, right_table);
    let left_only = left_columns
        .iter()
        .filter(|column| !right_columns.contains(column))
        .map(String::as_str)
        .collect::<Vec<_>>();
    let right_only = right_columns
        .iter()
        .filter(|column| !left_columns.contains(column))
        .map(String::as_str)
        .collect::<Vec<_>>();
    assert_eq!(left_only, expected_left_only, "{left_table} store-only");
    assert_eq!(right_only, expected_right_only, "{left_table} v3-only");
    let normalized = left_columns
        .into_iter()
        .filter(|column| !expected_left_only.contains(&column.as_str()))
        .collect::<Vec<_>>();
    let normalized_right = right_columns
        .into_iter()
        .filter(|column| !expected_right_only.contains(&column.as_str()))
        .collect::<Vec<_>>();
    assert_eq!(normalized.len(), shared_count, "{left_table} shared count");
    let mut normalized_names = normalized.clone();
    let mut normalized_right_names = normalized_right;
    normalized_names.sort();
    normalized_right_names.sort();
    assert_eq!(
        normalized_names, normalized_right_names,
        "{left_table} shared columns"
    );
    normalized
}

fn v3_schema_spec(table: &str) -> (usize, &'static [&'static str], &'static [&'static str]) {
    match table {
        "file_versions" => (
            6,
            &[
                "version_id",
                "extraction_epoch",
                "complete_l1",
                "complete_l2",
                "complete_l3",
            ],
            &["file_id", "indexed_at", "last_revision_id", "status"],
        ),
        "reference_sites" => (12, &["version_id", "level"], &["file_id"]),
        "symbols" => (29, &["version_id"], &["file_id"]),
        "identifiers" | "literals" => (16, &["version_id"], &["file_id"]),
        "relationships" => (14, &["version_id"], &["file_id"]),
        "pending_relationships" | "complexity_metrics" => (19, &["version_id"], &["file_id"]),
        "source_regions" | "parse_diagnostics" => (12, &["version_id"], &["file_id"]),
        "structural_facts" => (15, &["version_id"], &["file_id"]),
        "type_argument_usages" => (5, &["version_id"], &["file_id"]),
        "symbol_annotations" => (7, &["version_id"], &[]),
        "type_facts" => (8, &["version_id"], &[]),
        "type_arguments" => (5, &["version_id"], &[]),
        "parser_inventory" => (6, &["extraction_epoch"], &[]),
        "language_capabilities" => (15, &["extraction_epoch"], &[]),
        "language_capability_fixtures" => (4, &["extraction_epoch"], &[]),
        "language_capability_gaps" => (7, &["extraction_epoch"], &[]),
        other => panic!("missing v3 schema normalization for {other}"),
    }
}
