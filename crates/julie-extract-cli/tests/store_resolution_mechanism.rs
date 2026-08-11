#![cfg(feature = "test-store-resolution-contract")]

use julie_extract_artifact::store::{
    ManifestEntry, ManifestStore, ResolutionBaseReader, ResolutionBaseWriter,
    ResolutionIdentifierRow, ResolutionPendingRow, StoreConnectionFactory, StoreLayout,
    ensure_resolution_scope_feature,
};
use julie_extract_cli::resolution::{
    CandidateSymbol, EdgeOrigin, ReferenceKind, TierOutcome, TypeFact, UnresolvedEdge,
    WorkspaceCandidateIndex, resolve_one, resolve_with_candidate_lookup, run_resolution_session,
};
use julie_extract_cli::resolution_session::{
    ResolutionPassRequest, ResolutionPhase, ResolutionPhaseChunk, ResolutionSession,
    ResolutionWorklists, ResolutionWriteBatch, SemanticIdentifierId, SemanticPendingRelationshipId,
    SemanticSymbolId, SemanticVersionId,
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
    ] {
        assert!(
            !source.contains(forbidden),
            "Store adapter contains forbidden {forbidden}"
        );
    }
    assert!(source.contains("LIMIT ?"));
    assert!(source.contains("open_reader()"));
    assert!(source.contains("build_store_delta_scope"));
    assert!(source.contains("PriorOverlayReader"));
    assert_eq!(source.matches("ATTACH DATABASE ?1 AS prior_").count(), 2);
    assert_eq!(source.matches("DETACH DATABASE prior_").count(), 2);
    assert!(!source.contains("Incremental(String)"));
    assert!(!source.contains("IdentifierTotalityMissing"));
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
fn scoped_store_resolution_reuses_predecessor_and_carries_unselected_sibling() {
    let temp = TempDir::new().unwrap();
    let layout = StoreLayout::create(temp.path().join("family"), "family-a", "2.30.0").unwrap();
    let factory = StoreConnectionFactory::new(layout.clone(), "family-a", "2.30.0");
    let mut connection = factory.open_writer().unwrap();
    ensure_resolution_scope_feature(&connection).unwrap();
    ManifestStore::new(&mut connection)
        .ensure_view("view-a", "/repo")
        .unwrap();
    let old_target = insert_version(&connection, "src/target.rs", "rust", true);
    let new_target = insert_version(&connection, "src/target-new.rs", "rust", true);
    connection
        .execute(
            "UPDATE file_versions SET path='src/target.rs' WHERE version_id=?1",
            [new_target],
        )
        .unwrap();
    let user = insert_version(&connection, "src/user.rs", "rust", true);
    let untouched = insert_version(&connection, "src/untouched.rs", "rust", true);
    insert_named_symbol(&connection, old_target, "old-foo", "Foo", "src/target.rs");
    insert_named_symbol(&connection, new_target, "new-foo", "Foo", "src/target.rs");
    insert_named_symbol(&connection, user, "caller", "caller", "src/user.rs");
    insert_named_symbol(
        &connection,
        user,
        "sibling-target",
        "NeverDefined",
        "src/user.rs",
    );
    insert_named_symbol(
        &connection,
        untouched,
        "untouched-target",
        "UntouchedTarget",
        "src/untouched.rs",
    );
    insert_named_symbol(
        &connection,
        untouched,
        "untouched-caller",
        "untouched_caller",
        "src/untouched.rs",
    );
    connection
        .execute(
            "INSERT INTO reference_sites
             (version_id,reference_site_id,path,language,start_line,start_column,end_line,end_column,
              start_byte,end_byte,is_exact,provenance,level)
             VALUES (?1,'site-untouched-pending','src/untouched.rs','rust',2,1,2,5,5,9,1,
                     'target_token',2)",
            [untouched],
        )
        .unwrap();
    insert_named_identifier(
        &connection,
        old_target,
        "removed-use",
        "NeverDefined",
        "src/target.rs",
    );
    insert_named_identifier(&connection, user, "foo-use", "Foo", "src/user.rs");
    insert_named_identifier(
        &connection,
        user,
        "sibling-use",
        "NeverDefined",
        "src/user.rs",
    );
    connection
        .execute(
            "UPDATE identifiers
             SET start_line=3,end_line=3,start_byte=15,end_byte=19,
                 metadata_json='{\"receiver\":\"receiverOnly\"}'
             WHERE version_id=?1 AND identifier_id='sibling-use'",
            [user],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO pending_relationships
             (version_id,pending_relationship_id,reference_site_id,from_symbol_id,path,kind,
              target_display_name,target_terminal_name,target_namespace_json,start_line,start_column,
              end_line,end_column,start_byte,end_byte,confidence)
             VALUES (?1,?2,'site-foo-use','caller','src/user.rs','calls','Foo','Foo','[]',2,1,2,5,5,9,1.0)",
            params![user, "a-unresolved"],
        )
        .unwrap();
    for pending_relationship_id in ["delta-replace", "delta-tombstone", "scratch-authority"] {
        connection
            .execute(
                "INSERT INTO pending_relationships
                 (version_id,pending_relationship_id,reference_site_id,from_symbol_id,path,kind,
                  target_display_name,target_terminal_name,target_namespace_json,start_line,start_column,
                  end_line,end_column,start_byte,end_byte,confidence)
                 VALUES (?1,?2,'site-untouched-pending','untouched-caller','src/untouched.rs','calls',
                         'NeverPending','NeverPending','[]',3,1,3,5,15,19,1.0)",
                params![untouched, pending_relationship_id],
            )
            .unwrap();
    }
    connection
        .execute(
            "INSERT INTO pending_relationships
             (version_id,pending_relationship_id,reference_site_id,from_symbol_id,path,kind,
              target_display_name,target_terminal_name,target_namespace_json,start_line,start_column,
              end_line,end_column,start_byte,end_byte,confidence)
             VALUES (?1,?2,'site-foo-use','caller','src/user.rs','calls','Foo','Foo','[]',2,1,2,5,5,9,1.0)",
            params![user, "z-resolved"],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO relationships
             (version_id,relationship_id,reference_site_id,from_symbol_id,to_symbol_id,path,kind,
              start_line,start_column,end_line,end_column,start_byte,end_byte,confidence)
             VALUES (?1,'sibling-relationship','site-sibling-use','caller','sibling-target',
                     'src/user.rs','calls',3,1,3,5,15,19,1.0)",
            [user],
        )
        .unwrap();
    let first_entries = [
        manifest_entry(&connection, old_target),
        manifest_entry(&connection, user),
        manifest_entry(&connection, untouched),
    ];
    let first = publish_manifest(&mut connection, None, first_entries, "request-first");
    let base_path = layout.bases_dir().join("base-a.db");
    let mut base = ResolutionBaseWriter::new(&base_path, &first.manifest_hash, 6).unwrap();
    base.push_source_version(old_target).unwrap();
    base.push_source_version(user).unwrap();
    base.push_source_version(untouched).unwrap();
    base.push_identifier_resolution(ResolutionIdentifierRow {
        version_id: old_target,
        identifier_id: "removed-use".to_string(),
        target_version_id: None,
        target_symbol_id: None,
        tier: None,
        confidence: None,
        method: Some("removed-version".to_string()),
        outcome: "missing".to_string(),
        candidates: Some(0),
    })
    .unwrap();
    base.push_pending_resolution(ResolutionPendingRow {
        version_id: user,
        pending_relationship_id: "z-resolved".to_string(),
        target_version_id: old_target,
        target_symbol_id: "old-foo".to_string(),
        tier: 4,
        confidence: 1.0,
        method: "base-pending".to_string(),
    })
    .unwrap();
    for (pending_relationship_id, method) in [
        ("delta-replace", "base-delta-replace"),
        ("delta-tombstone", "base-delta-tombstone"),
        ("scratch-authority", "base-scratch-authority"),
    ] {
        base.push_pending_resolution(ResolutionPendingRow {
            version_id: untouched,
            pending_relationship_id: pending_relationship_id.to_string(),
            target_version_id: untouched,
            target_symbol_id: "untouched-target".to_string(),
            tier: 1,
            confidence: 0.9,
            method: method.to_string(),
        })
        .unwrap();
    }
    base.push_identifier_resolution(ResolutionIdentifierRow {
        version_id: user,
        identifier_id: "foo-use".to_string(),
        target_version_id: Some(old_target),
        target_symbol_id: Some("old-foo".to_string()),
        tier: Some(4),
        confidence: Some(1.0),
        method: Some("base-foo".to_string()),
        outcome: "resolved".to_string(),
        candidates: Some(1),
    })
    .unwrap();
    base.push_identifier_resolution(ResolutionIdentifierRow {
        version_id: user,
        identifier_id: "sibling-use".to_string(),
        target_version_id: Some(user),
        target_symbol_id: Some("sibling-target".to_string()),
        tier: Some(1),
        confidence: Some(0.95),
        method: Some("base-sibling".to_string()),
        outcome: "resolved".to_string(),
        candidates: None,
    })
    .unwrap();
    let base_identity = base.finish_with_target_lookup(|_, _| Ok(true)).unwrap();
    connection
        .execute(
            "INSERT INTO resolution_bases
         (base_id,manifest_hash,resolver_output_epoch,state,relative_path,identifier_count,
          pending_count,file_bytes,file_sha256,request_id,created_at,updated_at)
         VALUES ('base-a',?1,6,'ready','bases/base-a.db',3,4,?2,?3,'request-base',?4,?4)",
            params![
                first.manifest_hash,
                i64::try_from(base_identity.file_bytes).unwrap(),
                base_identity.file_sha256,
                NOW
            ],
        )
        .unwrap();
    for version_id in [old_target, user, untouched] {
        connection
            .execute(
                "INSERT INTO resolution_base_versions(base_id,version_id) VALUES ('base-a',?1)",
                [version_id],
            )
            .unwrap();
    }
    connection
        .execute(
            "INSERT INTO resolution_deltas
         (view_id,delta_generation,base_id,manifest_generation,manifest_hash,resolver_output_epoch,
          identifier_replacements,pending_replacements,pending_tombstones,exact_gap_rows,
          exact_gap_files,exact_gap_json,request_id,created_at)
         VALUES ('view-a',1,'base-a',?1,?2,6,0,2,1,0,0,'[]','request-base',?3)",
            params![
                i64::try_from(first.generation).unwrap(),
                first.manifest_hash,
                NOW
            ],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO resolution_pending_deltas
             (view_id,delta_generation,version_id,pending_relationship_id,operation,
              target_version_id,target_symbol_id,tier,confidence,method)
             VALUES ('view-a',1,?1,'delta-replace','replace',?1,'untouched-target',1,0.95,
                     'delta-replace')",
            [untouched],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO resolution_pending_deltas
             (view_id,delta_generation,version_id,pending_relationship_id,operation,
              target_version_id,target_symbol_id,tier,confidence,method)
             VALUES ('view-a',1,?1,'scratch-authority','replace',?1,'untouched-target',1,0.95,
                     'delta-shadowed')",
            [untouched],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO resolution_pending_deltas
             (view_id,delta_generation,version_id,pending_relationship_id,operation,
              target_version_id,target_symbol_id,tier,confidence,method)
             VALUES ('view-a',1,?1,'delta-tombstone','tombstone',NULL,NULL,NULL,NULL,NULL)",
            [untouched],
        )
        .unwrap();
    connection
        .execute(
            "UPDATE views SET resolution_state='exact',resolution_base_id='base-a',
         resolution_delta_generation=1,resolution_exact_at=?1 WHERE view_id='view-a'",
            [i64::try_from(first.generation).unwrap()],
        )
        .unwrap();
    let second_entries = [
        manifest_entry(&connection, new_target),
        manifest_entry(&connection, user),
        manifest_entry(&connection, untouched),
    ];
    let second = publish_manifest(
        &mut connection,
        Some(i64::try_from(first.generation).unwrap()),
        second_entries,
        "request-second",
    );
    drop(connection);

    let probe_identity = StoreManifestIdentity {
        family_id: "family-a".to_string(),
        view_id: "view-a".to_string(),
        generation: i64::try_from(second.generation).unwrap(),
        manifest_hash: second.manifest_hash.clone(),
    };
    let mut selected_probe = StoreScratchResolutionSession::new(
        factory.clone(),
        probe_identity.clone(),
        temp.path().join("selected-probe.db"),
        1,
        6,
    )
    .unwrap();
    assert!(selected_probe.prior_resolution_state().unwrap().is_some());
    selected_probe
        .open_resolution_pass(&ResolutionPassRequest { full: false })
        .unwrap();
    assert!(
        selected_probe
            .propagation_is_covered(&SemanticIdentifierId {
                version: SemanticVersionId::Store(user),
                local_id: "foo-use".to_string(),
            })
            .unwrap()
    );
    assert!(
        selected_probe
            .propagation_is_covered(&SemanticIdentifierId {
                version: SemanticVersionId::Store(user),
                local_id: "sibling-use".to_string(),
            })
            .unwrap()
    );
    let mut writes = ResolutionWriteBatch::default();
    writes.demote_identifier(SemanticIdentifierId {
        version: SemanticVersionId::Store(user),
        local_id: "foo-use".to_string(),
    });
    selected_probe.flush(writes).unwrap();
    let selected_worklists = ResolutionWorklists {
        effective_full: false,
        selected_versions: vec![SemanticVersionId::Store(user)],
        phase: ResolutionPhase::ResolvedIdentifiers,
        ..ResolutionWorklists::default()
    };
    let mut selected_ids = Vec::new();
    while let Some(ResolutionPhaseChunk::ResolvedIdentifiers(rows)) = selected_probe
        .next_phase_chunk(&selected_worklists)
        .unwrap()
    {
        selected_ids.extend(rows.into_iter().map(|row| row.identifier.identifier_id));
    }
    assert_eq!(selected_ids, ["sibling-use"]);

    let mut hydration_probe = StoreScratchResolutionSession::new(
        factory.clone(),
        StoreManifestIdentity {
            family_id: "family-a".to_string(),
            view_id: "view-a".to_string(),
            generation: i64::try_from(second.generation).unwrap(),
            manifest_hash: second.manifest_hash.clone(),
        },
        temp.path().join("hydration-probe.db"),
        1,
        6,
    )
    .unwrap();
    assert!(hydration_probe.prior_resolution_state().unwrap().is_some());
    hydration_probe
        .open_resolution_pass(&ResolutionPassRequest { full: false })
        .unwrap();
    let hydration_worklists = ResolutionWorklists {
        effective_full: false,
        selected_versions: vec![SemanticVersionId::Store(user)],
        phase: ResolutionPhase::ResolvedIdentifiers,
        ..ResolutionWorklists::default()
    };
    let first_hydrated = hydration_probe
        .next_phase_chunk(&hydration_worklists)
        .unwrap()
        .unwrap();
    assert!(matches!(
        first_hydrated,
        ResolutionPhaseChunk::ResolvedIdentifiers(rows)
            if rows[0].identifier.identifier_id == "foo-use"
    ));
    let mut writes = ResolutionWriteBatch::default();
    writes.demote_identifier(SemanticIdentifierId {
        version: SemanticVersionId::Store(user),
        local_id: "sibling-use".to_string(),
    });
    hydration_probe.flush(writes).unwrap();
    assert!(
        hydration_probe
            .next_phase_chunk(&hydration_worklists)
            .unwrap()
            .is_none()
    );

    let mut name_probe = StoreScratchResolutionSession::new(
        factory.clone(),
        probe_identity,
        temp.path().join("name-probe.db"),
        1,
        6,
    )
    .unwrap();
    assert!(name_probe.prior_resolution_state().unwrap().is_some());
    name_probe
        .open_resolution_pass(&ResolutionPassRequest { full: false })
        .unwrap();
    let name_worklists = ResolutionWorklists {
        effective_full: false,
        recheck_names: vec!["NeverDefined".to_string()],
        phase: ResolutionPhase::ResolvedIdentifiers,
        ..ResolutionWorklists::default()
    };
    let mut name_ids = Vec::new();
    while let Some(ResolutionPhaseChunk::ResolvedIdentifiers(rows)) =
        name_probe.next_phase_chunk(&name_worklists).unwrap()
    {
        name_ids.extend(rows.into_iter().map(|row| row.identifier.identifier_id));
    }
    assert_eq!(name_ids, ["sibling-use"]);

    let forced_path = temp.path().join("forced.db");
    let mut forced = StoreScratchResolutionSession::new(
        factory.clone(),
        StoreManifestIdentity {
            family_id: "family-a".to_string(),
            view_id: "view-a".to_string(),
            generation: i64::try_from(second.generation).unwrap(),
            manifest_hash: second.manifest_hash.clone(),
        },
        &forced_path,
        1,
        6,
    )
    .unwrap();
    let (_, forced_report) = run_resolution_session(&mut forced, true, true).unwrap();
    assert!(forced_report.rows.is_some());
    forced.finish_exact().unwrap();
    let forced_rows = ResolutionBaseReader::open(&forced_path)
        .unwrap()
        .identifiers()
        .unwrap();
    assert_eq!(forced_rows.len(), 2);
    assert_ne!(forced_rows[1].method.as_deref(), Some("base-sibling"));

    let exact_path = temp.path().join("exact.db");
    let mut session = StoreScratchResolutionSession::new(
        factory,
        StoreManifestIdentity {
            family_id: "family-a".to_string(),
            view_id: "view-a".to_string(),
            generation: i64::try_from(second.generation).unwrap(),
            manifest_hash: second.manifest_hash.clone(),
        },
        &exact_path,
        1,
        6,
    )
    .unwrap();
    let (_, report) = run_resolution_session(&mut session, false, true).unwrap();
    assert_eq!(report.status.as_str(), "partial");
    assert!(report.rows.is_none());
    let mut pending_writes = ResolutionWriteBatch::default();
    pending_writes.record_pending_resolution(
        SemanticPendingRelationshipId {
            version: SemanticVersionId::Store(untouched),
            local_id: "scratch-authority".to_string(),
        },
        SemanticSymbolId {
            version: SemanticVersionId::Store(untouched),
            local_id: "untouched-target".to_string(),
        },
        1,
        0.99,
        "scratch-authority",
        6,
    );
    session.flush(pending_writes).unwrap();
    session.finish_exact().unwrap();
    let rows = ResolutionBaseReader::open(&exact_path)
        .unwrap()
        .identifiers()
        .unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].identifier_id, "foo-use");
    assert_eq!(rows[0].target_version_id, Some(new_target));
    assert_eq!(rows[1].identifier_id, "sibling-use");
    assert_eq!(rows[1].target_version_id, Some(user));
    assert_eq!(rows[1].method.as_deref(), Some("base-sibling"));
    assert!(!rows.iter().any(|row| row.identifier_id == "removed-use"));
    let pending = ResolutionBaseReader::open(&exact_path)
        .unwrap()
        .pending()
        .unwrap();
    assert!(
        pending.iter().any(|row| {
            row.pending_relationship_id == "delta-replace" && row.method == "delta-replace"
        }),
        "{pending:?}"
    );
    assert!(
        !pending
            .iter()
            .any(|row| row.pending_relationship_id == "delta-tombstone")
    );
    assert!(pending.iter().any(|row| {
        row.pending_relationship_id == "scratch-authority" && row.method == "scratch-authority"
    }));

    let frozen_path = temp.path().join("frozen-state.db");
    let mut frozen = StoreScratchResolutionSession::new(
        StoreConnectionFactory::new(layout.clone(), "family-a", "2.30.0"),
        StoreManifestIdentity {
            family_id: "family-a".to_string(),
            view_id: "view-a".to_string(),
            generation: i64::try_from(second.generation).unwrap(),
            manifest_hash: second.manifest_hash.clone(),
        },
        &frozen_path,
        1,
        6,
    )
    .unwrap();
    run_resolution_session(&mut frozen, false, true).unwrap();
    Connection::open(layout.store_db())
        .unwrap()
        .execute(
            "UPDATE resolution_deltas SET pending_replacements=1
             WHERE view_id='view-a' AND delta_generation=1",
            [],
        )
        .unwrap();
    assert!(matches!(
        frozen.finish_exact().unwrap_err(),
        StoreResolutionError::Artifact(
            julie_extract_artifact::store::ResolutionValidationError::InvalidMetadata { .. }
        )
    ));
    assert!(!frozen_path.exists());

    let connection = Connection::open(layout.store_db()).unwrap();
    connection
        .execute(
            "UPDATE resolution_deltas SET identifier_replacements=1,pending_replacements=2
         WHERE view_id='view-a' AND delta_generation=1",
            [],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO resolution_identifier_deltas
         (view_id,delta_generation,version_id,identifier_id,target_version_id,target_symbol_id,
          tier,confidence,method,outcome,candidates)
         VALUES ('view-a',1,?1,'sibling-use',?2,'old-foo',4,1.0,'stale','resolved',1)",
            params![user, old_target],
        )
        .unwrap();
    drop(connection);
    let stale_path = temp.path().join("stale.db");
    let mut stale = StoreScratchResolutionSession::new(
        StoreConnectionFactory::new(layout.clone(), "family-a", "2.30.0"),
        StoreManifestIdentity {
            family_id: "family-a".to_string(),
            view_id: "view-a".to_string(),
            generation: i64::try_from(second.generation).unwrap(),
            manifest_hash: second.manifest_hash.clone(),
        },
        &stale_path,
        1,
        6,
    )
    .unwrap();
    run_resolution_session(&mut stale, false, true).unwrap();
    assert!(matches!(
        stale.finish_exact().unwrap_err(),
        StoreResolutionError::Artifact(
            julie_extract_artifact::store::ResolutionValidationError::TargetMissing { .. }
        )
    ));
    assert!(!stale_path.exists());

    let mut connection = Connection::open(layout.store_db()).unwrap();
    connection
        .execute(
            "DELETE FROM resolution_identifier_deltas
             WHERE view_id='view-a' AND delta_generation=1",
            [],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO resolution_identifier_deltas
             (view_id,delta_generation,version_id,identifier_id,target_version_id,target_symbol_id,
              tier,confidence,method,outcome,candidates)
             VALUES ('view-a',1,?1,'foo-use',?2,'new-foo',4,1.0,'cumulative','resolved',1)",
            params![user, new_target],
        )
        .unwrap();
    connection
        .execute(
            "UPDATE resolution_deltas
             SET manifest_generation=?1,manifest_hash=?2,identifier_replacements=1,
                 pending_replacements=2
             WHERE view_id='view-a' AND delta_generation=1",
            params![
                i64::try_from(second.generation).unwrap(),
                second.manifest_hash
            ],
        )
        .unwrap();
    connection
        .execute(
            "UPDATE views SET resolution_state='exact',resolution_base_id='base-a',
             resolution_delta_generation=1,resolution_exact_at=?1 WHERE view_id='view-a'",
            [i64::try_from(second.generation).unwrap()],
        )
        .unwrap();
    let latest_target = insert_version(&connection, "src/target-latest.rs", "rust", true);
    connection
        .execute(
            "UPDATE file_versions SET path='src/target.rs' WHERE version_id=?1",
            [latest_target],
        )
        .unwrap();
    insert_named_symbol(
        &connection,
        latest_target,
        "latest-foo",
        "Foo",
        "src/target.rs",
    );
    let third_entries = [
        manifest_entry(&connection, latest_target),
        manifest_entry(&connection, user),
        manifest_entry(&connection, untouched),
    ];
    let third = publish_manifest(
        &mut connection,
        Some(i64::try_from(second.generation).unwrap()),
        third_entries,
        "request-third",
    );
    let (base_manifest_hash, delta_manifest_hash): (String, String) = connection
        .query_row(
            "SELECT base.manifest_hash,delta.manifest_hash
             FROM resolution_bases AS base
             JOIN resolution_deltas AS delta ON delta.base_id=base.base_id
             WHERE base.base_id='base-a' AND delta.view_id='view-a' AND delta.delta_generation=1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(base_manifest_hash, first.manifest_hash);
    assert_ne!(base_manifest_hash, second.manifest_hash);
    assert_eq!(delta_manifest_hash, second.manifest_hash);
    drop(connection);

    let cumulative_path = temp.path().join("cumulative.db");
    let mut cumulative = StoreScratchResolutionSession::new(
        StoreConnectionFactory::new(layout, "family-a", "2.30.0"),
        StoreManifestIdentity {
            family_id: "family-a".to_string(),
            view_id: "view-a".to_string(),
            generation: i64::try_from(third.generation).unwrap(),
            manifest_hash: third.manifest_hash,
        },
        &cumulative_path,
        1,
        6,
    )
    .unwrap();
    let (_, cumulative_report) = run_resolution_session(&mut cumulative, false, true).unwrap();
    assert_eq!(cumulative_report.status.as_str(), "partial");
    assert!(cumulative_report.rows.is_none());
    cumulative.finish_exact().unwrap();
    let cumulative_reader = ResolutionBaseReader::open(&cumulative_path).unwrap();
    let cumulative_identifiers = cumulative_reader.identifiers().unwrap();
    assert_eq!(cumulative_identifiers.len(), 2);
    assert!(cumulative_identifiers.iter().any(|row| {
        row.identifier_id == "foo-use" && row.target_version_id == Some(latest_target)
    }));
    assert!(
        cumulative_identifiers.iter().any(|row| {
            row.identifier_id == "sibling-use" && row.target_version_id == Some(user)
        })
    );
    let cumulative_pending = cumulative_reader.pending().unwrap();
    assert!(cumulative_pending.iter().any(|row| {
        row.pending_relationship_id == "delta-replace" && row.method == "delta-replace"
    }));
    assert!(cumulative_pending.iter().any(|row| {
        row.pending_relationship_id == "scratch-authority" && row.method == "delta-shadowed"
    }));
    assert!(
        !cumulative_pending
            .iter()
            .any(|row| row.pending_relationship_id == "delta-tombstone")
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

fn manifest_entry(connection: &Connection, version_id: i64) -> ManifestEntry {
    connection
        .query_row(
            "SELECT path,language,content_hash FROM file_versions WHERE version_id=?1",
            [version_id],
            |row| {
                Ok(ManifestEntry::indexed(
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    version_id,
                    row.get::<_, String>(2)?,
                    NOW,
                ))
            },
        )
        .unwrap()
}

fn publish_manifest(
    connection: &mut Connection,
    expected_generation: Option<i64>,
    entries: impl IntoIterator<Item = ManifestEntry>,
    request_id: &str,
) -> julie_extract_artifact::store::ManifestPublishResult {
    ManifestStore::new(connection)
        .publish(
            "view-a",
            expected_generation.map(|value| value as u64),
            entries,
            request_id,
        )
        .unwrap()
}

fn insert_named_symbol(
    connection: &Connection,
    version_id: i64,
    symbol_id: &str,
    name: &str,
    path: &str,
) {
    connection.execute(
        "INSERT INTO symbols(version_id,symbol_id,path,language,name,kind,
         start_line,start_column,end_line,end_column,start_byte,end_byte,is_test,test_container,test_lifecycle)
         VALUES (?1,?2,?3,'rust',?4,'function',1,1,1,5,0,4,0,0,0)",
        params![version_id, symbol_id, path, name],
    ).unwrap();
}

fn insert_named_identifier(
    connection: &Connection,
    version_id: i64,
    identifier_id: &str,
    name: &str,
    path: &str,
) {
    let site_id = format!("site-{identifier_id}");
    connection
        .execute(
            "INSERT INTO reference_sites(version_id,reference_site_id,path,language,
         start_line,start_column,end_line,end_column,start_byte,end_byte,is_exact,provenance,level)
         VALUES (?1,?2,?3,'rust',2,1,2,5,5,9,1,'target_token',2)",
            params![version_id, site_id, path],
        )
        .unwrap();
    connection.execute(
        "INSERT INTO identifiers(version_id,identifier_id,reference_site_id,path,language,name,kind,
         start_line,start_column,end_line,end_column,start_byte,end_byte,confidence)
         VALUES (?1,?2,?3,?4,'rust',?5,'call',2,1,2,5,5,9,1.0)",
        params![version_id, identifier_id, site_id, path, name],
    ).unwrap();
}
