#![cfg(feature = "test-store-maintenance-contract")]

use std::path::Path;
use std::process::{Command, Output};

use julie_extract_artifact::store::StoreLayout;
use rusqlite::Connection;

const FAMILY_ID: &str = "f924890d-2554-4244-b7ff-e652c085ccaa";

#[test]
fn lifecycle_writers_honor_floors_escape_limits_retained_readers_and_monotonic_generations() {
    let running_version = env!("CARGO_PKG_VERSION");
    let future_version = next_minor_version(running_version);
    let fixture = tempfile::tempdir().unwrap();
    let store = fixture.path().join("store");
    let layout = StoreLayout::create(&store, FAMILY_ID, env!("CARGO_PKG_VERSION"), 7).unwrap();
    set_meta(layout.store_db(), "binary_version", &future_version);

    let refused = maintain(&store, false);
    assert!(!refused.status.success());
    assert_eq!(
        StoreLayout::open(&store).unwrap().generation_name(),
        "gen-001"
    );

    let first = maintain(&store, true);
    assert_success(&first);
    assert_eq!(
        StoreLayout::open(&store).unwrap().generation_name(),
        "gen-002"
    );
    assert_eq!(
        meta(&store.join("gen-002/store.db"), "binary_version"),
        future_version
    );
    assert_eq!(
        Connection::open(store.join("gen-001/store.db"))
            .unwrap()
            .query_row("PRAGMA quick_check", [], |row| row.get::<_, String>(0))
            .unwrap(),
        "ok"
    );

    let second = maintain(&store, true);
    assert_success(&second);
    assert_eq!(
        StoreLayout::open(&store).unwrap().generation_name(),
        "gen-003"
    );
    assert!(store.join("gen-001/store.db").is_file());
    assert!(store.join("gen-002/store.db").is_file());

    set_meta(
        &store.join("gen-003/store.db"),
        "min_reader_version",
        &future_version,
    );
    let floor_refused = maintain(&store, true);
    assert!(!floor_refused.status.success());
    assert_eq!(
        StoreLayout::open(&store).unwrap().generation_name(),
        "gen-003"
    );
    assert!(!store.join("gen-004").exists());
}

fn maintain(store: &Path, allow_downgrade: bool) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_julie-extract"));
    command.args([
        "store",
        "maintain",
        "promote",
        "--store",
        store.to_str().unwrap(),
        "--apply",
        "--json",
    ]);
    if allow_downgrade {
        command.env("MILLER_ALLOW_EXTRACTOR_DOWNGRADE", "1");
    }
    command.output().unwrap()
}

fn set_meta(database: &Path, key: &str, value: &str) {
    Connection::open(database)
        .unwrap()
        .execute("UPDATE store_meta SET value=?1 WHERE key=?2", [value, key])
        .unwrap();
}

fn meta(database: &Path, key: &str) -> String {
    Connection::open(database)
        .unwrap()
        .query_row("SELECT value FROM store_meta WHERE key=?1", [key], |row| {
            row.get(0)
        })
        .unwrap()
}

fn assert_success(output: &Output) {
    assert!(
        output.status.success(),
        "stdout: {} stderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn next_minor_version(version: &str) -> String {
    let mut components = version.split('.');
    let major = components.next().unwrap().parse::<u64>().unwrap();
    let minor = components.next().unwrap().parse::<u64>().unwrap();
    format!("{major}.{}.0", minor + 1)
}
