use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use julie_extract_artifact::store::{
    StoreConnectionError, StoreConnectionFactory, StoreLayout, StoreLayoutError, StoreSchemaError,
    create_coordinator_schema, create_store_schema,
};
use rusqlite::Connection;

#[test]
fn store_layout_creation_publishes_a_reopenable_generation() {
    let temp = TempStore::new("create");

    let created = StoreLayout::create(temp.path(), "family-a", "2.30.0").unwrap();

    assert_eq!(
        fs::read_to_string(temp.path().join("CURRENT")).unwrap(),
        "gen-001\n"
    );
    assert_eq!(created.generation_name(), "gen-001");
    assert!(created.store_db().is_file());
    assert!(created.coordinator_db().is_file());
    assert!(created.spool_dir().is_dir());
    assert!(created.scratch_dir().is_dir());
    assert!(created.bases_dir().is_dir());
    assert_eq!(created.bases_dir(), created.generation_dir().join("bases"));
    assert!(!temp.path().join("bases").exists());

    let metadata = store_metadata(created.store_db());
    assert_eq!(metadata_value(&metadata, "family_id"), "family-a");
    assert_eq!(metadata_value(&metadata, "extraction_identity_epoch"), "1");
    assert_eq!(metadata_value(&metadata, "min_reader_version"), "2.30.0");
    assert_eq!(metadata_value(&metadata, "min_writer_version"), "2.30.0");
    assert_eq!(metadata_value(&metadata, "created_by_version"), "2.30.0");
    assert_eq!(metadata_value(&metadata, "binary_version"), "2.30.0");

    let store = Connection::open(created.store_db()).unwrap();
    let coordinator = Connection::open(created.coordinator_db()).unwrap();
    assert_eq!(pragma_i64(&store, "page_size"), 4096);
    assert_eq!(pragma_i64(&coordinator, "page_size"), 4096);

    let reopened = StoreLayout::open(temp.path()).unwrap();
    assert_eq!(reopened.generation_name(), "gen-001");
    assert_eq!(reopened.store_db(), created.store_db());
}

#[test]
fn missing_current_is_typed_and_partial_current_is_never_opened() {
    let temp = TempStore::new("missing-current");
    fs::write(temp.path().join("CURRENT.partial"), "gen-001\n").unwrap();

    let error = StoreLayout::open(temp.path()).unwrap_err();

    assert!(matches!(error, StoreLayoutError::CurrentMissing { .. }));
}

#[test]
fn current_rejects_partial_and_traversal_generation_names() {
    for generation in [
        "",
        "gen-",
        "gen-01",
        "gen-001.partial",
        "../outside",
        "gen-001/..",
    ] {
        let temp = TempStore::new("invalid-current");
        fs::write(temp.path().join("CURRENT"), format!("{generation}\n")).unwrap();

        let error = StoreLayout::open(temp.path()).unwrap_err();

        assert!(
            matches!(error, StoreLayoutError::InvalidGeneration { .. }),
            "generation {generation:?} returned {error:?}"
        );
    }
}

#[cfg(unix)]
#[test]
fn current_rejects_a_generation_symlink_outside_the_family() {
    use std::os::unix::fs::symlink;

    let temp = TempStore::new("symlink-root");
    let outside = TempStore::new("symlink-outside");
    fs::write(outside.path().join("store.db"), b"outside").unwrap();
    symlink(outside.path(), temp.path().join("gen-002")).unwrap();
    fs::write(temp.path().join("CURRENT"), "gen-002\n").unwrap();

    let error = StoreLayout::open(temp.path()).unwrap_err();

    assert!(matches!(error, StoreLayoutError::PathEscapesRoot { .. }));
}

#[cfg(unix)]
#[test]
fn open_never_follows_current_outside_the_family() {
    use std::os::unix::fs::symlink;

    let temp = TempStore::new("current-symlink-root");
    let outside = TempStore::new("current-symlink-outside");
    let external_current = outside.path().join("CURRENT");
    fs::write(&external_current, "gen-001\n").unwrap();
    symlink(&external_current, temp.path().join("CURRENT")).unwrap();

    let error = StoreLayout::open(temp.path()).unwrap_err();

    assert!(matches!(error, StoreLayoutError::PathEscapesRoot { .. }));
}

#[test]
fn open_rejects_a_non_file_store_database() {
    let temp = TempStore::new("store-db-type");
    let layout = StoreLayout::create(temp.path(), "family-a", "2.30.0").unwrap();
    fs::remove_file(layout.store_db()).unwrap();
    fs::create_dir(layout.store_db()).unwrap();

    let error = StoreLayout::open(temp.path()).unwrap_err();

    assert!(matches!(
        error,
        StoreLayoutError::UnexpectedPathType {
            expected: "regular file",
            ..
        }
    ));
}

#[test]
fn connection_factory_refuses_the_wrong_family() {
    let temp = TempStore::new("wrong-family");
    let layout = StoreLayout::create(temp.path(), "family-a", "2.30.0").unwrap();
    let factory = StoreConnectionFactory::new(layout, "family-b", "2.30.0");

    let error = factory.open_reader().unwrap_err();

    assert!(matches!(
        error,
        StoreConnectionError::FamilyMismatch { expected, found }
            if expected == "family-b" && found == "family-a"
    ));
}

#[test]
fn connection_factory_preserves_unknown_schema_as_a_typed_refusal() {
    let temp = TempStore::new("newer-schema");
    let layout = StoreLayout::create(temp.path(), "family-a", "2.30.0").unwrap();
    Connection::open(layout.store_db())
        .unwrap()
        .pragma_update(None, "user_version", 99)
        .unwrap();
    let factory = StoreConnectionFactory::new(layout, "family-a", "2.30.0");

    let error = factory.open_reader().unwrap_err();

    assert!(matches!(
        error,
        StoreConnectionError::Schema(StoreSchemaError::NewerSchema {
            database: "store.db",
            found: 99,
            supported: 1,
        })
    ));
}

#[test]
fn below_reader_floor_is_typed_not_ready() {
    let temp = TempStore::new("reader-floor");
    let layout = StoreLayout::create(temp.path(), "family-a", "2.30.0").unwrap();
    set_metadata(layout.store_db(), "min_reader_version", "2.31.0");
    let factory = StoreConnectionFactory::new(layout, "family-a", "2.30.0");

    let error = factory.open_reader().unwrap_err();

    assert!(matches!(
        error,
        StoreConnectionError::ReaderVersionTooOld { running, required }
            if running == "2.30.0" && required == "2.31.0"
    ));
}

#[test]
fn below_writer_floor_can_open_read_only_but_not_for_writes() {
    let temp = TempStore::new("writer-floor");
    let layout = StoreLayout::create(temp.path(), "family-a", "2.30.0").unwrap();
    set_metadata(layout.store_db(), "min_writer_version", "2.31.0");
    let factory = StoreConnectionFactory::new(layout, "family-a", "2.30.0");

    let reader = factory.open_reader().unwrap();
    assert_eq!(pragma_i64(&reader, "query_only"), 1);
    assert!(
        reader
            .execute(
                "INSERT INTO store_meta (key, value) VALUES ('unexpected', 'write')",
                [],
            )
            .is_err()
    );
    let error = factory.open_writer().unwrap_err();
    assert!(matches!(
        error,
        StoreConnectionError::WriterVersionTooOld { running, required }
            if running == "2.30.0" && required == "2.31.0"
    ));
}

#[test]
fn writer_reasserts_and_reads_back_required_pragmas() {
    let temp = TempStore::new("writer-pragmas");
    let layout = StoreLayout::create(temp.path(), "family-a", "2.30.0").unwrap();
    let factory = StoreConnectionFactory::new(layout, "family-a", "2.30.0");

    let writer = factory.open_writer().unwrap();

    assert_eq!(pragma_text(&writer, "journal_mode").to_lowercase(), "wal");
    assert_eq!(pragma_i64(&writer, "synchronous"), 2);
    assert_eq!(pragma_i64(&writer, "foreign_keys"), 1);
    assert_eq!(pragma_i64(&writer, "secure_delete"), 1);
    assert_eq!(pragma_i64(&writer, "auto_vacuum"), 2);
    assert_eq!(pragma_i64(&writer, "page_size"), 4096);
    assert_eq!(pragma_i64(&writer, "wal_autocheckpoint"), 1000);
}

#[test]
fn writer_detects_a_late_auto_vacuum_no_op() {
    let temp = TempStore::new("late-auto-vacuum");
    let layout = StoreLayout::create(temp.path(), "family-a", "2.30.0").unwrap();
    let connection = Connection::open(layout.store_db()).unwrap();
    connection
        .execute_batch("PRAGMA auto_vacuum = NONE; VACUUM;")
        .unwrap();
    assert_eq!(pragma_i64(&connection, "auto_vacuum"), 0);
    drop(connection);
    let factory = StoreConnectionFactory::new(layout, "family-a", "2.30.0");

    let error = factory.open_writer().unwrap_err();

    assert!(matches!(
        error,
        StoreConnectionError::PragmaMismatch {
            pragma: "auto_vacuum",
            expected: 2,
            found: 0,
        }
    ));
}

#[test]
fn writer_advances_binary_version_and_refuses_an_older_direct_writer() {
    let temp = TempStore::new("binary-version");
    let layout = StoreLayout::create(temp.path(), "family-a", "2.30.0").unwrap();
    StoreConnectionFactory::new(layout.clone(), "family-a", "2.31.0")
        .open_writer()
        .unwrap();
    assert_eq!(
        metadata_value(&store_metadata(layout.store_db()), "binary_version"),
        "2.31.0"
    );

    let error = StoreConnectionFactory::new(layout.clone(), "family-a", "2.30.0")
        .open_writer()
        .unwrap_err();

    assert!(matches!(
        error,
        StoreConnectionError::WriterVersionTooOld { running, required }
            if running == "2.30.0" && required == "2.31.0"
    ));

    assert_eq!(
        metadata_value(&store_metadata(layout.store_db()), "binary_version"),
        "2.31.0"
    );
}

#[test]
fn creation_recovery_reaps_only_unpublished_scaffolding() {
    let temp = TempStore::new("recovery-reap");
    StoreLayout::create(temp.path(), "family-a", "2.30.0").unwrap();
    let scaffolding = temp.path().join(".gen-002.partial");
    let unreferenced_generation = temp.path().join("gen-099");
    fs::create_dir(&scaffolding).unwrap();
    fs::create_dir(&unreferenced_generation).unwrap();
    fs::write(temp.path().join("CURRENT.partial"), "gen-002\n").unwrap();

    let reopened = StoreLayout::create(temp.path(), "family-a", "2.30.0").unwrap();

    assert_eq!(reopened.generation_name(), "gen-001");
    assert!(!scaffolding.exists());
    assert!(!temp.path().join("CURRENT.partial").exists());
    assert!(unreferenced_generation.is_dir());
}

#[test]
fn creation_recovers_an_initialized_generation_not_yet_published() {
    let temp = TempStore::new("recover-publish");
    StoreLayout::create(temp.path(), "family-a", "2.30.0").unwrap();
    fs::remove_file(temp.path().join("CURRENT")).unwrap();

    let recovered = StoreLayout::create(temp.path(), "family-a", "2.30.0").unwrap();

    assert_eq!(recovered.generation_name(), "gen-001");
    assert_eq!(
        fs::read_to_string(temp.path().join("CURRENT")).unwrap(),
        "gen-001\n"
    );
}

#[test]
fn creation_reasserts_wal_before_publishing_an_adopted_generation() {
    let temp = TempStore::new("recover-delete-journal");
    let generation = temp.path().join("gen-001");
    fs::create_dir(&generation).unwrap();
    fs::create_dir(generation.join("bases")).unwrap();
    let store_db = generation.join("store.db");
    let connection = Connection::open(&store_db).unwrap();
    connection
        .execute_batch("PRAGMA page_size = 4096; PRAGMA auto_vacuum = INCREMENTAL;")
        .unwrap();
    create_store_schema(&connection).unwrap();
    connection
        .execute(
            "INSERT INTO store_meta (key, value) VALUES ('family_id', 'family-a')",
            [],
        )
        .unwrap();
    assert_eq!(pragma_text(&connection, "journal_mode"), "delete");
    drop(connection);

    let recovered = StoreLayout::create(temp.path(), "family-a", "2.30.0").unwrap();

    let connection = Connection::open(recovered.store_db()).unwrap();
    assert_eq!(pragma_text(&connection, "journal_mode"), "wal");
    assert_eq!(
        fs::read_to_string(temp.path().join("CURRENT")).unwrap(),
        "gen-001\n"
    );
}

#[test]
fn creation_refuses_to_publish_a_generation_initialized_with_late_auto_vacuum() {
    let temp = TempStore::new("recover-invalid-pragmas");
    let generation = temp.path().join("gen-001");
    fs::create_dir(&generation).unwrap();
    fs::create_dir(generation.join("bases")).unwrap();
    let store_db = generation.join("store.db");
    let connection = Connection::open(&store_db).unwrap();
    create_store_schema(&connection).unwrap();
    connection
        .execute(
            "INSERT INTO store_meta (key, value) VALUES ('family_id', 'family-a')",
            [],
        )
        .unwrap();
    connection
        .pragma_update(None, "auto_vacuum", "INCREMENTAL")
        .unwrap();
    assert_eq!(pragma_i64(&connection, "auto_vacuum"), 0);
    drop(connection);

    let error = StoreLayout::create(temp.path(), "family-a", "2.30.0").unwrap_err();

    assert!(matches!(
        error,
        StoreLayoutError::PragmaMismatch {
            pragma: "auto_vacuum",
            expected: 2,
            found: 0,
        }
    ));
    assert!(!temp.path().join("CURRENT").exists());
}

#[test]
fn creation_refuses_to_publish_an_adopted_generation_with_the_wrong_page_size() {
    let temp = TempStore::new("recover-invalid-page-size");
    let generation = temp.path().join("gen-001");
    fs::create_dir(&generation).unwrap();
    fs::create_dir(generation.join("bases")).unwrap();
    let store_db = generation.join("store.db");
    let connection = Connection::open(&store_db).unwrap();
    connection
        .execute_batch("PRAGMA page_size = 8192; PRAGMA auto_vacuum = INCREMENTAL;")
        .unwrap();
    create_store_schema(&connection).unwrap();
    connection
        .execute(
            "INSERT INTO store_meta (key, value) VALUES ('family_id', 'family-a')",
            [],
        )
        .unwrap();
    assert_eq!(pragma_i64(&connection, "page_size"), 8192);
    drop(connection);

    let error = StoreLayout::create(temp.path(), "family-a", "2.30.0").unwrap_err();

    assert!(matches!(
        error,
        StoreLayoutError::PragmaMismatch {
            pragma: "page_size",
            expected: 4096,
            found: 8192,
        }
    ));
    assert!(!temp.path().join("CURRENT").exists());
}

#[test]
fn creation_refuses_an_existing_coordinator_with_the_wrong_page_size() {
    let temp = TempStore::new("coord-invalid-page-size");
    let coordinator_db = temp.path().join("coord.db");
    let connection = Connection::open(&coordinator_db).unwrap();
    connection
        .execute_batch("PRAGMA page_size = 8192; PRAGMA auto_vacuum = INCREMENTAL;")
        .unwrap();
    create_coordinator_schema(&connection).unwrap();
    assert_eq!(pragma_i64(&connection, "page_size"), 8192);
    drop(connection);

    let error = StoreLayout::create(temp.path(), "family-a", "2.30.0").unwrap_err();

    assert!(matches!(
        error,
        StoreLayoutError::PragmaMismatch {
            pragma: "page_size",
            expected: 4096,
            found: 8192,
        }
    ));
    assert!(!temp.path().join("CURRENT").exists());
}

#[cfg(unix)]
#[test]
fn creation_never_opens_an_external_coordinator_symlink() {
    use std::os::unix::fs::symlink;

    let temp = TempStore::new("coord-symlink-root");
    let outside = TempStore::new("coord-symlink-outside");
    let external_coordinator = outside.path().join("coord.db");
    let connection = Connection::open(&external_coordinator).unwrap();
    connection
        .execute_batch(
            "PRAGMA page_size = 4096;
             PRAGMA auto_vacuum = INCREMENTAL;
             CREATE TABLE sentinel (value TEXT NOT NULL);
             INSERT INTO sentinel (value) VALUES ('unchanged');",
        )
        .unwrap();
    drop(connection);
    let original = fs::read(&external_coordinator).unwrap();
    symlink(&external_coordinator, temp.path().join("coord.db")).unwrap();

    let error = StoreLayout::create(temp.path(), "family-a", "2.30.0").unwrap_err();

    assert!(matches!(error, StoreLayoutError::PathEscapesRoot { .. }));
    let current_published = temp.path().join("CURRENT").exists();
    let external_changed = fs::read(&external_coordinator).unwrap() != original;
    assert!(
        !current_published && !external_changed,
        "CURRENT published: {current_published}; external target changed: {external_changed}"
    );
}

#[cfg(unix)]
#[test]
fn creation_rejects_an_internal_coordinator_symlink() {
    use std::os::unix::fs::symlink;

    let temp = TempStore::new("coord-internal-symlink");
    let internal_target = temp.path().join("other.db");
    let connection = Connection::open(&internal_target).unwrap();
    connection
        .execute_batch(
            "PRAGMA page_size = 4096;
             PRAGMA auto_vacuum = INCREMENTAL;
             CREATE TABLE sentinel (value TEXT NOT NULL);",
        )
        .unwrap();
    drop(connection);
    let original = fs::read(&internal_target).unwrap();
    symlink(&internal_target, temp.path().join("coord.db")).unwrap();

    let error = StoreLayout::create(temp.path(), "family-a", "2.30.0").unwrap_err();

    assert!(matches!(error, StoreLayoutError::UnexpectedPathType { .. }));
    assert!(!temp.path().join("CURRENT").exists());
    assert_eq!(fs::read(&internal_target).unwrap(), original);
}

#[cfg(unix)]
#[test]
fn creation_never_adopts_a_generation_symlink_outside_the_family() {
    use std::os::unix::fs::symlink;

    let temp = TempStore::new("create-symlink-root");
    let outside = TempStore::new("create-symlink-outside");
    Connection::open(outside.path().join("store.db")).unwrap();
    symlink(outside.path(), temp.path().join("gen-001")).unwrap();

    let error = StoreLayout::create(temp.path(), "family-a", "2.30.0").unwrap_err();

    assert!(matches!(error, StoreLayoutError::PathEscapesRoot { .. }));
    assert!(!temp.path().join("CURRENT").exists());
}

fn store_metadata(path: &Path) -> Vec<(String, String)> {
    let conn = Connection::open(path).unwrap();
    let mut statement = conn
        .prepare("SELECT key, value FROM store_meta ORDER BY key")
        .unwrap();
    statement
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

fn metadata_value<'a>(metadata: &'a [(String, String)], key: &str) -> &'a str {
    metadata
        .iter()
        .find_map(|(candidate, value)| (candidate == key).then_some(value.as_str()))
        .unwrap_or_else(|| panic!("missing metadata key {key}"))
}

fn set_metadata(path: &Path, key: &str, value: &str) {
    Connection::open(path)
        .unwrap()
        .execute(
            "UPDATE store_meta SET value = ?2 WHERE key = ?1",
            [key, value],
        )
        .unwrap();
}

fn pragma_i64(connection: &Connection, pragma: &str) -> i64 {
    connection
        .query_row(&format!("PRAGMA {pragma}"), [], |row| row.get(0))
        .unwrap()
}

fn pragma_text(connection: &Connection, pragma: &str) -> String {
    connection
        .query_row(&format!("PRAGMA {pragma}"), [], |row| row.get(0))
        .unwrap()
}

struct TempStore {
    path: PathBuf,
}

impl TempStore {
    fn new(name: &str) -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "julie-store-connection-{name}-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir_all(&path).unwrap();
        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempStore {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}
