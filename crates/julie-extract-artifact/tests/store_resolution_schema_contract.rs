use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use julie_extract_artifact::store::{
    ResolutionBaseBuilder, ResolutionBaseReader, ResolutionIdentifierRow, ResolutionPendingRow,
    ResolutionScratchDelta, ResolutionSemanticCounts, resolution_base_catalog_hash,
    resolution_scratch_catalog_hash,
};
use rusqlite::Connection;

const BASE_AUTHORITY: &str =
    include_str!("../../../docs/contracts/sqlite-resolution-base-schema-v1.md");
const SCRATCH_AUTHORITY: &str =
    include_str!("../../../docs/contracts/sqlite-resolution-delta-schema-v1.md");

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
            "julie-resolution-{label}-{}-{nonce}-{sequence}",
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

fn identifier(version_id: i64, identifier_id: &str) -> ResolutionIdentifierRow {
    ResolutionIdentifierRow {
        version_id,
        identifier_id: identifier_id.to_string(),
        target_version_id: Some(20),
        target_symbol_id: Some("symbol-4".to_string()),
        tier: Some(1),
        confidence: Some(0.95),
        method: Some("same_file".to_string()),
        outcome: "resolved".to_string(),
        candidates: Some(1),
    }
}

fn pending(version_id: i64, pending_relationship_id: &str) -> ResolutionPendingRow {
    ResolutionPendingRow {
        version_id,
        pending_relationship_id: pending_relationship_id.to_string(),
        target_version_id: 20,
        target_symbol_id: "symbol-4".to_string(),
        tier: 2,
        confidence: 0.85,
        method: "import_guided".to_string(),
    }
}

#[test]
fn production_catalogs_match_their_checked_in_hashes() {
    let temp = TempDir::new("catalog");
    let base_path = temp.path().join("base.db");
    let scratch_path = temp.path().join("scratch.db");
    let visible = BTreeSet::from([(20, "symbol-4".to_string())]);

    let expected_base_catalog = ResolutionBaseBuilder::new(&base_path, "manifest-a", 6, [10, 20])
        .unwrap()
        .catalog_hash();
    let mut base = ResolutionBaseBuilder::new(&base_path, "manifest-a", 6, [10, 20]).unwrap();
    base.push_identifier_resolution(identifier(10, "identifier-1"));
    base.push_pending_resolution(pending(10, "pending-2"));
    base.finish(&visible).unwrap();

    let expected_scratch_catalog = ResolutionScratchDelta::new(&scratch_path, "manifest-a", 6)
        .unwrap()
        .catalog_hash();
    let mut scratch = ResolutionScratchDelta::new(&scratch_path, "manifest-a", 6).unwrap();
    scratch.push_identifier_replacement(identifier(10, "identifier-1"));
    scratch.push_pending_replacement(pending(10, "pending-2"));
    scratch.finish().unwrap();

    let base_conn = Connection::open(&base_path).unwrap();
    let scratch_conn = Connection::open(&scratch_path).unwrap();
    assert_eq!(
        resolution_base_catalog_hash(&base_conn).unwrap(),
        expected_base_catalog
    );
    assert_eq!(
        resolution_scratch_catalog_hash(&scratch_conn).unwrap(),
        expected_scratch_catalog
    );
    assert_eq!(
        resolution_base_catalog_hash(&base_conn).unwrap(),
        authority_hash(BASE_AUTHORITY, "resolution-base-catalog-sha256")
    );
    assert_eq!(
        resolution_scratch_catalog_hash(&scratch_conn).unwrap(),
        authority_hash(SCRATCH_AUTHORITY, "resolution-scratch-catalog-sha256")
    );
    assert_eq!(
        base_conn
            .query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))
            .unwrap(),
        1
    );
    assert_eq!(
        scratch_conn
            .query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))
            .unwrap(),
        1
    );
}

#[test]
fn catalogs_have_exact_tables_columns_checks_and_named_indexes() {
    let temp = TempDir::new("shape");
    let base_path = temp.path().join("base.db");
    let scratch_path = temp.path().join("scratch.db");
    let visible = BTreeSet::from([(20, "symbol-4".to_string())]);
    let mut base = ResolutionBaseBuilder::new(&base_path, "manifest-a", 6, [10, 20]).unwrap();
    base.push_identifier_resolution(identifier(10, "identifier-1"));
    base.push_pending_resolution(pending(10, "pending-2"));
    base.finish(&visible).unwrap();
    let mut scratch = ResolutionScratchDelta::new(&scratch_path, "manifest-a", 6).unwrap();
    scratch.push_identifier_replacement(identifier(10, "identifier-1"));
    scratch.push_pending_replacement(pending(10, "pending-2"));
    scratch.push_pending_tombstone(10, "pending-3");
    scratch.finish().unwrap();

    let base = Connection::open(&base_path).unwrap();
    let scratch = Connection::open(&scratch_path).unwrap();
    assert_eq!(
        ordinary_tables(&base),
        [
            "base_meta",
            "identifier_resolutions",
            "pending_resolutions",
            "resolution_base_versions"
        ]
    );
    assert_eq!(
        ordinary_tables(&scratch),
        [
            "delta_meta",
            "identifier_replacements",
            "pending_replacements",
            "pending_tombstones"
        ]
    );
    assert_eq!(
        columns(&base, "identifier_resolutions"),
        [
            "version_id",
            "identifier_id",
            "target_version_id",
            "target_symbol_id",
            "tier",
            "confidence",
            "method",
            "outcome",
            "candidates"
        ]
    );
    assert_eq!(
        columns(&base, "pending_resolutions"),
        [
            "version_id",
            "pending_relationship_id",
            "target_version_id",
            "target_symbol_id",
            "tier",
            "confidence",
            "method"
        ]
    );
    assert_eq!(
        columns(&scratch, "pending_tombstones"),
        ["version_id", "pending_relationship_id"]
    );
    for (connection, table, checks) in [
        (
            &base,
            "identifier_resolutions",
            vec![
                "outcome IN",
                "outcome = 'resolved'",
                "version_id > 0",
                "length(identifier_id) > 0",
                "target_version_id IS NULL OR target_version_id > 0",
                "target_symbol_id IS NULL OR length(target_symbol_id) > 0",
            ],
        ),
        (
            &base,
            "pending_resolutions",
            vec![
                "version_id > 0",
                "length(pending_relationship_id) > 0",
                "target_version_id > 0",
                "confidence >= 0.0",
                "length(method) > 0",
            ],
        ),
        (
            &scratch,
            "identifier_replacements",
            vec![
                "outcome IN",
                "outcome = 'resolved'",
                "version_id > 0",
                "length(identifier_id) > 0",
                "target_symbol_id IS NULL OR length(target_symbol_id) > 0",
            ],
        ),
        (
            &scratch,
            "pending_replacements",
            vec!["version_id > 0", "length(pending_relationship_id) > 0"],
        ),
        (
            &scratch,
            "pending_tombstones",
            vec!["version_id > 0", "length(pending_relationship_id) > 0"],
        ),
    ] {
        let sql: String = connection
            .query_row(
                "SELECT sql FROM sqlite_master WHERE type='table' AND name=?1",
                [table],
                |row| row.get(0),
            )
            .unwrap();
        for check in checks {
            assert!(sql.contains(check), "{table} omitted {check}");
        }
    }
    assert_eq!(foreign_key_count(&base, "identifier_resolutions"), 2);
    assert_eq!(foreign_key_count(&base, "pending_resolutions"), 2);
    assert_eq!(
        named_indexes(&base),
        [
            "idx_export_resolution_identifiers_order",
            "idx_export_resolution_pending_order",
            "idx_read_resolution_identifiers_target",
            "idx_read_resolution_pending_target"
        ]
    );
    assert_eq!(
        named_indexes(&scratch),
        [
            "idx_export_resolution_identifier_replacements_order",
            "idx_export_resolution_pending_replacements_order",
            "idx_export_resolution_pending_tombstones_order",
            "idx_read_resolution_identifier_replacements_target",
            "idx_read_resolution_pending_replacements_target"
        ]
    );
}

#[test]
fn shuffled_input_produces_identical_semantic_bytes() {
    let temp = TempDir::new("deterministic");
    let first_path = temp.path().join("first.db");
    let second_path = temp.path().join("second.db");
    let visible = BTreeSet::from([(20, "symbol-4".to_string())]);
    let mut first = ResolutionBaseBuilder::new(&first_path, "manifest-a", 6, [10, 20]).unwrap();
    first.push_identifier_resolution(identifier(20, "identifier-2"));
    first.push_identifier_resolution(identifier(10, "identifier-1"));
    first.push_pending_resolution(pending(20, "pending-2"));
    first.push_pending_resolution(pending(10, "pending-1"));
    first.finish(&visible).unwrap();
    let mut second = ResolutionBaseBuilder::new(&second_path, "manifest-a", 6, [20, 10]).unwrap();
    second.push_pending_resolution(pending(10, "pending-1"));
    second.push_identifier_resolution(identifier(10, "identifier-1"));
    second.push_pending_resolution(pending(20, "pending-2"));
    second.push_identifier_resolution(identifier(20, "identifier-2"));
    second.finish(&visible).unwrap();
    let first_bytes = fs::read(&first_path).unwrap();
    let second_bytes = fs::read(&second_path).unwrap();
    assert_eq!(first_bytes, second_bytes);

    let first_delta_path = temp.path().join("first-delta.db");
    let second_delta_path = temp.path().join("second-delta.db");
    let mut first_delta = ResolutionScratchDelta::new(&first_delta_path, "manifest-a", 6).unwrap();
    first_delta.push_identifier_replacement(identifier(20, "identifier-2"));
    first_delta.push_identifier_replacement(identifier(10, "identifier-1"));
    first_delta.push_pending_replacement(pending(20, "pending-2"));
    first_delta.push_pending_replacement(pending(10, "pending-1"));
    first_delta.push_pending_tombstone(20, "pending-3");
    first_delta.finish().unwrap();
    let mut second_delta =
        ResolutionScratchDelta::new(&second_delta_path, "manifest-a", 6).unwrap();
    second_delta.push_pending_tombstone(20, "pending-3");
    second_delta.push_pending_replacement(pending(10, "pending-1"));
    second_delta.push_identifier_replacement(identifier(10, "identifier-1"));
    second_delta.push_pending_replacement(pending(20, "pending-2"));
    second_delta.push_identifier_replacement(identifier(20, "identifier-2"));
    second_delta.finish().unwrap();
    assert_eq!(
        fs::read(&first_delta_path).unwrap(),
        fs::read(&second_delta_path).unwrap()
    );
}

#[test]
fn readers_reject_corrupt_metadata_counts_and_targets() {
    let temp = TempDir::new("corrupt");
    let visible = BTreeSet::from([(20, "symbol-4".to_string())]);
    let path = temp.path().join("base-1.db");
    let mut builder = ResolutionBaseBuilder::new(&path, "manifest-a", 6, [10, 20]).unwrap();
    builder.push_identifier_resolution(identifier(10, "identifier-1"));
    builder.push_pending_resolution(pending(10, "pending-2"));
    builder.finish(&visible).unwrap();

    let connection = Connection::open(&path).unwrap();
    connection
        .execute(
            "UPDATE base_meta SET value='wrong' WHERE key='catalog_sha256'",
            [],
        )
        .unwrap();
    let error = ResolutionBaseReader::open(&path).unwrap_err();
    assert!(matches!(
        error,
        julie_extract_artifact::store::ResolutionValidationError::CatalogHashMismatch { .. }
    ));
    drop(connection);

    let path = temp.path().join("base-2.db");
    let mut builder = ResolutionBaseBuilder::new(&path, "manifest-a", 6, [10, 20]).unwrap();
    builder.push_identifier_resolution(identifier(10, "identifier-1"));
    builder.push_pending_resolution(pending(10, "pending-2"));
    builder.finish(&visible).unwrap();
    let connection = Connection::open(&path).unwrap();
    connection
        .execute(
            "UPDATE base_meta SET value='99' WHERE key='identifier_count'",
            [],
        )
        .unwrap();
    drop(connection);
    let error = ResolutionBaseReader::open(&path).unwrap_err();
    assert!(matches!(
        error,
        julie_extract_artifact::store::ResolutionValidationError::RowCountMismatch { .. }
    ));

    let path = temp.path().join("base-3.db");
    let mut builder = ResolutionBaseBuilder::new(&path, "manifest-a", 6, [10, 20]).unwrap();
    builder.push_identifier_resolution(identifier(10, "identifier-1"));
    builder.push_pending_resolution(pending(10, "pending-2"));
    builder.finish(&visible).unwrap();
    let connection = Connection::open(&path).unwrap();
    connection
        .execute(
            "UPDATE identifier_resolutions SET target_symbol_id='missing' WHERE version_id=10",
            [],
        )
        .unwrap();
    drop(connection);
    let reader = ResolutionBaseReader::open(&path).unwrap();
    let error = reader.validate_targets(&visible).unwrap_err();
    assert!(matches!(
        error,
        julie_extract_artifact::store::ResolutionValidationError::TargetMissing { .. }
    ));
}

#[test]
fn readers_reject_completed_files_with_corrupt_integrity_or_identity() {
    let temp = TempDir::new("reader-integrity");
    let visible = BTreeSet::from([(20, "symbol-4".to_string())]);

    let base_path = temp.path().join("base-fk.db");
    let mut base = ResolutionBaseBuilder::new(&base_path, "manifest-a", 6, [10, 20]).unwrap();
    base.push_identifier_resolution(identifier(10, "identifier-1"));
    base.finish(&visible).unwrap();
    let connection = Connection::open(&base_path).unwrap();
    connection
        .execute_batch("PRAGMA foreign_keys=OFF;")
        .unwrap();
    connection
        .execute(
            "INSERT INTO identifier_resolutions (version_id,identifier_id,target_version_id,target_symbol_id,outcome) VALUES (999,'orphan',888,'symbol-4','resolved')",
            [],
        )
        .unwrap();
    connection
        .execute(
            "UPDATE base_meta SET value='2' WHERE key='identifier_count'",
            [],
        )
        .unwrap();
    drop(connection);
    assert!(matches!(
        ResolutionBaseReader::open(&base_path),
        Err(julie_extract_artifact::store::ResolutionValidationError::InvalidMetadata { key, .. })
            if key == "foreign_key_check"
    ));

    let base_key_path = temp.path().join("base-check.db");
    let mut base = ResolutionBaseBuilder::new(&base_key_path, "manifest-a", 6, [10, 20]).unwrap();
    base.push_identifier_resolution(identifier(10, "identifier-1"));
    base.finish(&visible).unwrap();
    let connection = Connection::open(&base_key_path).unwrap();
    connection
        .execute_batch("PRAGMA ignore_check_constraints=ON;")
        .unwrap();
    connection
        .execute(
            "UPDATE identifier_resolutions SET identifier_id='' WHERE version_id=10",
            [],
        )
        .unwrap();
    drop(connection);
    assert!(matches!(
        ResolutionBaseReader::open(&base_key_path),
        Err(julie_extract_artifact::store::ResolutionValidationError::InvalidMetadata { key, .. })
            if key == "row_check"
    ));

    let scratch_path = temp.path().join("scratch-check.db");
    let mut scratch = ResolutionScratchDelta::new(&scratch_path, "manifest-a", 6).unwrap();
    scratch.push_identifier_replacement(identifier(10, "identifier-1"));
    scratch.finish().unwrap();
    let connection = Connection::open(&scratch_path).unwrap();
    connection
        .execute_batch("PRAGMA ignore_check_constraints=ON;")
        .unwrap();
    connection
        .execute(
            "UPDATE identifier_replacements SET identifier_id='' WHERE version_id=10",
            [],
        )
        .unwrap();
    drop(connection);
    assert!(matches!(
        julie_extract_artifact::store::ResolutionScratchReader::open(&scratch_path),
        Err(julie_extract_artifact::store::ResolutionValidationError::InvalidMetadata { key, .. })
            if key == "row_check"
    ));

    let base_meta_path = temp.path().join("base-meta.db");
    let mut base = ResolutionBaseBuilder::new(&base_meta_path, "manifest-a", 6, [10, 20]).unwrap();
    base.push_identifier_resolution(identifier(10, "identifier-1"));
    base.finish(&visible).unwrap();
    let connection = Connection::open(&base_meta_path).unwrap();
    connection
        .execute(
            "UPDATE base_meta SET value='' WHERE key='manifest_hash'",
            [],
        )
        .unwrap();
    drop(connection);
    assert!(matches!(
        ResolutionBaseReader::open(&base_meta_path),
        Err(julie_extract_artifact::store::ResolutionValidationError::InvalidMetadata { key, .. })
            if key == "manifest_hash"
    ));

    let scratch_meta_path = temp.path().join("scratch-meta.db");
    let mut scratch = ResolutionScratchDelta::new(&scratch_meta_path, "manifest-a", 6).unwrap();
    scratch.push_identifier_replacement(identifier(10, "identifier-1"));
    scratch.finish().unwrap();
    let connection = Connection::open(&scratch_meta_path).unwrap();
    connection
        .execute(
            "UPDATE delta_meta SET value='' WHERE key='manifest_hash'",
            [],
        )
        .unwrap();
    drop(connection);
    assert!(matches!(
        julie_extract_artifact::store::ResolutionScratchReader::open(&scratch_meta_path),
        Err(julie_extract_artifact::store::ResolutionValidationError::InvalidMetadata { key, .. })
            if key == "manifest_hash"
    ));

    let collision_path = temp.path().join("scratch-collision.db");
    let mut scratch = ResolutionScratchDelta::new(&collision_path, "manifest-a", 6).unwrap();
    scratch.push_pending_replacement(pending(10, "pending-2"));
    scratch.finish().unwrap();
    let connection = Connection::open(&collision_path).unwrap();
    connection
        .execute(
            "INSERT INTO pending_tombstones(version_id,pending_relationship_id) VALUES (10,'pending-2')",
            [],
        )
        .unwrap();
    connection
        .execute(
            "UPDATE delta_meta SET value='1' WHERE key='pending_tombstone_count'",
            [],
        )
        .unwrap();
    drop(connection);
    assert!(matches!(
        julie_extract_artifact::store::ResolutionScratchReader::open(&collision_path),
        Err(julie_extract_artifact::store::ResolutionValidationError::InvalidMetadata { key, .. })
            if key == "row_check"
    ));
}

#[test]
fn base_rejects_target_version_missing_from_version_roots() {
    let temp = TempDir::new("missing-root");
    let path = temp.path().join("base.db");
    let visible = BTreeSet::from([(20, "symbol-4".to_string())]);
    let mut builder = ResolutionBaseBuilder::new(&path, "manifest-a", 6, [10]).unwrap();
    builder.push_identifier_resolution(identifier(10, "identifier-1"));
    let error = builder.finish(&visible).unwrap_err();
    assert!(matches!(
        error,
        julie_extract_artifact::store::ResolutionValidationError::VersionRootMissing {
            version_id: 20
        }
    ));
    assert!(!path.exists());

    let pending_path = temp.path().join("pending-missing-root.db");
    let mut pending_builder =
        ResolutionBaseBuilder::new(&pending_path, "manifest-a", 6, [10]).unwrap();
    pending_builder.push_pending_resolution(pending(10, "pending-2"));
    let error = pending_builder.finish(&visible).unwrap_err();
    assert!(matches!(
        error,
        julie_extract_artifact::store::ResolutionValidationError::VersionRootMissing {
            version_id: 20
        }
    ));
    assert!(!pending_path.exists());

    let source_path = temp.path().join("source-missing-root.db");
    let mut source_builder =
        ResolutionBaseBuilder::new(&source_path, "manifest-a", 6, [10]).unwrap();
    source_builder.push_identifier_resolution(identifier(20, "identifier-1"));
    let error = source_builder.finish(&visible).unwrap_err();
    assert!(matches!(
        error,
        julie_extract_artifact::store::ResolutionValidationError::VersionRootMissing {
            version_id: 20
        }
    ));
    assert!(!source_path.exists());
}

#[test]
fn scratch_reader_rejects_incomplete_file() {
    let temp = TempDir::new("incomplete");
    let path = temp.path().join("scratch.db");
    let connection = Connection::open(&path).unwrap();
    connection
        .execute_batch(julie_extract_artifact::store::RESOLUTION_SCRATCH_SQL)
        .unwrap();
    let hash = resolution_scratch_catalog_hash(&connection).unwrap();
    connection
        .execute_batch(&format!("INSERT INTO delta_meta(key,value) VALUES ('format_version','1'),('catalog_sha256','{hash}'),('manifest_hash','manifest-a'),('resolver_output_epoch','6'),('identifier_replacement_count','0'),('pending_replacement_count','0'),('pending_tombstone_count','0'),('completed','0')"))
        .unwrap();
    drop(connection);
    let error = julie_extract_artifact::store::ResolutionScratchReader::open(&path).unwrap_err();
    assert!(matches!(
        error,
        julie_extract_artifact::store::ResolutionValidationError::CatalogHashMismatch { .. }
            | julie_extract_artifact::store::ResolutionValidationError::IncompleteFile
    ));
}

#[test]
fn base_reader_requires_complete_metadata_and_proves_targets() {
    let temp = TempDir::new("reader");
    let path = temp.path().join("base.db");
    let visible = BTreeSet::from([(20, "symbol-4".to_string())]);
    let mut builder = ResolutionBaseBuilder::new(&path, "manifest-a", 6, [10, 20]).unwrap();
    builder.push_identifier_resolution(identifier(10, "identifier-1"));
    builder.push_pending_resolution(pending(10, "pending-2"));
    let identity = builder.finish(&visible).unwrap();

    let reader = ResolutionBaseReader::open(&path).unwrap();
    assert_eq!(reader.file_identity(), &identity);
    assert_eq!(reader.source_versions().unwrap(), vec![10, 20]);
    assert_eq!(
        reader.semantic_counts(),
        ResolutionSemanticCounts {
            identifiers: 1,
            pending: 1
        }
    );
    assert_eq!(
        reader.identifiers().unwrap(),
        vec![identifier(10, "identifier-1")]
    );
    assert_eq!(reader.pending().unwrap(), vec![pending(10, "pending-2")]);
}

#[test]
fn scratch_paths_must_be_contained_and_readers_reject_incomplete_files() {
    let temp = TempDir::new("paths");
    let outside = temp.path().join("outside.db");
    let escaped = temp
        .path()
        .join("nested")
        .join("..")
        .join("..")
        .join("outside.db");
    let error =
        ResolutionScratchDelta::new_contained(temp.path(), &escaped, "manifest-a", 6).unwrap_err();
    assert!(error.is_path_error());
    assert!(!outside.exists());

    let external = temp.path().join("external.db");
    fs::write(&outside, b"not sqlite").unwrap();
    assert!(ResolutionScratchDelta::new(&outside, "manifest-a", 6).is_err());
    assert_eq!(fs::read(&outside).unwrap(), b"not sqlite");
    #[cfg(unix)]
    std::os::unix::fs::symlink(&outside, &external).unwrap();
    #[cfg(unix)]
    assert!(ResolutionScratchDelta::new(&external, "manifest-a", 6).is_err());

    #[cfg(unix)]
    {
        let internal = temp.path().join("internal");
        fs::create_dir(&internal).unwrap();
        let link = temp.path().join("link");
        std::os::unix::fs::symlink(&internal, &link).unwrap();
        let child = link.join("new.db");
        assert!(ResolutionScratchDelta::new(&child, "manifest-a", 6).is_err());
        assert!(
            ResolutionScratchDelta::new_contained(temp.path(), &child, "manifest-a", 6).is_err()
        );
        assert!(!child.exists());
    }

    let dangling = temp.path().join("dangling.db");
    #[cfg(unix)]
    std::os::unix::fs::symlink(temp.path().join("missing.db"), &dangling).unwrap();
    #[cfg(unix)]
    assert!(ResolutionScratchDelta::new(&dangling, "manifest-a", 6).is_err());

    let directory = temp.path().join("directory.db");
    fs::create_dir(&directory).unwrap();
    assert!(ResolutionScratchDelta::new(&directory, "manifest-a", 6).is_err());

    let scratch_path = temp.path().join("scratch.db");
    let mut scratch = ResolutionScratchDelta::new(&scratch_path, "manifest-a", 6).unwrap();
    scratch.push_identifier_replacement(identifier(10, "identifier-1"));
    let incomplete = scratch.path().to_path_buf();
    scratch.abort().unwrap();
    assert!(!incomplete.exists());
}

fn authority_hash(document: &str, key: &str) -> String {
    document
        .lines()
        .find_map(|line| line.strip_prefix(&format!("{key}: ")))
        .expect("authority hash")
        .to_string()
}

fn ordinary_tables(connection: &Connection) -> Vec<String> {
    let mut rows = connection
        .prepare("SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%' ORDER BY name")
        .unwrap()
        .query_map([], |row| row.get::<_, String>(0))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    rows.sort();
    rows
}

fn columns(connection: &Connection, table: &str) -> Vec<String> {
    connection
        .prepare("SELECT name FROM pragma_table_info(?1) ORDER BY cid")
        .unwrap()
        .query_map([table], |row| row.get::<_, String>(0))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap()
}

fn foreign_key_count(connection: &Connection, table: &str) -> usize {
    let sql = format!("PRAGMA foreign_key_list({table})");
    connection
        .prepare(&sql)
        .unwrap()
        .query_map([], |_| Ok(()))
        .unwrap()
        .count()
}

fn named_indexes(connection: &Connection) -> Vec<String> {
    connection
        .prepare("SELECT name FROM sqlite_master WHERE type='index' AND name NOT LIKE 'sqlite_%' ORDER BY name")
        .unwrap()
        .query_map([], |row| row.get::<_, String>(0))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap()
}
