use std::path::PathBuf;
use xtask::test_tiers::{CommandSpec, plan_from_args, tier_names};

#[test]
fn test_default_tier_runs_plain_extractor_tests() {
    let plan = plan_from_args(["test", "default"]).expect("default plan");

    assert_eq!(
        plan.commands,
        vec![CommandSpec::new(
            "cargo",
            ["test", "-p", "julie-extractors",]
        )]
    );
}

#[test]
fn test_language_tier_filters_one_language_module() {
    let plan = plan_from_args(["test", "language", "rust"]).expect("language plan");

    assert_eq!(
        plan.commands,
        vec![CommandSpec::new(
            "cargo",
            ["test", "-p", "julie-extractors", "--lib", "tests::rust::",]
        )]
    );
}

#[test]
fn test_contract_tier_runs_golden_and_capability_gates_with_features() {
    let plan = plan_from_args(["test", "contract"]).expect("contract plan");

    assert_eq!(
        plan.commands,
        vec![
            CommandSpec::new(
                "cargo",
                [
                    "test",
                    "-p",
                    "julie-extractors",
                    "--features",
                    "test-golden",
                    "--lib",
                    "golden",
                ]
            ),
            CommandSpec::new(
                "cargo",
                [
                    "test",
                    "-p",
                    "julie-extractors",
                    "--features",
                    "test-capability-matrix",
                    "--lib",
                    "capability_matrix",
                ]
            ),
            CommandSpec::new(
                "cargo",
                [
                    "test",
                    "-p",
                    "julie-extractors",
                    "--features",
                    "test-downstream-smoke",
                    "--test",
                    "downstream_smoke",
                    "julie_extractors_works_as_path_dependency_in_downstream_crate",
                ]
            ),
        ]
    );
}

#[test]
fn test_certification_tier_selects_parser_upgrade_feature() {
    let plan = plan_from_args(["test", "certification"]).expect("certification plan");

    assert_eq!(
        plan.commands,
        vec![CommandSpec::new(
            "cargo",
            [
                "test",
                "-p",
                "julie-extractors",
                "--features",
                "test-certification",
                "--lib",
                "parser_upgrade",
            ]
        )]
    );
}

#[test]
fn test_real_world_tier_selects_every_real_fixture_gate() {
    let plan = plan_from_args(["test", "real-world"]).expect("real-world plan");

    assert_eq!(
        plan.commands,
        vec![
            CommandSpec::new(
                "cargo",
                [
                    "test",
                    "-p",
                    "julie-extractors",
                    "--features",
                    "test-real-world",
                    "--lib",
                    "tests::qml::real_world::",
                ]
            ),
            CommandSpec::new(
                "cargo",
                [
                    "test",
                    "-p",
                    "julie-extractors",
                    "--features",
                    "test-real-world",
                    "--lib",
                    "tests::r::real_world::",
                ]
            ),
            CommandSpec::new(
                "cargo",
                [
                    "test",
                    "-p",
                    "julie-extractors",
                    "--features",
                    "test-real-world",
                    "--lib",
                    "tests::r::file_integration_bug::",
                ]
            ),
            CommandSpec::new(
                "cargo",
                [
                    "test",
                    "-p",
                    "julie-extractors",
                    "--features",
                    "test-real-world",
                    "--lib",
                    "test_real_world_jsonl_memories_fixture",
                ]
            ),
        ]
    );
}

#[test]
fn test_tier_names_are_stable_for_docs_and_help() {
    assert_eq!(
        tier_names(),
        [
            "default",
            "language <name>",
            "golden",
            "capability",
            "contract",
            "certification",
            "real-world",
        ]
    );
}

#[test]
fn test_parser_rejects_extra_tier_arguments() {
    let error = plan_from_args(["test", "default", "unexpected"]).expect_err("extra arg");

    assert!(
        error.message().contains("unexpected argument `unexpected`"),
        "unexpected error: {error}"
    );
}

#[test]
fn test_language_tier_requires_language_name() {
    let error = plan_from_args(["test", "language"]).expect_err("missing language");

    assert!(
        error.message().contains("missing language name"),
        "unexpected error: {error}"
    );
}

#[test]
fn test_cargo_xtask_alias_invokes_xtask_package() {
    let config_path = repo_root().join(".cargo/config.toml");
    let config = std::fs::read_to_string(&config_path)
        .unwrap_or_else(|err| panic!("failed to read {}: {err}", config_path.display()));

    assert!(
        config.contains("[alias]"),
        "{} must define Cargo aliases",
        config_path.display()
    );
    assert!(
        config.contains("xtask = \"run -p xtask --\""),
        "`cargo xtask` must dispatch to `cargo run -p xtask --`"
    );
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask crate should live under repo root")
        .to_path_buf()
}
