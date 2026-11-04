use std::collections::BTreeMap;

use sha2::{Digest, Sha256};
use xtask::compat::{ArtifactDump, TableDump};
use xtask::performance::comparison::{
    AllowedRowChange, ApprovedBudgetDelta, BaselineExpectationReceipt, BaselineSnapshotExpectation,
    BaselineSnapshotPin, BudgetApproval, BudgetApprovalContext, BudgetApprovalFile, BudgetMetric,
    BuildEvidence, BuildMetadata, BuildSourceIdentity, ComparisonHealth, ComparisonProvenance,
    ExtractorBuildMetadata, RowChangeAction, RowDelta, SampleMetrics,
    approved_budget_deltas_sha256, artifact_dump_sha256, budget_violations, compare_artifact_dumps,
    compare_artifact_dumps_for_scope, compare_health, evaluate_budgets, normalized_metrics,
    sample_spread_is_conclusive, validate_allowlist_scope_usage, validate_baseline_snapshot,
    validate_budget_approval_context, validate_budget_approval_file, validate_build_metadata_pair,
    validate_operation_fresh_equivalence, validate_provenance_pair,
};

fn dump(rows: &[&str]) -> ArtifactDump {
    ArtifactDump {
        tables: vec![TableDump {
            table: "symbols".to_string(),
            text: format!("#columns\tpath\tlanguage\tname\n{}", rows.join("\n")),
        }],
    }
}

fn allowed(
    table: &str,
    action: RowChangeAction,
    canonical_row: &str,
    count: usize,
) -> AllowedRowChange {
    AllowedRowChange {
        id: "symbol-kind-v1".to_string(),
        ledger_marker: "symbol-kind-output-change".to_string(),
        language: None,
        level: None,
        workload: None,
        table: table.to_string(),
        action,
        canonical_row: canonical_row.to_string(),
        row_sha256: format!("{:x}", Sha256::digest(canonical_row.as_bytes())),
        count,
    }
}

fn provenance(manifest_sha256: &str, binary_sha256: &str) -> ComparisonProvenance {
    let build = build_metadata(binary_sha256, '1', '2');
    ComparisonProvenance {
        manifest_sha256: manifest_sha256.to_string(),
        binary_path: std::path::PathBuf::from("target/release/julie-extract"),
        binary_sha256: binary_sha256.to_string(),
        binary_version: "julie-extract 3.11.0".to_string(),
        build_metadata_path: std::path::PathBuf::from("target/release/julie-extract.build.json"),
        build_metadata_sha256: "d".repeat(64),
        build_metadata_format_version: 1,
        source: build.source,
        build: build.build,
        toolchain: "rustc 1.99.0; cargo 1.99.0".to_string(),
        profile: "release".to_string(),
        level: "full".to_string(),
        jobs: 1,
        os: "linux x86_64".to_string(),
        cpu: "reference cpu".to_string(),
        memory_bytes: 64 * 1024 * 1024 * 1024,
        storage: "nvme".to_string(),
        cache_policy: "warm page cache".to_string(),
        peak_rss_method: "GNU time max RSS in KiB".to_string(),
    }
}

fn build_metadata(binary_sha256: &str, commit: char, tree: char) -> BuildMetadata {
    let recipe = vec![
        "cargo".to_string(),
        "build".to_string(),
        "--release".to_string(),
    ];
    BuildMetadata {
        format_version: 1,
        binary_sha256: binary_sha256.to_string(),
        source: BuildSourceIdentity {
            commit: commit.to_string().repeat(40),
            tree_hash_algorithm: "git-ls-tree-r-v1-sha256".to_string(),
            tree_sha256: tree.to_string().repeat(64),
            dirty: false,
            dirty_state_sha256: None,
        },
        build: ExtractorBuildMetadata {
            rustc: "rustc 1.99.0".to_string(),
            cargo: "cargo 1.99.0".to_string(),
            host_target: "x86_64-unknown-linux-gnu".to_string(),
            target: "x86_64-unknown-linux-gnu".to_string(),
            profile: "release".to_string(),
            features: vec!["default".to_string()],
            options: BTreeMap::new(),
            recipe_sha256: format!("{:x}", Sha256::digest(serde_json::to_vec(&recipe).unwrap())),
            recipe,
            evidence: BuildEvidence {
                kind: "release-log".to_string(),
                reference: "build/123".to_string(),
                sha256: "e".repeat(64),
            },
        },
    }
}

fn health() -> ComparisonHealth {
    ComparisonHealth {
        files_scanned: 1,
        files_failed: 0,
        files_unsupported: 0,
        errors: 0,
        skipped: BTreeMap::new(),
        warnings: BTreeMap::new(),
        parse_diagnostics: BTreeMap::new(),
    }
}

fn approved_delta() -> ApprovedBudgetDelta {
    let canonical_row = "text:src/a.ts\ttext:typescript\ttext:render-v2".to_string();
    ApprovedBudgetDelta {
        table: "symbols".to_string(),
        action: RowChangeAction::Add,
        row_sha256: format!("{:x}", Sha256::digest(canonical_row.as_bytes())),
        canonical_row,
        count: 1,
        allowlist_id: "symbol-kind-v2".to_string(),
        ledger_marker: "symbol-kind-output-change".to_string(),
        source_state_sha256: "a".repeat(64),
    }
}

fn budget_approval() -> BudgetApproval {
    let approved_deltas = vec![approved_delta()];
    BudgetApproval {
        id: "typescript-full-fresh-elapsed".to_string(),
        owner: "language coverage owner".to_string(),
        approval_reference: "decision-42".to_string(),
        approval_evidence_sha256: "d".repeat(64),
        baseline_binary_sha256: "1".repeat(64),
        candidate_binary_sha256: "2".repeat(64),
        manifest_sha256: "3".repeat(64),
        language: "typescript".to_string(),
        level: "full".to_string(),
        workload: "fresh_scan".to_string(),
        baseline_profile: "release".to_string(),
        candidate_profile: "release".to_string(),
        metric: BudgetMetric::ElapsedMs,
        maximum_growth_percent: 30,
        maximum_growth_absolute: 300,
        approved_deltas_sha256: approved_budget_deltas_sha256(&approved_deltas),
        table_row_counts: BTreeMap::from([("symbols".to_string(), 1)]),
        approved_deltas,
        reason: "Approved exact symbol additions for this comparison".to_string(),
    }
}

fn budget_context<'a>(
    baseline_binary_sha256: &'a str,
    candidate_binary_sha256: &'a str,
    manifest_sha256: &'a str,
) -> BudgetApprovalContext<'a> {
    BudgetApprovalContext {
        baseline_binary_sha256,
        candidate_binary_sha256,
        manifest_sha256,
        language: "typescript",
        level: "full",
        workload: "fresh_scan",
        baseline_profile: "release",
        candidate_profile: "release",
    }
}

fn added_row_delta(source_state_sha256: &str) -> RowDelta {
    let approved = approved_delta();
    RowDelta {
        table: approved.table,
        action: approved.action,
        canonical_row: approved.canonical_row,
        row_sha256: approved.row_sha256,
        count: approved.count,
        allowlist_id: approved.allowlist_id,
        ledger_marker: approved.ledger_marker,
        allowed: true,
        source_state_sha256: Some(source_state_sha256.to_string()),
    }
}

#[test]
fn identical_multisets_preserve_duplicate_rows_and_array_payloads() {
    let row = "text:src/a.ts\ttext:typescript\ttext:render\ttext:symbol-owner-1\ttext:reference-site-1\ttext:[\"x\",\"y\"]";
    assert!(compare_artifact_dumps(&dump(&[row, row]), &dump(&[row, row]), &[], "").is_ok());
}

#[test]
fn table_and_column_contract_changes_fail_closed() {
    let baseline = dump(&[]);
    let candidate_table = ArtifactDump {
        tables: vec![
            baseline.tables[0].clone(),
            xtask::compat::TableDump {
                table: "new_fact_table".to_string(),
                text: "#columns\tvalue\n".to_string(),
            },
        ],
    };
    assert!(
        compare_artifact_dumps(&baseline, &candidate_table, &[], "")
            .unwrap_err()
            .contains("table sets differ")
    );

    let candidate_columns = ArtifactDump {
        tables: vec![xtask::compat::TableDump {
            table: "symbols".to_string(),
            text: "#columns\tpath\tlanguage\tname\textra\n".to_string(),
        }],
    };
    assert!(
        compare_artifact_dumps(&baseline, &candidate_columns, &[], "")
            .unwrap_err()
            .contains("column contract differs")
    );
}

#[test]
fn lost_rows_fail_without_a_matching_ledger_allowance() {
    let error = compare_artifact_dumps(
        &dump(&["text:src/a.ts\ttext:typescript\ttext:render"]),
        &dump(&[]),
        &[],
        "",
    )
    .expect_err("removed facts fail closed");
    assert!(error.contains("unallowed remove"));
}

#[test]
fn parse_diagnostic_rows_are_compared_as_exact_durable_facts() {
    let baseline = ArtifactDump {
        tables: vec![xtask::compat::TableDump {
            table: "parse_diagnostics".to_string(),
            text: "#columns\tpath\tlanguage\tmessage\ntext:src/a.ts\ttext:typescript\ttext:ERROR\n"
                .to_string(),
        }],
    };
    let candidate = ArtifactDump {
        tables: vec![xtask::compat::TableDump {
            table: "parse_diagnostics".to_string(),
            text: "#columns\tpath\tlanguage\tmessage\ntext:src/a.ts\ttext:typescript\ttext:ERROR\ntext:src/b.ts\ttext:typescript\ttext:ERROR\n".to_string(),
        }],
    };
    let error = compare_artifact_dumps(&baseline, &candidate, &[], "")
        .expect_err("diagnostic changes are exact row changes");
    assert!(error.contains("unallowed add"));
    assert!(error.contains("src/b.ts"));
}

#[test]
fn exact_row_change_passes_only_with_a_matching_allowlist_and_ledger_marker() {
    let row = "text:src/a.ts\ttext:typescript\ttext:render";
    let entry = allowed("symbols", RowChangeAction::Add, row, 1);
    assert!(
        compare_artifact_dumps(
            &dump(&[]),
            &dump(&[row]),
            &[entry],
            "symbol-kind-output-change"
        )
        .is_ok()
    );
}

#[test]
fn exact_row_removal_passes_only_with_a_matching_allowlist_and_ledger_marker() {
    let row = "text:src/a.ts\ttext:typescript\ttext:render";
    let entry = allowed("symbols", RowChangeAction::Remove, row, 1);
    assert!(
        compare_artifact_dumps(
            &dump(&[row]),
            &dump(&[]),
            &[entry],
            "symbol-kind-output-change"
        )
        .is_ok()
    );
}

#[test]
fn language_scope_must_match_the_canonical_row_language() {
    let row = "text:src/a.ts\ttext:typescript\ttext:render";
    let mut entry = allowed("symbols", RowChangeAction::Add, row, 1);
    entry.language = Some("rust".to_string());
    let error = compare_artifact_dumps(
        &dump(&[]),
        &dump(&[row]),
        &[entry],
        "symbol-kind-output-change",
    )
    .expect_err("a language scope cannot authorize another language row");
    assert!(error.contains("unallowed add"));

    let mut entry = allowed("symbols", RowChangeAction::Add, row, 1);
    entry.language = Some("typescript".to_string());
    assert!(
        compare_artifact_dumps(
            &dump(&[]),
            &dump(&[row]),
            &[entry],
            "symbol-kind-output-change",
        )
        .is_ok()
    );
}

#[test]
fn duplicate_allowlist_ids_are_rejected() {
    let row = "text:src/a.ts\ttext:typescript\ttext:render";
    let entry = allowed("symbols", RowChangeAction::Add, row, 1);
    let error = compare_artifact_dumps(
        &dump(&[]),
        &dump(&[row]),
        &[entry.clone(), entry],
        "symbol-kind-output-change",
    )
    .expect_err("duplicate allowances are ambiguous");
    assert!(error.contains("duplicate allowlist id"));
}

#[test]
fn stale_allowlist_entries_fail_when_the_change_is_absent() {
    let row = "text:src/a.ts\ttext:typescript\ttext:render";
    let error = compare_artifact_dumps(
        &dump(&[]),
        &dump(&[]),
        &[allowed("symbols", RowChangeAction::Add, row, 1)],
        "symbol-kind-output-change",
    )
    .expect_err("unused entries must not leave blanket permission");
    assert!(error.contains("unused allowlist entry"));
}

#[test]
fn every_allowlisted_row_requires_its_output_ledger_marker() {
    let row = "text:src/a.ts\ttext:typescript\ttext:render";
    let error = compare_artifact_dumps(
        &dump(&[]),
        &dump(&[row]),
        &[allowed("symbols", RowChangeAction::Add, row, 1)],
        "",
    )
    .expect_err("an undocumented change is not allowed");
    assert!(error.contains("ledger marker"));

    let duplicated_marker = "symbol-kind-output-change\nsymbol-kind-output-change";
    let error = compare_artifact_dumps(
        &dump(&[]),
        &dump(&[row]),
        &[allowed("symbols", RowChangeAction::Add, row, 1)],
        duplicated_marker,
    )
    .expect_err("a repeated ledger marker is ambiguous");
    assert!(error.contains("exactly one"));
}

#[test]
fn growth_must_exceed_relative_and_absolute_budget_to_fail() {
    let baseline = SampleMetrics {
        elapsed_ms: 1_000,
        peak_rss_bytes: 100 * 1024 * 1024,
        artifact_bytes: 1_000_000,
    };
    let below_floor = SampleMetrics {
        elapsed_ms: 1_100,
        peak_rss_bytes: baseline.peak_rss_bytes,
        artifact_bytes: baseline.artifact_bytes,
    };
    assert!(budget_violations(&baseline, &below_floor).is_empty());

    let over_all = SampleMetrics {
        elapsed_ms: 1_250,
        peak_rss_bytes: 150 * 1024 * 1024,
        artifact_bytes: 2_200_000,
    };
    let violations = budget_violations(&baseline, &over_all);
    assert_eq!(violations.len(), 3);
}

#[test]
fn new_skips_and_warnings_fail_health_comparison() {
    let mut candidate = health();
    candidate
        .skipped
        .insert("slow_file_skipped:src/a.ts".to_string(), 1);
    let error = compare_health(&health(), &candidate).expect_err("new skips cannot pass");
    assert!(error.contains("skipped"));

    let mut candidate = health();
    candidate
        .warnings
        .insert("payload_conflict:src/a.ts".to_string(), 1);
    assert!(
        compare_health(&health(), &candidate)
            .unwrap_err()
            .contains("warning")
    );
}

#[test]
fn warning_counts_are_preserved_by_file_and_category() {
    let mut baseline = health();
    baseline
        .warnings
        .insert("parser_recovery:src/a.ts".to_string(), 2);
    assert!(compare_health(&baseline, &baseline).is_ok());

    let mut candidate = baseline.clone();
    candidate
        .warnings
        .insert("parser_recovery:src/a.ts".to_string(), 3);
    assert!(
        compare_health(&baseline, &candidate)
            .unwrap_err()
            .contains("parser_recovery:src/a.ts")
    );

    let mut candidate = baseline.clone();
    candidate
        .warnings
        .insert("parser_recovery:src/b.ts".to_string(), 1);
    assert!(
        compare_health(&baseline, &candidate)
            .unwrap_err()
            .contains("src/b.ts")
    );
}

#[test]
fn inherited_warning_disappearance_fails_the_file_category_health_gate() {
    let mut baseline = health();
    baseline
        .warnings
        .insert("parser_recovery:src/a.ts".to_string(), 1);
    let error = compare_health(&baseline, &health())
        .expect_err("a disappearing warning can hide lost work");
    assert!(error.contains("warning category/file counts changed"));
}

#[test]
fn latency_memory_and_size_budgets_fail_independently() {
    let baseline = SampleMetrics {
        elapsed_ms: 1_000,
        peak_rss_bytes: 100 * 1024 * 1024,
        artifact_bytes: 1_000_000,
    };
    let latency = SampleMetrics {
        elapsed_ms: 1_101,
        ..baseline
    };
    assert_eq!(
        budget_violations(&baseline, &latency)[0].metric,
        "elapsed_ms"
    );

    let memory = SampleMetrics {
        peak_rss_bytes: baseline.peak_rss_bytes + 32 * 1024 * 1024 + 1,
        ..baseline
    };
    assert_eq!(
        budget_violations(&baseline, &memory)[0].metric,
        "peak_rss_bytes"
    );

    let size = SampleMetrics {
        artifact_bytes: baseline.artifact_bytes + 1_048_577,
        ..baseline
    };
    assert_eq!(
        budget_violations(&baseline, &size)[0].metric,
        "artifact_bytes"
    );
}

#[test]
fn provenance_requires_same_pinned_inputs_and_environment() {
    let baseline = provenance(&"b".repeat(64), &"a".repeat(64));
    let candidate = provenance(&"b".repeat(64), &"c".repeat(64));
    assert!(validate_provenance_pair(&baseline, &candidate).is_ok());

    let changed_manifest = provenance(&"d".repeat(64), &"c".repeat(64));
    assert!(
        validate_provenance_pair(&baseline, &changed_manifest)
            .unwrap_err()
            .contains("manifest")
    );

    let mut changed_rss_method = provenance(&"b".repeat(64), &"c".repeat(64));
    changed_rss_method.peak_rss_method = "another collector".to_string();
    assert!(
        validate_provenance_pair(&baseline, &changed_rss_method)
            .unwrap_err()
            .contains("peak RSS method")
    );
}

#[test]
fn unstable_samples_are_inconclusive_after_batching() {
    assert!(sample_spread_is_conclusive(&[
        2_000, 2_010, 2_005, 2_015, 2_020
    ]));
    assert!(!sample_spread_is_conclusive(&[
        2_000, 2_010, 2_005, 2_500, 2_020
    ]));
}

#[test]
fn allowlist_count_must_match_the_full_multiset_delta() {
    let row = "text:src/a.ts\ttext:typescript\ttext:render";
    let error = compare_artifact_dumps(
        &dump(&[]),
        &dump(&[row, row]),
        &[allowed("symbols", RowChangeAction::Add, row, 1)],
        "symbol-kind-output-change",
    )
    .expect_err("a one-row allowance cannot hide a duplicate output row");
    assert!(error.contains("unallowed add"));
}

#[test]
fn allowlist_hash_must_match_the_canonical_row() {
    let row = "text:src/a.ts\ttext:typescript\ttext:render";
    let mut entry = allowed("symbols", RowChangeAction::Add, row, 1);
    entry.row_sha256 = "0".repeat(64);
    let error = compare_artifact_dumps(
        &dump(&[]),
        &dump(&[row]),
        &[entry],
        "symbol-kind-output-change",
    )
    .expect_err("a hash cannot authorize another row");
    assert!(error.contains("row hash mismatch"));
}

#[test]
fn candidate_diagnostics_may_decrease_but_cannot_increase() {
    let mut baseline = health();
    baseline
        .parse_diagnostics
        .insert("src/a.ts:ERROR".to_string(), 1);
    assert!(compare_health(&baseline, &health()).is_ok());

    let mut candidate = health();
    candidate
        .parse_diagnostics
        .insert("src/a.ts:ERROR".to_string(), 1);
    assert!(
        compare_health(&health(), &candidate)
            .unwrap_err()
            .contains("diagnostic")
    );
}

#[test]
fn candidate_scan_health_cannot_lose_a_file() {
    let mut candidate = health();
    candidate.files_scanned = 0;
    assert!(
        compare_health(&health(), &candidate)
            .unwrap_err()
            .contains("scanned")
    );
}

#[test]
fn provenance_hashes_must_be_sha256_values() {
    let mut candidate = provenance(&"b".repeat(64), &"c".repeat(64));
    candidate.binary_sha256 = "not-a-digest".to_string();
    assert!(
        validate_provenance_pair(&provenance(&"b".repeat(64), &"a".repeat(64)), &candidate)
            .unwrap_err()
            .contains("SHA-256")
    );
}

#[test]
fn provenance_requires_binary_version_and_path() {
    let baseline = provenance(&"b".repeat(64), &"a".repeat(64));
    let mut candidate = provenance(&"b".repeat(64), &"c".repeat(64));
    candidate.binary_version.clear();
    assert!(
        validate_provenance_pair(&baseline, &candidate)
            .unwrap_err()
            .contains("binary identity")
    );

    let mut candidate = provenance(&"b".repeat(64), &"c".repeat(64));
    candidate.binary_path.clear();
    assert!(
        validate_provenance_pair(&baseline, &candidate)
            .unwrap_err()
            .contains("binary identity")
    );
}

#[test]
fn extractor_build_metadata_is_hash_bound_and_source_complete() {
    let baseline = build_metadata(&"a".repeat(64), '1', '2');
    let candidate = build_metadata(&"b".repeat(64), '3', '4');
    assert!(
        validate_build_metadata_pair(&baseline, &candidate, &"a".repeat(64), &"b".repeat(64))
            .is_ok()
    );

    let mut relocated_recipe = candidate.clone();
    relocated_recipe
        .build
        .recipe
        .push("--remap-path-prefix=/other".to_string());
    relocated_recipe.build.recipe_sha256 =
        xtask::performance::comparison::build_recipe_sha256(&relocated_recipe.build.recipe);
    assert!(
        validate_build_metadata_pair(
            &baseline,
            &relocated_recipe,
            &"a".repeat(64),
            &"b".repeat(64)
        )
        .is_ok()
    );

    assert!(
        validate_build_metadata_pair(&baseline, &candidate, &"c".repeat(64), &"b".repeat(64))
            .unwrap_err()
            .contains("binary SHA-256")
    );

    let mut dirty_without_identity = candidate.clone();
    dirty_without_identity.source.dirty = true;
    assert!(
        validate_build_metadata_pair(
            &baseline,
            &dirty_without_identity,
            &"a".repeat(64),
            &"b".repeat(64)
        )
        .unwrap_err()
        .contains("dirty source")
    );

    let mut incompatible = candidate.clone();
    incompatible.build.features.push("simd".to_string());
    assert!(
        validate_build_metadata_pair(&baseline, &incompatible, &"a".repeat(64), &"b".repeat(64))
            .unwrap_err()
            .contains("features differ")
    );
}

#[test]
fn baseline_receipt_pins_old_rows_and_diagnostics_to_binary_manifest_and_source() {
    let old_diagnostic = "text:src/a.ts\ttext:typescript\ttext:ERROR";
    let old_fact = "text:src/a.ts\ttext:typescript\ttext:render";
    let new_fact = "text:src/a.ts\ttext:typescript\ttext:render-v2";
    let baseline = ArtifactDump {
        tables: vec![
            xtask::compat::TableDump {
                table: "parse_diagnostics".to_string(),
                text: format!(
                    "#columns\tpath\tlanguage\tmessage\n{}\n",
                    [old_diagnostic; 9].join("\n")
                ),
            },
            xtask::compat::TableDump {
                table: "symbols".to_string(),
                text: format!("#columns\tpath\tlanguage\tname\n{old_fact}\n"),
            },
        ],
    };
    let candidate = ArtifactDump {
        tables: vec![
            xtask::compat::TableDump {
                table: "parse_diagnostics".to_string(),
                text: "#columns\tpath\tlanguage\tmessage\n".to_string(),
            },
            xtask::compat::TableDump {
                table: "symbols".to_string(),
                text: format!("#columns\tpath\tlanguage\tname\n{old_fact}\n{new_fact}\n"),
            },
        ],
    };
    let source = BuildSourceIdentity {
        commit: "1".repeat(40),
        tree_hash_algorithm: "git-ls-tree-r-v1-sha256".to_string(),
        tree_sha256: "2".repeat(64),
        dirty: false,
        dirty_state_sha256: None,
    };
    let manifest_sha256 = "c".repeat(64);
    let binary_sha256 = "a".repeat(64);
    let mut pinned_health = health();
    pinned_health
        .parse_diagnostics
        .insert("reported_total".to_string(), 9);
    let receipt = BaselineExpectationReceipt {
        format_version: 1,
        baseline_binary_sha256: binary_sha256.clone(),
        manifest_sha256: manifest_sha256.clone(),
        source: source.clone(),
        snapshots: vec![BaselineSnapshotExpectation {
            language: "typescript".to_string(),
            level: "full".to_string(),
            workload: "single_file_update".to_string(),
            source_state_sha256: "d".repeat(64),
            semantic_dump_sha256: artifact_dump_sha256(&baseline)
                .expect("semantic dump has a valid table shape"),
            diagnostic_counts_by_file: BTreeMap::from([("src/a.ts".to_string(), 9)]),
            health: pinned_health.clone(),
        }],
    };
    let baseline_pin = BaselineSnapshotPin {
        binary_sha256: &binary_sha256,
        manifest_sha256: &manifest_sha256,
        source: &source,
        language: "typescript",
        level: "full",
        workload: "single_file_update",
        source_state_sha256: &"d".repeat(64),
    };
    assert!(validate_baseline_snapshot(&receipt, baseline_pin, &pinned_health, &baseline,).is_ok());

    let ledger = "symbol-kind-output-change";
    let mut diagnostic_delta = allowed(
        "parse_diagnostics",
        RowChangeAction::Remove,
        old_diagnostic,
        9,
    );
    diagnostic_delta.id = "diagnostics-fixed".to_string();
    diagnostic_delta.language = Some("typescript".to_string());
    diagnostic_delta.level = Some("full".to_string());
    diagnostic_delta.workload = Some("single_file_update".to_string());
    let mut fact_delta = allowed("symbols", RowChangeAction::Add, new_fact, 1);
    fact_delta.id = "new-exact-fact".to_string();
    fact_delta.language = Some("typescript".to_string());
    fact_delta.level = Some("full".to_string());
    fact_delta.workload = Some("single_file_update".to_string());
    assert!(
        compare_artifact_dumps_for_scope(
            &baseline,
            &candidate,
            &[diagnostic_delta.clone(), fact_delta.clone()],
            ledger,
            "typescript",
            "full",
            "single_file_update",
        )
        .is_ok()
    );

    let wrong_source_state_sha256 = "f".repeat(64);
    let wrong_snapshot_pin = BaselineSnapshotPin {
        source_state_sha256: &wrong_source_state_sha256,
        ..baseline_pin
    };
    let wrong_pin =
        validate_baseline_snapshot(&receipt, wrong_snapshot_pin, &pinned_health, &baseline);
    assert!(wrong_pin.unwrap_err().contains("no baseline expectation"));

    let wrong_output = ArtifactDump {
        tables: vec![baseline.tables[0].clone()],
    };
    assert!(
        validate_baseline_snapshot(&receipt, baseline_pin, &pinned_health, &wrong_output,)
            .unwrap_err()
            .contains("semantic dump SHA-256")
    );
}

#[test]
fn exact_allowances_for_disjoint_levels_do_not_conflict() {
    let row = "text:src/a.ts\ttext:typescript\ttext:render-v2";
    let mut references = allowed("symbols", RowChangeAction::Add, row, 1);
    references.id = "references-row".to_string();
    references.language = Some("typescript".to_string());
    references.level = Some("references".to_string());
    references.workload = Some("fresh_scan".to_string());
    let mut full = references.clone();
    full.id = "full-row".to_string();
    full.level = Some("full".to_string());
    let baseline = dump(&[]);
    let candidate = dump(&[row]);
    assert!(
        compare_artifact_dumps_for_scope(
            &baseline,
            &candidate,
            &[references.clone(), full.clone()],
            "symbol-kind-output-change",
            "typescript",
            "references",
            "fresh_scan",
        )
        .is_ok()
    );
    assert!(
        compare_artifact_dumps_for_scope(
            &baseline,
            &candidate,
            &[references, full],
            "symbol-kind-output-change",
            "typescript",
            "full",
            "fresh_scan",
        )
        .is_ok()
    );
}

#[test]
fn overlapping_allowlist_scopes_are_rejected_and_each_scope_must_be_used() {
    let row = "text:src/a.ts\ttext:typescript\ttext:render-v2";
    let mut references = allowed("symbols", RowChangeAction::Add, row, 1);
    references.id = "references-row".to_string();
    references.language = Some("typescript".to_string());
    references.level = Some("references".to_string());
    references.workload = Some("fresh_scan".to_string());
    let mut duplicate = references.clone();
    duplicate.id = "duplicate-overlap".to_string();
    let error = compare_artifact_dumps_for_scope(
        &dump(&[]),
        &dump(&[row]),
        &[references.clone(), duplicate],
        "symbol-kind-output-change",
        "typescript",
        "references",
        "fresh_scan",
    )
    .expect_err("overlapping scopes are ambiguous");
    assert!(error.contains("overlapping allowlist entries"));

    let mut full = references.clone();
    full.id = "full-row".to_string();
    full.level = Some("full".to_string());
    let error = validate_allowlist_scope_usage(
        &[references, full],
        &["full-row".to_string()],
        "typescript",
        "references",
        "fresh_scan",
    )
    .expect_err("the references scope cannot borrow use from full");
    assert!(error.contains("references/fresh_scan scope"));
}

#[test]
fn each_incremental_state_must_match_its_own_fresh_scan() {
    let updated = "text:src/a.ts\ttext:typescript\ttext:updated";
    let restored = "text:src/a.ts\ttext:typescript\ttext:restored";
    let states = [
        ("updated", dump(&[updated]), dump(&[])),
        ("restored", dump(&[restored]), dump(&[restored])),
    ];
    let failures = states
        .iter()
        .filter_map(|(state, operation, fresh)| {
            validate_operation_fresh_equivalence(operation, fresh, state).err()
        })
        .collect::<Vec<_>>();
    assert_eq!(failures.len(), 1);
    assert!(failures[0].contains("updated"));
    assert!(
        validate_operation_fresh_equivalence(&dump(&[restored]), &dump(&[restored]), "restored")
            .is_ok()
    );
}

#[test]
fn normalized_metrics_report_per_operation_file_and_row_denominators() {
    let normalized = normalized_metrics(12_000_000_000, 4, 8, 20, 6_000);
    assert_eq!(normalized.operation_count, 4);
    assert_eq!(normalized.elapsed_nanos_per_operation, Some(3_000_000_000));
    assert_eq!(normalized.file_count, 8);
    assert_eq!(normalized.elapsed_nanos_per_file, Some(1_500_000_000));
    assert_eq!(normalized.semantic_row_total, 20);
    assert_eq!(normalized.bytes_per_row_numerator, 6_000);
    assert_eq!(normalized.bytes_per_row_denominator, 20);
    assert_eq!(
        normalized_metrics(100, 0, 0, 0, 0).elapsed_nanos_per_operation,
        None
    );
    assert_eq!(
        normalized_metrics(100, 0, 0, 0, 0).elapsed_nanos_per_file,
        None
    );
}

#[test]
fn budget_approval_passes_only_for_pinned_exact_rows_and_raised_limits() {
    let approval = budget_approval();
    let file = BudgetApprovalFile {
        format_version: 1,
        approvals: vec![approval.clone()],
    };
    validate_budget_approval_file(&file).expect("approval file is exact and complete");
    let baseline_hash = "1".repeat(64);
    let candidate_hash = "2".repeat(64);
    let manifest_hash = "3".repeat(64);
    let evaluation = evaluate_budgets(
        &SampleMetrics {
            elapsed_ms: 1_000,
            peak_rss_bytes: 1_000,
            artifact_bytes: 1_000,
        },
        &SampleMetrics {
            elapsed_ms: 1_201,
            peak_rss_bytes: 1_000,
            artifact_bytes: 1_000,
        },
        &[added_row_delta(&"a".repeat(64))],
        &[approval],
        budget_context(&baseline_hash, &candidate_hash, &manifest_hash),
    );
    assert!(evaluation.violations.is_empty());
    assert!(evaluation.failures.is_empty());
    assert_eq!(
        evaluation.used_approval_ids,
        ["typescript-full-fresh-elapsed"]
    );
}

#[test]
fn budget_approval_rejects_a_different_source_state_row_set() {
    let baseline_hash = "1".repeat(64);
    let candidate_hash = "2".repeat(64);
    let manifest_hash = "3".repeat(64);
    let evaluation = evaluate_budgets(
        &SampleMetrics {
            elapsed_ms: 1_000,
            peak_rss_bytes: 1_000,
            artifact_bytes: 1_000,
        },
        &SampleMetrics {
            elapsed_ms: 1_201,
            peak_rss_bytes: 1_000,
            artifact_bytes: 1_000,
        },
        &[added_row_delta(&"b".repeat(64))],
        &[budget_approval()],
        budget_context(&baseline_hash, &candidate_hash, &manifest_hash),
    );
    assert_eq!(evaluation.violations.len(), 1);
    assert!(
        evaluation
            .failures
            .iter()
            .any(|failure| { failure.contains("does not match the exact added-row set") })
    );
    assert!(evaluation.used_approval_ids.is_empty());
}

#[test]
fn unused_budget_approval_fails_when_default_budgets_already_pass() {
    let baseline_hash = "1".repeat(64);
    let candidate_hash = "2".repeat(64);
    let manifest_hash = "3".repeat(64);
    let evaluation = evaluate_budgets(
        &SampleMetrics {
            elapsed_ms: 1_000,
            peak_rss_bytes: 1_000,
            artifact_bytes: 1_000,
        },
        &SampleMetrics {
            elapsed_ms: 1_050,
            peak_rss_bytes: 1_000,
            artifact_bytes: 1_000,
        },
        &[added_row_delta(&"a".repeat(64))],
        &[budget_approval()],
        budget_context(&baseline_hash, &candidate_hash, &manifest_hash),
    );
    assert!(evaluation.violations.is_empty());
    assert!(evaluation.used_approval_ids.is_empty());
    assert!(
        evaluation
            .failures
            .iter()
            .any(|failure| { failure.contains("unused budget approval") })
    );
}

#[test]
fn budget_approval_must_match_selected_scope_and_pinned_inputs() {
    let approval = budget_approval();
    let baseline_hash = "1".repeat(64);
    let candidate_hash = "2".repeat(64);
    let manifest_hash = "3".repeat(64);
    assert!(
        validate_budget_approval_context(
            std::slice::from_ref(&approval),
            budget_context(&baseline_hash, &candidate_hash, &manifest_hash),
        )
        .is_ok()
    );
    let wrong_source = BudgetApprovalContext {
        language: "all",
        ..budget_context(&baseline_hash, &candidate_hash, &manifest_hash)
    };
    let error = validate_budget_approval_context(std::slice::from_ref(&approval), wrong_source)
        .expect_err("a per-language approval cannot cover an aggregate run");
    assert!(error.contains("outside the selected"));
    let wrong_candidate_hash = "4".repeat(64);
    let wrong_binary = BudgetApprovalContext {
        candidate_binary_sha256: &wrong_candidate_hash,
        ..budget_context(&baseline_hash, &candidate_hash, &manifest_hash)
    };
    let error = validate_budget_approval_context(std::slice::from_ref(&approval), wrong_binary)
        .expect_err("approval pins cannot transfer to another candidate binary");
    assert!(error.contains("pinned to different binaries"));
}
