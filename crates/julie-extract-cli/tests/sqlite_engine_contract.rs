const FIRST_SQLITE_WITH_WAL_RESET_FIX: i32 = 3_053_001;

#[test]
fn bundled_sqlite_includes_the_wal_reset_corruption_fix() {
    assert!(
        rusqlite::version_number() >= FIRST_SQLITE_WITH_WAL_RESET_FIX,
        "bundled SQLite {} predates the WAL-reset fix",
        rusqlite::version()
    );
}
