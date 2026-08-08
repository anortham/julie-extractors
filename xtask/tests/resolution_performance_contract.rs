use std::path::PathBuf;
use std::process::Command;

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
        time_to_exact_ms: 1_710,
        integrity_ms: 200,
        identifier_rows: 50_000,
        pending_rows: 1_000,
        peak_rss_bytes: 10,
        base_bytes: 20,
        delta_bytes: 30,
        semantic_differences: 0,
        applied_differences: 0,
        exact_gap_mismatches: 0,
        foreground_bind_ms: 1,
        foreground_identifier_work: 0,
        background_pipeline_ms: REFUTED_BIND_CONTROL_MS - 1,
    }
}

fn complete_samples() -> Vec<ResolutionPerformanceSample> {
    (1..=3)
        .flat_map(|run| FIXED_PAIRS.map(move |pair| passing_sample(pair, run)))
        .collect()
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
    samples[0].identifier_rows = 49_999;
    samples[1].identifier_rows = 500_000;

    let summary = evaluate_samples(3, samples).unwrap();
    let failed = summary
        .samples
        .iter()
        .find(|sample| sample.sample.run == 1 && sample.sample.pair == FIXED_PAIRS[0])
        .unwrap();
    assert!(!summary.passed);
    assert!(!failed.g3a_passed);
    assert!(summary.samples.iter().any(|sample| sample.g3a_passed));
}

#[test]
fn summary_rejects_vacuous_resolution_tables() {
    let mut samples = complete_samples();
    samples[0].pending_rows = 0;

    assert!(evaluate_samples(3, samples).is_err());
}

#[test]
fn thresholds_are_inclusive_and_every_gate_is_reported_per_sample() {
    let mut samples = complete_samples();
    let sample = &mut samples[0];
    sample.identifier_rows = MIN_IDENTIFIER_ROWS_PER_SECOND as u64;
    sample.diff_ms = 250;
    sample.delta_write_ms = 250;
    sample.resolution_compute_ms = 1_000;
    sample.time_to_exact_ms = MAX_TIME_TO_EXACT_MS;
    sample.background_pipeline_ms = REFUTED_BIND_CONTROL_MS - 1;

    let summary = evaluate_samples(3, samples).unwrap();
    let verdict = &summary.samples[0];
    assert_eq!(
        verdict.identifier_rows_per_second,
        MIN_IDENTIFIER_ROWS_PER_SECOND
    );
    assert_eq!(verdict.diff_overhead_ratio, MAX_DIFF_OVERHEAD_RATIO);
    assert!(verdict.g1_passed);
    assert!(verdict.g2_passed);
    assert!(verdict.g3a_passed);
    assert!(verdict.g3b_passed);
    assert!(verdict.g3c_passed);
    assert!(verdict.g4_passed);
    assert!(verdict.g5_passed);
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
