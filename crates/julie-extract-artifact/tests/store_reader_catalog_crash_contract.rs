#![cfg(feature = "test-store-crash")]

include!("store_reader_catalog_contract.rs");

#[test]
fn aborted_catalog_install_publishes_neither_objects_nor_registrations() {
    let temp = TempStore::new("aborted-reader-catalog");
    let layout = legacy_store(&temp);
    let boundary = "reader_catalog_installed_before_floor";
    let marker = temp.path().join(format!(".{boundary}.reached"));
    assert!(!marker.exists(), "stale crash marker: {}", marker.display());
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--ignored",
            "--exact",
            "reader_catalog_install_crash_child",
            "--nocapture",
            "--test-threads=1",
        ])
        .env("JULIE_READER_CATALOG_CRASH_ROOT", temp.path())
        .env("JULIE_EXTRACT_STORE_TEST_CRASH_AT", boundary)
        .env("JULIE_EXTRACT_STORE_TEST_CRASH_MARKER", &marker)
        .output()
        .unwrap();
    let reached = std::fs::read_to_string(&marker).unwrap_or_else(|error| {
        panic!(
            "boundary={boundary} was not reached: {error}; child status={} stdout={} stderr={}",
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        )
    });
    assert_eq!(
        reached,
        boundary,
        "child reached a different crash boundary; status={} stdout={} stderr={}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    std::fs::remove_file(marker).unwrap();
    assert!(
        !output.status.success(),
        "child survived crash boundary; stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );

    let coordinator = Connection::open(layout.coordinator_db()).unwrap();
    assert!(reader_catalog(&coordinator).is_empty());
    assert_eq!(
        coordinator
            .query_row(
                "SELECT source_min_writer_version FROM maintenance_intent
                 WHERE resource='store-maintenance'",
                [],
                |row| row.get::<_, String>(0),
            )
            .unwrap(),
        "2.39.0"
    );
    assert_eq!(store_floor(&layout), "2.40.0");
    drop(coordinator);

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    loop {
        match activate(&layout, "aborted-reader-catalog-recovery") {
            Ok(()) => break,
            Err(MaintenanceError::MaintenanceBusy) if std::time::Instant::now() < deadline => {
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            Err(error) => panic!("reader catalog recovery failed: {error}"),
        }
    }

    let fresh = Connection::open_in_memory().unwrap();
    create_coordinator_schema(&fresh).unwrap();
    let coordinator = Connection::open(layout.coordinator_db()).unwrap();
    assert_eq!(reader_catalog(&coordinator), reader_catalog(&fresh));
    assert_eq!(registration_count(&coordinator), 0);
    assert_eq!(maintenance_owner_count(&coordinator), 0);
    assert_eq!(store_floor(&layout), "2.40.0");
}

#[test]
#[ignore = "subprocess crash probe"]
fn reader_catalog_install_crash_child() {
    let root = std::env::var_os("JULIE_READER_CATALOG_CRASH_ROOT").unwrap();
    let layout = StoreLayout::open(root).unwrap();
    MaintenanceExecutor::activate_reader_writer_floor(
        StoreConnectionFactory::new(layout, "family-a", "2.40.0"),
        MaintenanceRun::new(
            "aborted-reader-catalog-child",
            "catalog-owner",
            std::process::id(),
            100,
            1_000,
        ),
    )
    .unwrap();
}
