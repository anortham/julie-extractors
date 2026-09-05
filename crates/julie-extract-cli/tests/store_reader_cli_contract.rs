use std::process::{Command, Output};

use clap::Parser;
use julie_extract_cli::store::args::StoreCli;
use rusqlite::Connection;
use serde_json::Value;

const FAMILY_ID: &str = "9f8c2c9a-3b92-4f38-9b0d-0e2b8c7a4d11";
const NONCE: &str = "0123456789abcdef0123456789abcdef";

fn julie_extract(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_julie-extract"))
        .args(args)
        .output()
        .expect("julie-extract should start")
}

fn reader_acquire(
    store: &std::path::Path,
    generation: &str,
    owner: &str,
    owner_pid: &str,
    nonce: &str,
    json: bool,
) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_julie-extract"));
    command
        .args(["store", "reader", "acquire", "--store"])
        .arg(store)
        .args([
            "--family",
            FAMILY_ID,
            "--view",
            "default",
            "--generation",
            generation,
            "--owner",
            owner,
            "--owner-pid",
            owner_pid,
            "--nonce",
            nonce,
            "--lease-ms",
            "30000",
        ]);
    if json {
        command.arg("--json");
    }
    command.output().expect("julie-extract should start")
}

fn reader_renew(store: &std::path::Path, pin: &str, owner_pid: &str, nonce: &str) -> Output {
    Command::new(env!("CARGO_BIN_EXE_julie-extract"))
        .args(["store", "reader", "renew", "--store"])
        .arg(store)
        .args([
            "--family",
            FAMILY_ID,
            "--pin",
            pin,
            "--nonce",
            nonce,
            "--owner-pid",
            owner_pid,
            "--lease-ms",
            "30000",
            "--json",
        ])
        .output()
        .expect("julie-extract should start")
}

fn reader_release(store: &std::path::Path, pin: &str, nonce: &str, json: bool) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_julie-extract"));
    command
        .args(["store", "reader", "release", "--store"])
        .arg(store)
        .args(["--family", FAMILY_ID, "--pin", pin, "--nonce", nonce]);
    if json {
        command.arg("--json");
    }
    command.output().expect("julie-extract should start")
}

fn import_view(fixture: &std::path::Path, store: &std::path::Path) {
    let root = fixture.join("source");
    std::fs::create_dir(&root).unwrap();
    std::fs::write(root.join("lib.rs"), "pub fn answer() -> u32 { 7 }\n").unwrap();
    let output = julie_extract(&[
        "store",
        "import",
        "--store",
        store.to_str().unwrap(),
        "--family",
        FAMILY_ID,
        "--root",
        root.to_str().unwrap(),
        "--view",
        "default",
        "--request-id",
        "reader-fixture",
        "--idempotency-key",
        "reader-fixture",
        "--json",
    ]);
    assert_eq!(
        output.status.code(),
        Some(0),
        "stdout: {} stderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn json_report(output: &Output, expected_exit: i32) -> Value {
    assert_eq!(
        output.status.code(),
        Some(expected_exit),
        "stdout: {} stderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    assert_eq!(
        output.stdout.iter().filter(|&&byte| byte == b'\n').count(),
        1
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

#[test]
fn reader_help_is_stable() {
    let output = julie_extract(&["store", "reader", "--help"]);

    assert_eq!(
        output.status.code(),
        Some(0),
        "stdout: {} stderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "Usage: julie-extract store reader <COMMAND>\n\nCommands:\n  acquire  Register one immutable manifest snapshot\n  renew    Renew an authenticated reader registration\n  release  Release an authenticated reader registration\n  help     Print this message or the help of the given subcommand(s)\n\nOptions:\n  -h, --help  Print help\n"
    );
}

#[test]
fn reader_subcommand_help_freezes_the_required_arguments() {
    let acquire = julie_extract(&["store", "reader", "acquire", "--help"]);
    assert_eq!(acquire.status.code(), Some(0));
    assert!(acquire.stderr.is_empty());
    let acquire = String::from_utf8(acquire.stdout).unwrap();
    assert!(acquire.contains(
        "Usage: julie-extract store reader acquire [OPTIONS] --store <STORE> --family <FAMILY> --view <VIEW> --generation <GENERATION> --owner <OWNER> --owner-pid <OWNER_PID> --nonce <NONCE> --lease-ms <LEASE_MS>"
    ));

    let renew = julie_extract(&["store", "reader", "renew", "--help"]);
    assert_eq!(renew.status.code(), Some(0));
    assert!(renew.stderr.is_empty());
    let renew = String::from_utf8(renew.stdout).unwrap();
    assert!(renew.contains(
        "Usage: julie-extract store reader renew [OPTIONS] --store <STORE> --family <FAMILY> --pin <PIN> --nonce <NONCE> --owner-pid <OWNER_PID> --lease-ms <LEASE_MS>"
    ));

    let release = julie_extract(&["store", "reader", "release", "--help"]);
    assert_eq!(release.status.code(), Some(0));
    assert!(release.stderr.is_empty());
    let release = String::from_utf8(release.stdout).unwrap();
    assert!(release.contains(
        "Usage: julie-extract store reader release [OPTIONS] --store <STORE> --family <FAMILY> --pin <PIN> --nonce <NONCE>"
    ));
}

#[test]
fn parsed_reader_arguments_redact_the_nonce_from_debug() {
    let secret = "WRONG_READER_NONCE_SECRET_123456";
    let parsed = StoreCli::try_parse_from([
        "julie-extract",
        "store",
        "reader",
        "release",
        "--store",
        "missing",
        "--family",
        FAMILY_ID,
        "--pin",
        "reader-test",
        "--nonce",
        secret,
    ])
    .unwrap();

    let debug = format!("{parsed:?}");
    assert!(debug.contains("<redacted>"));
    assert!(!debug.contains(secret));
}

#[test]
fn acquire_renew_release_json_is_one_line() {
    let fixture = tempfile::tempdir().unwrap();
    let store = fixture.path().join("store");
    import_view(fixture.path(), &store);
    let owner_pid = std::process::id().to_string();

    let acquired = julie_extract(&[
        "store",
        "reader",
        "acquire",
        "--store",
        store.to_str().unwrap(),
        "--family",
        FAMILY_ID,
        "--view",
        "default",
        "--generation",
        "gen-001",
        "--owner",
        "miller",
        "--owner-pid",
        &owner_pid,
        "--nonce",
        NONCE,
        "--lease-ms",
        "30000",
        "--json",
    ]);
    let acquired = json_report(&acquired, 0);
    assert_eq!(acquired["report_schema_version"], 1);
    assert_eq!(acquired["operation"], "reader_acquire");
    assert_eq!(acquired["state"], "acquired");
    assert_eq!(acquired["family_id"], FAMILY_ID);
    assert_eq!(acquired["view_id"], "default");
    assert_eq!(acquired["generation_name"], "gen-001");
    assert_eq!(acquired["owner_nonce"], NONCE);
    assert_eq!(acquired["owner_pid"], std::process::id());
    assert_eq!(acquired["protected_manifest_count"], 1);
    assert_eq!(acquired["failure_class"], Value::Null);
    assert_eq!(acquired["error"], Value::Null);
    for forbidden in [
        "owner_birth_identity",
        "index_level",
        "level_stamps",
        "file_versions",
        "files",
    ] {
        assert!(acquired.get(forbidden).is_none());
    }
    let mut keys: Vec<_> = acquired
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    keys.sort_unstable();
    assert_eq!(
        keys,
        [
            "error",
            "expires_at",
            "extraction_identity_epoch",
            "failure_class",
            "family_id",
            "generation_name",
            "manifest_generation",
            "manifest_hash",
            "min_retained_store_log_sequence",
            "operation",
            "owner_nonce",
            "owner_pid",
            "pin_id",
            "protected_manifest_count",
            "report_schema_version",
            "served_store_log_sequence",
            "snapshot_fingerprint",
            "state",
            "store_instance_id",
            "view_id",
            "warning",
        ]
    );
    let pin = acquired["pin_id"].as_str().unwrap().to_string();
    assert!(pin.starts_with("reader-"));
    assert_eq!(pin.len(), 39);
    assert_ne!(pin, NONCE);
    assert_eq!(
        Connection::open(store.join("coord.db"))
            .unwrap()
            .query_row("SELECT COUNT(*) FROM reader_registrations", [], |row| {
                row.get::<_, i64>(0)
            })
            .unwrap(),
        1
    );

    let renewed = julie_extract(&[
        "store",
        "reader",
        "renew",
        "--store",
        store.to_str().unwrap(),
        "--family",
        FAMILY_ID,
        "--pin",
        &pin,
        "--nonce",
        NONCE,
        "--owner-pid",
        &owner_pid,
        "--lease-ms",
        "30000",
        "--json",
    ]);
    let renewed = json_report(&renewed, 0);
    assert_eq!(renewed["operation"], "reader_renew");
    assert_eq!(renewed["state"], "renewed");
    assert_eq!(renewed["pin_id"], pin);
    assert_eq!(
        renewed["snapshot_fingerprint"],
        acquired["snapshot_fingerprint"]
    );
    assert_eq!(
        Connection::open(store.join("coord.db"))
            .unwrap()
            .query_row(
                "SELECT expires_at FROM reader_registrations WHERE pin_id=?1",
                [&pin],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
        renewed["expires_at"].as_i64().unwrap()
    );

    let released = julie_extract(&[
        "store",
        "reader",
        "release",
        "--store",
        store.to_str().unwrap(),
        "--family",
        FAMILY_ID,
        "--pin",
        &pin,
        "--nonce",
        NONCE,
        "--json",
    ]);
    let released = json_report(&released, 0);
    assert_eq!(released["operation"], "reader_release");
    assert_eq!(released["state"], "released");
    assert_eq!(released["family_id"], FAMILY_ID);
    assert_eq!(released["pin_id"], pin);
    assert_eq!(released["released"], true);
    assert!(released.get("owner_nonce").is_none());
    for field in [
        "view_id",
        "generation_name",
        "manifest_generation",
        "owner_pid",
        "store_instance_id",
        "manifest_hash",
        "extraction_identity_epoch",
        "served_store_log_sequence",
        "min_retained_store_log_sequence",
        "snapshot_fingerprint",
        "protected_manifest_count",
        "expires_at",
        "warning",
        "failure_class",
        "error",
    ] {
        assert_eq!(released[field], Value::Null, "{field}");
    }

    let released_again = julie_extract(&[
        "store",
        "reader",
        "release",
        "--store",
        store.to_str().unwrap(),
        "--family",
        FAMILY_ID,
        "--pin",
        &pin,
        "--nonce",
        NONCE,
        "--json",
    ]);
    let released_again = json_report(&released_again, 0);
    assert_eq!(released_again["released"], false);
    assert!(released_again.get("owner_nonce").is_none());

    let coordinator = Connection::open(store.join("coord.db")).unwrap();
    assert_eq!(
        coordinator
            .query_row("SELECT COUNT(*) FROM reader_registrations", [], |row| row
                .get::<_, i64>(
                0
            ))
            .unwrap(),
        0
    );
}

#[test]
fn parse_failures_are_one_line_and_never_echo_reader_values() {
    let secret = "WRONG_READER_NONCE_SECRET_123456";
    let oversized = "🔒".repeat(513);
    for nonce in [format!("{secret}\n"), oversized] {
        let output = Command::new(env!("CARGO_BIN_EXE_julie-extract"))
            .args(["store", "reader", "acquire", "--store", "missing"])
            .args([
                "--family",
                FAMILY_ID,
                "--view",
                "default",
                "--generation",
                "gen-001",
                "--owner",
                "miller",
                "--owner-pid",
                "1",
                "--nonce",
                &nonce,
                "--lease-ms",
                "30000",
                "--json",
            ])
            .output()
            .unwrap();
        let report = json_report(&output, 2);
        assert_eq!(report["operation"], "reader_acquire");
        assert_eq!(report["state"], "refused");
        assert_eq!(report["failure_class"], "invalid_arguments");
        assert_eq!(report["error"], "reader arguments are invalid");
        assert_eq!(report["family_id"], Value::Null);
        assert_eq!(report["pin_id"], Value::Null);
        assert!(report.get("owner_nonce").is_none());
        let rendered = String::from_utf8(output.stdout).unwrap();
        assert!(!rendered.contains(secret));
        assert!(!rendered.contains('🔒'));
    }

    let unknown = julie_extract(&["store", "reader", secret, "--json"]);
    let report = json_report(&unknown, 2);
    assert_eq!(report["operation"], "reader");
    assert!(!String::from_utf8(unknown.stdout).unwrap().contains(secret));

    let human = Command::new(env!("CARGO_BIN_EXE_julie-extract"))
        .args(["store", "reader", "release", "--store", "missing"])
        .args([
            "--family",
            FAMILY_ID,
            "--pin",
            "reader-test",
            "--nonce",
            &format!("{secret}\n"),
        ])
        .output()
        .unwrap();
    assert_eq!(human.status.code(), Some(2));
    assert!(human.stdout.is_empty());
    let stderr = String::from_utf8(human.stderr).unwrap();
    assert_eq!(
        stderr,
        "refused operation=reader_release failure=invalid_arguments error=reader_arguments_are_invalid\n"
    );
    assert!(!stderr.contains(secret));

    for (argument, value) in [
        ("--owner-pid", secret),
        ("--lease-ms", secret),
        ("--generation", secret),
    ] {
        let mut command = Command::new(env!("CARGO_BIN_EXE_julie-extract"));
        command
            .args(["store", "reader", "acquire", "--store", "missing"])
            .args([
                "--family",
                FAMILY_ID,
                "--view",
                "default",
                "--generation",
                "gen-001",
                "--owner",
                "miller",
                "--owner-pid",
                "1",
                "--nonce",
                NONCE,
                "--lease-ms",
                "30000",
                "--json",
            ])
            .args([argument, value]);
        let output = command.output().unwrap();
        let report = json_report(&output, 2);
        assert_eq!(report["failure_class"], "invalid_arguments");
        assert!(!String::from_utf8(output.stdout).unwrap().contains(secret));
    }
}

#[test]
fn reader_numeric_bounds_refuse_before_opening_a_store() {
    for (owner_pid, lease_ms) in [
        ("0", "30000"),
        ("4294967296", "30000"),
        ("1", "0"),
        ("1", "9223372036854775808"),
        ("1", "18446744073709551616"),
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_julie-extract"))
            .args(["store", "reader", "acquire", "--store", "missing"])
            .args([
                "--family",
                FAMILY_ID,
                "--view",
                "default",
                "--generation",
                "gen-001",
                "--owner",
                "miller",
                "--owner-pid",
                owner_pid,
                "--nonce",
                NONCE,
                "--lease-ms",
                lease_ms,
                "--json",
            ])
            .output()
            .unwrap();
        let report = json_report(&output, 2);
        assert_eq!(report["failure_class"], "invalid_arguments");
    }
}

#[test]
fn unicode_reader_identity_uses_character_bounds() {
    let fixture = tempfile::tempdir().unwrap();
    let store = fixture.path().join("store");
    import_view(fixture.path(), &store);
    let owner = "🦀".repeat(128);
    let nonce = "🔐".repeat(32);
    let owner_pid = std::process::id().to_string();

    let acquired = reader_acquire(&store, "gen-001", &owner, &owner_pid, &nonce, true);
    let acquired = json_report(&acquired, 0);
    assert_eq!(acquired["owner_nonce"], nonce);
    let pin = acquired["pin_id"].as_str().unwrap();
    let released = reader_release(&store, pin, &nonce, true);
    assert_eq!(json_report(&released, 0)["released"], true);

    let oversized_owner = "🦀".repeat(129);
    let refused = reader_acquire(
        std::path::Path::new("store-must-not-open"),
        "gen-001",
        &oversized_owner,
        &owner_pid,
        NONCE,
        true,
    );
    assert_eq!(
        json_report(&refused, 2)["failure_class"],
        "invalid_arguments"
    );
}

#[test]
fn acquire_replay_returns_the_original_snapshot_after_manifest_changes() {
    let fixture = tempfile::tempdir().unwrap();
    let store = fixture.path().join("store");
    import_view(fixture.path(), &store);
    let owner_pid = std::process::id().to_string();
    let first = reader_acquire(&store, "gen-001", "miller", &owner_pid, NONCE, true);
    let first = json_report(&first, 0);

    Connection::open(store.join("gen-001/store.db"))
        .unwrap()
        .execute_batch(
            "INSERT INTO manifests(view_id,generation,manifest_hash,request_id,created_at)
             VALUES ('default',999,'later-manifest','later-request','2026-09-04T00:00:00Z');
             UPDATE views SET current_generation=999 WHERE view_id='default';",
        )
        .unwrap();

    let replayed = reader_acquire(&store, "gen-001", "miller", &owner_pid, NONCE, true);
    let replayed = json_report(&replayed, 0);
    assert_eq!(replayed, first);
    assert_eq!(
        Connection::open(store.join("coord.db"))
            .unwrap()
            .query_row("SELECT COUNT(*) FROM reader_registrations", [], |row| {
                row.get::<_, i64>(0)
            })
            .unwrap(),
        1
    );
}

#[test]
fn stale_generation_and_missing_registration_use_stable_failures() {
    let fixture = tempfile::tempdir().unwrap();
    let store = fixture.path().join("store");
    import_view(fixture.path(), &store);
    let owner_pid = std::process::id().to_string();

    let stale = reader_acquire(&store, "gen-999", "miller", &owner_pid, NONCE, true);
    let stale = json_report(&stale, 1);
    assert_eq!(stale["failure_class"], "stale_snapshot");
    assert_eq!(stale["family_id"], Value::Null);

    let missing = reader_renew(&store, "reader-missing", &owner_pid, NONCE);
    let missing = json_report(&missing, 1);
    assert_eq!(missing["failure_class"], "reader_not_found");
    assert_eq!(missing["pin_id"], Value::Null);
}

#[test]
fn wrong_nonce_is_sanitized_and_preserves_the_registration() {
    let fixture = tempfile::tempdir().unwrap();
    let store = fixture.path().join("store");
    import_view(fixture.path(), &store);
    let owner_pid = std::process::id().to_string();
    let acquired = reader_acquire(&store, "gen-001", "miller", &owner_pid, NONCE, true);
    let acquired = json_report(&acquired, 0);
    let pin = acquired["pin_id"].as_str().unwrap();
    let wrong = "WRONG_READER_NONCE_SECRET_123456";

    let refused = reader_release(&store, pin, wrong, true);
    let refused = json_report(&refused, 1);
    assert_eq!(refused["failure_class"], "reader_owner_mismatch");
    assert_eq!(refused["family_id"], Value::Null);
    assert_eq!(refused["pin_id"], Value::Null);
    assert!(refused.get("owner_nonce").is_none());
    assert!(!refused.to_string().contains(wrong));
    assert_eq!(
        Connection::open(store.join("coord.db"))
            .unwrap()
            .query_row("SELECT COUNT(*) FROM reader_registrations", [], |row| {
                row.get::<_, i64>(0)
            })
            .unwrap(),
        1
    );

    let human = reader_release(&store, pin, wrong, false);
    assert_eq!(human.status.code(), Some(1));
    assert!(human.stdout.is_empty());
    let stderr = String::from_utf8(human.stderr).unwrap();
    assert_eq!(
        stderr,
        "refused operation=reader_release failure=reader_owner_mismatch error=reader_authentication_failed\n"
    );
    assert!(!stderr.contains(wrong));
}

#[test]
fn failure_reports_null_every_snapshot_and_owner_field() {
    let fixture = tempfile::tempdir().unwrap();
    let store = fixture.path().join("store");
    import_view(fixture.path(), &store);
    let owner_pid = std::process::id().to_string();
    let output = reader_acquire(&store, "gen-999", "miller", &owner_pid, NONCE, true);
    let report = json_report(&output, 1);

    for field in [
        "family_id",
        "view_id",
        "pin_id",
        "generation_name",
        "manifest_generation",
        "owner_pid",
        "store_instance_id",
        "manifest_hash",
        "extraction_identity_epoch",
        "served_store_log_sequence",
        "min_retained_store_log_sequence",
        "snapshot_fingerprint",
        "protected_manifest_count",
        "expires_at",
        "warning",
    ] {
        assert_eq!(report[field], Value::Null, "{field}");
    }
    assert!(report.get("owner_nonce").is_none());
    assert!(report.get("owner_birth_identity").is_none());
    assert!(report.get("released").is_none());
}

#[test]
fn renew_and_release_coexist_with_an_ordinary_writer_lease() {
    let fixture = tempfile::tempdir().unwrap();
    let store = fixture.path().join("store");
    import_view(fixture.path(), &store);
    let owner_pid = std::process::id().to_string();
    let acquired = reader_acquire(&store, "gen-001", "miller", &owner_pid, NONCE, true);
    let acquired = json_report(&acquired, 0);
    let pin = acquired["pin_id"].as_str().unwrap();
    Connection::open(store.join("coord.db"))
        .unwrap()
        .execute(
            "INSERT INTO writer_lease
             (resource,holder_id,holder_version,holder_pid,heartbeat_at,expires_at,fencing_token)
             VALUES ('store-writer','foreign','2.40.0',?1,1,9223372036854775807,1)",
            [std::process::id()],
        )
        .unwrap();

    let renewed = reader_renew(&store, pin, &owner_pid, NONCE);
    assert_eq!(json_report(&renewed, 0)["state"], "renewed");
    let released = reader_release(&store, pin, NONCE, true);
    assert_eq!(json_report(&released, 0)["released"], true);
}

#[test]
fn renew_and_release_use_the_registration_after_current_changes() {
    let fixture = tempfile::tempdir().unwrap();
    let store = fixture.path().join("store");
    import_view(fixture.path(), &store);
    let owner_pid = std::process::id().to_string();
    let acquired = reader_acquire(&store, "gen-001", "miller", &owner_pid, NONCE, true);
    let acquired = json_report(&acquired, 0);
    let pin = acquired["pin_id"].as_str().unwrap();

    let promoted = julie_extract(&[
        "store",
        "maintain",
        "promote",
        "--store",
        store.to_str().unwrap(),
        "--apply",
        "--json",
    ]);
    assert_eq!(
        promoted.status.code(),
        Some(0),
        "stdout: {} stderr: {}",
        String::from_utf8_lossy(&promoted.stdout),
        String::from_utf8_lossy(&promoted.stderr)
    );
    assert_eq!(
        std::fs::read_to_string(store.join("CURRENT")).unwrap(),
        "gen-002\n"
    );

    let renewed = reader_renew(&store, pin, &owner_pid, NONCE);
    let renewed = json_report(&renewed, 0);
    assert_eq!(renewed["generation_name"], "gen-001");
    assert_eq!(
        renewed["snapshot_fingerprint"],
        acquired["snapshot_fingerprint"]
    );
    let released = reader_release(&store, pin, NONCE, true);
    assert_eq!(json_report(&released, 0)["released"], true);
}

#[test]
fn renew_and_release_refuse_live_foreign_maintenance() {
    let fixture = tempfile::tempdir().unwrap();
    let store = fixture.path().join("store");
    import_view(fixture.path(), &store);
    let owner_pid = std::process::id().to_string();
    let acquired = reader_acquire(&store, "gen-001", "miller", &owner_pid, NONCE, true);
    let acquired = json_report(&acquired, 0);
    let pin = acquired["pin_id"].as_str().unwrap();
    Connection::open(store.join("coord.db"))
        .unwrap()
        .execute(
            "INSERT INTO maintenance_intent
             (resource,run_id,action,source_generation_name,owner_id,owner_pid,
              fencing_token,heartbeat_at,expires_at,started_at,plan_fingerprint,
              source_min_writer_version)
             VALUES ('store-maintenance','foreign','gc','gen-001','foreign',?1,
                     1,1,9223372036854775807,1,'foreign-plan','2.40.0')",
            [std::process::id()],
        )
        .unwrap();

    let other_nonce = "abcdef0123456789abcdef0123456789";
    let acquired = reader_acquire(&store, "gen-001", "miller", &owner_pid, other_nonce, true);
    assert_eq!(json_report(&acquired, 1)["failure_class"], "busy");
    let renewed = reader_renew(&store, pin, &owner_pid, NONCE);
    assert_eq!(json_report(&renewed, 1)["failure_class"], "busy");
    let released = reader_release(&store, pin, NONCE, true);
    assert_eq!(json_report(&released, 1)["failure_class"], "busy");
    assert_eq!(
        Connection::open(store.join("coord.db"))
            .unwrap()
            .query_row("SELECT COUNT(*) FROM reader_registrations", [], |row| {
                row.get::<_, i64>(0)
            })
            .unwrap(),
        1
    );
}

#[test]
fn unknown_process_identity_refuses_before_registration() {
    let fixture = tempfile::tempdir().unwrap();
    let store = fixture.path().join("store");
    import_view(fixture.path(), &store);

    let output = reader_acquire(
        &store,
        "gen-001",
        "miller",
        &u32::MAX.to_string(),
        NONCE,
        true,
    );
    let report = json_report(&output, 1);
    assert_eq!(report["failure_class"], "reader_identity_unknown");
    assert_eq!(
        Connection::open(store.join("coord.db"))
            .unwrap()
            .query_row("SELECT COUNT(*) FROM reader_registrations", [], |row| {
                row.get::<_, i64>(0)
            })
            .unwrap(),
        0
    );
}

#[test]
fn acquire_activates_the_reader_floor_once_outside_admission() {
    let fixture = tempfile::tempdir().unwrap();
    let store = fixture.path().join("store");
    import_view(fixture.path(), &store);
    let store_db = store.join("gen-001/store.db");
    Connection::open(&store_db)
        .unwrap()
        .execute(
            "UPDATE store_meta SET value='2.39.0' WHERE key='min_writer_version'",
            [],
        )
        .unwrap();
    let owner_pid = std::process::id().to_string();

    let output = reader_acquire(&store, "gen-001", "miller", &owner_pid, NONCE, true);
    assert_eq!(json_report(&output, 0)["state"], "acquired");
    assert_eq!(
        Connection::open(&store_db)
            .unwrap()
            .query_row(
                "SELECT value FROM store_meta WHERE key='min_writer_version'",
                [],
                |row| row.get::<_, String>(0),
            )
            .unwrap(),
        "2.40.0"
    );
}

#[test]
fn unsafe_floor_activation_and_live_maintenance_refuse_as_busy() {
    let fixture = tempfile::tempdir().unwrap();
    let store = fixture.path().join("store");
    import_view(fixture.path(), &store);
    let store_db = store.join("gen-001/store.db");
    Connection::open(&store_db)
        .unwrap()
        .execute(
            "UPDATE store_meta SET value='2.39.0' WHERE key='min_writer_version'",
            [],
        )
        .unwrap();
    Connection::open(store.join("coord.db"))
        .unwrap()
        .execute(
            "INSERT INTO maintenance_intent
             (resource,run_id,action,source_generation_name,owner_id,owner_pid,
              fencing_token,heartbeat_at,expires_at,started_at,plan_fingerprint,
              source_min_writer_version)
             VALUES ('store-maintenance','foreign','gc','gen-001','foreign',?1,
                     1,1,9223372036854775807,1,'foreign-plan','2.39.0')",
            [std::process::id()],
        )
        .unwrap();
    let owner_pid = std::process::id().to_string();

    let output = reader_acquire(&store, "gen-001", "miller", &owner_pid, NONCE, true);
    let report = json_report(&output, 1);
    assert_eq!(report["failure_class"], "busy");
    assert_eq!(
        Connection::open(&store_db)
            .unwrap()
            .query_row(
                "SELECT value FROM store_meta WHERE key='min_writer_version'",
                [],
                |row| row.get::<_, String>(0),
            )
            .unwrap(),
        "2.39.0"
    );
}

#[test]
fn incompatible_newer_floor_uses_exit_three() {
    let fixture = tempfile::tempdir().unwrap();
    let store = fixture.path().join("store");
    import_view(fixture.path(), &store);
    Connection::open(store.join("gen-001/store.db"))
        .unwrap()
        .execute(
            "UPDATE store_meta SET value='999.0.0' WHERE key='min_writer_version'",
            [],
        )
        .unwrap();
    let owner_pid = std::process::id().to_string();

    let output = reader_acquire(&store, "gen-001", "miller", &owner_pid, NONCE, true);
    let report = json_report(&output, 3);
    assert_eq!(report["failure_class"], "incompatible_store");
}

#[test]
fn future_store_and_coordinator_schemas_use_exit_three() {
    for database in ["gen-001/store.db", "coord.db"] {
        let fixture = tempfile::tempdir().unwrap();
        let store = fixture.path().join("store");
        import_view(fixture.path(), &store);
        Connection::open(store.join(database))
            .unwrap()
            .execute_batch("PRAGMA user_version=999;")
            .unwrap();
        let owner_pid = std::process::id().to_string();

        let output = reader_acquire(&store, "gen-001", "miller", &owner_pid, NONCE, true);
        let report = json_report(&output, 3);
        assert_eq!(report["failure_class"], "incompatible_store", "{database}");
        assert_eq!(
            report["error"], "store is incompatible with reader operations",
            "{database}"
        );
    }
}

#[test]
fn missing_reader_catalog_refuses_without_recreating_it() {
    let fixture = tempfile::tempdir().unwrap();
    let store = fixture.path().join("store");
    import_view(fixture.path(), &store);
    let coordinator_path = store.join("coord.db");
    Connection::open(&coordinator_path)
        .unwrap()
        .execute("DROP TABLE reader_registrations", [])
        .unwrap();
    let owner_pid = std::process::id().to_string();

    let output = reader_acquire(&store, "gen-001", "miller", &owner_pid, NONCE, true);
    let report = json_report(&output, 1);
    assert_eq!(report["failure_class"], "operational");
    assert_eq!(
        Connection::open(&coordinator_path)
            .unwrap()
            .query_row(
                "SELECT COUNT(*) FROM sqlite_schema WHERE type='table' AND name='reader_registrations'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
        0
    );
}

#[test]
fn malformed_legacy_reader_catalog_refuses_floor_activation() {
    let fixture = tempfile::tempdir().unwrap();
    let store = fixture.path().join("store");
    import_view(fixture.path(), &store);
    let store_db = store.join("gen-001/store.db");
    Connection::open(&store_db)
        .unwrap()
        .execute(
            "UPDATE store_meta SET value='2.39.0' WHERE key='min_writer_version'",
            [],
        )
        .unwrap();
    let coordinator_path = store.join("coord.db");
    Connection::open(&coordinator_path)
        .unwrap()
        .execute_batch(
            "DROP TABLE reader_registrations;
             CREATE TABLE reader_registrations(pin_id TEXT);",
        )
        .unwrap();
    let owner_pid = std::process::id().to_string();

    let output = reader_acquire(&store, "gen-001", "miller", &owner_pid, NONCE, true);
    let report = json_report(&output, 3);
    assert_eq!(report["failure_class"], "incompatible_store");
    assert_eq!(
        Connection::open(&store_db)
            .unwrap()
            .query_row(
                "SELECT value FROM store_meta WHERE key='min_writer_version'",
                [],
                |row| row.get::<_, String>(0),
            )
            .unwrap(),
        "2.39.0"
    );
    assert_eq!(
        Connection::open(&coordinator_path)
            .unwrap()
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_info('reader_registrations')",
                [],
                |row| { row.get::<_, i64>(0) }
            )
            .unwrap(),
        1
    );
}

#[test]
fn pruned_store_log_reports_zero_sequences() {
    let fixture = tempfile::tempdir().unwrap();
    let store = fixture.path().join("store");
    import_view(fixture.path(), &store);
    Connection::open(store.join("gen-001/store.db"))
        .unwrap()
        .execute("DELETE FROM store_log", [])
        .unwrap();
    let owner_pid = std::process::id().to_string();

    let output = reader_acquire(&store, "gen-001", "miller", &owner_pid, NONCE, true);
    let report = json_report(&output, 0);
    assert_eq!(report["served_store_log_sequence"], 0);
    assert_eq!(report["min_retained_store_log_sequence"], 0);
    assert!(!store.join(".miller").exists());
    assert!(!fixture.path().join(".miller").exists());
}

#[test]
fn human_success_never_prints_the_nonce() {
    let fixture = tempfile::tempdir().unwrap();
    let store = fixture.path().join("store");
    import_view(fixture.path(), &store);
    let owner_pid = std::process::id().to_string();

    let output = reader_acquire(&store, "gen-001", "miller", &owner_pid, NONCE, false);
    assert_eq!(output.status.code(), Some(0));
    assert!(output.stderr.is_empty());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.starts_with("acquired family="));
    assert!(!stdout.contains(NONCE));
}
