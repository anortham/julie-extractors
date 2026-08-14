#![cfg(feature = "test-store-maintenance-contract")]

use julie_extract_artifact::store::{
    CapacityProvider, MaintenanceCapacity, MaintenanceClock, MaintenanceInspector,
    MaintenancePolicy, MaintenanceSnapshot, PlanBinding, StoreConnectionFactory, StoreLayout,
    VersionFact, plan_maintenance,
};
use rusqlite::{Connection, params};
#[cfg(unix)]
use std::process::Command;

#[test]
fn lifecycle_cohorts_obey_exact_version_wal_and_capacity_bounds_at_scale() {
    let small = plan_with_versions(10_000, 1, u64::MAX);
    assert_eq!(small.demotion_cohort.len(), 100);
    assert!(small.capacity.promotion_fits);

    let wal_bound = plan_with_versions(10_000, 1024 * 1024, u64::MAX);
    assert_eq!(wal_bound.demotion_cohort.len(), 64);
    assert_eq!(
        wal_bound.capacity.demotion_wal_headroom_bytes,
        64 * 1024 * 1024
    );

    let refused = plan_with_versions(10_000, 1024 * 1024, 1);
    assert!(!refused.capacity.promotion_fits);
    assert_eq!(refused.versions.len(), 10_000);
}

#[test]
fn lifecycle_sqlite_windows_and_rss_remain_bounded_at_miller_scale() {
    if std::env::var_os("JULIE_MAINTENANCE_SCALE_ROWS").is_none() {
        #[cfg(unix)]
        let small = timed_worker(2_000);
        #[cfg(unix)]
        let large = timed_worker(20_000);

        #[cfg(not(unix))]
        {
            run_scale_worker(2_000);
            run_scale_worker(20_000);
        }

        #[cfg(unix)]
        assert!(
            large <= small + 64 * 1024 * 1024,
            "small={small} large={large}"
        );
    }
}

#[test]
fn lifecycle_scale_worker() {
    let Some(rows) = std::env::var_os("JULIE_MAINTENANCE_SCALE_ROWS") else {
        return;
    };
    let rows = rows.to_string_lossy().parse::<i64>().unwrap();
    let fixture = tempfile::tempdir().unwrap();
    let layout =
        StoreLayout::create(fixture.path(), "scale-family", env!("CARGO_PKG_VERSION")).unwrap();
    let mut connection = Connection::open(layout.store_db()).unwrap();
    let transaction = connection.transaction().unwrap();
    {
        let mut insert = transaction
            .prepare(
                "INSERT INTO file_versions
                 (version_id,path,content_hash,extraction_epoch,language,content_bytes,line_count,
                  complete_l1,complete_l2,complete_l3)
                 VALUES (?1,?2,?3,1,'rust',1024,1,1,2,3)",
            )
            .unwrap();
        for version_id in 1..=rows {
            insert
                .execute(params![
                    version_id,
                    format!("src/{version_id}.rs"),
                    format!("blake3:{version_id:064x}")
                ])
                .unwrap();
        }
    }
    transaction.commit().unwrap();
    drop(connection);
    let plan = MaintenanceInspector::new(
        StoreConnectionFactory::new(layout, "scale-family", env!("CARGO_PKG_VERSION")),
        FixedClock,
        FixedCapacity,
    )
    .with_window_size(64)
    .inspect()
    .unwrap();
    assert_eq!(plan.versions.len(), rows as usize);
    assert!(plan.max_observed_window <= 64);
}

#[cfg(not(unix))]
fn run_scale_worker(rows: i64) -> std::process::Output {
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "lifecycle_scale_worker",
            "--nocapture",
            "--test-threads=1",
        ])
        .env("JULIE_MAINTENANCE_SCALE_ROWS", rows.to_string())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

#[cfg(unix)]
fn timed_worker(rows: i64) -> u64 {
    let mut command = Command::new("/usr/bin/time");
    #[cfg(target_os = "macos")]
    command.arg("-l");
    #[cfg(all(unix, not(target_os = "macos")))]
    command.arg("-v");
    command.arg(std::env::current_exe().unwrap());
    parse_peak_rss(&run_scale_worker_with_command(&mut command, rows).stderr)
}

#[cfg(unix)]
fn run_scale_worker_with_command(command: &mut Command, rows: i64) -> std::process::Output {
    let output = command
        .args([
            "--exact",
            "lifecycle_scale_worker",
            "--nocapture",
            "--test-threads=1",
        ])
        .env("JULIE_MAINTENANCE_SCALE_ROWS", rows.to_string())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

#[cfg(unix)]
fn parse_peak_rss(stderr: &[u8]) -> u64 {
    let text = String::from_utf8_lossy(stderr);
    #[cfg(target_os = "macos")]
    return text
        .lines()
        .find(|line| line.contains("maximum resident set size"))
        .and_then(|line| line.split_whitespace().next())
        .and_then(|value| value.parse().ok())
        .unwrap();
    #[cfg(all(unix, not(target_os = "macos")))]
    return text
        .lines()
        .find(|line| line.contains("Maximum resident set size"))
        .and_then(|line| line.split(':').nth(1))
        .and_then(|value| value.trim().parse::<u64>().ok())
        .unwrap()
        * 1_024;
}

#[derive(Clone, Copy)]
struct FixedClock;

impl MaintenanceClock for FixedClock {
    fn now_ms(&self) -> i64 {
        20 * 86_400_000
    }
}

#[derive(Clone, Copy)]
struct FixedCapacity;

impl CapacityProvider for FixedCapacity {
    fn free_bytes(&self, _: &std::path::Path) -> Result<u64, std::io::Error> {
        Ok(u64::MAX)
    }

    fn staged_generation_bytes(&self, _: &std::path::Path) -> Result<u64, std::io::Error> {
        Ok(0)
    }
}

fn plan_with_versions(
    count: i64,
    logical_bytes: u64,
    free_bytes: u64,
) -> julie_extract_artifact::store::MaintenancePlan {
    let snapshot = MaintenanceSnapshot {
        binding: PlanBinding {
            family_id: "scale-family".to_string(),
            current_generation: "gen-001".to_string(),
            store_root_fingerprint: "sha256:store".to_string(),
            coordinator_root_fingerprint: "sha256:coord".to_string(),
            store_log_max: count,
            request_watermark: count,
            allocator_marks: Vec::new(),
        },
        versions: (1..=count)
            .map(|version_id| VersionFact {
                version_id,
                path: format!("src/{version_id}.rs"),
                logical_bytes,
                complete_l1: true,
                complete_l2: true,
                complete_l3: true,
            })
            .collect(),
        capacity: MaintenanceCapacity {
            free_bytes,
            staged_generation_bytes: 128 * 1024 * 1024,
            ..MaintenanceCapacity::default()
        },
        ..MaintenanceSnapshot::default()
    };
    plan_maintenance(&snapshot, &MaintenancePolicy::default()).unwrap()
}
