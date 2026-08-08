use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

#[test]
fn public_store_import_reaches_the_production_executor() {
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path().join("root");
    let store = fixture.path().join("store");
    std::fs::create_dir(&root).unwrap();
    std::fs::write(root.join("lib.rs"), "pub fn answer() -> u32 { 42 }\n").unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_julie-extract"))
        .args([
            "store",
            "import",
            "--store",
            store.to_str().unwrap(),
            "--family",
            "9f8c2c9a-3b92-4f38-9b0d-0e2b8c7a4d11",
            "--root",
            root.to_str().unwrap(),
            "--view",
            "view-main",
            "--level",
            "l1",
            "--request-id",
            "request-public-parse",
            "--idempotency-key",
            "idem-public-parse",
            "--json",
        ])
        .output()
        .unwrap();

    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let connection = rusqlite::Connection::open(store.join("gen-001/store.db")).unwrap();
    let (path, complete_l1, complete_l2, complete_l3): (
        String,
        Option<i64>,
        Option<i64>,
        Option<i64>,
    ) = connection
        .query_row(
            "SELECT path, complete_l1, complete_l2, complete_l3 FROM file_versions",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .unwrap();
    assert_eq!(path, "lib.rs");
    assert!(complete_l1.is_some());
    assert_eq!(complete_l2, None);
    assert_eq!(complete_l3, None);
    let manifest_version: i64 = connection
        .query_row(
            "SELECT version_id FROM manifest_entries WHERE view_id = 'view-main' AND path = 'lib.rs'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(manifest_version > 0);
    let events = connection
        .prepare("SELECT event_kind FROM store_log ORDER BY sequence")
        .unwrap()
        .query_map([], |row| row.get::<_, String>(0))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(
        events,
        [
            "version_level_completed",
            "manifest_flipped",
            "store_import_completed",
        ]
    );
}

#[test]
fn full_import_publishes_l1_before_committing_l2_and_l3() {
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path().join("root");
    let store = fixture.path().join("store");
    std::fs::create_dir(&root).unwrap();
    std::fs::write(
        root.join("lib.rs"),
        "pub fn answer(input: u32) -> u32 { input + 42 }\n",
    )
    .unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_julie-extract"))
        .args([
            "store",
            "import",
            "--store",
            store.to_str().unwrap(),
            "--family",
            "9f8c2c9a-3b92-4f38-9b0d-0e2b8c7a4d11",
            "--root",
            root.to_str().unwrap(),
            "--view",
            "view-main",
            "--level",
            "full",
            "--request-id",
            "request-full",
            "--idempotency-key",
            "idem-full",
            "--json",
        ])
        .output()
        .unwrap();
    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let connection = rusqlite::Connection::open(store.join("gen-001/store.db")).unwrap();
    let stamps: (Option<i64>, Option<i64>, Option<i64>) = connection
        .query_row(
            "SELECT complete_l1, complete_l2, complete_l3 FROM file_versions",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert!(stamps.0 < stamps.1 && stamps.1 < stamps.2);
    let events = connection
        .prepare("SELECT event_kind, level FROM store_log ORDER BY sequence")
        .unwrap()
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, Option<i64>>(1)?))
        })
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    let l1 = events
        .iter()
        .position(|event| event == &("version_level_completed".to_string(), Some(1)))
        .unwrap();
    let manifest = events
        .iter()
        .position(|event| event.0 == "manifest_flipped")
        .unwrap();
    let l2 = events
        .iter()
        .position(|event| event == &("version_level_completed".to_string(), Some(2)))
        .unwrap();
    let l3 = events
        .iter()
        .position(|event| event == &("version_level_completed".to_string(), Some(3)))
        .unwrap();
    let terminal = events
        .iter()
        .position(|event| event.0 == "store_import_completed")
        .unwrap();
    assert!(l1 < manifest && manifest < l2 && l2 < l3 && l3 < terminal);
}

#[test]
fn unchanged_completed_full_import_reuses_without_extraction_or_new_level_effects() {
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path().join("root");
    let store = fixture.path().join("store");
    let progress = fixture.path().join("retry.progress");
    std::fs::create_dir(&root).unwrap();
    std::fs::write(root.join("lib.rs"), "pub fn answer() -> u32 { 42 }\n").unwrap();

    for (request_id, idempotency_key, progress_file) in [
        ("request-reuse-first", "idem-reuse-first", None),
        (
            "request-reuse-second",
            "idem-reuse-second",
            Some(progress.as_path()),
        ),
    ] {
        let mut command = Command::new(env!("CARGO_BIN_EXE_julie-extract"));
        command.args([
            "store",
            "import",
            "--store",
            store.to_str().unwrap(),
            "--family",
            "9f8c2c9a-3b92-4f38-9b0d-0e2b8c7a4d11",
            "--root",
            root.to_str().unwrap(),
            "--view",
            "view-main",
            "--level",
            "full",
            "--request-id",
            request_id,
            "--idempotency-key",
            idempotency_key,
            "--json",
        ]);
        if let Some(progress_file) = progress_file {
            command.args(["--progress-file", progress_file.to_str().unwrap()]);
        }
        let output = command.output().unwrap();
        assert_eq!(
            output.status.code(),
            Some(0),
            "stderr: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let repeated = Command::new(env!("CARGO_BIN_EXE_julie-extract"))
        .args([
            "store",
            "import",
            "--store",
            store.to_str().unwrap(),
            "--family",
            "9f8c2c9a-3b92-4f38-9b0d-0e2b8c7a4d11",
            "--root",
            root.to_str().unwrap(),
            "--view",
            "view-main",
            "--level",
            "full",
            "--request-id",
            "request-reuse-second",
            "--idempotency-key",
            "idem-reuse-second",
            "--progress-file",
            progress.to_str().unwrap(),
            "--json",
        ])
        .output()
        .unwrap();
    assert_eq!(repeated.status.code(), Some(0));

    let connection = rusqlite::Connection::open(store.join("gen-001/store.db")).unwrap();
    let version_count: i64 = connection
        .query_row("SELECT COUNT(*) FROM file_versions", [], |row| row.get(0))
        .unwrap();
    let level_effect_count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM store_log WHERE event_kind = 'version_level_completed'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(version_count, 1);
    assert_eq!(level_effect_count, 3);
    let repeated_terminal: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM store_log
             WHERE request_id = 'request-reuse-second' AND event_kind = 'store_import_completed'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(repeated_terminal, 1);

    let progress = std::fs::read_to_string(progress).unwrap();
    let final_record: serde_json::Value =
        serde_json::from_str(progress.lines().last().unwrap()).unwrap();
    assert_eq!(final_record["files_extracted"], 0);
}

#[test]
fn full_deepening_refuses_a_persisted_l1_natural_key_mismatch() {
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path().join("root");
    let store = fixture.path().join("store");
    std::fs::create_dir(&root).unwrap();
    std::fs::write(root.join("lib.rs"), "pub fn answer() -> u32 { 42 }\n").unwrap();

    let first = Command::new(env!("CARGO_BIN_EXE_julie-extract"))
        .args([
            "store",
            "import",
            "--store",
            store.to_str().unwrap(),
            "--family",
            "9f8c2c9a-3b92-4f38-9b0d-0e2b8c7a4d11",
            "--root",
            root.to_str().unwrap(),
            "--view",
            "view-main",
            "--level",
            "l1",
            "--request-id",
            "request-l1-seed",
            "--idempotency-key",
            "idem-l1-seed",
            "--json",
        ])
        .output()
        .unwrap();
    assert_eq!(first.status.code(), Some(0));

    let database = store.join("gen-001/store.db");
    let connection = rusqlite::Connection::open(&database).unwrap();
    let changed = connection
        .execute(
            "UPDATE complexity_metrics SET decision_count = decision_count + 1",
            [],
        )
        .unwrap();
    assert!(changed > 0);
    drop(connection);

    let full = Command::new(env!("CARGO_BIN_EXE_julie-extract"))
        .args([
            "store",
            "import",
            "--store",
            store.to_str().unwrap(),
            "--family",
            "9f8c2c9a-3b92-4f38-9b0d-0e2b8c7a4d11",
            "--root",
            root.to_str().unwrap(),
            "--view",
            "view-main",
            "--level",
            "full",
            "--request-id",
            "request-full-mismatch",
            "--idempotency-key",
            "idem-full-mismatch",
            "--json",
        ])
        .output()
        .unwrap();
    assert_ne!(full.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&full.stdout).contains("l1_projection_mismatch"));

    let connection = rusqlite::Connection::open(database).unwrap();
    let stamps: (Option<i64>, Option<i64>) = connection
        .query_row(
            "SELECT complete_l2, complete_l3 FROM file_versions",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(stamps, (None, None));
    let deeper_rows: i64 = connection
        .query_row(
            "SELECT
                (SELECT COUNT(*) FROM identifiers) +
                (SELECT COUNT(*) FROM reference_sites WHERE level = 2) +
                (SELECT COUNT(*) FROM type_argument_usages) +
                (SELECT COUNT(*) FROM type_arguments) +
                (SELECT COUNT(*) FROM literals) +
                (SELECT COUNT(*) FROM source_regions) +
                (SELECT COUNT(*) FROM structural_facts)",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(deeper_rows, 0);
}

#[test]
fn retry_after_l1_manifest_progress_resumes_deepening_without_republishing() {
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path().join("root");
    let store = fixture.path().join("store");
    std::fs::create_dir(&root).unwrap();
    for index in 0..101 {
        std::fs::write(
            root.join(format!("file_{index:03}.rs")),
            format!("pub fn answer_{index}() -> usize {{ {index} }}\n"),
        )
        .unwrap();
    }

    let mut child = Command::new(env!("CARGO_BIN_EXE_julie-extract"))
        .args([
            "store",
            "import",
            "--store",
            store.to_str().unwrap(),
            "--family",
            "9f8c2c9a-3b92-4f38-9b0d-0e2b8c7a4d11",
            "--root",
            root.to_str().unwrap(),
            "--view",
            "view-main",
            "--level",
            "full",
            "--request-id",
            "request-crash-resume",
            "--idempotency-key",
            "idem-crash-resume",
            "--json",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let database = store.join("gen-001/store.db");
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        assert!(
            Instant::now() < deadline,
            "manifest progress was not observed"
        );
        if database.exists()
            && rusqlite::Connection::open(&database)
                .and_then(|connection| {
                    connection.query_row(
                        "SELECT EXISTS(SELECT 1 FROM store_log WHERE event_kind = 'manifest_flipped')",
                        [],
                        |row| row.get::<_, bool>(0),
                    )
                })
                .unwrap_or(false)
        {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    child.kill().unwrap();
    child.wait().unwrap();

    let retry = Command::new(env!("CARGO_BIN_EXE_julie-extract"))
        .args([
            "store",
            "import",
            "--store",
            store.to_str().unwrap(),
            "--family",
            "9f8c2c9a-3b92-4f38-9b0d-0e2b8c7a4d11",
            "--root",
            root.to_str().unwrap(),
            "--view",
            "view-main",
            "--level",
            "full",
            "--request-id",
            "request-crash-resume",
            "--idempotency-key",
            "idem-crash-resume",
            "--json",
        ])
        .output()
        .unwrap();
    assert_eq!(
        retry.status.code(),
        Some(0),
        "stdout: {} stderr: {}",
        String::from_utf8_lossy(&retry.stdout),
        String::from_utf8_lossy(&retry.stderr)
    );

    let connection = rusqlite::Connection::open(database).unwrap();
    let completed: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM file_versions
             WHERE complete_l1 IS NOT NULL AND complete_l2 IS NOT NULL AND complete_l3 IS NOT NULL",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let l1_effects: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM store_log
             WHERE event_kind = 'version_level_completed' AND level = 1",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let manifest_effects: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM store_log WHERE event_kind = 'manifest_flipped'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let terminal_effects: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM store_log WHERE event_kind = 'store_import_completed'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let chunk_span: (i64, i64) = connection
        .query_row(
            "SELECT COUNT(*), COALESCE(MAX(chunk_index), -1) + 1
             FROM request_chunks WHERE request_id = 'request-crash-resume'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    let l1_chunks: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM request_chunks
             WHERE request_id = 'request-crash-resume' AND level = 1",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(completed, 101);
    assert_eq!(l1_effects, 101);
    assert_eq!(manifest_effects, 1);
    assert_eq!(terminal_effects, 1);
    assert_eq!(chunk_span.0, chunk_span.1);
    assert_eq!(l1_chunks, 2);
}

#[test]
fn zero_chunk_override_runs_one_version_per_quantum_with_global_indices() {
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path().join("root");
    let store = fixture.path().join("store");
    std::fs::create_dir(&root).unwrap();
    for index in 0..3 {
        std::fs::write(
            root.join(format!("file_{index}.rs")),
            format!("pub fn answer_{index}() -> usize {{ {index} }}\n"),
        )
        .unwrap();
    }
    let output = Command::new(env!("CARGO_BIN_EXE_julie-extract"))
        .env("MILLER_STORE_CHUNK_VERSIONS", "0")
        .args([
            "store",
            "import",
            "--store",
            store.to_str().unwrap(),
            "--family",
            "9f8c2c9a-3b92-4f38-9b0d-0e2b8c7a4d11",
            "--root",
            root.to_str().unwrap(),
            "--view",
            "view-main",
            "--level",
            "full",
            "--request-id",
            "request-one-version",
            "--idempotency-key",
            "idem-one-version",
            "--json",
        ])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(0));

    let connection = rusqlite::Connection::open(store.join("gen-001/store.db")).unwrap();
    let chunks = connection
        .prepare(
            "SELECT chunk_index, level FROM request_chunks
             WHERE request_id = 'request-one-version' ORDER BY chunk_index",
        )
        .unwrap()
        .query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(
        chunks,
        [
            (0, 1),
            (1, 1),
            (2, 1),
            (3, 2),
            (4, 2),
            (5, 2),
            (6, 3),
            (7, 3)
        ]
    );
}

#[test]
fn default_chunk_limit_processes_101_l1_versions_in_two_quanta() {
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path().join("root");
    let store = fixture.path().join("store");
    std::fs::create_dir(&root).unwrap();
    for index in 0..101 {
        std::fs::write(
            root.join(format!("file_{index:03}.rs")),
            format!("pub fn answer_{index}() -> usize {{ {index} }}\n"),
        )
        .unwrap();
    }
    let output = Command::new(env!("CARGO_BIN_EXE_julie-extract"))
        .args([
            "store",
            "import",
            "--store",
            store.to_str().unwrap(),
            "--family",
            "9f8c2c9a-3b92-4f38-9b0d-0e2b8c7a4d11",
            "--root",
            root.to_str().unwrap(),
            "--view",
            "view-main",
            "--level",
            "l1",
            "--request-id",
            "request-101",
            "--idempotency-key",
            "idem-101",
            "--json",
        ])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(0));
    let connection = rusqlite::Connection::open(store.join("gen-001/store.db")).unwrap();
    let progress_quanta: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM request_chunks WHERE request_id = 'request-101' AND level = 1",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let terminal: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM store_log
             WHERE request_id = 'request-101' AND event_kind = 'store_import_completed'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let versions: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM file_versions WHERE complete_l1 IS NOT NULL",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(progress_quanta, 1);
    assert_eq!(terminal, 1);
    assert_eq!(versions, 101);
}

#[test]
fn first_failed_path_has_no_version_and_prior_good_failure_is_preserved() {
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path().join("root");
    let store = fixture.path().join("store");
    std::fs::create_dir(&root).unwrap();
    std::fs::write(root.join("new.rs"), [0xff, 0xfe, 0x00]).unwrap();

    let first = Command::new(env!("CARGO_BIN_EXE_julie-extract"))
        .args([
            "store",
            "import",
            "--store",
            store.to_str().unwrap(),
            "--family",
            "9f8c2c9a-3b92-4f38-9b0d-0e2b8c7a4d11",
            "--root",
            root.to_str().unwrap(),
            "--view",
            "view-main",
            "--level",
            "l1",
            "--request-id",
            "request-new-failed",
            "--idempotency-key",
            "idem-new-failed",
            "--json",
        ])
        .output()
        .unwrap();
    assert_eq!(first.status.code(), Some(0));
    let database = store.join("gen-001/store.db");
    let connection = rusqlite::Connection::open(&database).unwrap();
    let first_entry: (String, Option<i64>) = connection
        .query_row(
            "SELECT status, version_id FROM manifest_entries WHERE path = 'new.rs'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(first_entry, ("failed".to_string(), None));
    let version_count: i64 = connection
        .query_row("SELECT COUNT(*) FROM file_versions", [], |row| row.get(0))
        .unwrap();
    assert_eq!(version_count, 0);
    drop(connection);

    std::fs::write(root.join("new.rs"), "pub fn good() -> u32 { 1 }\n").unwrap();
    let good = Command::new(env!("CARGO_BIN_EXE_julie-extract"))
        .args([
            "store",
            "import",
            "--store",
            store.to_str().unwrap(),
            "--family",
            "9f8c2c9a-3b92-4f38-9b0d-0e2b8c7a4d11",
            "--root",
            root.to_str().unwrap(),
            "--view",
            "view-main",
            "--level",
            "l1",
            "--request-id",
            "request-good",
            "--idempotency-key",
            "idem-good",
            "--json",
        ])
        .output()
        .unwrap();
    assert_eq!(good.status.code(), Some(0));
    let connection = rusqlite::Connection::open(&database).unwrap();
    let prior: (i64, i64) = connection
        .query_row(
            "SELECT version_id, complete_l1 FROM file_versions",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    drop(connection);

    std::fs::write(root.join("new.rs"), [0xff, 0xfe, 0x00]).unwrap();
    let failed = Command::new(env!("CARGO_BIN_EXE_julie-extract"))
        .args([
            "store",
            "import",
            "--store",
            store.to_str().unwrap(),
            "--family",
            "9f8c2c9a-3b92-4f38-9b0d-0e2b8c7a4d11",
            "--root",
            root.to_str().unwrap(),
            "--view",
            "view-main",
            "--level",
            "l1",
            "--request-id",
            "request-preserved",
            "--idempotency-key",
            "idem-preserved",
            "--json",
        ])
        .output()
        .unwrap();
    assert_eq!(failed.status.code(), Some(0));
    let connection = rusqlite::Connection::open(database).unwrap();
    let preserved: (String, Option<i64>) = connection
        .query_row(
            "SELECT status, version_id FROM manifest_entries
             WHERE view_id = 'view-main' ORDER BY generation DESC LIMIT 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    let retained_stamp: i64 = connection
        .query_row(
            "SELECT complete_l1 FROM file_versions WHERE version_id = ?1",
            [prior.0],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(preserved, ("failed_preserved".to_string(), Some(prior.0)));
    assert_eq!(retained_stamp, prior.1);
}

#[test]
fn source_change_between_waves_keeps_published_l1_and_requires_a_new_request() {
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path().join("root");
    let store = fixture.path().join("store");
    let ready = fixture.path().join("l1.ready");
    let resume = fixture.path().join("l1.resume");
    std::fs::create_dir(&root).unwrap();
    std::fs::write(root.join("lib.rs"), "pub fn answer() -> u32 { 1 }\n").unwrap();

    let child = Command::new(env!("CARGO_BIN_EXE_julie-extract"))
        .env("JULIE_EXTRACT_STORE_TEST_L1_READY_FILE", &ready)
        .env("JULIE_EXTRACT_STORE_TEST_L1_RESUME_FILE", &resume)
        .args([
            "store",
            "import",
            "--store",
            store.to_str().unwrap(),
            "--family",
            "9f8c2c9a-3b92-4f38-9b0d-0e2b8c7a4d11",
            "--root",
            root.to_str().unwrap(),
            "--view",
            "view-main",
            "--level",
            "full",
            "--request-id",
            "request-changing",
            "--idempotency-key",
            "idem-changing",
            "--json",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(30);
    while !ready.exists() {
        assert!(Instant::now() < deadline, "L1 hook was not reached");
        std::thread::sleep(Duration::from_millis(2));
    }
    std::fs::write(root.join("lib.rs"), "pub fn answer() -> u32 { 2 }\n").unwrap();
    std::fs::write(&resume, b"resume").unwrap();
    let output = child.wait_with_output().unwrap();
    assert_eq!(output.status.code(), Some(1));
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["failure_class"], "changed_between_waves");

    let database = store.join("gen-001/store.db");
    let connection = rusqlite::Connection::open(&database).unwrap();
    let published: (i64, String, Option<i64>, Option<i64>) = connection
        .query_row(
            "SELECT fv.version_id, fv.content_hash, fv.complete_l2, fv.complete_l3
             FROM views v JOIN manifest_entries me
               ON me.view_id = v.view_id AND me.generation = v.current_generation
             JOIN file_versions fv ON fv.version_id = me.version_id
             WHERE v.view_id = 'view-main' AND me.path = 'lib.rs'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .unwrap();
    assert_eq!((published.2, published.3), (None, None));
    drop(connection);

    let retry = Command::new(env!("CARGO_BIN_EXE_julie-extract"))
        .args([
            "store",
            "import",
            "--store",
            store.to_str().unwrap(),
            "--family",
            "9f8c2c9a-3b92-4f38-9b0d-0e2b8c7a4d11",
            "--root",
            root.to_str().unwrap(),
            "--view",
            "view-main",
            "--level",
            "full",
            "--request-id",
            "request-new-hash",
            "--idempotency-key",
            "idem-new-hash",
            "--json",
        ])
        .output()
        .unwrap();
    assert_eq!(retry.status.code(), Some(0));
    let connection = rusqlite::Connection::open(database).unwrap();
    let current_version: i64 = connection
        .query_row(
            "SELECT me.version_id FROM views v JOIN manifest_entries me
               ON me.view_id = v.view_id AND me.generation = v.current_generation
             WHERE v.view_id = 'view-main' AND me.path = 'lib.rs'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_ne!(current_version, published.0);
}

#[test]
fn extraction_epoch_change_creates_a_new_version_for_unchanged_content() {
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path().join("root");
    let store = fixture.path().join("store");
    std::fs::create_dir(&root).unwrap();
    std::fs::write(root.join("lib.rs"), "pub fn answer() -> u32 { 42 }\n").unwrap();
    let run = |request: &str, idempotency: &str| {
        Command::new(env!("CARGO_BIN_EXE_julie-extract"))
            .args([
                "store",
                "import",
                "--store",
                store.to_str().unwrap(),
                "--family",
                "9f8c2c9a-3b92-4f38-9b0d-0e2b8c7a4d11",
                "--root",
                root.to_str().unwrap(),
                "--view",
                "view-main",
                "--level",
                "l1",
                "--request-id",
                request,
                "--idempotency-key",
                idempotency,
                "--json",
            ])
            .output()
            .unwrap()
    };
    assert_eq!(
        run("request-epoch-a", "idem-epoch-a").status.code(),
        Some(0)
    );
    let database = store.join("gen-001/store.db");
    let connection = rusqlite::Connection::open(&database).unwrap();
    connection
        .execute(
            "UPDATE file_versions SET extraction_epoch = extraction_epoch + 1",
            [],
        )
        .unwrap();
    drop(connection);
    assert_eq!(
        run("request-epoch-b", "idem-epoch-b").status.code(),
        Some(0)
    );
    let connection = rusqlite::Connection::open(database).unwrap();
    let versions: i64 = connection
        .query_row("SELECT COUNT(*) FROM file_versions", [], |row| row.get(0))
        .unwrap();
    let epochs: i64 = connection
        .query_row(
            "SELECT COUNT(DISTINCT extraction_epoch) FROM file_versions",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(versions, 2);
    assert_eq!(epochs, 2);
}

#[test]
fn existing_view_refuses_a_different_root_without_republishing() {
    let fixture = tempfile::tempdir().unwrap();
    let root_a = fixture.path().join("root-a");
    let root_b = fixture.path().join("root-b");
    let store = fixture.path().join("store");
    std::fs::create_dir(&root_a).unwrap();
    std::fs::create_dir(&root_b).unwrap();
    std::fs::write(root_a.join("lib.rs"), "pub fn a() {}\n").unwrap();
    std::fs::write(root_b.join("lib.rs"), "pub fn b() {}\n").unwrap();
    let first = Command::new(env!("CARGO_BIN_EXE_julie-extract"))
        .args([
            "store",
            "import",
            "--store",
            store.to_str().unwrap(),
            "--family",
            "9f8c2c9a-3b92-4f38-9b0d-0e2b8c7a4d11",
            "--root",
            root_a.to_str().unwrap(),
            "--view",
            "view-main",
            "--level",
            "l1",
            "--request-id",
            "request-root-a",
            "--idempotency-key",
            "idem-root-a",
            "--json",
        ])
        .output()
        .unwrap();
    assert_eq!(first.status.code(), Some(0));
    let second = Command::new(env!("CARGO_BIN_EXE_julie-extract"))
        .args([
            "store",
            "import",
            "--store",
            store.to_str().unwrap(),
            "--family",
            "9f8c2c9a-3b92-4f38-9b0d-0e2b8c7a4d11",
            "--root",
            root_b.to_str().unwrap(),
            "--view",
            "view-main",
            "--level",
            "l1",
            "--request-id",
            "request-root-b",
            "--idempotency-key",
            "idem-root-b",
            "--json",
        ])
        .output()
        .unwrap();
    assert_eq!(second.status.code(), Some(1));
    let report: serde_json::Value = serde_json::from_slice(&second.stdout).unwrap();
    assert_eq!(report["failure_class"], "view_root_mismatch");
    let connection = rusqlite::Connection::open(store.join("gen-001/store.db")).unwrap();
    let manifests: i64 = connection
        .query_row("SELECT COUNT(*) FROM manifests", [], |row| row.get(0))
        .unwrap();
    assert_eq!(manifests, 1);
}

#[test]
fn import_honors_ignore_spool_progress_jobs_and_parent_supervision_controls() {
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path().join("root");
    let store = fixture.path().join("store");
    let spool = fixture.path().join("spool");
    let progress = fixture.path().join("scan.progress");
    let ignore = fixture.path().join("extra.ignore");
    std::fs::create_dir(&root).unwrap();
    std::fs::create_dir(&spool).unwrap();
    std::fs::write(root.join("kept.rs"), "pub fn kept() {}\n").unwrap();
    std::fs::write(root.join("ignored.rs"), "pub fn ignored() {}\n").unwrap();
    std::fs::write(&ignore, "ignored.rs\n").unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_julie-extract"))
        .args([
            "store",
            "import",
            "--store",
            store.to_str().unwrap(),
            "--family",
            "9f8c2c9a-3b92-4f38-9b0d-0e2b8c7a4d11",
            "--root",
            root.to_str().unwrap(),
            "--view",
            "view-main",
            "--level",
            "l1",
            "--request-id",
            "request-controls",
            "--idempotency-key",
            "idem-controls",
            "--ignore-file",
            ignore.to_str().unwrap(),
            "--jobs",
            "1",
            "--spool-dir",
            spool.to_str().unwrap(),
            "--progress-file",
            progress.to_str().unwrap(),
            "--json",
        ])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(0));
    let connection = rusqlite::Connection::open(store.join("gen-001/store.db")).unwrap();
    let paths = connection
        .prepare("SELECT path FROM manifest_entries ORDER BY path")
        .unwrap()
        .query_map([], |row| row.get::<_, String>(0))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(paths, ["kept.rs"]);
    let progress = std::fs::read_to_string(progress).unwrap();
    let final_progress: serde_json::Value =
        serde_json::from_str(progress.lines().last().unwrap()).unwrap();
    assert_eq!(final_progress["phase"], "complete");
    assert_eq!(final_progress["files_extracted"], 1);
    assert!(std::fs::read_dir(spool).unwrap().next().is_none());

    let supervised_store = fixture.path().join("supervised-store");
    let supervised = Command::new(env!("CARGO_BIN_EXE_julie-extract"))
        .args([
            "store",
            "import",
            "--store",
            supervised_store.to_str().unwrap(),
            "--family",
            "9f8c2c9a-3b92-4f38-9b0d-0e2b8c7a4d11",
            "--root",
            root.to_str().unwrap(),
            "--view",
            "view-main",
            "--level",
            "l1",
            "--request-id",
            "request-supervised",
            "--idempotency-key",
            "idem-supervised",
            "--parent-pid",
            &u32::MAX.to_string(),
            "--json",
        ])
        .output()
        .unwrap();
    assert_eq!(supervised.status.code(), Some(1));
    let connection = rusqlite::Connection::open(supervised_store.join("gen-001/store.db")).unwrap();
    let versions: i64 = connection
        .query_row("SELECT COUNT(*) FROM file_versions", [], |row| row.get(0))
        .unwrap();
    assert_eq!(versions, 0);
}

#[test]
fn full_import_persists_two_distinct_language_parsers() {
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path().join("root");
    let store = fixture.path().join("store");
    std::fs::create_dir(&root).unwrap();
    std::fs::write(
        root.join("lib.rs"),
        "pub fn answer(input: u32) -> u32 { input + 1 }\n",
    )
    .unwrap();
    std::fs::write(
        root.join("module.py"),
        "def answer(value):\n    return value + 1\n",
    )
    .unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_julie-extract"))
        .args([
            "store",
            "import",
            "--store",
            store.to_str().unwrap(),
            "--family",
            "9f8c2c9a-3b92-4f38-9b0d-0e2b8c7a4d11",
            "--root",
            root.to_str().unwrap(),
            "--view",
            "view-main",
            "--level",
            "full",
            "--request-id",
            "request-languages",
            "--idempotency-key",
            "idem-languages",
            "--json",
        ])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(0));
    let connection = rusqlite::Connection::open(store.join("gen-001/store.db")).unwrap();
    let rows = connection
        .prepare(
            "SELECT fv.language, COUNT(DISTINCT s.symbol_id), COUNT(DISTINCT i.identifier_id)
             FROM file_versions fv
             JOIN symbols s ON s.version_id = fv.version_id
             JOIN identifiers i ON i.version_id = fv.version_id
             GROUP BY fv.language ORDER BY fv.language",
        )
        .unwrap()
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)?,
            ))
        })
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(rows.len(), 2);
    assert!(rows.iter().all(|row| row.1 > 0 && row.2 > 0));
}
