#![cfg(feature = "test-store-resolution")]

use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use julie_extract_artifact::store::{
    ManifestEntry, ManifestStore, ResolutionBaseBegin, ResolutionBaseCatalog, ResolutionBaseWriter,
    ResolutionBindingError, ResolutionBindingStore, ResolutionExactPublish, ResolutionGapFact,
    ResolutionGapKind, ResolutionGapTable, ResolutionIdentifierRow, ResolutionPendingOperation,
    ResolutionPendingRow, ResolutionPinOwnerKind, ResolutionPublicationFence,
    ResolutionPublicationMarker, ResolutionScratchDelta, ResolutionScratchReader,
    StoreConnectionFactory, StoreLayout, ViewResolutionState,
};
use rusqlite::Connection;

const FAMILY_ID: &str = "123e4567-e89b-12d3-a456-426614174000";
const VERSION: &str = "2.30.0";
const NOW: &str = "2026-08-08T20:30:00Z";

static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "julie-resolution-binding-{}-{nonce}-{}",
            std::process::id(),
            NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}

#[test]
fn exact_publish_is_atomic_and_stale_binding_cas_publishes_nothing() {
    let temp = TempDir::new();
    let layout = StoreLayout::create(&temp.0, FAMILY_ID, VERSION).unwrap();
    let (base_hash, base_version) = publish_manifest(&layout, "view-a", None, "src/a.rs", "a");
    let factory = StoreConnectionFactory::new(layout.clone(), FAMILY_ID, VERSION);
    let base = ready_empty_base(&factory, &base_hash, base_version, "request-base");
    let bindings = ResolutionBindingStore::new(factory.clone());

    publish_manifest(&layout, "view-c", None, "src/a.rs", "a");
    let (manifest_hash, exact_version) =
        publish_manifest(&layout, "view-c", Some(1), "src/b.rs", "b");
    insert_symbol(&layout, exact_version, "symbol-1", "src/b.rs");
    let converging = bindings
        .bind_base("view-c", 7, "request-bind", NOW)
        .unwrap();
    assert_eq!(converging.state, ViewResolutionState::Converging);

    let scratch_path = temp.0.join("exact-delta.db");
    let mut scratch = ResolutionScratchDelta::new(&scratch_path, &manifest_hash, 7).unwrap();
    scratch.push_identifier_replacement(ResolutionIdentifierRow {
        version_id: exact_version,
        identifier_id: "identifier-1".to_string(),
        target_version_id: Some(exact_version),
        target_symbol_id: Some("symbol-1".to_string()),
        tier: Some(2),
        confidence: Some(0.9),
        method: Some("exact".to_string()),
        outcome: "resolved".to_string(),
        candidates: Some(1),
    });
    scratch.push_pending_replacement(ResolutionPendingRow {
        version_id: exact_version,
        pending_relationship_id: "pending-1".to_string(),
        target_version_id: exact_version,
        target_symbol_id: "symbol-1".to_string(),
        tier: 2,
        confidence: 0.9,
        method: "exact".to_string(),
    });
    scratch.push_pending_tombstone(exact_version, "pending-2");
    scratch.finish().unwrap();
    let scratch = ResolutionScratchReader::open(&scratch_path).unwrap();
    let publication = ResolutionExactPublish {
        view_id: "view-c".to_string(),
        manifest_generation: 2,
        manifest_hash: manifest_hash.clone(),
        base_id: base,
        previous_delta_generation: converging.delta_generation,
        resolver_output_epoch: 7,
        request_id: "request-exact".to_string(),
        created_at: NOW.to_string(),
    };
    let fence = publication_fence(&layout, &publication.request_id);
    let gaps = vec![
        ResolutionGapFact {
            table: ResolutionGapTable::Identifier,
            version_id: exact_version,
            local_id: "identifier-1".to_string(),
            kind: ResolutionGapKind::Replaced,
        },
        ResolutionGapFact {
            table: ResolutionGapTable::Pending,
            version_id: exact_version,
            local_id: "pending-2".to_string(),
            kind: ResolutionGapKind::Removed,
        },
    ];
    let before_failures = publication_counts(&layout);
    for invalid in [
        ResolutionExactPublish {
            view_id: "missing-view".to_string(),
            ..publication.clone()
        },
        ResolutionExactPublish {
            manifest_generation: 1,
            ..publication.clone()
        },
        ResolutionExactPublish {
            base_id: "wrong-base".to_string(),
            ..publication.clone()
        },
        ResolutionExactPublish {
            previous_delta_generation: converging.delta_generation + 1,
            ..publication.clone()
        },
    ] {
        assert!(matches!(
            bindings.publish_exact(&invalid, &fence, &scratch, &gaps, 1, || Ok(())),
            Err(ResolutionBindingError::CasLost { .. })
                | Err(ResolutionBindingError::ViewNotFound { .. })
        ));
        assert_eq!(publication_counts(&layout), before_failures);
    }
    let wrong_scratch_path = temp.0.join("wrong-manifest-delta.db");
    ResolutionScratchDelta::new(&wrong_scratch_path, "wrong-manifest", 7)
        .unwrap()
        .finish()
        .unwrap();
    let wrong_scratch = ResolutionScratchReader::open(&wrong_scratch_path).unwrap();
    let wrong_manifest = ResolutionExactPublish {
        manifest_hash: "wrong-manifest".to_string(),
        ..publication.clone()
    };
    assert!(matches!(
        bindings.publish_exact(&wrong_manifest, &fence, &wrong_scratch, &[], 1, || Ok(())),
        Err(ResolutionBindingError::CasLost { .. })
    ));
    let wrong_fence = ResolutionPublicationFence {
        fencing_token: 8,
        ..fence.clone()
    };
    assert!(matches!(
        bindings.publish_exact(&publication, &wrong_fence, &scratch, &gaps, 1, || Ok(())),
        Err(ResolutionBindingError::FenceLost { .. })
    ));
    let missing_claim = ResolutionExactPublish {
        request_id: "request-missing-claim".to_string(),
        ..publication.clone()
    };
    assert!(matches!(
        bindings.publish_exact(&missing_claim, &fence, &scratch, &gaps, 1, || Ok(())),
        Err(ResolutionBindingError::FenceLost { .. })
    ));
    let invalid_target_path = temp.0.join("invalid-target-delta.db");
    let mut invalid_target =
        ResolutionScratchDelta::new(&invalid_target_path, &manifest_hash, 7).unwrap();
    invalid_target.push_identifier_replacement(ResolutionIdentifierRow {
        version_id: exact_version,
        identifier_id: "identifier-invalid-target".to_string(),
        target_version_id: Some(base_version),
        target_symbol_id: Some("symbol-not-visible".to_string()),
        tier: Some(2),
        confidence: Some(0.9),
        method: Some("exact".to_string()),
        outcome: "resolved".to_string(),
        candidates: Some(1),
    });
    invalid_target.finish().unwrap();
    let invalid_target = ResolutionScratchReader::open(&invalid_target_path).unwrap();
    assert!(matches!(
        bindings.publish_exact(&publication, &fence, &invalid_target, &gaps, 1, || Ok(())),
        Err(ResolutionBindingError::InvalidPublication { .. })
    ));
    assert_eq!(publication_counts(&layout), before_failures);

    let mut publication_markers = Vec::new();
    let mut heartbeat_count = 0u32;
    let exact = bindings
        .publish_exact_with_markers(
            &publication,
            &fence,
            &scratch,
            &gaps,
            1,
            || {
                heartbeat_count += 1;
                Ok(())
            },
            |marker| {
                publication_markers.push(marker);
            },
        )
        .unwrap();
    assert_eq!(heartbeat_count, 1);
    assert_eq!(
        publication_markers,
        [
            ResolutionPublicationMarker::StoreTransactionStart,
            ResolutionPublicationMarker::StoreTransactionEnd,
        ]
    );
    assert_eq!(exact.state, ViewResolutionState::Exact);
    assert_eq!(exact.exact_at, Some(2));
    assert_eq!(exact.delta_generation, converging.delta_generation + 1);
    Connection::open(layout.coordinator_db())
        .unwrap()
        .execute("DELETE FROM writer_lease WHERE resource='store-writer'", [])
        .unwrap();
    bindings
        .open_pin(
            "pin-exact",
            ResolutionPinOwnerKind::Reader,
            "reader-exact",
            "view-c",
            "2026-08-08T20:40:00Z",
            NOW,
        )
        .unwrap();
    let identifiers = bindings
        .pinned_identifier_delta_window("pin-exact", NOW, None, 1)
        .unwrap();
    assert_eq!(identifiers.len(), 1);
    assert_eq!(identifiers[0].identifier_id, "identifier-1");
    let pending = bindings
        .pinned_pending_delta_window("pin-exact", NOW, None, 2)
        .unwrap();
    assert_eq!(pending.len(), 2);
    assert_eq!(pending[0].operation, ResolutionPendingOperation::Replace);
    assert_eq!(pending[1].operation, ResolutionPendingOperation::Tombstone);

    let connection = Connection::open(layout.store_db()).unwrap();
    let counts: (i64, i64, i64, i64) = connection
        .query_row(
            "SELECT identifier_replacements,pending_replacements,pending_tombstones,exact_gap_rows
             FROM resolution_deltas WHERE view_id='view-c' AND delta_generation=?1",
            [exact.delta_generation],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .unwrap();
    assert_eq!(counts, (1, 1, 1, 2));
    assert_eq!(
        connection
            .query_row(
                "SELECT COUNT(*) FROM store_log
                 WHERE request_id='request-exact' AND event_kind='resolution_exact_published'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
        1
    );
    let before: (i64, i64, i64) = connection
        .query_row(
            "SELECT
               (SELECT COUNT(*) FROM resolution_deltas),
               (SELECT COUNT(*) FROM resolution_identifier_deltas),
               (SELECT COUNT(*) FROM store_log)",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    drop(connection);

    let now_ms = wall_now_ms();
    let coordinator = Connection::open(layout.coordinator_db()).unwrap();
    coordinator
        .execute(
            "INSERT INTO writer_lease
             (resource,holder_id,holder_version,holder_pid,heartbeat_at,expires_at,fencing_token)
             VALUES ('store-writer','holder-1',?1,42,?2,?3,7)",
            rusqlite::params![VERSION, now_ms, now_ms + 60_000],
        )
        .unwrap();
    coordinator
        .execute(
            "UPDATE writer_lease SET fencing_token=8 WHERE resource='store-writer'",
            [],
        )
        .unwrap();
    assert!(matches!(
        bindings.publish_exact(&publication, &fence, &scratch, &gaps, 1, || Ok(())),
        Err(ResolutionBindingError::FenceLost { .. })
    ));
    Connection::open(layout.coordinator_db())
        .unwrap()
        .execute(
            "UPDATE writer_lease SET fencing_token=7 WHERE resource='store-writer'",
            [],
        )
        .unwrap();

    let stale = ResolutionExactPublish {
        previous_delta_generation: converging.delta_generation,
        request_id: "request-stale".to_string(),
        ..publication
    };
    claim_resolution_request(&layout, &stale.request_id);
    let stale_result = bindings.publish_exact(&stale, &fence, &scratch, &gaps, 1, || Ok(()));
    assert!(
        matches!(stale_result, Err(ResolutionBindingError::CasLost { .. })),
        "{stale_result:?}"
    );
    let connection = Connection::open(layout.store_db()).unwrap();
    let after: (i64, i64, i64) = connection
        .query_row(
            "SELECT
               (SELECT COUNT(*) FROM resolution_deltas),
               (SELECT COUNT(*) FROM resolution_identifier_deltas),
               (SELECT COUNT(*) FROM store_log)",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert_eq!(after, before);
}

#[test]
fn exact_publish_rejects_wall_clock_expired_lease_even_when_fence_now_is_stale() {
    let temp = TempDir::new();
    let layout = StoreLayout::create(&temp.0, FAMILY_ID, VERSION).unwrap();
    let (base_hash, base_version) = publish_manifest(&layout, "view-a", None, "src/a.rs", "a");
    let factory = StoreConnectionFactory::new(layout.clone(), FAMILY_ID, VERSION);
    let base = ready_empty_base(&factory, &base_hash, base_version, "request-base");
    let bindings = ResolutionBindingStore::new(factory);
    let (manifest_hash, exact_version) =
        publish_manifest(&layout, "view-wall", None, "src/b.rs", "b");
    insert_symbol(&layout, exact_version, "symbol-1", "src/b.rs");
    let converging = bindings
        .bind_base("view-wall", 7, "request-bind-wall", NOW)
        .unwrap();
    let scratch_path = temp.0.join("wall-expired-delta.db");
    let mut scratch = ResolutionScratchDelta::new(&scratch_path, &manifest_hash, 7).unwrap();
    scratch.push_identifier_replacement(ResolutionIdentifierRow {
        version_id: exact_version,
        identifier_id: "identifier-1".to_string(),
        target_version_id: Some(exact_version),
        target_symbol_id: Some("symbol-1".to_string()),
        tier: Some(2),
        confidence: Some(0.9),
        method: Some("exact".to_string()),
        outcome: "resolved".to_string(),
        candidates: Some(1),
    });
    scratch.finish().unwrap();
    let scratch = ResolutionScratchReader::open(&scratch_path).unwrap();
    let publication = ResolutionExactPublish {
        view_id: "view-wall".to_string(),
        manifest_generation: 1,
        manifest_hash,
        base_id: base,
        previous_delta_generation: converging.delta_generation,
        resolver_output_epoch: 7,
        request_id: "request-wall-expired".to_string(),
        created_at: NOW.to_string(),
    };
    claim_resolution_request(&layout, &publication.request_id);
    let now_ms = wall_now_ms();
    // expires_at is past wall clock, but still after a stale fence.now_ms.
    // Old logic compared expires_at > fence.now_ms and would accept this lease.
    let expires_at = now_ms - 1_000;
    let stale_now_ms = now_ms - 60_000;
    Connection::open(layout.coordinator_db())
        .unwrap()
        .execute(
            "INSERT OR REPLACE INTO writer_lease
             (resource,holder_id,holder_version,holder_pid,heartbeat_at,expires_at,fencing_token)
             VALUES ('store-writer','holder-1',?1,42,?2,?3,7)",
            rusqlite::params![VERSION, stale_now_ms, expires_at],
        )
        .unwrap();
    assert!(expires_at > stale_now_ms);
    assert!(expires_at < now_ms);
    let fence = ResolutionPublicationFence {
        claim_owner: "holder-1".to_string(),
        holder_id: "holder-1".to_string(),
        holder_pid: 42,
        fencing_token: 7,
        now_ms: stale_now_ms,
    };
    let before = view_publication_counts(&layout, "view-wall");
    let result = bindings.publish_exact(&publication, &fence, &scratch, &[], 1, || Ok(()));
    assert!(
        matches!(result, Err(ResolutionBindingError::FenceLost { .. })),
        "{result:?}"
    );
    assert_eq!(view_publication_counts(&layout, "view-wall"), before);
    let state: String = Connection::open(layout.store_db())
        .unwrap()
        .query_row(
            "SELECT resolution_state FROM views WHERE view_id='view-wall'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_ne!(state, "exact");
}

#[test]
fn exact_publish_heartbeats_once_before_begin_immediate_and_rolls_back_on_heartbeat_loss() {
    let temp = TempDir::new();
    let layout = StoreLayout::create(&temp.0, FAMILY_ID, VERSION).unwrap();
    let (base_hash, base_version) = publish_manifest(&layout, "view-a", None, "src/a.rs", "a");
    let factory = StoreConnectionFactory::new(layout.clone(), FAMILY_ID, VERSION);
    let base = ready_empty_base(&factory, &base_hash, base_version, "request-base");
    let bindings = ResolutionBindingStore::new(factory);
    let (manifest_hash, exact_version) =
        publish_manifest(&layout, "view-hb", None, "src/b.rs", "b");
    insert_symbol(&layout, exact_version, "symbol-1", "src/b.rs");
    let converging = bindings
        .bind_base("view-hb", 7, "request-bind-hb", NOW)
        .unwrap();
    let scratch_path = temp.0.join("heartbeat-delta.db");
    let mut scratch = ResolutionScratchDelta::new(&scratch_path, &manifest_hash, 7).unwrap();
    scratch.push_identifier_replacement(ResolutionIdentifierRow {
        version_id: exact_version,
        identifier_id: "identifier-1".to_string(),
        target_version_id: Some(exact_version),
        target_symbol_id: Some("symbol-1".to_string()),
        tier: Some(2),
        confidence: Some(0.9),
        method: Some("exact".to_string()),
        outcome: "resolved".to_string(),
        candidates: Some(1),
    });
    scratch.finish().unwrap();
    let scratch = ResolutionScratchReader::open(&scratch_path).unwrap();
    let publication = ResolutionExactPublish {
        view_id: "view-hb".to_string(),
        manifest_generation: 1,
        manifest_hash,
        base_id: base,
        previous_delta_generation: converging.delta_generation,
        resolver_output_epoch: 7,
        request_id: "request-hb".to_string(),
        created_at: NOW.to_string(),
    };
    let fence = publication_fence(&layout, &publication.request_id);
    let mut markers = Vec::new();
    let mut heartbeat_count = 0u32;
    let before = view_publication_counts(&layout, "view-hb");
    let result = bindings.publish_exact_with_markers(
        &publication,
        &fence,
        &scratch,
        &[],
        1,
        || {
            heartbeat_count += 1;
            Err(ResolutionBindingError::FenceLost {
                request_id: publication.request_id.clone(),
            })
        },
        |marker| markers.push(marker),
    );
    assert_eq!(heartbeat_count, 1);
    assert_eq!(
        markers,
        [ResolutionPublicationMarker::StoreTransactionStart]
    );
    assert!(
        matches!(result, Err(ResolutionBindingError::FenceLost { .. })),
        "{result:?}"
    );
    assert_eq!(view_publication_counts(&layout, "view-hb"), before);
    let state: String = Connection::open(layout.store_db())
        .unwrap()
        .query_row(
            "SELECT resolution_state FROM views WHERE view_id='view-hb'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_ne!(state, "exact");
}

fn publication_counts(layout: &StoreLayout) -> (i64, i64, i64, i64, i64) {
    view_publication_counts(layout, "view-c")
}

fn view_publication_counts(layout: &StoreLayout, view_id: &str) -> (i64, i64, i64, i64, i64) {
    Connection::open(layout.store_db())
        .unwrap()
        .query_row(
            "SELECT
               (SELECT COUNT(*) FROM resolution_deltas),
               (SELECT COUNT(*) FROM resolution_identifier_deltas),
               (SELECT COUNT(*) FROM resolution_pending_deltas),
               (SELECT COUNT(*) FROM store_log),
               (SELECT resolution_delta_generation FROM views WHERE view_id=?1)",
            [view_id],
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

#[test]
fn cleanup_removes_only_unpinned_superseded_deltas_and_reaps_expired_pins() {
    let temp = TempDir::new();
    let layout = StoreLayout::create(&temp.0, FAMILY_ID, VERSION).unwrap();
    let (manifest_hash, version_id) = publish_manifest(&layout, "view-a", None, "src/a.rs", "a");
    let factory = StoreConnectionFactory::new(layout.clone(), FAMILY_ID, VERSION);
    ready_empty_base(&factory, &manifest_hash, version_id, "request-base");
    let bindings = ResolutionBindingStore::new(factory);
    let first = bindings
        .bind_base("view-a", 7, "request-bind-1", NOW)
        .unwrap();
    assert_eq!(first.state, ViewResolutionState::Exact);
    bindings
        .open_pin(
            "pin-long",
            ResolutionPinOwnerKind::Reader,
            "reader-long",
            "view-a",
            "2026-08-08T20:40:00Z",
            NOW,
        )
        .unwrap();
    bindings
        .open_pin(
            "pin-short",
            ResolutionPinOwnerKind::Reader,
            "reader-short",
            "view-a",
            "2026-08-08T20:31:00Z",
            NOW,
        )
        .unwrap();

    publish_manifest(&layout, "view-a", Some(1), "src/a.rs", "a2");
    let second = bindings
        .bind_base("view-a", 7, "request-bind-2", NOW)
        .unwrap();
    assert_ne!(second.delta_generation, first.delta_generation);
    assert!(
        bindings
            .pinned_delta("pin-short", "2026-08-08T20:32:00Z")
            .unwrap()
            .is_none()
    );
    assert_eq!(
        bindings.cleanup_superseded_deltas("view-a", NOW).unwrap(),
        0
    );
    assert!(
        bindings
            .release_pin("pin-long", ResolutionPinOwnerKind::Reader, "reader-long",)
            .unwrap()
    );
    assert_eq!(
        bindings
            .cleanup_superseded_deltas("view-a", "2026-08-08T20:32:00Z")
            .unwrap(),
        1
    );
    let connection = Connection::open(layout.store_db()).unwrap();
    assert_eq!(
        connection
            .query_row(
                "SELECT COUNT(*) FROM resolution_pins WHERE view_id='view-a'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
        0
    );
    assert_eq!(
        connection
            .query_row(
                "SELECT COUNT(*) FROM resolution_deltas WHERE view_id='view-a'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
        1
    );
}

#[test]
fn binding_never_reuses_an_exact_head_from_another_resolver_epoch() {
    let temp = TempDir::new();
    let layout = StoreLayout::create(&temp.0, FAMILY_ID, VERSION).unwrap();
    let (manifest_hash, version_id) = publish_manifest(&layout, "view-a", None, "src/a.rs", "a");
    let factory = StoreConnectionFactory::new(layout, FAMILY_ID, VERSION);
    let base7 = ready_empty_base_epoch(&factory, &manifest_hash, version_id, 7, "request-base-7");
    let base8 = ready_empty_base_epoch(&factory, &manifest_hash, version_id, 8, "request-base-8");
    let bindings = ResolutionBindingStore::new(factory);
    let first = bindings
        .bind_base("view-a", 7, "request-bind-7", NOW)
        .unwrap();
    let second = bindings
        .bind_base("view-a", 8, "request-bind-8", NOW)
        .unwrap();
    assert_eq!(first.base_id, base7);
    assert_eq!(second.base_id, base8);
    assert_ne!(first.delta_generation, second.delta_generation);
    assert_eq!(second.state, ViewResolutionState::Exact);
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn identical_manifest_reuses_ready_base_exactly_and_changed_manifest_binds_nearest() {
    let temp = TempDir::new();
    let layout = StoreLayout::create(&temp.0, FAMILY_ID, VERSION).unwrap();
    let (first_hash, first_version) = publish_manifest(&layout, "view-a", None, "src/a.rs", "a");
    let factory = StoreConnectionFactory::new(layout.clone(), FAMILY_ID, VERSION);
    let base = ready_empty_base(&factory, &first_hash, first_version, "request-base");
    let bindings = ResolutionBindingStore::new(factory.clone());

    let first = bindings
        .bind_base("view-a", 7, "request-bind-a", NOW)
        .unwrap();
    assert_eq!(first.state, ViewResolutionState::Exact);
    assert_eq!(first.base_id, base);
    assert_eq!(first.exact_at, Some(1));
    assert_eq!(
        bindings
            .open_pin(
                "pin-fractional",
                ResolutionPinOwnerKind::Reader,
                "reader-fractional",
                "view-a",
                "2026-08-08T20:30:00.1Z",
                NOW,
            )
            .unwrap()
            .expires_at,
        "2026-08-08T20:30:00.1Z"
    );
    bindings
        .release_pin(
            "pin-fractional",
            ResolutionPinOwnerKind::Reader,
            "reader-fractional",
        )
        .unwrap();

    publish_manifest(&layout, "view-b", None, "src/a.rs", "a");
    let reused = bindings
        .bind_base("view-b", 7, "request-bind-b", NOW)
        .unwrap();
    assert_eq!(reused.state, ViewResolutionState::Exact);
    assert_eq!(reused.base_id, base);

    publish_manifest(&layout, "view-c", None, "src/a.rs", "a");
    publish_manifest(&layout, "view-c", Some(1), "src/b.rs", "b");
    let nearest = bindings
        .bind_base("view-c", 7, "request-bind-c", NOW)
        .unwrap();
    assert_eq!(nearest.state, ViewResolutionState::Converging);
    assert_eq!(nearest.base_id, base);
    assert_eq!(nearest.exact_at, None);
    assert_eq!(
        Connection::open(layout.store_db())
            .unwrap()
            .query_row("SELECT COUNT(*) FROM resolution_deltas", [], |row| {
                row.get::<_, i64>(0)
            })
            .unwrap(),
        3
    );

    let reader_pin = bindings
        .open_pin(
            "pin-reader",
            ResolutionPinOwnerKind::Reader,
            "reader-1",
            "view-a",
            "2026-08-08T20:35:00Z",
            NOW,
        )
        .unwrap();
    assert_eq!(reader_pin.delta_generation, Some(first.delta_generation));
    assert_eq!(
        bindings
            .pinned_delta("pin-reader", NOW)
            .unwrap()
            .unwrap()
            .delta_generation,
        first.delta_generation
    );

    let resolve_pin = bindings
        .open_pin(
            "pin-resolve",
            ResolutionPinOwnerKind::Resolve,
            "resolve-1",
            "view-c",
            "2026-08-08T20:35:00Z",
            NOW,
        )
        .unwrap();
    assert_eq!(resolve_pin.delta_generation, None);
    assert!(bindings.pinned_delta("pin-resolve", NOW).unwrap().is_none());
    assert!(
        bindings
            .pinned_identifier_delta_window("pin-resolve", NOW, None, 1)
            .unwrap()
            .is_empty()
    );
    assert!(
        bindings
            .pinned_pending_delta_window("pin-resolve", NOW, None, 1)
            .unwrap()
            .is_empty()
    );
    assert!(matches!(
        bindings.renew_pin(
            "pin-reader",
            ResolutionPinOwnerKind::Reader,
            "reader-2",
            "2026-08-08T20:40:00Z",
            NOW,
        ),
        Err(ResolutionBindingError::PinOwnerMismatch { .. })
    ));
    assert_eq!(
        bindings
            .renew_pin(
                "pin-reader",
                ResolutionPinOwnerKind::Reader,
                "reader-1",
                "2026-08-08T20:40:00Z",
                NOW,
            )
            .unwrap()
            .expires_at,
        "2026-08-08T20:40:00Z"
    );
    assert!(
        bindings
            .release_pin("pin-reader", ResolutionPinOwnerKind::Reader, "reader-1",)
            .unwrap()
    );
}

fn wall_now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| i64::try_from(duration.as_millis()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}

fn publication_fence(layout: &StoreLayout, request_id: &str) -> ResolutionPublicationFence {
    claim_resolution_request(layout, request_id);
    let now_ms = wall_now_ms();
    let connection = Connection::open(layout.coordinator_db()).unwrap();
    connection
        .execute(
            "INSERT OR REPLACE INTO writer_lease
             (resource,holder_id,holder_version,holder_pid,heartbeat_at,expires_at,fencing_token)
             VALUES ('store-writer','holder-1',?1,42,?2,?3,7)",
            rusqlite::params![VERSION, now_ms, now_ms + 60_000],
        )
        .unwrap();
    ResolutionPublicationFence {
        claim_owner: "holder-1".to_string(),
        holder_id: "holder-1".to_string(),
        holder_pid: 42,
        fencing_token: 7,
        now_ms,
    }
}

fn claim_resolution_request(layout: &StoreLayout, request_id: &str) {
    let connection = Connection::open(layout.coordinator_db()).unwrap();
    connection
        .execute(
            "INSERT OR REPLACE INTO requests
             (request_id,idempotency_key,kind,payload_json,state,requester_id,
              requester_deadline,claim_owner,claim_heartbeat_at,terminal_log_sequence,
              result_json,error_json,created_at,updated_at)
             VALUES (?1,?2,'resolve','{}','claimed','requester',NULL,'holder-1',1000,
                     NULL,NULL,NULL,1000,1000)",
            rusqlite::params![request_id, format!("key-{request_id}")],
        )
        .unwrap();
}

fn insert_symbol(layout: &StoreLayout, version_id: i64, symbol_id: &str, path: &str) {
    Connection::open(layout.store_db())
        .unwrap()
        .execute(
            "INSERT INTO symbols
             (version_id,symbol_id,path,language,name,kind,start_line,start_column,
              end_line,end_column,start_byte,end_byte)
             VALUES (?1,?2,?3,'rust','symbol','function',1,0,1,1,0,1)",
            rusqlite::params![version_id, symbol_id, path],
        )
        .unwrap();
}

fn publish_manifest(
    layout: &StoreLayout,
    view_id: &str,
    expected: Option<u64>,
    path: &str,
    hash: &str,
) -> (String, i64) {
    let mut connection = Connection::open(layout.store_db()).unwrap();
    connection
        .execute(
            "INSERT OR IGNORE INTO file_versions
             (path,content_hash,extraction_epoch,language,content_bytes,complete_l1,complete_l2)
             VALUES (?1,?2,1,'rust',1,1,2)",
            rusqlite::params![path, format!("blake3:{hash}")],
        )
        .unwrap();
    let version_id = connection
        .query_row(
            "SELECT version_id FROM file_versions
             WHERE path=?1 AND content_hash=?2 AND extraction_epoch=1",
            rusqlite::params![path, format!("blake3:{hash}")],
            |row| row.get(0),
        )
        .unwrap();
    let mut manifests = ManifestStore::new(&mut connection);
    if expected.is_none() {
        manifests.ensure_view(view_id, "/repo").unwrap();
    }
    let result = manifests
        .publish(
            view_id,
            expected,
            [ManifestEntry::indexed(
                path,
                "rust",
                version_id,
                format!("blake3:{hash}"),
                NOW,
            )],
            &format!("manifest-{view_id}-{hash}"),
        )
        .unwrap();
    let manifest_hash = connection
        .query_row(
            "SELECT manifest_hash FROM manifests WHERE view_id=?1 AND generation=?2",
            rusqlite::params![view_id, i64::try_from(result.generation).unwrap()],
            |row| row.get(0),
        )
        .unwrap();
    (manifest_hash, version_id)
}

fn ready_empty_base(
    factory: &StoreConnectionFactory,
    manifest_hash: &str,
    version_id: i64,
    request_id: &str,
) -> String {
    ready_empty_base_epoch(factory, manifest_hash, version_id, 7, request_id)
}

fn ready_empty_base_epoch(
    factory: &StoreConnectionFactory,
    manifest_hash: &str,
    version_id: i64,
    resolver_output_epoch: i64,
    request_id: &str,
) -> String {
    let catalog = ResolutionBaseCatalog::new(factory.clone());
    let build = match catalog
        .begin_build(manifest_hash, resolver_output_epoch, request_id, NOW)
        .unwrap()
    {
        ResolutionBaseBegin::Build(build) => build,
        other => panic!("expected build, got {other:?}"),
    };
    let mut writer =
        ResolutionBaseWriter::new(&build.scratch_path, manifest_hash, resolver_output_epoch)
            .unwrap();
    writer.push_source_version(version_id).unwrap();
    writer.finish_with_target_lookup(|_, _| Ok(true)).unwrap();
    catalog.publish_scratch(&build).unwrap();
    catalog.mark_ready(&build, NOW).unwrap().base_id
}
