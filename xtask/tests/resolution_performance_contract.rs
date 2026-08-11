use std::path::PathBuf;
use std::process::Command;

use serde_json::{Value, json};
use xtask::resolution_performance::{
    FIXED_PAIRS, MAX_DIFF_OVERHEAD_RATIO, MAX_TIME_TO_EXACT_MS, MIN_IDENTIFIER_ROWS_PER_SECOND,
    REFUTED_BIND_CONTROL_MS, ResolutionPerformanceSample, evaluate_samples, harness_command_args,
    plan_from_args,
};

fn passing_sample(pair: &str, run: usize) -> ResolutionPerformanceSample {
    ResolutionPerformanceSample {
        pair: pair.to_string(),
        run,
        resolution_compute_ms: 1_000,
        store_fresh_ms: 1_200,
        diff_ms: 200,
        delta_write_ms: 300,
        publish_ms: 10,
        time_to_exact_ms: if pair == "miller-unchanged" {
            2_000
        } else {
            1_710
        },
        integrity_ms: 200,
        identifier_rows: 50_000,
        pending_rows: 1_000,
        peak_rss_bytes: 10,
        base_bytes: 20,
        delta_bytes: 22,
        semantic_differences: 0,
        applied_differences: 0,
        foreground_bind_ms: 1,
        foreground_identifier_work: None,
        background_pipeline_ms: REFUTED_BIND_CONTROL_MS - 1,
        resolution_mode: if pair == FIXED_PAIRS[0] {
            "full".to_string()
        } else {
            "scoped".to_string()
        },
        fallback_reason: (pair == FIXED_PAIRS[0])
            .then(|| "incremental_resolution_disabled".to_string()),
        canonical_semantic_digest: "digest-a".to_string(),
        row_level_differences: 0,
        fixture_snapshot_digest: "snapshot-a".to_string(),
        exact_gap_rows: u64::from(pair == FIXED_PAIRS[0]),
        exact_gap_files: u64::from(pair == FIXED_PAIRS[0]),
        cumulative_gap_bytes_before: if pair == FIXED_PAIRS[0] {
            22
        } else {
            64 * 1024 * 1024 + 1
        },
        cumulative_gap_bytes_after: if pair == FIXED_PAIRS[0] { 137_460 } else { 22 },
        cumulative_delta_rows_before: 0,
        cumulative_delta_rows_after: if pair == FIXED_PAIRS[0] { 1_176 } else { 0 },
        rebased: pair == FIXED_PAIRS[1],
    }
}

fn complete_samples() -> Vec<ResolutionPerformanceSample> {
    (1..=3)
        .flat_map(|run| FIXED_PAIRS.map(move |pair| passing_sample(pair, run)))
        .collect()
}

fn mutate_serialized_sample(
    samples: &mut [ResolutionPerformanceSample],
    pair: &str,
    run: usize,
    key: &str,
    value: Value,
) {
    let sample = samples
        .iter_mut()
        .find(|sample| sample.pair == pair && sample.run == run)
        .unwrap();
    let mut serialized = serde_json::to_value(&*sample).unwrap();
    serialized
        .as_object_mut()
        .unwrap()
        .insert(key.to_string(), value);
    *sample = serde_json::from_value(serialized).unwrap();
}

#[test]
fn parser_requires_three_runs_and_defaults_the_output_directory() {
    let plan = plan_from_args(["store-resolution", "--runs", "3"]).unwrap();
    assert_eq!(plan.runs, 3);
    assert_eq!(
        plan.out_dir,
        PathBuf::from("target/performance/store-resolution")
    );

    assert!(plan_from_args(["store-resolution", "--runs", "2"]).is_err());
    assert!(plan_from_args(["store-resolution", "--runs", "three"]).is_err());
    assert!(plan_from_args(["store-resolution", "--runs", "3", "--other"]).is_err());
}

#[test]
fn summary_requires_every_fixed_pair_in_every_run_exactly_once() {
    let mut missing = complete_samples();
    missing.pop();
    assert!(evaluate_samples(3, missing).is_err());

    let mut duplicate = complete_samples();
    duplicate.push(duplicate[0].clone());
    assert!(evaluate_samples(3, duplicate).is_err());
}

#[test]
fn one_failed_sample_is_not_hidden_by_other_runs() {
    let mut samples = complete_samples();
    samples[0].semantic_differences = 1;

    let summary = evaluate_samples(3, samples).unwrap();
    let failed = summary
        .samples
        .iter()
        .find(|sample| sample.sample.run == 1 && sample.sample.pair == FIXED_PAIRS[0])
        .unwrap();
    assert!(!summary.passed);
    assert!(!failed.g1_passed);
    assert!(summary.samples.iter().any(|sample| sample.g1_passed));
}

#[test]
fn caller_row_mismatch_fails_the_summary_even_when_legacy_diff_fields_are_zero() {
    let mut samples = complete_samples();
    mutate_serialized_sample(
        &mut samples,
        "miller-mutated",
        1,
        "row_level_differences",
        json!(1),
    );

    assert!(!evaluate_samples(3, samples).unwrap().passed);
}

#[test]
fn caller_scoped_fallback_fails_the_summary() {
    let mut samples = complete_samples();
    mutate_serialized_sample(
        &mut samples,
        "miller-mutated",
        1,
        "fallback_reason",
        json!("resolution_prior_overlay_unavailable"),
    );

    assert!(!evaluate_samples(3, samples).unwrap().passed);
}

#[test]
fn caller_gap_storage_mismatch_fails_the_summary() {
    let mut samples = complete_samples();
    mutate_serialized_sample(
        &mut samples,
        "miller-mutated",
        1,
        "cumulative_gap_bytes_after",
        json!(64 * 1024 * 1024 + 1),
    );

    assert!(!evaluate_samples(3, samples).unwrap().passed);
}

#[test]
fn caller_fixture_snapshot_mismatch_fails_the_summary() {
    let mut samples = complete_samples();
    mutate_serialized_sample(
        &mut samples,
        "miller-mutated",
        1,
        "fixture_snapshot_digest",
        json!("snapshot-b"),
    );

    assert!(!evaluate_samples(3, samples).unwrap().passed);
}

#[test]
fn summary_rejects_vacuous_resolution_tables() {
    let mut samples = complete_samples();
    samples[0].pending_rows = 0;

    assert!(evaluate_samples(3, samples).is_err());
}

#[test]
fn forced_full_control_is_report_only_while_scoped_must_be_under_budget_and_faster() {
    let mut samples = complete_samples();
    for sample in &mut samples {
        if sample.pair == "miller-unchanged" {
            sample.time_to_exact_ms = MAX_TIME_TO_EXACT_MS + 1;
            sample.background_pipeline_ms = REFUTED_BIND_CONTROL_MS + 1;
        } else {
            sample.time_to_exact_ms = MAX_TIME_TO_EXACT_MS;
        }
    }

    assert!(evaluate_samples(3, samples).unwrap().passed);
}

#[test]
fn scoped_replay_fails_when_it_is_not_faster_than_the_forced_full_control() {
    let mut samples = complete_samples();
    for sample in &mut samples {
        if sample.run == 1 && sample.pair == "miller-mutated" {
            sample.time_to_exact_ms = 2_001;
        }
    }

    let summary = evaluate_samples(3, samples).unwrap();
    assert!(!summary.passed);
    assert!(
        summary
            .samples
            .iter()
            .filter(|sample| sample.sample.run == 1)
            .all(|sample| !sample.g6_passed)
    );
}

#[test]
fn thresholds_are_inclusive_and_every_gate_is_reported_per_sample() {
    let mut samples = complete_samples();
    let sample = samples
        .iter_mut()
        .find(|sample| sample.run == 1 && sample.pair == FIXED_PAIRS[1])
        .unwrap();
    sample.identifier_rows = MIN_IDENTIFIER_ROWS_PER_SECOND as u64;
    sample.diff_ms = 250;
    sample.delta_write_ms = 250;
    sample.resolution_compute_ms = 1_000;
    sample.time_to_exact_ms = MAX_TIME_TO_EXACT_MS;
    sample.background_pipeline_ms = REFUTED_BIND_CONTROL_MS - 1;
    samples
        .iter_mut()
        .find(|sample| sample.run == 1 && sample.pair == FIXED_PAIRS[0])
        .unwrap()
        .time_to_exact_ms = MAX_TIME_TO_EXACT_MS + 1;

    let summary = evaluate_samples(3, samples).unwrap();
    let verdict = summary
        .samples
        .iter()
        .find(|sample| sample.sample.run == 1 && sample.sample.pair == FIXED_PAIRS[1])
        .unwrap();
    assert_eq!(
        verdict.identifier_rows_per_second,
        MIN_IDENTIFIER_ROWS_PER_SECOND
    );
    assert_eq!(verdict.diff_overhead_ratio, MAX_DIFF_OVERHEAD_RATIO);
    assert!(verdict.g1_passed);
    assert!(verdict.g2_passed);
    let serialized = serde_json::to_value(verdict).unwrap();
    assert_eq!(serialized["identifier_throughput_status"], "report_only");
    assert_eq!(serialized["diff_overhead_status"], "report_only");
    assert!(serialized.get("g3a_passed").is_none());
    assert!(serialized.get("g3b_passed").is_none());
    assert_eq!(verdict.g3c_passed, Some(true));
    assert_eq!(verdict.g4_passed, Some(true));
    assert!(verdict.g5_passed);
    assert!(verdict.g6_passed);
}

#[test]
fn command_routes_store_resolution_before_the_legacy_performance_parser() {
    let output = Command::new(env!("CARGO_BIN_EXE_xtask"))
        .args(["performance", "store-resolution", "--runs", "2"])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("at least 3"));
}

#[test]
fn performance_harness_measures_the_release_profile() {
    let args = harness_command_args();
    assert!(args.windows(2).any(|args| args == ["test", "--release"]));
}
