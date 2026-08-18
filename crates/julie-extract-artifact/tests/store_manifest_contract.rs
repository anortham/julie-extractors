use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Barrier};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[cfg(windows)]
use julie_extract_artifact::store::same_path_identity;
use julie_extract_artifact::store::{
    GenerationFence, MANIFEST_HASH_ALGORITHM, MANIFEST_PUBLISH_MAX_RETRIES, ManifestBuilder,
    ManifestEntry, ManifestPublishDisposition, ManifestStore, ManifestStoreError,
    StoreConnectionFactory, StoreLayout, StoreLevel, StoreLog, StoreLogEntry, StoreLogError,
    StoreWriterConnection, ViewEnsureDisposition, create_store_schema,
};
use rusqlite::{Connection, params};

const INDEXED_AT: &str = "2026-08-07T12:00:00Z";

#[test]
fn canonical_hash_is_order_independent_and_cold_publish_uses_cas() {
    let mut connection = Connection::open_in_memory().unwrap();
    create_store_schema(&connection).unwrap();
    let first_version = insert_version(&connection, "src/a.rs", "blake3:a");
    let second_version = insert_version(&connection, "src/b.rs", "blake3:b");
    let first = ManifestEntry::indexed("src/a.rs", "rust", first_version, "blake3:a", INDEXED_AT);
    let second = ManifestEntry::indexed("src/b.rs", "rust", second_version, "blake3:b", INDEXED_AT);

    let forward = ManifestBuilder::from_entries([first.clone(), second.clone()])
        .build(&connection)
        .unwrap();
    let reverse = ManifestBuilder::from_entries([second, first])
        .build(&connection)
        .unwrap();

    assert_eq!(forward.manifest_hash, reverse.manifest_hash);
    assert_eq!(
        forward
            .entries
            .iter()
            .map(|entry| entry.path.as_str())
            .collect::<Vec<_>>(),
        ["src/a.rs", "src/b.rs"]
    );

    let mut manifests = ManifestStore::new(&mut connection);
    manifests.ensure_view("view-a", "/repo").unwrap();
    let published = manifests
        .publish("view-a", None, forward.entries, "request-a")
        .unwrap();

    assert_eq!(published.generation, 1);
    assert_eq!(published.disposition, ManifestPublishDisposition::Created);
    assert!(published.effect_sequence.is_some());
}

#[test]
fn no_state_header_only_publication_does_not_read_symbols() {
    let mut connection = Connection::open_in_memory().unwrap();
    create_store_schema(&connection).unwrap();
    ManifestStore::new(&mut connection)
        .ensure_view("view-a", "/repo")
        .unwrap();
    connection.execute_batch("DROP TABLE symbols").unwrap();

    let published = ManifestStore::new(&mut connection)
        .publish(
            "view-a",
            None,
            [ManifestEntry::failed(
                "src/lib.rs",
                "rust",
                "blake3:unavailable",
                INDEXED_AT,
                "read",
                r#"{"message":"unavailable"}"#,
            )],
            "request-first",
        )
        .unwrap();

    assert_eq!(published.generation, 1);
}

#[test]
fn manifest_hash_v2_is_language_sensitive_and_language_roundtrips() {
    let mut connection = Connection::open_in_memory().unwrap();
    create_store_schema(&connection).unwrap();
    let rust = ManifestBuilder::from_entries([ManifestEntry::failed(
        "src/a.txt",
        "rust",
        "blake3:a",
        INDEXED_AT,
        "read",
        r#"{"message":"failed"}"#,
    )])
    .build(&connection)
    .unwrap();
    let python = ManifestBuilder::from_entries([ManifestEntry::failed(
        "src/a.txt",
        "python",
        "blake3:a",
        INDEXED_AT,
        "read",
        r#"{"message":"failed"}"#,
    )])
    .build(&connection)
    .unwrap();

    assert_ne!(rust.manifest_hash, python.manifest_hash);
    let mut store = ManifestStore::new(&mut connection);
    store.ensure_view("view-a", "/repo").unwrap();
    store
        .publish("view-a", None, rust.entries, "request-a")
        .unwrap();
    assert_eq!(store.entries("view-a", 1).unwrap()[0].language, "rust");
}

#[test]
fn import_create_and_update_require_are_distinct_and_identical_sets_reuse() {
    let mut connection = Connection::open_in_memory().unwrap();
    create_store_schema(&connection).unwrap();
    let version_id = insert_version(&connection, "src/lib.rs", "blake3:lib");
    let entry = ManifestEntry::indexed("src/lib.rs", "rust", version_id, "blake3:lib", INDEXED_AT);
    let hash = ManifestBuilder::from_entries([entry.clone()])
        .build(&connection)
        .unwrap()
        .manifest_hash;
    let mut manifests = ManifestStore::new(&mut connection);

    let missing = manifests.require_view("view-a", "/repo").unwrap_err();
    assert!(matches!(missing, ManifestStoreError::ViewNotFound { .. }));
    assert_eq!(missing.code(), "view_not_found");
    assert_eq!(
        manifests.ensure_view("view-a", "/repo").unwrap(),
        ViewEnsureDisposition::Created
    );
    assert_eq!(
        manifests.ensure_view("view-a", "/repo").unwrap(),
        ViewEnsureDisposition::Existing
    );
    let mismatch = manifests.require_view("view-a", "/other").unwrap_err();
    assert!(matches!(
        mismatch,
        ManifestStoreError::ViewRootMismatch { .. }
    ));
    assert_eq!(mismatch.code(), "view_root_mismatch");

    let created = manifests
        .publish("view-a", None, [entry.clone()], "request-create")
        .unwrap();
    let reused = manifests
        .publish("view-a", Some(1), [entry], "request-reuse")
        .unwrap();

    assert_eq!(created.disposition, ManifestPublishDisposition::Created);
    assert_eq!(reused.disposition, ManifestPublishDisposition::Reused);
    assert_eq!(reused.generation, 1);
    assert_eq!(reused.manifest_hash, hash);
    assert_eq!(reused.effect_sequence, None);
    assert_eq!(
        connection
            .query_row("SELECT COUNT(*) FROM manifests", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        1
    );
    assert_eq!(
        connection
            .query_row("SELECT COUNT(*) FROM store_log", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        1
    );
}

#[cfg(windows)]
#[test]
fn view_root_identity_accepts_drive_and_unc_verbatim_spellings_but_rejects_mismatches() {
    assert!(same_path_identity(r"C:\", r"\\?\C:\"));
    assert!(same_path_identity(r"C:\", r"C:\\"));
    assert!(!same_path_identity(r"C:\", r"C:"));
    assert!(!same_path_identity(r"C:\", r"D:\"));
    assert!(same_path_identity(r"C:\Repo\Source", r"\\?\c:/repo/source",));
    assert!(same_path_identity(
        r"\\Server\Share\Repo",
        r"\\?\UNC\server/share/repo",
    ));
    assert!(!same_path_identity(
        r"\\Server\Share\Repo",
        r"\\Server\OtherShare\Repo",
    ));

    let mut connection = Connection::open_in_memory().unwrap();
    create_store_schema(&connection).unwrap();
    let mut manifests = ManifestStore::new(&mut connection);
    manifests
        .ensure_view("view-drive", r"C:\Repo\Source")
        .unwrap();
    assert_eq!(
        manifests
            .ensure_view("view-drive", r"\\?\c:/repo/source")
            .unwrap(),
        ViewEnsureDisposition::Existing
    );
    let mismatch = manifests
        .require_view("view-drive", r"C:\Repo\Other")
        .unwrap_err();
    assert_eq!(mismatch.code(), "view_root_mismatch");
}

#[test]
fn publishing_an_unknown_view_reports_view_not_found_before_manifest_lookup() {
    let mut connection = Connection::open_in_memory().unwrap();
    create_store_schema(&connection).unwrap();
    let error = ManifestStore::new(&mut connection)
        .publish(
            "missing-view",
            Some(7),
            [ManifestEntry::failed(
                "src/missing.rs",
                "rust",
                "blake3:missing",
                INDEXED_AT,
                "read",
                "{}",
            )],
            "request-missing-view",
        )
        .unwrap_err();

    assert!(matches!(error, ManifestStoreError::ViewNotFound { .. }));
    assert_eq!(error.code(), "view_not_found");
}

#[test]
fn multi_delete_changes_only_the_next_entry_set_and_old_heads_remain_readable() {
    let mut connection = Connection::open_in_memory().unwrap();
    create_store_schema(&connection).unwrap();
    let a = ManifestEntry::indexed(
        "src/a.rs",
        "rust",
        insert_version(&connection, "src/a.rs", "blake3:a"),
        "blake3:a",
        INDEXED_AT,
    );
    let b = ManifestEntry::indexed(
        "src/b.rs",
        "rust",
        insert_version(&connection, "src/b.rs", "blake3:b"),
        "blake3:b",
        INDEXED_AT,
    );
    let c = ManifestEntry::indexed(
        "src/c.rs",
        "rust",
        insert_version(&connection, "src/c.rs", "blake3:c"),
        "blake3:c",
        INDEXED_AT,
    );
    let mut manifests = ManifestStore::new(&mut connection);
    manifests.ensure_view("view-a", "/repo").unwrap();

    manifests
        .publish("view-a", None, [a, b, c.clone()], "request-import")
        .unwrap();
    manifests
        .publish("view-a", Some(1), [c], "request-multi-delete")
        .unwrap();
    manifests
        .publish("view-a", Some(2), [], "request-path-delete")
        .unwrap();

    assert_eq!(manifests.current_generation("view-a").unwrap(), Some(3));
    assert_eq!(
        entry_paths(manifests.entries("view-a", 1).unwrap()),
        ["src/a.rs", "src/b.rs", "src/c.rs",]
    );
    assert_eq!(
        entry_paths(manifests.entries("view-a", 2).unwrap()),
        ["src/c.rs"]
    );
    assert!(manifests.entries("view-a", 3).unwrap().is_empty());
    assert_eq!(
        connection
            .query_row("SELECT COUNT(*) FROM file_versions", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        3
    );
    let resolution = connection
        .query_row(
            "SELECT resolution_state, resolution_base_id,
                    resolution_delta_generation, resolution_exact_at
             FROM views WHERE view_id = 'view-a'",
            [],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, Option<i64>>(2)?,
                    row.get::<_, Option<i64>>(3)?,
                ))
            },
        )
        .unwrap();
    assert_eq!(resolution, ("unbound".to_string(), None, None, None));
}

#[test]
fn store_log_enforces_chunk_order_request_matching_and_terminal_semantics() {
    let mut connection = Connection::open_in_memory().unwrap();
    create_store_schema(&connection).unwrap();
    let progress = StoreLogEntry::new("request-a", "chunk_completed", "{}", INDEXED_AT)
        .with_level(StoreLevel::L1);

    let mismatch_transaction = connection.transaction().unwrap();
    let sequence = StoreLog::append_effect(&mismatch_transaction, &progress).unwrap();
    let mismatch = StoreLog::record_progress(
        &mismatch_transaction,
        "request-b",
        0,
        sequence,
        None,
        "{}",
        INDEXED_AT,
    )
    .unwrap_err();
    assert!(matches!(mismatch, StoreLogError::RequestMismatch { .. }));
    mismatch_transaction.rollback().unwrap();

    let transaction = connection.transaction().unwrap();
    let first_progress = StoreLog::append_progress(&transaction, &progress, 0).unwrap();
    transaction.commit().unwrap();
    assert!(
        StoreLog::committed_in_fact(&connection, "request-a")
            .unwrap()
            .is_none()
    );

    let transaction = connection.transaction().unwrap();
    let out_of_order = StoreLog::append_progress(&transaction, &progress, 2).unwrap_err();
    assert!(matches!(
        out_of_order,
        StoreLogError::ChunkOutOfOrder {
            expected: 1,
            actual: 2,
            ..
        }
    ));
    transaction.rollback().unwrap();

    let transaction = connection.transaction().unwrap();
    let second_progress = StoreLog::append_progress(
        &transaction,
        &StoreLogEntry::new("request-a", "chunk_completed", "{}", INDEXED_AT)
            .with_level(StoreLevel::L2),
        1,
    )
    .unwrap();
    transaction.commit().unwrap();
    assert!(second_progress > first_progress);
    assert_eq!(
        connection
            .prepare("SELECT chunk_index, level FROM request_chunks ORDER BY chunk_index")
            .unwrap()
            .query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap(),
        [(0, 1), (1, 2)]
    );

    let terminal = StoreLogEntry::new("request-a", "request_committed", "{}", INDEXED_AT);
    let transaction = connection.transaction().unwrap();
    let terminal_sequence = StoreLog::append_terminal(&transaction, &terminal).unwrap();
    transaction.commit().unwrap();
    let committed = StoreLog::committed_in_fact(&connection, "request-a")
        .unwrap()
        .unwrap();
    assert_eq!(committed.sequence, terminal_sequence);
    assert!(committed.terminal);
    assert!(terminal_sequence > first_progress);

    let transaction = connection.transaction().unwrap();
    let after_terminal = StoreLog::append_progress(&transaction, &progress, 2).unwrap_err();
    assert!(matches!(
        after_terminal,
        StoreLogError::RequestAlreadyTerminal { .. }
    ));
    transaction.rollback().unwrap();

    let transaction = connection.transaction().unwrap();
    let duplicate = StoreLog::append_terminal(&transaction, &terminal).unwrap_err();
    assert!(matches!(
        duplicate,
        StoreLogError::TerminalAlreadyExists { .. }
    ));
    transaction.rollback().unwrap();
}

#[test]
fn store_log_autoincrement_is_the_only_unique_monotonic_allocator() {
    let mut connection = Connection::open_in_memory().unwrap();
    create_store_schema(&connection).unwrap();
    let first = append_effect(&mut connection, "request-a");
    let removed = append_effect(&mut connection, "request-b");
    connection
        .execute("DELETE FROM store_log WHERE sequence = ?1", [removed])
        .unwrap();
    let later = append_effect(&mut connection, "request-c");

    assert!(first < removed);
    assert!(removed < later);
    assert_eq!(
        connection
            .query_row(
                "SELECT COUNT(*) FROM store_meta WHERE key LIKE '%log%sequence%'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
        0
    );
}

#[test]
fn canonical_hash_uses_semantic_identity_utf8_order_and_length_delimited_errors() {
    let first = Connection::open_in_memory().unwrap();
    create_store_schema(&first).unwrap();
    let first_id = insert_version(&first, "src/z.rs", "blake3:z");
    let accent_id = insert_version(&first, "src/é.rs", "blake3:accent");
    let beta_id = insert_version(&first, "src/β.rs", "blake3:beta");
    let first_manifest = ManifestBuilder::from_entries([
        ManifestEntry::indexed("src/β.rs", "rust", beta_id, "blake3:beta", INDEXED_AT),
        ManifestEntry::indexed("src/z.rs", "rust", first_id, "blake3:z", INDEXED_AT),
        ManifestEntry::indexed("src/é.rs", "rust", accent_id, "blake3:accent", INDEXED_AT),
    ])
    .build(&first)
    .unwrap();

    let second = Connection::open_in_memory().unwrap();
    create_store_schema(&second).unwrap();
    insert_version(&second, "src/noise.rs", "blake3:noise");
    let second_z = insert_version(&second, "src/z.rs", "blake3:z");
    let second_accent = insert_version(&second, "src/é.rs", "blake3:accent");
    let second_beta = insert_version(&second, "src/β.rs", "blake3:beta");
    let second_manifest = ManifestBuilder::from_entries([
        ManifestEntry::indexed(
            "src/z.rs",
            "rust",
            second_z,
            "blake3:z",
            "2026-08-07T13:00:00Z",
        ),
        ManifestEntry::indexed(
            "src/é.rs",
            "rust",
            second_accent,
            "blake3:accent",
            "2026-08-07T13:00:00Z",
        ),
        ManifestEntry::indexed(
            "src/β.rs",
            "rust",
            second_beta,
            "blake3:beta",
            "2026-08-07T13:00:00Z",
        ),
    ])
    .build(&second)
    .unwrap();

    assert_eq!(MANIFEST_HASH_ALGORITHM, "sha256");
    assert_eq!(first_manifest.manifest_hash.len(), 64);
    assert!(
        first_manifest
            .manifest_hash
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    );
    assert_eq!(first_manifest.manifest_hash, second_manifest.manifest_hash);
    assert_eq!(
        entry_paths(first_manifest.entries),
        ["src/z.rs", "src/é.rs", "src/β.rs",]
    );

    let delimiter_left = ManifestBuilder::from_entries([ManifestEntry::failed(
        "src/fail.rs",
        "rust",
        "blake3:failed",
        INDEXED_AT,
        "a|b",
        r#"{"message":"c|d\n"}"#,
    )])
    .build(&first)
    .unwrap();
    let delimiter_right = ManifestBuilder::from_entries([ManifestEntry::failed(
        "src/fail.rs",
        "rust",
        "blake3:failed",
        INDEXED_AT,
        "a",
        r#"{"message":"b|c|d\n"}"#,
    )])
    .build(&first)
    .unwrap();
    assert_ne!(delimiter_left.manifest_hash, delimiter_right.manifest_hash);
}

#[test]
fn duplicate_paths_status_changes_and_invalid_version_coherence_are_rejected_or_hashed() {
    let connection = Connection::open_in_memory().unwrap();
    create_store_schema(&connection).unwrap();
    let version_id = insert_version(&connection, "src/a.rs", "blake3:a");
    let indexed = ManifestEntry::indexed("src/a.rs", "rust", version_id, "blake3:a", INDEXED_AT);
    assert!(matches!(
        ManifestBuilder::from_entries([ManifestEntry::indexed(
            "src/a.rs", "python", version_id, "blake3:a", INDEXED_AT,
        )])
        .build(&connection),
        Err(ManifestStoreError::VersionLanguageMismatch { .. })
    ));
    let mut missing_language = ManifestEntry::failed(
        "src/new.rs",
        "rust",
        "blake3:new",
        INDEXED_AT,
        "read",
        r#"{"message":"failed"}"#,
    );
    missing_language.language.clear();
    assert!(matches!(
        ManifestBuilder::from_entries([missing_language]).build(&connection),
        Err(ManifestStoreError::InvalidEntry { .. })
    ));
    let duplicate = ManifestBuilder::from_entries([
        indexed.clone(),
        ManifestEntry::indexed("src\\a.rs", "rust", version_id, "blake3:a", INDEXED_AT),
    ])
    .build(&connection)
    .unwrap_err();
    assert!(matches!(
        duplicate,
        ManifestStoreError::DuplicatePath { .. }
    ));

    let indexed_hash = ManifestBuilder::from_entries([indexed])
        .build(&connection)
        .unwrap()
        .manifest_hash;
    let failed_hash = ManifestBuilder::from_entries([ManifestEntry::failed_preserved(
        "src/a.rs",
        "rust",
        version_id,
        "blake3:new-observation",
        INDEXED_AT,
        "parse",
        r#"{"message":"failed"}"#,
    )])
    .build(&connection)
    .unwrap()
    .manifest_hash;
    assert_ne!(indexed_hash, failed_hash);

    let mut invalid_failed = ManifestEntry::failed(
        "src/new.rs",
        "rust",
        "blake3:new",
        INDEXED_AT,
        "read",
        r#"{"message":"failed"}"#,
    );
    invalid_failed.version_id = Some(version_id);
    assert!(matches!(
        ManifestBuilder::from_entries([invalid_failed]).build(&connection),
        Err(ManifestStoreError::InvalidEntry { .. })
    ));

    connection
        .execute(
            "INSERT INTO file_versions
             (path, content_hash, extraction_epoch, language, content_bytes)
             VALUES ('src/incomplete.rs', 'blake3:incomplete', 1, 'rust', 1)",
            [],
        )
        .unwrap();
    let incomplete_id = connection.last_insert_rowid();
    assert!(matches!(
        ManifestBuilder::from_entries([ManifestEntry::indexed(
            "src/incomplete.rs",
            "rust",
            incomplete_id,
            "blake3:incomplete",
            INDEXED_AT,
        )])
        .build(&connection),
        Err(ManifestStoreError::VersionIncomplete { .. })
    ));
}

#[test]
fn failed_statuses_and_view_local_fields_never_pollute_file_versions() {
    let mut connection = Connection::open_in_memory().unwrap();
    create_store_schema(&connection).unwrap();
    let prior = insert_version(&connection, "src/prior.rs", "blake3:prior");
    let mut manifests = ManifestStore::new(&mut connection);
    manifests.ensure_view("view-a", "/repo").unwrap();
    manifests
        .publish(
            "view-a",
            None,
            [
                ManifestEntry::failed_preserved(
                    "src/prior.rs",
                    "rust",
                    prior,
                    "blake3:observed-new",
                    INDEXED_AT,
                    "parse",
                    r#"{"message":"preserved"}"#,
                ),
                ManifestEntry::failed(
                    "src/new.rs",
                    "rust",
                    "blake3:new",
                    INDEXED_AT,
                    "read",
                    r#"{"message":"new failed"}"#,
                ),
            ],
            "request-failed",
        )
        .unwrap();
    let entries = manifests.entries("view-a", 1).unwrap();
    assert_eq!(entries[0].path, "src/new.rs");
    assert_eq!(entries[0].version_id, None);
    assert_eq!(entries[1].path, "src/prior.rs");
    assert_eq!(entries[1].version_id, Some(prior));
    let columns = connection
        .prepare("PRAGMA table_info(file_versions)")
        .unwrap()
        .query_map([], |row| row.get::<_, String>(1))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    for view_local in [
        "indexed_at",
        "last_revision_id",
        "status",
        "observed_content_hash",
        "error_class",
        "error_json",
    ] {
        assert!(!columns.iter().any(|column| column == view_local));
    }
}

#[test]
fn stale_cas_and_flip_boundary_failure_leave_no_manifest_entry_or_log_orphans() {
    let mut connection = Connection::open_in_memory().unwrap();
    create_store_schema(&connection).unwrap();
    let first = ManifestEntry::indexed(
        "src/a.rs",
        "rust",
        insert_version(&connection, "src/a.rs", "blake3:a"),
        "blake3:a",
        INDEXED_AT,
    );
    let second = ManifestEntry::indexed(
        "src/b.rs",
        "rust",
        insert_version(&connection, "src/b.rs", "blake3:b"),
        "blake3:b",
        INDEXED_AT,
    );
    {
        let mut manifests = ManifestStore::new(&mut connection);
        manifests.ensure_view("view-a", "/repo").unwrap();
        manifests
            .publish("view-a", None, [first.clone()], "request-first")
            .unwrap();
        let stale = manifests
            .publish(
                "view-a",
                None,
                [first.clone(), second.clone()],
                "request-stale",
            )
            .unwrap_err();
        assert!(matches!(
            stale,
            ManifestStoreError::GenerationMismatch {
                expected: None,
                actual: Some(1)
            }
        ));
    }
    assert_manifest_counts(&connection, 1, 1, 1);

    {
        let mut manifests = ManifestStore::new(&mut connection);
        manifests
            .publish(
                "view-a",
                Some(1),
                [first.clone(), second.clone()],
                "request-second",
            )
            .unwrap();
        let later_stale = manifests
            .publish("view-a", Some(1), [first.clone()], "request-later-stale")
            .unwrap_err();
        assert!(matches!(
            later_stale,
            ManifestStoreError::GenerationMismatch {
                expected: Some(1),
                actual: Some(2)
            }
        ));
    }
    assert_manifest_counts(&connection, 2, 3, 2);

    connection
        .execute_batch(
            "CREATE TEMP TRIGGER fail_manifest_flip
             BEFORE UPDATE OF current_generation ON views
             BEGIN
               SELECT RAISE(ABORT, 'flip boundary failure');
             END;",
        )
        .unwrap();
    {
        let mut manifests = ManifestStore::new(&mut connection);
        assert!(
            manifests
                .publish("view-a", Some(2), [], "request-boundary")
                .is_err()
        );
    }
    assert_manifest_counts(&connection, 2, 3, 2);
    assert_eq!(
        connection
            .query_row(
                "SELECT current_generation FROM views WHERE view_id = 'view-a'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
        2
    );
}

#[test]
fn concurrent_cold_imports_create_one_first_generation_with_bounded_loser_recompute() {
    let store = TestStore::new("cold-concurrency");
    let setup = store.connection();
    let version_id = insert_version(&setup, "src/lib.rs", "blake3:lib");
    drop(setup);
    let mut view_connection = store.connection();
    ManifestStore::new(&mut view_connection)
        .ensure_view("view-a", "/repo")
        .unwrap();
    drop(view_connection);
    let entry = ManifestEntry::indexed("src/lib.rs", "rust", version_id, "blake3:lib", INDEXED_AT);
    let first_connection = store.connection();
    let second_connection = store.connection();
    let (first, second) = run_gate(&store, || {
        let barrier = Arc::new(Barrier::new(3));
        let first = spawn_publish(
            first_connection,
            Arc::clone(&barrier),
            None,
            vec![entry.clone()],
            "request-cold-a",
        );
        let second = spawn_publish(
            second_connection,
            Arc::clone(&barrier),
            None,
            vec![entry],
            "request-cold-b",
        );
        barrier.wait();
        (first, second)
    });
    let first = first.join().unwrap().unwrap();
    let second = second.join().unwrap().unwrap();
    let results = [first, second];

    assert_eq!(
        results
            .iter()
            .filter(|result| result.disposition == ManifestPublishDisposition::Created)
            .count(),
        1
    );
    assert_eq!(
        results
            .iter()
            .filter(|result| result.disposition == ManifestPublishDisposition::Reused)
            .count(),
        1
    );
    assert!(results.iter().any(|result| result.retries > 0));
    assert!(
        results
            .iter()
            .all(|result| result.retries <= MANIFEST_PUBLISH_MAX_RETRIES)
    );
    let connection = store.connection();
    assert_manifest_counts(&connection, 1, 1, 1);
}

#[test]
fn concurrent_disjoint_changes_rebase_the_loser_delta_without_lost_updates() {
    let store = TestStore::new("changed-concurrency");
    let mut setup = store.connection();
    let a1 = insert_version(&setup, "src/a.rs", "blake3:a1");
    let b1 = insert_version(&setup, "src/b.rs", "blake3:b1");
    let a2 = insert_version(&setup, "src/a.rs", "blake3:a2");
    let b2 = insert_version(&setup, "src/b.rs", "blake3:b2");
    let mut manifests = ManifestStore::new(&mut setup);
    manifests.ensure_view("view-a", "/repo").unwrap();
    manifests
        .publish(
            "view-a",
            None,
            [
                ManifestEntry::indexed("src/a.rs", "rust", a1, "blake3:a1", INDEXED_AT),
                ManifestEntry::indexed("src/b.rs", "rust", b1, "blake3:b1", INDEXED_AT),
            ],
            "request-base",
        )
        .unwrap();
    drop(setup);
    let first_connection = store.connection();
    let second_connection = store.connection();
    let (first, second) = run_gate(&store, || {
        let barrier = Arc::new(Barrier::new(3));
        let first = spawn_publish(
            first_connection,
            Arc::clone(&barrier),
            Some(1),
            vec![
                ManifestEntry::indexed("src/a.rs", "rust", a2, "blake3:a2", INDEXED_AT),
                ManifestEntry::indexed("src/b.rs", "rust", b1, "blake3:b1", INDEXED_AT),
            ],
            "request-update-a",
        );
        let second = spawn_publish(
            second_connection,
            Arc::clone(&barrier),
            Some(1),
            vec![
                ManifestEntry::indexed("src/a.rs", "rust", a1, "blake3:a1", INDEXED_AT),
                ManifestEntry::indexed("src/b.rs", "rust", b2, "blake3:b2", INDEXED_AT),
            ],
            "request-update-b",
        );
        barrier.wait();
        (first, second)
    });
    let results = [
        first.join().unwrap().unwrap(),
        second.join().unwrap().unwrap(),
    ];
    assert!(results.iter().any(|result| result.retries > 0));
    assert!(
        results
            .iter()
            .all(|result| result.retries <= MANIFEST_PUBLISH_MAX_RETRIES)
    );

    let mut connection = store.connection();
    let manifests = ManifestStore::new(&mut connection);
    assert_eq!(manifests.current_generation("view-a").unwrap(), Some(3));
    let head = manifests.entries("view-a", 3).unwrap();
    assert_eq!(
        head.iter()
            .map(|entry| (entry.path.as_str(), entry.version_id))
            .collect::<Vec<_>>(),
        [("src/a.rs", Some(a2)), ("src/b.rs", Some(b2))]
    );
    assert_eq!(
        entry_paths(manifests.entries("view-a", 1).unwrap()),
        ["src/a.rs", "src/b.rs",]
    );
    assert_manifest_counts(&connection, 3, 6, 3);
}

#[test]
fn hash_reuse_is_view_scoped_and_already_missing_deletes_are_no_ops() {
    let mut connection = Connection::open_in_memory().unwrap();
    create_store_schema(&connection).unwrap();
    let version_id = insert_version(&connection, "src/lib.rs", "blake3:lib");
    let entry = ManifestEntry::indexed("src/lib.rs", "rust", version_id, "blake3:lib", INDEXED_AT);
    let mut manifests = ManifestStore::new(&mut connection);
    manifests.ensure_view("view-a", "/repo/a").unwrap();
    manifests.ensure_view("view-b", "/repo/b").unwrap();

    let first = manifests
        .publish("view-a", None, [entry.clone()], "request-first")
        .unwrap();
    let deleted = manifests
        .publish("view-a", Some(1), [], "request-delete")
        .unwrap();
    let missing_delete = manifests
        .publish("view-a", Some(2), [], "request-delete-missing")
        .unwrap();
    let restored = manifests
        .publish("view-a", Some(2), [entry.clone()], "request-restore")
        .unwrap();
    let other_view = manifests
        .publish("view-b", None, [entry], "request-other-view")
        .unwrap();

    assert_eq!(deleted.generation, 2);
    assert_eq!(missing_delete.generation, 2);
    assert_eq!(
        missing_delete.disposition,
        ManifestPublishDisposition::Reused
    );
    assert_eq!(missing_delete.effect_sequence, None);
    assert_eq!(restored.generation, 1);
    assert_eq!(restored.disposition, ManifestPublishDisposition::Reused);
    assert!(restored.effect_sequence.is_some());
    assert_eq!(other_view.generation, 1);
    assert_eq!(first.manifest_hash, restored.manifest_hash);
    assert_eq!(first.manifest_hash, other_view.manifest_hash);
    assert_eq!(
        connection
            .query_row("SELECT COUNT(*) FROM manifests", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        3
    );
    assert_eq!(
        connection
            .query_row("SELECT COUNT(*) FROM store_log", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        4
    );
}

#[test]
fn manifest_effect_is_nonterminal_and_only_a_separate_final_transaction_commits_in_fact() {
    let mut connection = Connection::open_in_memory().unwrap();
    create_store_schema(&connection).unwrap();
    let version_id = insert_version(&connection, "src/lib.rs", "blake3:lib");
    let mut manifests = ManifestStore::new(&mut connection);
    manifests.ensure_view("view-a", "/repo").unwrap();
    let published = manifests
        .publish(
            "view-a",
            None,
            [ManifestEntry::indexed(
                "src/lib.rs",
                "rust",
                version_id,
                "blake3:lib",
                INDEXED_AT,
            )],
            "request-a",
        )
        .unwrap();
    assert!(
        StoreLog::committed_in_fact(&connection, "request-a")
            .unwrap()
            .is_none()
    );
    let effect_sequence = published.effect_sequence.unwrap();
    assert!(
        !connection
            .query_row(
                "SELECT terminal FROM store_log WHERE sequence = ?1",
                [effect_sequence],
                |row| row.get::<_, bool>(0),
            )
            .unwrap()
    );

    let transaction = connection.transaction().unwrap();
    let terminal_sequence = StoreLog::append_terminal(
        &transaction,
        &StoreLogEntry::new("request-a", "request_committed", "{}", INDEXED_AT),
    )
    .unwrap();
    transaction.commit().unwrap();
    let committed = StoreLog::committed_in_fact(&connection, "request-a")
        .unwrap()
        .unwrap();
    assert_eq!(committed.sequence, terminal_sequence);
    assert!(terminal_sequence > effect_sequence);
}

#[test]
fn cross_platform_path_policy_json_canonicalization_and_generation_overflow_are_explicit() {
    let mut connection = Connection::open_in_memory().unwrap();
    create_store_schema(&connection).unwrap();
    let version_id = insert_version(&connection, "src/a.rs", "blake3:a");
    let canonical = ManifestBuilder::from_entries([ManifestEntry::indexed(
        ".\\src\\a.rs",
        "rust",
        version_id,
        "blake3:a",
        INDEXED_AT,
    )])
    .build(&connection)
    .unwrap();
    assert_eq!(canonical.entries[0].path, "src/a.rs");

    for path in [
        "/src/a.rs",
        "\\src\\a.rs",
        "\\\\server\\share\\a.rs",
        "C:\\src\\a.rs",
        "C:foo",
        "c:foo/bar",
        "Z:relative.rs",
        "src/name:part.rs",
        "src/../a.rs",
        "src/nul\0path.rs",
    ] {
        assert!(matches!(
            ManifestBuilder::from_entries([ManifestEntry::failed(
                path, "rust", "blake3:a", INDEXED_AT, "read", "{}",
            )])
            .build(&connection),
            Err(ManifestStoreError::InvalidPath { .. })
        ));
    }

    let first_error = ManifestBuilder::from_entries([ManifestEntry::failed(
        "src/fail.rs",
        "rust",
        "blake3:fail",
        INDEXED_AT,
        "read",
        r#"{"b":2,"a":1}"#,
    )])
    .build(&connection)
    .unwrap();
    let second_error = ManifestBuilder::from_entries([ManifestEntry::failed(
        "src/fail.rs",
        "rust",
        "blake3:fail",
        INDEXED_AT,
        "read",
        r#"{ "a": 1, "b": 2 }"#,
    )])
    .build(&connection)
    .unwrap();
    assert_eq!(first_error.manifest_hash, second_error.manifest_hash);
    assert_eq!(
        first_error.entries[0].error_json.as_deref(),
        Some(r#"{"a":1,"b":2}"#)
    );

    let mut manifests = ManifestStore::new(&mut connection);
    manifests.ensure_view("view-a", "/repo").unwrap();
    manifests
        .publish("view-a", None, canonical.entries.clone(), "request-first")
        .unwrap();
    let overflow = manifests
        .publish(
            "view-a",
            Some(u64::MAX),
            canonical.entries,
            "request-overflow",
        )
        .unwrap_err();
    assert!(matches!(
        overflow,
        ManifestStoreError::GenerationOutOfRange {
            generation: u64::MAX
        }
    ));
    assert_eq!(overflow.code(), "manifest_generation_out_of_range");
}

#[test]
fn allocating_after_sqlite_max_generation_is_a_typed_overflow() {
    let mut connection = Connection::open_in_memory().unwrap();
    create_store_schema(&connection).unwrap();
    let first_version = insert_version(&connection, "src/lib.rs", "blake3:first");
    let second_version = insert_version(&connection, "src/lib.rs", "blake3:second");
    let first = ManifestBuilder::from_entries([ManifestEntry::indexed(
        "src/lib.rs",
        "rust",
        first_version,
        "blake3:first",
        INDEXED_AT,
    )])
    .build(&connection)
    .unwrap();
    ManifestStore::new(&mut connection)
        .ensure_view("view-a", "/repo")
        .unwrap();
    let transaction = connection.transaction().unwrap();
    transaction
        .execute(
            "INSERT INTO manifests
             (view_id, generation, manifest_hash, request_id, created_at)
             VALUES ('view-a', ?1, ?2, 'request-seed', ?3)",
            params![i64::MAX, first.manifest_hash, INDEXED_AT],
        )
        .unwrap();
    transaction
        .execute(
            "INSERT INTO manifest_entries
             (view_id, generation, path, language, version_id, status, observed_content_hash, indexed_at)
             VALUES ('view-a', ?1, 'src/lib.rs', 'rust', ?2, 'indexed', 'blake3:first', ?3)",
            params![i64::MAX, first_version, INDEXED_AT],
        )
        .unwrap();
    transaction
        .execute(
            "UPDATE views SET current_generation = ?1 WHERE view_id = 'view-a'",
            [i64::MAX],
        )
        .unwrap();
    transaction.commit().unwrap();

    let error = ManifestStore::new(&mut connection)
        .publish(
            "view-a",
            Some(i64::MAX as u64),
            [ManifestEntry::indexed(
                "src/lib.rs",
                "rust",
                second_version,
                "blake3:second",
                INDEXED_AT,
            )],
            "request-overflow",
        )
        .unwrap_err();

    assert!(matches!(
        error,
        ManifestStoreError::GenerationOutOfRange { .. }
    ));
    assert_eq!(error.code(), "manifest_generation_out_of_range");
    assert_manifest_counts(&connection, 1, 1, 0);
}

fn assert_manifest_counts(connection: &Connection, manifests: i64, entries: i64, log: i64) {
    for (table, expected) in [
        ("manifests", manifests),
        ("manifest_entries", entries),
        ("store_log", log),
    ] {
        assert_eq!(
            connection
                .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                    row.get::<_, i64>(0)
                })
                .unwrap(),
            expected
        );
    }
}

fn append_effect(connection: &mut Connection, request_id: &str) -> i64 {
    let transaction = connection.transaction().unwrap();
    let sequence = StoreLog::append_effect(
        &transaction,
        &StoreLogEntry::new(request_id, "effect", "{}", INDEXED_AT),
    )
    .unwrap();
    transaction.commit().unwrap();
    sequence
}

fn entry_paths(entries: Vec<ManifestEntry>) -> Vec<String> {
    entries.into_iter().map(|entry| entry.path).collect()
}

fn insert_version(connection: &Connection, path: &str, content_hash: &str) -> i64 {
    connection
        .execute(
            "INSERT INTO file_versions
             (path, content_hash, extraction_epoch, language, content_bytes, complete_l1)
             VALUES (?1, ?2, 1, 'rust', 1, 1)",
            params![path, content_hash],
        )
        .unwrap();
    connection.last_insert_rowid()
}

fn spawn_publish(
    mut connection: StoreWriterConnection,
    barrier: Arc<Barrier>,
    expected_generation: Option<u64>,
    entries: Vec<ManifestEntry>,
    request_id: &'static str,
) -> std::thread::JoinHandle<
    Result<julie_extract_artifact::store::ManifestPublishResult, ManifestStoreError>,
> {
    std::thread::spawn(move || {
        connection.busy_timeout(Duration::from_secs(5)).unwrap();
        barrier.wait();
        ManifestStore::new(&mut connection).publish(
            "view-a",
            expected_generation,
            entries,
            request_id,
        )
    })
}

fn run_gate<T>(store: &TestStore, action: impl FnOnce() -> T) -> T {
    let gate = store.connection();
    gate.execute_batch("BEGIN IMMEDIATE").unwrap();
    let result = action();
    std::thread::sleep(Duration::from_millis(100));
    gate.execute_batch("COMMIT").unwrap();
    result
}

struct TestStore {
    path: PathBuf,
    layout: StoreLayout,
}

impl TestStore {
    fn new(name: &str) -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "julie-store-manifest-{name}-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir_all(&path).unwrap();
        let layout = StoreLayout::create(&path, "family-manifest", "2.30.0").unwrap();
        Connection::open(layout.coordinator_db())
            .unwrap()
            .execute(
                "INSERT INTO writer_lease
                 (resource, holder_id, holder_version, holder_pid, heartbeat_at, expires_at,
                  fencing_token)
                 VALUES ('store-writer', 'manifest-test', '2.30.0', 7, 1,
                         9223372036854775807, 41)",
                [],
            )
            .unwrap();
        Self { path, layout }
    }

    fn connection(&self) -> StoreWriterConnection {
        StoreConnectionFactory::new(self.layout.clone(), "family-manifest", "2.30.0")
            .with_generation_fence(GenerationFence::writer(
                &self.layout,
                "manifest-test",
                7,
                41,
                10,
            ))
            .open_writer()
            .unwrap()
    }
}

impl Drop for TestStore {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}
