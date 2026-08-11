#![cfg(feature = "test-store-resolution-contract")]

use julie_extract_artifact::store::{
    ManifestStore, ResolutionBaseReader, StoreConnectionFactory, StoreLayout,
};
use julie_extract_cli::resolution::{
    CandidateSymbol, EdgeOrigin, ReferenceKind, TierOutcome, TypeFact, UnresolvedEdge,
    WorkspaceCandidateIndex, resolve_one, resolve_with_candidate_lookup, run_resolution_session,
};
use julie_extract_cli::resolution_session::{
    ResolutionPassRequest, ResolutionPhase, ResolutionSession, ResolutionWorklists,
};
use julie_extract_cli::store::resolution_session::{
    StoreManifestIdentity, StoreResolutionError, StoreScratchResolutionSession,
};
use julie_extractors::SymbolKind;
use rusqlite::{Connection, params};
use serde_json::{Value, json};
use std::path::Path;
use std::process::Command;
use tempfile::TempDir;

const NOW: &str = "2026-08-08T12:00:00Z";

#[test]
fn store_resolution_source_uses_only_bounded_ports() {
    let source = include_str!("../src/store/resolution_session.rs");
    for forbidden in [
        "WorkspaceCandidateIndex",
        "IdentifierLocator",
        "CurrentResolutionOverlay",
        "HashSet<",
        "ATTACH",
    ] {
        assert!(
            !source.contains(forbidden),
            "Store adapter contains forbidden {forbidden}"
        );
    }
    assert!(source.contains("LIMIT ?"));
    assert!(source.contains("open_reader()"));
}

#[test]
fn store_resolution_rejects_windows_above_sqlite_parameter_budget() {
    let temp = TempDir::new().unwrap();
    let layout = StoreLayout::create(temp.path().join("family"), "family-a", "2.30.0").unwrap();
    let error = StoreScratchResolutionSession::new(
        StoreConnectionFactory::new(layout, "family-a", "2.30.0"),
        StoreManifestIdentity {
            family_id: "family-a".to_string(),
            view_id: "view-a".to_string(),
            generation: 1,
            manifest_hash: "manifest-a".to_string(),
        },
        temp.path().join("exact.db"),
        301,
        6,
    )
    .unwrap_err();
    assert!(matches!(
        error,
        StoreResolutionError::InvalidWindowSize {
            requested: 301,
            maximum: 300
        }
    ));
}

#[cfg(unix)]
#[test]
fn store_resolution_refuses_symlinked_scratch_parent_without_outside_file() {
    use std::os::unix::fs::symlink;

    let temp = TempDir::new().unwrap();
    let layout = StoreLayout::create(temp.path().join("family"), "family-a", "2.30.0").unwrap();
    let factory = StoreConnectionFactory::new(layout, "family-a", "2.30.0");
    let mut connection = factory.open_writer().unwrap();
    ManifestStore::new(&mut connection)
        .ensure_view("view-a", "/repo")
        .unwrap();
    let version = insert_version(&connection, "src/lib.rs", "rust", true);
    connection.execute("INSERT INTO manifests(view_id,generation,manifest_hash,request_id,created_at) VALUES ('view-a',1,'manifest-a','request-a',?1)", [NOW]).unwrap();
    connection.execute("INSERT INTO manifest_entries(view_id,generation,path,language,version_id,status,observed_content_hash,indexed_at) VALUES ('view-a',1,'src/lib.rs','rust',?1,'indexed','hash',?2)", params![version, NOW]).unwrap();
    drop(connection);

    let outside = temp.path().join("outside");
    std::fs::create_dir(&outside).unwrap();
    let redirected = temp.path().join("redirected");
    symlink(&outside, &redirected).unwrap();
    let error = StoreScratchResolutionSession::new(
        factory,
        StoreManifestIdentity {
            family_id: "family-a".to_string(),
            view_id: "view-a".to_string(),
            generation: 1,
            manifest_hash: "manifest-a".to_string(),
        },
        redirected.join("exact.db"),
        7,
        6,
    )
    .unwrap_err();

    assert!(matches!(
        error,
        StoreResolutionError::Artifact(
            julie_extract_artifact::store::ResolutionValidationError::SymlinkPath { .. }
        )
    ));
    assert!(!outside.join("exact.db.work").exists());
}

#[test]
fn manifest_windows_scope_versions_and_preserve_failed_path_facts() {
    let temp = TempDir::new().unwrap();
    let exact_path = temp.path().join("exact.db");
    let layout = StoreLayout::create(temp.path().join("family"), "family-a", "2.30.0").unwrap();
    let factory = StoreConnectionFactory::new(layout, "family-a", "2.30.0");
    let mut connection = factory.open_writer().unwrap();
    ManifestStore::new(&mut connection)
        .ensure_view("view-a", "/repo")
        .unwrap();
    let indexed = insert_version(&connection, "src/indexed.rs", "rust", true);
    let preserved = insert_version(&connection, "src/preserved.ts", "typescript", true);
    let retained = insert_version(&connection, "src/retained.rs", "rust", true);
    connection
        .execute(
            "INSERT INTO manifests(view_id,generation,manifest_hash,request_id,created_at)
             VALUES ('view-a',1,'manifest-a','request-a',?1)",
            [NOW],
        )
        .unwrap();
    for (path, language, version, status, hash, error) in [
        (
            "src/indexed.rs",
            "rust",
            Some(indexed),
            "indexed",
            "hash-indexed",
            None,
        ),
        (
            "src/preserved.ts",
            "typescript",
            Some(preserved),
            "failed_preserved",
            "hash-preserved",
            Some("parse"),
        ),
        (
            "src/failed.py",
            "python",
            None,
            "failed",
            "hash-failed",
            Some("parse"),
        ),
    ] {
        connection
            .execute(
                "INSERT INTO manifest_entries
                 (view_id,generation,path,language,version_id,status,observed_content_hash,indexed_at,error_class,error_json)
                 VALUES ('view-a',1,?1,?2,?3,?4,?5,?6,?7,?8)",
                params![path, language, version, status, hash, NOW, error, error.map(|_| "{}")],
            )
            .unwrap();
    }
    drop(connection);

    let session = StoreScratchResolutionSession::new(
        factory,
        StoreManifestIdentity {
            family_id: "family-a".to_string(),
            view_id: "view-a".to_string(),
            generation: 1,
            manifest_hash: "manifest-a".to_string(),
        },
        &exact_path,
        2,
        6,
    )
    .unwrap();
    let first = session.manifest_window(None).unwrap();
    let second = session
        .manifest_window(first.last().map(|entry| entry.path.as_str()))
        .unwrap();
    let entries = first.into_iter().chain(second).collect::<Vec<_>>();

    assert_eq!(entries.len(), 3);
    assert_eq!(entries[0].path, "src/failed.py");
    assert_eq!(entries[0].language, None);
    assert_eq!(entries[0].version_id, None);
    assert_eq!(entries[1].path, "src/indexed.rs");
    assert_eq!(entries[1].language.as_deref(), Some("rust"));
    assert_eq!(entries[2].path, "src/preserved.ts");
    assert_eq!(entries[2].language.as_deref(), Some("typescript"));
    assert!(
        !entries
            .iter()
            .any(|entry| entry.version_id == Some(retained))
    );
    assert_eq!(
        session.extraction_versions_window(None).unwrap(),
        vec![indexed, preserved]
    );
}

#[test]
fn incomplete_l2_refuses_before_creating_exact_output() {
    let temp = TempDir::new().unwrap();
    let exact_path = temp.path().join("exact.db");
    let layout = StoreLayout::create(temp.path().join("family"), "family-a", "2.30.0").unwrap();
    let factory = StoreConnectionFactory::new(layout, "family-a", "2.30.0");
    let mut connection = factory.open_writer().unwrap();
    ManifestStore::new(&mut connection)
        .ensure_view("view-a", "/repo")
        .unwrap();
    let version = insert_version(&connection, "src/lib.rs", "rust", false);
    connection.execute("INSERT INTO manifests(view_id,generation,manifest_hash,request_id,created_at) VALUES ('view-a',1,'manifest-a','request-a',?1)", [NOW]).unwrap();
    connection.execute("INSERT INTO manifest_entries(view_id,generation,path,language,version_id,status,observed_content_hash,indexed_at) VALUES ('view-a',1,'src/lib.rs','rust',?1,'indexed','hash',?2)", params![version, NOW]).unwrap();
    drop(connection);

    let error = StoreScratchResolutionSession::new(
        factory,
        StoreManifestIdentity {
            family_id: "family-a".to_string(),
            view_id: "view-a".to_string(),
            generation: 1,
            manifest_hash: "manifest-a".to_string(),
        },
        &exact_path,
        2,
        6,
    )
    .unwrap_err();

    assert_eq!(error.code(), "resolution_input_incomplete");
    assert!(!exact_path.exists());
}

#[test]
fn high_collision_store_lookup_matches_legacy_and_caps_ambiguity_evidence() {
    let temp = TempDir::new().unwrap();
    let layout = StoreLayout::create(temp.path().join("family"), "family-a", "2.30.0").unwrap();
    let factory = StoreConnectionFactory::new(layout, "family-a", "2.30.0");
    let mut connection = factory.open_writer().unwrap();
    ManifestStore::new(&mut connection)
        .ensure_view("view-a", "/repo")
        .unwrap();
    let version = insert_version(&connection, "src/lib.rs", "rust", true);
    connection.execute("INSERT INTO manifests(view_id,generation,manifest_hash,request_id,created_at) VALUES ('view-a',1,'manifest-a','request-a',?1)", [NOW]).unwrap();
    connection.execute("INSERT INTO manifest_entries(view_id,generation,path,language,version_id,status,observed_content_hash,indexed_at) VALUES ('view-a',1,'src/lib.rs','rust',?1,'indexed','hash',?2)", params![version, NOW]).unwrap();
    let transaction = connection.transaction().unwrap();
    {
        let mut insert = transaction
            .prepare(
                "INSERT INTO symbols(
               version_id,symbol_id,path,language,name,kind,
               start_line,start_column,end_line,end_column,start_byte,end_byte,
               is_test,test_container,test_lifecycle
             ) VALUES (?1,?2,'src/lib.rs','rust','collision','function',1,1,1,1,0,1,0,0,0)",
            )
            .unwrap();
        for index in 0..10_000 {
            insert
                .execute(params![version, format!("symbol-{index:05}")])
                .unwrap();
        }
    }
    transaction
        .execute(
            "INSERT INTO reference_sites(version_id,reference_site_id,path,language,
         start_line,start_column,end_line,end_column,start_byte,end_byte,is_exact,provenance,level)
         VALUES (?1,'collision-site','src/lib.rs','rust',2,1,2,10,2,11,1,'target_token',2)",
            [version],
        )
        .unwrap();
    transaction.execute(
        "INSERT INTO identifiers(version_id,identifier_id,reference_site_id,path,language,name,kind,
         start_line,start_column,end_line,end_column,start_byte,end_byte,confidence)
         VALUES (?1,'collision-use','collision-site','src/lib.rs','rust','collision','call',2,1,2,10,2,11,1.0)",
        [version],
    ).unwrap();
    transaction.commit().unwrap();
    drop(connection);

    let exact_path = temp.path().join("exact.db");
    let mut session = StoreScratchResolutionSession::new(
        factory,
        StoreManifestIdentity {
            family_id: "family-a".to_string(),
            view_id: "view-a".to_string(),
            generation: 1,
            manifest_hash: "manifest-a".to_string(),
        },
        &exact_path,
        300,
        6,
    )
    .unwrap();
    let edge = UnresolvedEdge {
        origin: EdgeOrigin::Identifier,
        kind: ReferenceKind::Call,
        language: "rust".to_string(),
        file_id: version.to_string(),
        terminal_name: "collision".to_string(),
        receiver: None,
        caller_scope_symbol_id: None,
        import_context: None,
        receiver_qualifier: None,
        source_confidence: 1.0,
    };
    let legacy = WorkspaceCandidateIndex::build(
        (0..10_000)
            .map(|index| CandidateSymbol {
                symbol_id: format!("symbol-{index:05}"),
                file_id: version.to_string(),
                language: "rust".to_string(),
                name: "collision".to_string(),
                kind: SymbolKind::Function,
                parent_symbol_id: None,
                visibility: None,
                signature: None,
                is_static: None,
            })
            .collect(),
        vec![],
        vec![],
    );

    let legacy_outcome = resolve_one(&edge, &legacy);
    let store_outcome = resolve_with_candidate_lookup(&session, &edge).unwrap();
    assert!(
        matches!(legacy_outcome, TierOutcome::Ambiguous { ref candidates, exact_count: 10_000 } if candidates.len() == 2)
    );
    assert!(
        matches!(store_outcome, TierOutcome::Ambiguous { ref candidates, exact_count: 10_000 } if candidates.len() == 2)
    );
    assert!(session.max_store_read_page() <= 300);
    assert_eq!(session.max_store_read_page(), 2);
    run_resolution_session(&mut session, true, true).unwrap();
    session.finish_exact().unwrap();
    let rows = ResolutionBaseReader::open(&exact_path)
        .unwrap()
        .identifiers()
        .unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].candidates, Some(10_000));
}

#[test]
fn third_same_name_receiver_contributes_type_fact_in_legacy_and_store() {
    let temp = TempDir::new().unwrap();
    let layout = StoreLayout::create(temp.path().join("family"), "family-a", "2.30.0").unwrap();
    let factory = StoreConnectionFactory::new(layout, "family-a", "2.30.0");
    let mut connection = factory.open_writer().unwrap();
    ManifestStore::new(&mut connection)
        .ensure_view("view-a", "/repo")
        .unwrap();
    let version = insert_version(&connection, "src/lib.rs", "rust", true);
    connection.execute("INSERT INTO manifests(view_id,generation,manifest_hash,request_id,created_at) VALUES ('view-a',1,'manifest-a','request-a',?1)", [NOW]).unwrap();
    connection.execute("INSERT INTO manifest_entries(view_id,generation,path,language,version_id,status,observed_content_hash,indexed_at) VALUES ('view-a',1,'src/lib.rs','rust',?1,'indexed','hash',?2)", params![version, NOW]).unwrap();
    for (symbol_id, name, kind, parent) in [
        ("alpha", "recv", "variable", None),
        ("mid", "recv", "variable", None),
        ("zeta", "recv", "variable", None),
        ("type-c", "TypeC", "class", None),
        ("member", "hit", "method", Some("type-c")),
    ] {
        connection.execute(
            "INSERT INTO symbols(version_id,symbol_id,path,language,name,kind,parent_symbol_id,
             start_line,start_column,end_line,end_column,start_byte,end_byte,is_test,test_container,test_lifecycle)
             VALUES (?1,?2,'src/lib.rs','rust',?3,?4,?5,1,1,1,1,0,1,0,0,0)",
            params![version, symbol_id, name, kind, parent],
        ).unwrap();
    }
    for fact_id in ["fact-zeta-a", "fact-zeta-b"] {
        connection.execute(
            "INSERT INTO type_facts(version_id,type_fact_id,symbol_id,language,resolved_type,is_inferred)
             VALUES (?1,?2,'zeta','rust','TypeC',0)",
            params![version, fact_id],
        ).unwrap();
    }
    drop(connection);

    let session = StoreScratchResolutionSession::new(
        factory,
        StoreManifestIdentity {
            family_id: "family-a".to_string(),
            view_id: "view-a".to_string(),
            generation: 1,
            manifest_hash: "manifest-a".to_string(),
        },
        temp.path().join("exact.db"),
        2,
        6,
    )
    .unwrap();
    let edge = UnresolvedEdge {
        origin: EdgeOrigin::Identifier,
        kind: ReferenceKind::Call,
        language: "rust".to_string(),
        file_id: version.to_string(),
        terminal_name: "hit".to_string(),
        receiver: Some("recv".to_string()),
        caller_scope_symbol_id: None,
        import_context: None,
        receiver_qualifier: None,
        source_confidence: 1.0,
    };
    let legacy_symbols = [
        ("alpha", "recv", SymbolKind::Variable, None),
        ("mid", "recv", SymbolKind::Variable, None),
        ("zeta", "recv", SymbolKind::Variable, None),
        ("type-c", "TypeC", SymbolKind::Class, None),
        ("member", "hit", SymbolKind::Method, Some("type-c")),
    ]
    .into_iter()
    .map(|(symbol_id, name, kind, parent)| CandidateSymbol {
        symbol_id: symbol_id.to_string(),
        file_id: version.to_string(),
        language: "rust".to_string(),
        name: name.to_string(),
        kind,
        parent_symbol_id: parent.map(str::to_string),
        visibility: None,
        signature: None,
        is_static: None,
    })
    .collect();
    let legacy = WorkspaceCandidateIndex::build(
        legacy_symbols,
        vec![
            TypeFact {
                symbol_id: "zeta".to_string(),
                resolved_type: "TypeC".to_string(),
                is_inferred: false,
            },
            TypeFact {
                symbol_id: "zeta".to_string(),
                resolved_type: "TypeC".to_string(),
                is_inferred: false,
            },
        ],
        vec![],
    );

    for outcome in [
        resolve_one(&edge, &legacy),
        resolve_with_candidate_lookup(&session, &edge).unwrap(),
    ] {
        assert!(matches!(
            outcome,
            TierOutcome::Resolved { target_symbol_id, .. } if target_symbol_id.local_id == "member"
        ));
    }
    assert_eq!(session.max_store_read_page(), 2);
}

#[test]
fn store_phase_windows_freeze_membership_and_emit_identical_exact_bases() {
    let temp = TempDir::new().unwrap();
    let layout = StoreLayout::create(temp.path().join("family"), "family-a", "2.30.0").unwrap();
    let factory = StoreConnectionFactory::new(layout, "family-a", "2.30.0");
    let mut connection = factory.open_writer().unwrap();
    ManifestStore::new(&mut connection)
        .ensure_view("view-a", "/repo")
        .unwrap();
    let version = insert_version(&connection, "src/lib.rs", "rust", true);
    connection.execute("INSERT INTO manifests(view_id,generation,manifest_hash,request_id,created_at) VALUES ('view-a',1,'manifest-a','request-a',?1)", [NOW]).unwrap();
    connection.execute("INSERT INTO manifest_entries(view_id,generation,path,language,version_id,status,observed_content_hash,indexed_at) VALUES ('view-a',1,'src/lib.rs','rust',?1,'indexed','hash',?2)", params![version, NOW]).unwrap();
    insert_resolution_fixture(&mut connection, version, 23);
    drop(connection);

    let identity = StoreManifestIdentity {
        family_id: "family-a".to_string(),
        view_id: "view-a".to_string(),
        generation: 1,
        manifest_hash: "manifest-a".to_string(),
    };
    let mut small = StoreScratchResolutionSession::new(
        factory.clone(),
        identity.clone(),
        temp.path().join("small.db"),
        1,
        6,
    )
    .unwrap();
    assert_eq!(
        small.scratch_pragma_values().unwrap(),
        (4096, 2, "wal".to_string(), 2, 1, 1, 8000)
    );
    run_resolution_session(&mut small, true, true).unwrap();
    assert_eq!(small.max_emitted_chunk_size(), 1);
    assert_eq!(small.max_store_read_page(), 1);
    assert!(small.max_candidate_cache_entries() <= 3);
    let small_reader_opens = small.phase_reader_opens();
    assert_eq!(small_reader_opens, 26);
    let small_identity = small.finish_exact().unwrap();

    let mut large = StoreScratchResolutionSession::new(
        factory.clone(),
        identity.clone(),
        temp.path().join("large.db"),
        7,
        6,
    )
    .unwrap();
    run_resolution_session(&mut large, true, true).unwrap();
    assert!(large.max_emitted_chunk_size() <= 7);
    assert!(large.max_emitted_chunk_size() > 1);
    assert!(large.max_store_read_page() <= 7);
    assert!(large.max_candidate_cache_entries() <= 21);
    let large_reader_opens = large.phase_reader_opens();
    assert_eq!(large_reader_opens, 7);
    let large_identity = large.finish_exact().unwrap();
    assert!(small_reader_opens > large_reader_opens * 3);

    let small = ResolutionBaseReader::open(small_identity.path).unwrap();
    let large = ResolutionBaseReader::open(large_identity.path).unwrap();
    assert_eq!(small.identifiers().unwrap(), large.identifiers().unwrap());
    assert_eq!(small.pending().unwrap(), large.pending().unwrap());
    assert_eq!(small.identifiers().unwrap().len(), 23);
    assert_eq!(small.pending().unwrap().len(), 23);

    let mut corrupt =
        StoreScratchResolutionSession::new(factory, identity, temp.path().join("corrupt.db"), 7, 6)
            .unwrap();
    corrupt
        .inject_frozen_phase_key_for_test(ResolutionPhase::Pending, version, "missing-pending")
        .unwrap();
    let error = corrupt
        .next_phase_chunk(&ResolutionWorklists {
            phase: ResolutionPhase::Pending,
            ..ResolutionWorklists::default()
        })
        .unwrap_err();
    assert!(matches!(
        error,
        StoreResolutionError::PhaseHydrationMismatch {
            phase: "pending",
            expected: 1,
            actual: 0
        }
    ));
}

#[test]
fn visible_version_roots_commit_once_per_bounded_store_page() {
    let temp = TempDir::new().unwrap();
    let layout = StoreLayout::create(temp.path().join("family"), "family-a", "2.30.0").unwrap();
    let factory = StoreConnectionFactory::new(layout, "family-a", "2.30.0");
    let mut connection = factory.open_writer().unwrap();
    ManifestStore::new(&mut connection)
        .ensure_view("view-a", "/repo")
        .unwrap();
    connection.execute("INSERT INTO manifests(view_id,generation,manifest_hash,request_id,created_at) VALUES ('view-a',1,'manifest-a','request-a',?1)", [NOW]).unwrap();
    let mut versions = Vec::new();
    for index in 0..5 {
        let path = format!("src/file-{index}.rs");
        let version = insert_version(&connection, &path, "rust", true);
        connection.execute("INSERT INTO manifest_entries(view_id,generation,path,language,version_id,status,observed_content_hash,indexed_at) VALUES ('view-a',1,?1,'rust',?2,'indexed',?3,?4)", params![path, version, format!("hash-{index}"), NOW]).unwrap();
        versions.push(version);
    }
    drop(connection);

    let mut session = StoreScratchResolutionSession::new(
        factory,
        StoreManifestIdentity {
            family_id: "family-a".to_string(),
            view_id: "view-a".to_string(),
            generation: 1,
            manifest_hash: "manifest-a".to_string(),
        },
        temp.path().join("exact.db"),
        2,
        6,
    )
    .unwrap();
    session
        .open_resolution_pass(&ResolutionPassRequest::full())
        .unwrap();

    assert_eq!(session.visible_root_batches(), 3);
    assert!(session.max_store_read_page() <= 2);
    let exact = session.finish_exact().unwrap();
    assert_eq!(
        ResolutionBaseReader::open(exact.path)
            .unwrap()
            .source_versions()
            .unwrap(),
        versions
    );
}

#[test]
fn pinned_fixture_store_session_matches_legacy_semantics() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().join("repo");
    copy_fixture(&fixture_root(), &root);
    std::fs::create_dir_all(root.join("failed-preserved")).unwrap();
    std::fs::write(root.join("failed-preserved/broken.rs"), [0xff, 0xfe, 0xfd]).unwrap();
    let legacy_db = temp.path().join("legacy.db");
    let legacy = Command::new(env!("CARGO_BIN_EXE_julie-extract"))
        .args([
            "scan",
            "--root",
            root.to_str().unwrap(),
            "--db",
            legacy_db.to_str().unwrap(),
            "--json",
        ])
        .output()
        .unwrap();
    assert_eq!(
        legacy.status.code(),
        Some(1),
        "{}",
        String::from_utf8_lossy(&legacy.stderr)
    );

    let store_root = temp.path().join("store");
    let store = Command::new(env!("CARGO_BIN_EXE_julie-extract"))
        .args([
            "store",
            "import",
            "--store",
            store_root.to_str().unwrap(),
            "--family",
            "9f8c2c9a-3b92-4f38-9b0d-0e2b8c7a4d11",
            "--root",
            root.to_str().unwrap(),
            "--view",
            "view-parity",
            "--level",
            "full",
            "--request-id",
            "request-parity",
            "--idempotency-key",
            "idem-parity",
            "--json",
        ])
        .output()
        .unwrap();
    assert_eq!(
        store.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&store.stderr)
    );
    let layout = StoreLayout::open(&store_root).unwrap();
    let connection = Connection::open(layout.store_db()).unwrap();
    let (generation, manifest_hash): (i64, String) = connection
        .query_row(
            "SELECT v.current_generation,m.manifest_hash FROM views AS v
         JOIN manifests AS m ON m.view_id=v.view_id AND m.generation=v.current_generation
         WHERE v.view_id='view-parity'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    drop(connection);
    let factory = StoreConnectionFactory::new(
        layout.clone(),
        "9f8c2c9a-3b92-4f38-9b0d-0e2b8c7a4d11",
        env!("CARGO_PKG_VERSION"),
    );
    let exact_path = temp.path().join("store-exact.db");
    let mut session = StoreScratchResolutionSession::new(
        factory,
        StoreManifestIdentity {
            family_id: "9f8c2c9a-3b92-4f38-9b0d-0e2b8c7a4d11".to_string(),
            view_id: "view-parity".to_string(),
            generation,
            manifest_hash,
        },
        &exact_path,
        3,
        6,
    )
    .unwrap();
    run_resolution_session(&mut session, true, true).unwrap();
    session.finish_exact().unwrap();

    assert_eq!(
        store_semantics(layout.store_db(), &exact_path),
        legacy_semantics(&legacy_db)
    );
}

fn fixture_root() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/store-resolution/legacy-v3")
}

fn copy_fixture(source: &Path, target: &Path) {
    std::fs::create_dir_all(target).unwrap();
    for entry in std::fs::read_dir(source).unwrap() {
        let entry = entry.unwrap();
        if entry.file_name() == "expected.semantic.json" {
            continue;
        }
        let destination = target.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_fixture(&entry.path(), &destination);
        } else {
            std::fs::copy(entry.path(), destination).unwrap();
        }
    }
}

fn legacy_semantics(path: &Path) -> Value {
    let connection = Connection::open(path).unwrap();
    let identifiers = connection
        .prepare(
            "SELECT f.path,i.identifier_id,target.path,ir.target_symbol_id,ir.tier,ir.confidence,
                ir.method,ir.outcome,ir.candidates
         FROM identifier_resolutions AS ir
         JOIN identifiers AS i ON i.identifier_id=ir.identifier_id
         JOIN files AS f ON f.file_id=i.file_id
         LEFT JOIN symbols AS target ON target.symbol_id=ir.target_symbol_id
         ORDER BY f.path,i.identifier_id",
        )
        .unwrap()
        .query_map([], |row| {
            Ok(json!([
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, Option<i64>>(4)?,
                row.get::<_, Option<f64>>(5)?,
                row.get::<_, Option<String>>(6)?,
                row.get::<_, String>(7)?,
                row.get::<_, Option<i64>>(8)?,
            ]))
        })
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    let pending = connection
        .prepare(
            "SELECT f.path,pr.pending_relationship_id,target.path,res.target_symbol_id,
                res.tier,res.confidence,res.method
         FROM pending_resolutions AS res
         JOIN pending_relationships AS pr ON pr.pending_relationship_id=res.pending_relationship_id
         JOIN files AS f ON f.file_id=pr.file_id
         JOIN symbols AS target ON target.symbol_id=res.target_symbol_id
         ORDER BY f.path,pr.pending_relationship_id",
        )
        .unwrap()
        .query_map([], |row| {
            Ok(json!([
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, i64>(4)?,
                row.get::<_, f64>(5)?,
                row.get::<_, String>(6)?,
            ]))
        })
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    json!({"identifiers": identifiers, "pending": pending})
}

fn store_semantics(store: impl AsRef<Path>, exact: &Path) -> Value {
    let store = Connection::open(store).unwrap();
    let base = Connection::open(exact).unwrap();
    let path_for = |version_id: i64| -> String {
        store
            .query_row(
                "SELECT path FROM file_versions WHERE version_id=?1",
                [version_id],
                |row| row.get(0),
            )
            .unwrap()
    };
    let identifiers = base.prepare(
        "SELECT version_id,identifier_id,target_version_id,target_symbol_id,tier,confidence,method,outcome,candidates
         FROM identifier_resolutions ORDER BY version_id,identifier_id",
    ).unwrap().query_map([], |row| Ok((
        row.get::<_, i64>(0)?, row.get::<_, String>(1)?, row.get::<_, Option<i64>>(2)?,
        row.get::<_, Option<String>>(3)?, row.get::<_, Option<i64>>(4)?,
        row.get::<_, Option<f64>>(5)?, row.get::<_, Option<String>>(6)?,
        row.get::<_, String>(7)?, row.get::<_, Option<i64>>(8)?,
    ))).unwrap().collect::<Result<Vec<_>, _>>().unwrap().into_iter().map(|row| json!([
        path_for(row.0), row.1, row.2.map(&path_for), row.3, row.4, row.5, row.6, row.7, row.8,
    ])).collect::<Vec<_>>();
    let pending = base.prepare(
        "SELECT version_id,pending_relationship_id,target_version_id,target_symbol_id,tier,confidence,method
         FROM pending_resolutions ORDER BY version_id,pending_relationship_id",
    ).unwrap().query_map([], |row| Ok((
        row.get::<_, i64>(0)?, row.get::<_, String>(1)?, row.get::<_, i64>(2)?,
        row.get::<_, String>(3)?, row.get::<_, i64>(4)?, row.get::<_, f64>(5)?, row.get::<_, String>(6)?,
    ))).unwrap().collect::<Result<Vec<_>, _>>().unwrap().into_iter().map(|row| json!([
        path_for(row.0), row.1, path_for(row.2), row.3, row.4, row.5, row.6,
    ])).collect::<Vec<_>>();
    json!({"identifiers": identifiers, "pending": pending})
}

fn insert_resolution_fixture(connection: &mut Connection, version: i64, count: usize) {
    let transaction = connection.transaction().unwrap();
    transaction.execute(
        "INSERT INTO symbols(version_id,symbol_id,path,language,name,kind,
         start_line,start_column,end_line,end_column,start_byte,end_byte,is_test,test_container,test_lifecycle)
         VALUES (?1,'target','src/lib.rs','rust','target','function',1,1,1,10,0,10,0,0,0)",
        [version],
    ).unwrap();
    transaction.execute(
        "INSERT INTO symbols(version_id,symbol_id,path,language,name,kind,
         start_line,start_column,end_line,end_column,start_byte,end_byte,is_test,test_container,test_lifecycle)
         VALUES (?1,'caller','src/lib.rs','rust','caller','function',2,1,200,1,11,10000,0,0,0)",
        [version],
    ).unwrap();
    for index in 0..count {
        let site = format!("site-{index:04}");
        let identifier = format!("identifier-{index:04}");
        let pending = format!("pending-{index:04}");
        let start = 100 + i64::try_from(index).unwrap() * 10;
        transaction.execute(
            "INSERT INTO reference_sites(version_id,reference_site_id,path,language,containing_symbol_id,
             start_line,start_column,end_line,end_column,start_byte,end_byte,is_exact,provenance,level)
             VALUES (?1,?2,'src/lib.rs','rust','caller',?3,1,?3,7,?4,?5,1,'target_token',2)",
            params![version, site, index as i64 + 3, start, start + 6],
        ).unwrap();
        transaction.execute(
            "INSERT INTO identifiers(version_id,identifier_id,reference_site_id,path,language,name,kind,
             containing_symbol_id,start_line,start_column,end_line,end_column,start_byte,end_byte,confidence)
             VALUES (?1,?2,?3,'src/lib.rs','rust','target','call','caller',?4,1,?4,7,?5,?6,1.0)",
            params![version, identifier, site, index as i64 + 3, start, start + 6],
        ).unwrap();
        transaction.execute(
            "INSERT INTO pending_relationships(version_id,pending_relationship_id,reference_site_id,
             from_symbol_id,caller_scope_symbol_id,path,kind,target_display_name,target_terminal_name,
             target_namespace_json,start_line,start_column,end_line,end_column,start_byte,end_byte,confidence)
             VALUES (?1,?2,?3,'caller','caller','src/lib.rs','calls','target','target','[]',?4,1,?4,7,?5,?6,1.0)",
            params![version, pending, site, index as i64 + 3, start, start + 6],
        ).unwrap();
    }
    transaction.commit().unwrap();
}

#[test]
fn store_session_rss_child() {
    let Ok(count) = std::env::var("JULIE_STORE_RSS_ROWS") else {
        return;
    };
    let count: usize = count.parse().unwrap();
    let temp = TempDir::new().unwrap();
    let layout = StoreLayout::create(temp.path().join("family"), "family-a", "2.30.0").unwrap();
    let factory = StoreConnectionFactory::new(layout, "family-a", "2.30.0");
    let mut connection = factory.open_writer().unwrap();
    ManifestStore::new(&mut connection)
        .ensure_view("view-a", "/repo")
        .unwrap();
    let version = insert_version(&connection, "src/lib.rs", "rust", true);
    connection.execute("INSERT INTO manifests(view_id,generation,manifest_hash,request_id,created_at) VALUES ('view-a',1,'manifest-rss','request-rss',?1)", [NOW]).unwrap();
    connection.execute("INSERT INTO manifest_entries(view_id,generation,path,language,version_id,status,observed_content_hash,indexed_at) VALUES ('view-a',1,'src/lib.rs','rust',?1,'indexed','hash',?2)", params![version, NOW]).unwrap();
    insert_resolution_fixture(&mut connection, version, count);
    drop(connection);
    let mut session = StoreScratchResolutionSession::new(
        factory,
        StoreManifestIdentity {
            family_id: "family-a".to_string(),
            view_id: "view-a".to_string(),
            generation: 1,
            manifest_hash: "manifest-rss".to_string(),
        },
        temp.path().join("exact.db"),
        32,
        6,
    )
    .unwrap();
    run_resolution_session(&mut session, true, true).unwrap();
    assert!(session.max_store_read_page() <= 32);
    let identity = session.finish_exact().unwrap();
    assert_eq!(identity.counts.identifiers as usize, count);
    assert_eq!(identity.counts.pending as usize, count);
}

#[test]
#[cfg(target_os = "macos")]
fn synthetic_store_session_rss_growth_is_bounded_by_windows() {
    let small = measured_child_rss(1_000);
    let large = measured_child_rss(8_000);
    assert!(
        large <= small + 24 * 1024 * 1024,
        "small={small} large={large}"
    );
}

#[cfg(target_os = "macos")]
fn measured_child_rss(rows: usize) -> u64 {
    let output = Command::new("/usr/bin/time")
        .args([
            "-l",
            std::env::current_exe().unwrap().to_str().unwrap(),
            "store_session_rss_child",
            "--exact",
            "--nocapture",
        ])
        .env("JULIE_STORE_RSS_ROWS", rows.to_string())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stderr)
        .lines()
        .find(|line| line.contains("maximum resident set size"))
        .and_then(|line| line.split_whitespace().next())
        .and_then(|value| value.parse().ok())
        .expect("time -l reports maximum resident set size")
}

fn insert_version(connection: &Connection, path: &str, language: &str, complete_l2: bool) -> i64 {
    connection.execute(
        "INSERT INTO file_versions(path,content_hash,extraction_epoch,language,content_bytes,complete_l1,complete_l2)
         VALUES (?1,?2,1,?3,1,1,?4)",
        params![path, format!("hash-{path}"), language, complete_l2.then_some(1)],
    ).unwrap();
    connection.last_insert_rowid()
}
