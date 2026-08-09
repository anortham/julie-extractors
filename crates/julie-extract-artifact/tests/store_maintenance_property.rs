use julie_extract_artifact::store::{
    MaintenanceCapacity, MaintenancePolicy, MaintenanceSnapshot, ManifestFact, ManifestVersionFact,
    PlanBinding, VersionFact, plan_maintenance,
};

#[test]
fn planner_matches_a_small_reference_model_across_deterministic_permutations() {
    for seed in 0..64_u64 {
        let mut snapshot = MaintenanceSnapshot {
            binding: PlanBinding {
                family_id: "family-a".to_string(),
                current_generation: "gen-001".to_string(),
                store_root_fingerprint: "sha256:store".to_string(),
                coordinator_root_fingerprint: "sha256:coord".to_string(),
                store_log_max: seed as i64,
                request_watermark: seed as i64,
                allocator_marks: Vec::new(),
            },
            now_ms: 20 * 86_400_000,
            capacity: MaintenanceCapacity::default(),
            ..MaintenanceSnapshot::default()
        };
        let count = 1 + (seed % 40) as i64;
        for version_id in 1..=count {
            snapshot.versions.push(VersionFact {
                version_id,
                path: format!("src/{}.rs", version_id % 3),
                logical_bytes: (version_id as u64) * 13,
                complete_l1: true,
                complete_l2: version_id % 2 == 0,
                complete_l3: version_id % 3 == 0,
            });
            snapshot.manifests.push(ManifestFact {
                view_id: "view-a".to_string(),
                generation: version_id,
                created_at_ms: (version_id % 10) * 86_400_000,
                current: version_id == count,
            });
            snapshot.manifest_versions.push(ManifestVersionFact {
                view_id: "view-a".to_string(),
                generation: version_id,
                version_id,
                path: format!("src/{}.rs", version_id % 3),
                failed_preserved: version_id % 7 == 0,
            });
        }

        let expected_eligible = reference_eligible(&snapshot, 7, 24);
        let plan = plan_maintenance(&snapshot, &MaintenancePolicy::default()).unwrap();

        assert_eq!(plan.eligible_manifests, expected_eligible, "seed={seed}");
        snapshot.versions.reverse();
        snapshot.manifests.reverse();
        snapshot.manifest_versions.reverse();
        assert_eq!(
            plan.fingerprint,
            plan_maintenance(&snapshot, &MaintenancePolicy::default())
                .unwrap()
                .fingerprint,
            "seed={seed}"
        );
    }
}

fn reference_eligible(
    snapshot: &MaintenanceSnapshot,
    retention_days: i64,
    path_cap: usize,
) -> Vec<(String, i64)> {
    let cutoff = snapshot.now_ms - retention_days * 86_400_000;
    let mut eligible = Vec::new();
    for manifest in snapshot
        .manifests
        .iter()
        .filter(|manifest| !manifest.current)
    {
        if manifest.created_at_ms > cutoff {
            continue;
        }
        let entries = snapshot.manifest_versions.iter().filter(|entry| {
            entry.view_id == manifest.view_id && entry.generation == manifest.generation
        });
        let mut beyond_cap = false;
        for entry in entries {
            let newer = snapshot
                .manifest_versions
                .iter()
                .filter(|candidate| candidate.path == entry.path)
                .filter_map(|candidate| {
                    snapshot
                        .manifests
                        .iter()
                        .find(|other| {
                            other.view_id == candidate.view_id
                                && other.generation == candidate.generation
                                && !other.current
                        })
                        .map(|other| {
                            (
                                other.created_at_ms,
                                other.generation,
                                other.view_id.as_str(),
                            )
                        })
                })
                .filter(|key| {
                    *key > (
                        manifest.created_at_ms,
                        manifest.generation,
                        manifest.view_id.as_str(),
                    )
                })
                .count();
            beyond_cap |= newer >= path_cap;
        }
        if beyond_cap {
            eligible.push((manifest.view_id.clone(), manifest.generation));
        }
    }
    eligible.sort();
    eligible
}
