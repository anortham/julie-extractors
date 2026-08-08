#![cfg(feature = "test-store-contract")]

use std::fs;
use std::process::Command;

use julie_extract_artifact::store::{
    CoordinatorError, LeaseDisposition, LeaseHolder, StoreConnectionError, StoreConnectionFactory,
    StoreCoordinator, StoreLayout,
};
use rusqlite::Connection;

const FAMILY_ID: &str = "8d19be9c-6ca0-43d2-8f25-0818869bb901";

#[test]
fn older_writer_requires_the_explicit_escape_and_never_lowers_stored_floors() {
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path().join("root");
    let store = fixture.path().join("store");
    fs::create_dir(&root).unwrap();
    fs::write(root.join("lib.rs"), "pub fn value() -> i32 { 1 }\n").unwrap();
    assert!(
        run_store(
            &store,
            &root,
            [
                "import",
                "--family",
                FAMILY_ID,
                "--level",
                "l1",
                "--request-id",
                "request-seed",
                "--idempotency-key",
                "idem-seed",
            ],
            false,
        )
        .status
        .success()
    );

    let database = store.join("gen-001/store.db");
    set_meta(&database, "binary_version", "2.31.0");
    fs::write(root.join("lib.rs"), "pub fn value() -> i32 { 2 }\n").unwrap();
    let refused = run_store(
        &store,
        &root,
        [
            "update",
            "--file",
            "lib.rs",
            "--level",
            "l1",
            "--request-id",
            "request-refused",
            "--idempotency-key",
            "idem-refused",
        ],
        false,
    );
    assert!(!refused.status.success());
    assert_eq!(current_generation(&database), 1);

    let allowed = run_store(
        &store,
        &root,
        [
            "update",
            "--file",
            "lib.rs",
            "--level",
            "l1",
            "--request-id",
            "request-allowed",
            "--idempotency-key",
            "idem-allowed",
        ],
        true,
    );
    assert!(
        allowed.status.success(),
        "stdout: {} stderr: {}",
        String::from_utf8_lossy(&allowed.stdout),
        String::from_utf8_lossy(&allowed.stderr)
    );
    assert_eq!(current_generation(&database), 2);
    assert_eq!(meta(&database, "binary_version"), "2.31.0");
    assert_eq!(meta(&database, "min_writer_version"), "2.30.0");
    assert_eq!(meta(&database, "min_reader_version"), "2.30.0");
}

#[test]
fn downgrade_escape_never_bypasses_reader_or_writer_floors() {
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path().join("root");
    let store = fixture.path().join("store");
    fs::create_dir(&root).unwrap();
    fs::write(root.join("lib.rs"), "pub fn value() -> i32 { 1 }\n").unwrap();
    assert!(
        run_store(
            &store,
            &root,
            [
                "import",
                "--family",
                FAMILY_ID,
                "--level",
                "l1",
                "--request-id",
                "request-floor-seed",
                "--idempotency-key",
                "idem-floor-seed",
            ],
            false,
        )
        .status
        .success()
    );
    let layout = StoreLayout::open(&store).unwrap();
    let database = layout.store_db().to_path_buf();
    set_meta(&database, "binary_version", "2.31.0");
    set_meta(&database, "min_writer_version", "2.31.0");
    fs::write(root.join("lib.rs"), "pub fn value() -> i32 { 3 }\n").unwrap();

    let refused = run_store(
        &store,
        &root,
        [
            "update",
            "--file",
            "lib.rs",
            "--level",
            "l1",
            "--request-id",
            "request-floor-refused",
            "--idempotency-key",
            "idem-floor-refused",
        ],
        true,
    );
    assert!(!refused.status.success());
    assert_eq!(current_generation(&database), 1);
    assert_eq!(meta(&database, "min_writer_version"), "2.31.0");

    set_meta(&database, "min_writer_version", "2.30.0");
    set_meta(&database, "min_reader_version", "2.31.0");
    let factory = StoreConnectionFactory::new(layout, FAMILY_ID, "2.30.0");
    let error = factory.open_reader().unwrap_err();
    assert!(matches!(
        error,
        StoreConnectionError::ReaderVersionTooOld { running, required }
            if running == "2.30.0" && required == "2.31.0"
    ));
    assert_eq!(meta(&database, "min_reader_version"), "2.31.0");
}

#[test]
fn coordinator_uses_the_same_binary_floor_and_escape_policy_as_connections() {
    let fixture = tempfile::tempdir().unwrap();
    let layout = StoreLayout::create(fixture.path(), FAMILY_ID, "2.30.0").unwrap();
    set_meta(layout.store_db(), "binary_version", "2.31.0");
    let mut coordinator = StoreCoordinator::open(&layout).unwrap();
    let error = coordinator
        .try_acquire_or_takeover(LeaseHolder::new("old-holder", "2.30.0", 41), 10)
        .unwrap_err();
    assert!(matches!(
        error,
        CoordinatorError::WriterVersionTooOld { running, required }
            if running == "2.30.0" && required == "2.31.0"
    ));

    let output = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "mixed_version_coordinator_escape_worker",
            "--nocapture",
            "--test-threads=1",
        ])
        .env("JULIE_TEST_MIXED_STORE", fixture.path())
        .env("MILLER_ALLOW_EXTRACTOR_DOWNGRADE", "1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stdout: {} stderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(meta(layout.store_db(), "binary_version"), "2.31.0");
}

#[test]
fn mixed_version_coordinator_escape_worker() {
    let Ok(store) = std::env::var("JULIE_TEST_MIXED_STORE") else {
        return;
    };
    let layout = StoreLayout::open(store).unwrap();
    let mut coordinator = StoreCoordinator::open(&layout).unwrap();
    let holder = LeaseHolder::new("escape-holder", "2.30.0", std::process::id());
    let LeaseDisposition::Acquired { fencing_token } = coordinator
        .try_acquire_or_takeover(holder.clone(), 20)
        .unwrap()
    else {
        panic!("escape holder did not acquire");
    };
    assert!(coordinator.release_lease(&holder, fencing_token).unwrap());
}

fn run_store<const N: usize>(
    store: &std::path::Path,
    root: &std::path::Path,
    arguments: [&str; N],
    allow_downgrade: bool,
) -> std::process::Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_julie-extract"));
    command
        .arg("store")
        .arg(arguments[0])
        .args(["--store", store.to_str().unwrap()])
        .args(["--root", root.to_str().unwrap()])
        .args(["--view", "view-main"])
        .args(&arguments[1..])
        .args(["--jobs", "1", "--json"]);
    if allow_downgrade {
        command.env("MILLER_ALLOW_EXTRACTOR_DOWNGRADE", "1");
    }
    command.output().unwrap()
}

fn set_meta(database: &std::path::Path, key: &str, value: &str) {
    Connection::open(database)
        .unwrap()
        .execute(
            "UPDATE store_meta SET value = ?1 WHERE key = ?2",
            [value, key],
        )
        .unwrap();
}

fn meta(database: &std::path::Path, key: &str) -> String {
    Connection::open(database)
        .unwrap()
        .query_row(
            "SELECT value FROM store_meta WHERE key = ?1",
            [key],
            |row| row.get(0),
        )
        .unwrap()
}

fn current_generation(database: &std::path::Path) -> i64 {
    Connection::open(database)
        .unwrap()
        .query_row(
            "SELECT current_generation FROM views WHERE view_id = 'view-main'",
            [],
            |row| row.get(0),
        )
        .unwrap()
}
