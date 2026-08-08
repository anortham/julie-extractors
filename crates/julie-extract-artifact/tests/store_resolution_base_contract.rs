#![cfg(feature = "test-store-resolution")]

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Barrier};
use std::time::{SystemTime, UNIX_EPOCH};

use julie_extract_artifact::store::{
    ManifestEntry, ManifestStore, ResolutionBaseBegin, ResolutionBaseCatalog,
    ResolutionBaseCatalogError, ResolutionBaseReader, ResolutionBaseRecovery, ResolutionBaseWriter,
    ResolutionIdentifierRow, ResolutionValidationError, StoreConnectionFactory, StoreLayout,
};
use rusqlite::{Connection, params};

const FAMILY_ID: &str = "123e4567-e89b-12d3-a456-426614174000";
const VERSION: &str = "2.30.0";
const NOW: &str = "2026-08-08T19:20:00Z";

static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

struct TempDir(PathBuf);

impl TempDir {
    fn new(label: &str) -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let sequence = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "julie-resolution-base-{label}-{}-{nonce}-{sequence}",
            std::process::id()
        ));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn base_registration_roots_versions_before_off_lease_build_and_ready_publication() {
    let temp = TempDir::new("lifecycle");
    let (layout, manifest_hash, version_id) = store_with_manifest(temp.path());
    let factory = StoreConnectionFactory::new(layout.clone(), FAMILY_ID, VERSION);
    let catalog = ResolutionBaseCatalog::new(factory);

    let build = match catalog
        .begin_build(&manifest_hash, 7, "request-a", NOW)
        .unwrap()
    {
        ResolutionBaseBegin::Build(build) => build,
        other => panic!("expected a new build, got {other:?}"),
    };

    let store = Connection::open(layout.store_db()).unwrap();
    assert_eq!(
        store
            .query_row(
                "SELECT state FROM resolution_bases WHERE base_id=?1",
                [&build.record.base_id],
                |row| row.get::<_, String>(0),
            )
            .unwrap(),
        "building"
    );
    assert_eq!(
        store
            .query_row(
                "SELECT version_id FROM resolution_base_versions WHERE base_id=?1",
                [&build.record.base_id],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
        version_id
    );
    drop(store);
    assert!(!build.final_path.exists());

    let mut writer = ResolutionBaseWriter::new(&build.scratch_path, &manifest_hash, 7).unwrap();
    writer.push_source_version(version_id).unwrap();
    let scratch_identity = writer.finish_with_target_lookup(|_, _| Ok(true)).unwrap();
    let published_identity = catalog.publish_scratch(&build).unwrap();
    assert_eq!(published_identity.file_sha256, scratch_identity.file_sha256);
    assert!(!build.scratch_path.exists());
    assert!(build.final_path.exists());

    let ready = catalog.mark_ready(&build, NOW).unwrap();
    assert_eq!(ready.state.as_str(), "ready");
    assert_eq!(
        ready.file_sha256.as_deref(),
        Some(&*published_identity.file_sha256)
    );
    assert_eq!(
        ResolutionBaseReader::open(&build.final_path)
            .unwrap()
            .source_versions()
            .unwrap(),
        vec![version_id]
    );
    assert_eq!(
        catalog.find_ready(&manifest_hash, 7).unwrap(),
        Some(ready.clone())
    );

    assert_eq!(
        catalog
            .begin_build(&manifest_hash, 7, "request-b", NOW)
            .unwrap(),
        ResolutionBaseBegin::Ready(ready)
    );
}

#[test]
fn recovery_promotes_a_valid_final_file_after_a_catalog_tear() {
    let temp = TempDir::new("recover-final");
    let (layout, manifest_hash, version_id) = store_with_manifest(temp.path());
    let catalog = catalog(&layout);
    let build = new_build(&catalog, &manifest_hash, "request-a");
    write_empty_base(&build.scratch_path, &manifest_hash, version_id);
    catalog.publish_scratch(&build).unwrap();

    let recovered = catalog
        .recover(&manifest_hash, 7, "request-b", false, NOW)
        .unwrap();
    let ready = match recovered {
        ResolutionBaseRecovery::Ready(ready) => ready,
        other => panic!("expected ready recovery, got {other:?}"),
    };
    assert_eq!(ready.request_id, "request-b");
    assert!(build.final_path.exists());
    assert_eq!(catalog.find_ready(&manifest_hash, 7).unwrap(), Some(ready));
}

#[test]
fn recovery_never_reaps_a_live_owner_file_but_rebuilds_after_owner_death() {
    let temp = TempDir::new("owner-proof");
    let (layout, manifest_hash, _) = store_with_manifest(temp.path());
    let catalog = catalog(&layout);
    let build = new_build(&catalog, &manifest_hash, "request-a");
    fs::write(&build.final_path, b"not sqlite").unwrap();
    fs::write(&build.scratch_path, b"live scratch").unwrap();

    assert!(matches!(
        catalog
            .recover(&manifest_hash, 7, "request-b", true, NOW)
            .unwrap(),
        ResolutionBaseRecovery::LiveOwner(_)
    ));
    assert_eq!(fs::read(&build.final_path).unwrap(), b"not sqlite");
    assert_eq!(fs::read(&build.scratch_path).unwrap(), b"live scratch");

    let replacement = match catalog
        .recover(&manifest_hash, 7, "request-b", false, NOW)
        .unwrap()
    {
        ResolutionBaseRecovery::Rebuild(build) => build,
        other => panic!("expected rebuild, got {other:?}"),
    };
    assert_eq!(replacement.record.request_id, "request-b");
    assert!(!build.final_path.exists());
    assert!(!build.scratch_path.exists());
}

#[test]
fn missing_ready_file_is_not_reset_while_a_pin_protects_the_base() {
    let temp = TempDir::new("pin-proof");
    let (layout, manifest_hash, version_id) = store_with_manifest(temp.path());
    let catalog = catalog(&layout);
    let build = new_build(&catalog, &manifest_hash, "request-a");
    write_empty_base(&build.scratch_path, &manifest_hash, version_id);
    catalog.publish_scratch(&build).unwrap();
    let ready = catalog.mark_ready(&build, NOW).unwrap();
    let connection = Connection::open(layout.store_db()).unwrap();
    connection
        .execute(
            "INSERT INTO resolution_pins
             (pin_id,owner_kind,owner_id,view_id,manifest_generation,base_id,
              delta_generation,expires_at,created_at)
             VALUES ('pin-a','reader','reader-a','view-a',1,?1,NULL,?2,?2)",
            params![ready.base_id, NOW],
        )
        .unwrap();
    fs::remove_file(&build.final_path).unwrap();

    assert!(matches!(
        catalog
            .recover(&manifest_hash, 7, "request-b", false, NOW)
            .unwrap_err(),
        ResolutionBaseCatalogError::FileProtected { .. }
    ));
    assert_eq!(
        connection
            .query_row(
                "SELECT state FROM resolution_bases WHERE base_id=?1",
                [&ready.base_id],
                |row| row.get::<_, String>(0),
            )
            .unwrap(),
        "ready"
    );
    connection
        .execute("DELETE FROM resolution_pins WHERE pin_id='pin-a'", [])
        .unwrap();
    drop(connection);

    assert!(matches!(
        catalog
            .recover(&manifest_hash, 7, "request-b", false, NOW)
            .unwrap(),
        ResolutionBaseRecovery::Rebuild(_)
    ));
}

#[test]
fn concurrent_identical_registration_has_one_builder_and_one_catalog_identity() {
    let temp = TempDir::new("concurrent");
    let (layout, manifest_hash, _) = store_with_manifest(temp.path());
    let barrier = Arc::new(Barrier::new(2));
    let mut handles = Vec::new();
    for request_id in ["request-a", "request-b"] {
        let layout = layout.clone();
        let manifest_hash = manifest_hash.clone();
        let barrier = barrier.clone();
        handles.push(std::thread::spawn(move || {
            barrier.wait();
            catalog(&layout)
                .begin_build(&manifest_hash, 7, request_id, NOW)
                .unwrap()
        }));
    }
    let outcomes = handles
        .into_iter()
        .map(|handle| handle.join().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        outcomes
            .iter()
            .filter(|outcome| matches!(outcome, ResolutionBaseBegin::Build(_)))
            .count(),
        1
    );
    assert_eq!(
        outcomes
            .iter()
            .filter(|outcome| matches!(outcome, ResolutionBaseBegin::Building(_)))
            .count(),
        1
    );
    let base_ids = outcomes
        .iter()
        .map(|outcome| match outcome {
            ResolutionBaseBegin::Build(build) => build.record.base_id.as_str(),
            ResolutionBaseBegin::Building(record) => record.base_id.as_str(),
            ResolutionBaseBegin::Ready(record) => record.base_id.as_str(),
        })
        .collect::<Vec<_>>();
    assert_eq!(base_ids[0], base_ids[1]);
    assert_eq!(
        Connection::open(layout.store_db())
            .unwrap()
            .query_row("SELECT COUNT(*) FROM resolution_bases", [], |row| {
                row.get::<_, i64>(0)
            })
            .unwrap(),
        1
    );
}

#[test]
fn stale_and_successor_builders_publish_one_final_identity_and_clean_the_loser() {
    let temp = TempDir::new("concurrent-publish");
    let (layout, manifest_hash, version_id) = store_with_manifest(temp.path());
    let catalog = catalog(&layout);
    let stale = new_build(&catalog, &manifest_hash, "request-a");
    let successor = match catalog
        .recover(&manifest_hash, 7, "request-b", false, NOW)
        .unwrap()
    {
        ResolutionBaseRecovery::Rebuild(build) => build,
        other => panic!("expected successor rebuild, got {other:?}"),
    };
    write_empty_base(&stale.scratch_path, &manifest_hash, version_id);
    write_empty_base(&successor.scratch_path, &manifest_hash, version_id);
    let barrier = Arc::new(Barrier::new(2));
    let stale_handle = {
        let catalog = catalog.clone();
        let build = stale.clone();
        let barrier = barrier.clone();
        std::thread::spawn(move || {
            barrier.wait();
            catalog.publish_scratch(&build)
        })
    };
    let successor_handle = {
        let catalog = catalog.clone();
        let build = successor.clone();
        let barrier = barrier.clone();
        std::thread::spawn(move || {
            barrier.wait();
            catalog.publish_scratch(&build)
        })
    };
    stale_handle.join().unwrap().unwrap();
    successor_handle.join().unwrap().unwrap();

    let ready = catalog.mark_ready(&successor, NOW).unwrap();
    assert!(matches!(
        catalog.mark_ready(&stale, NOW).unwrap_err(),
        ResolutionBaseCatalogError::BuildOwnerMismatch { .. }
    ));
    assert_eq!(ready.base_id, successor.record.base_id);
    assert!(successor.final_path.is_file());
    assert!(!stale.scratch_path.exists());
    assert!(!successor.scratch_path.exists());
    assert_eq!(
        fs::read_dir(layout.bases_dir())
            .unwrap()
            .filter(|entry| entry
                .as_ref()
                .is_ok_and(|entry| { entry.path().extension().is_some_and(|value| value == "db") }))
            .count(),
        1
    );
}

#[test]
fn catalog_revalidates_targets_against_the_manifest_visible_store_before_publication() {
    let temp = TempDir::new("target-check");
    let (layout, manifest_hash, version_id) = store_with_manifest(temp.path());
    let catalog = catalog(&layout);
    let build = new_build(&catalog, &manifest_hash, "request-a");
    let mut writer = ResolutionBaseWriter::new(&build.scratch_path, &manifest_hash, 7).unwrap();
    writer.push_source_version(version_id).unwrap();
    writer
        .push_identifier_resolution(ResolutionIdentifierRow {
            version_id,
            identifier_id: "identifier-a".to_string(),
            target_version_id: Some(version_id),
            target_symbol_id: Some("missing-symbol".to_string()),
            tier: Some(1),
            confidence: Some(1.0),
            method: Some("test".to_string()),
            outcome: "resolved".to_string(),
            candidates: Some(1),
        })
        .unwrap();
    writer.finish_with_target_lookup(|_, _| Ok(true)).unwrap();

    assert!(matches!(
        catalog.publish_scratch(&build).unwrap_err(),
        ResolutionBaseCatalogError::Validation(ResolutionValidationError::TargetMissing { .. })
    ));
    assert!(!build.final_path.exists());
    assert!(build.scratch_path.exists());
}

#[test]
fn incomplete_manifest_version_refuses_before_catalog_or_scratch_mutation() {
    let temp = TempDir::new("incomplete");
    let layout = StoreLayout::create(temp.path(), FAMILY_ID, VERSION).unwrap();
    let mut connection = Connection::open(layout.store_db()).unwrap();
    connection
        .execute(
            "INSERT INTO file_versions
             (path,content_hash,extraction_epoch,language,content_bytes,complete_l1)
             VALUES ('src/lib.rs','blake3:incomplete',1,'rust',1,1)",
            [],
        )
        .unwrap();
    let version_id = connection.last_insert_rowid();
    {
        let mut manifests = ManifestStore::new(&mut connection);
        manifests.ensure_view("view-a", "/repo").unwrap();
        manifests
            .publish(
                "view-a",
                None,
                [ManifestEntry::indexed(
                    "src/lib.rs",
                    "rust",
                    version_id,
                    "blake3:incomplete",
                    NOW,
                )],
                "manifest-request",
            )
            .unwrap();
    }
    let manifest_hash: String = connection
        .query_row("SELECT manifest_hash FROM manifests", [], |row| row.get(0))
        .unwrap();

    assert!(matches!(
        catalog(&layout)
            .begin_build(&manifest_hash, 7, "request-a", NOW)
            .unwrap_err(),
        ResolutionBaseCatalogError::IncompleteVersion { version_id: found }
            if found == version_id
    ));
    assert_eq!(
        connection
            .query_row("SELECT COUNT(*) FROM resolution_bases", [], |row| {
                row.get::<_, i64>(0)
            })
            .unwrap(),
        0
    );
    assert_eq!(fs::read_dir(layout.scratch_dir()).unwrap().count(), 0);
    assert_eq!(fs::read_dir(layout.bases_dir()).unwrap().count(), 0);
}

fn catalog(layout: &StoreLayout) -> ResolutionBaseCatalog {
    ResolutionBaseCatalog::new(StoreConnectionFactory::new(
        layout.clone(),
        FAMILY_ID,
        VERSION,
    ))
}

fn new_build(
    catalog: &ResolutionBaseCatalog,
    manifest_hash: &str,
    request_id: &str,
) -> julie_extract_artifact::store::ResolutionBaseBuild {
    match catalog
        .begin_build(manifest_hash, 7, request_id, NOW)
        .unwrap()
    {
        ResolutionBaseBegin::Build(build) => build,
        other => panic!("expected build, got {other:?}"),
    }
}

fn write_empty_base(path: &Path, manifest_hash: &str, version_id: i64) {
    let mut writer = ResolutionBaseWriter::new(path, manifest_hash, 7).unwrap();
    writer.push_source_version(version_id).unwrap();
    writer.finish_with_target_lookup(|_, _| Ok(true)).unwrap();
}

fn store_with_manifest(root: &Path) -> (StoreLayout, String, i64) {
    let layout = StoreLayout::create(root, FAMILY_ID, VERSION).unwrap();
    let mut connection = Connection::open(layout.store_db()).unwrap();
    connection
        .execute(
            "INSERT INTO file_versions
             (path,content_hash,extraction_epoch,language,content_bytes,complete_l1,complete_l2)
             VALUES ('src/lib.rs','blake3:source',1,'rust',1,1,2)",
            [],
        )
        .unwrap();
    let version_id = connection.last_insert_rowid();
    let mut manifests = ManifestStore::new(&mut connection);
    manifests.ensure_view("view-a", "/repo").unwrap();
    let manifest = manifests
        .publish(
            "view-a",
            None,
            [ManifestEntry::indexed(
                "src/lib.rs",
                "rust",
                version_id,
                "blake3:source",
                NOW,
            )],
            "manifest-request",
        )
        .unwrap();
    let hash = connection
        .query_row(
            "SELECT manifest_hash FROM manifests WHERE view_id='view-a' AND generation=?1",
            params![i64::try_from(manifest.generation).unwrap()],
            |row| row.get(0),
        )
        .unwrap();
    (layout, hash, version_id)
}
