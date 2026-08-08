use std::path::PathBuf;
use std::time::Duration;
use xtask::test_tiers::{CommandSpec, plan_from_args, tier_names};

#[test]
fn test_default_tier_runs_plain_extractor_tests() {
    let plan = plan_from_args(["test", "default"]).expect("default plan");

    assert_eq!(plan.max_duration, Some(Duration::from_secs(90)));
    assert_eq!(
        plan.commands,
        vec![
            CommandSpec::new("cargo", ["test", "-p", "julie-extractors",]),
            CommandSpec::new("cargo", ["test", "-p", "julie-extract-artifact",]),
            CommandSpec::new("cargo", ["test", "-p", "julie-extract-cli",])
        ]
    );
    assert!(
        !plan
            .commands
            .iter()
            .flat_map(|command| command.args.iter())
            .any(|arg| arg == "test-certification"
                || arg == "test-capability-matrix"
                || arg == "test-real-world"),
        "default tier must not include certification, capability-matrix, or real-world gates"
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
fn test_language_tier_rejects_unknown_languages_and_maps_variants() {
    let unknown = plan_from_args(["test", "language", "does_not_exist"])
        .expect_err("unknown language must not run a zero-test cargo filter");
    assert!(
        unknown
            .message()
            .contains("unsupported language `does_not_exist`"),
        "unexpected error: {unknown}"
    );

    let tsx = plan_from_args(["test", "language", "tsx"]).expect("tsx language plan");
    assert_eq!(
        tsx.commands,
        vec![CommandSpec::new(
            "cargo",
            [
                "test",
                "-p",
                "julie-extractors",
                "--lib",
                "tests::typescript::tsx",
            ]
        )]
    );

    let jsx = plan_from_args(["test", "language", "jsx"]).expect("jsx language plan");
    assert_eq!(
        jsx.commands,
        vec![CommandSpec::new(
            "cargo",
            [
                "test",
                "-p",
                "julie-extractors",
                "--lib",
                "tests::javascript::jsx",
            ]
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
                    "test-capability-matrix",
                    "--lib",
                    "pending_shape_contract",
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
            CommandSpec::new(
                "cargo",
                [
                    "test",
                    "-p",
                    "julie-extract-artifact",
                    "--test",
                    "schema_contract",
                ]
            ),
            CommandSpec::new(
                "cargo",
                [
                    "test",
                    "-p",
                    "julie-extract-artifact",
                    "--test",
                    "report_contract",
                ]
            ),
            CommandSpec::new(
                "cargo",
                [
                    "test",
                    "-p",
                    "julie-extract-artifact",
                    "--test",
                    "jsonl_contract",
                ]
            ),
            CommandSpec::new(
                "cargo",
                ["test", "-p", "julie-extract-cli", "--test", "cli_contract",]
            ),
            CommandSpec::new(
                "cargo",
                ["test", "-p", "julie-extract-cli", "--test", "path_policy",]
            ),
            CommandSpec::new(
                "cargo",
                [
                    "test",
                    "-p",
                    "julie-extract-cli",
                    "--test",
                    "operations_contract",
                ]
            ),
            CommandSpec::new(
                "cargo",
                [
                    "test",
                    "-p",
                    "julie-extract-cli",
                    "--test",
                    "determinism_contract",
                ]
            ),
            CommandSpec::new(
                "cargo",
                [
                    "test",
                    "-p",
                    "julie-extract-cli",
                    "--features",
                    "test-heavy-contracts",
                    "--test",
                    "deep_recursion_contract",
                ]
            ),
            CommandSpec::new(
                "cargo",
                [
                    "test",
                    "-p",
                    "julie-extract-cli",
                    "--features",
                    "test-heavy-contracts",
                    "--test",
                    "reference_site_identity",
                ]
            ),
            CommandSpec::new(
                "cargo",
                [
                    "test",
                    "-p",
                    "julie-extract-artifact",
                    "--features",
                    "test-store-crash",
                    "--test",
                    "store_crash_contract",
                    "--",
                    "--test-threads=1",
                ]
            ),
            CommandSpec::new(
                "cargo",
                [
                    "test",
                    "-p",
                    "julie-extract-cli",
                    "--features",
                    "test-store-contract",
                    "--test",
                    "store_equivalence",
                    "--",
                    "--test-threads=1",
                ]
            ),
            CommandSpec::new(
                "cargo",
                [
                    "test",
                    "-p",
                    "julie-extract-cli",
                    "--features",
                    "test-store-contract",
                    "--test",
                    "store_mixed_version",
                    "--",
                    "--test-threads=1",
                ]
            ),
            CommandSpec::new(
                "cargo",
                [
                    "test",
                    "-p",
                    "julie-extract-cli",
                    "--features",
                    "test-store-contract",
                    "--test",
                    "store_import_contract",
                    "--",
                    "--test-threads=1",
                ]
            ),
            CommandSpec::new(
                "cargo",
                [
                    "test",
                    "-p",
                    "julie-extract-cli",
                    "--features",
                    "test-store-contract",
                    "--test",
                    "store_operations_contract",
                    "--",
                    "--test-threads=1",
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
        vec![
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
                    "test-capability-matrix",
                    "--lib",
                    "pending_shape_contract",
                ]
            ),
            CommandSpec::new(
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
            ),
        ]
    );
}

#[test]
fn test_real_world_tier_selects_every_real_fixture_gate() {
    let plan = plan_from_args(["test", "real-world"]).expect("real-world plan");

    assert_eq!(
        plan,
        plan_from_args(["test", "real-world-release"]).expect("real-world release plan")
    );

    assert!(
        plan.commands.contains(&CommandSpec::new(
            "cargo",
            [
                "test",
                "-p",
                "julie-extract-cli",
                "--features",
                "test-real-world",
                "--test",
                "erlang_corpus",
            ]
        )),
        "real-world tier must run the vendored hex.pm Erlang corpus gate"
    );
}

#[test]
fn test_real_world_smoke_and_release_profiles_are_separate() {
    let smoke = plan_from_args(["test", "real-world-smoke"]).expect("real-world smoke plan");
    let release = plan_from_args(["test", "real-world-release"]).expect("real-world release plan");

    assert_ne!(smoke.commands, release.commands);
    assert!(
        smoke.commands.len() < release.commands.len(),
        "smoke profile should be narrower than release profile"
    );
    assert!(
        !plan_from_args(["test", "default"])
            .unwrap()
            .commands
            .iter()
            .flat_map(|command| command.args.iter())
            .any(|arg| arg == "test-real-world"),
        "default tier must not include real-world gates"
    );

    assert_eq!(
        smoke.commands,
        vec![CommandSpec::new(
            "cargo",
            [
                "test",
                "-p",
                "julie-extractors",
                "--features",
                "test-real-world",
                "--lib",
                "test_real_world_jsonl_memories_fixture",
            ],
        )]
    );

    assert_eq!(
        release.commands,
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
            CommandSpec::new(
                "cargo",
                [
                    "test",
                    "-p",
                    "julie-extract-cli",
                    "--features",
                    "test-real-world",
                    "--test",
                    "erlang_corpus",
                ]
            ),
        ]
    );
}

#[test]
fn test_changed_parser_dependency_paths_trigger_certification_gate() {
    for path in [
        "Cargo.lock",
        "crates/julie-extractors/Cargo.toml",
        "crates/julie-extractors/src/language_spec/specs.rs",
        "crates/julie-extractors/src/registry.rs",
        "fixtures/extraction/capabilities.json",
        "fixtures/extraction/rust/basic/source.rs",
        "crates/julie-extractors/src/tests/capability_matrix.rs",
    ] {
        let parser_change =
            plan_from_args(["test", "changed", path]).expect("changed parser dependency plan");
        assert!(
            parser_change.commands.contains(&CommandSpec::new(
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
            )),
            "parser dependency change `{path}` must run capability certification"
        );
        assert!(
            parser_change.commands.contains(&CommandSpec::new(
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
            )),
            "parser dependency change `{path}` must run parser certification"
        );
    }

    let expected_output_change = plan_from_args([
        "test",
        "changed",
        "fixtures/extraction/rust/basic/expected.json",
        "crates/julie-extractors/src/lib.rs",
    ])
    .expect("changed golden expected output plan with contract review");
    assert!(
        expected_output_change.commands.contains(&CommandSpec::new(
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
        )),
        "golden expected output changes must run parser certification"
    );

    let cli_change = plan_from_args(["test", "changed", "crates/julie-extract-cli/src/main.rs"])
        .expect("changed non-parser plan");
    assert!(
        !cli_change
            .commands
            .iter()
            .flat_map(|command| command.args.iter())
            .any(|arg| arg == "test-certification"),
        "ordinary CLI changes should not trigger parser certification"
    );
}

#[test]
fn test_changed_expected_golden_output_requires_extractor_contract_review() {
    let error = plan_from_args([
        "test",
        "changed",
        "fixtures/extraction/typescript/basic/expected.json",
    ])
    .expect_err("expected golden output change without contract path should fail");
    assert!(
        error
            .message()
            .contains("golden expected output changed without extractor contract review"),
        "unexpected error: {error}"
    );

    let reviewed_change = plan_from_args([
        "test",
        "changed",
        "fixtures/extraction/typescript/basic/expected.json",
        "crates/julie-extractors/src/lib.rs",
    ])
    .expect("expected golden output change with contract path should plan gates");
    assert!(
        reviewed_change.commands.contains(&CommandSpec::new(
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
        )),
        "expected golden output changes should still run parser certification"
    );
}

#[test]
fn test_changed_xtask_paths_run_xtask_tests() {
    for path in ["xtask/src/test_tiers.rs", "xtask/tests/test_tiers.rs"] {
        let xtask_change =
            plan_from_args(["test", "changed", path]).expect("changed xtask path plan");
        assert!(
            xtask_change
                .commands
                .contains(&CommandSpec::new("cargo", ["test", "-p", "xtask"])),
            "xtask change `{path}` must run xtask tests"
        );
    }
}

#[test]
fn test_changed_tier_combines_multiple_paths_and_requires_at_least_one_path() {
    let mixed_change = plan_from_args([
        "test",
        "changed",
        "docs/testing-strategy.md",
        "crates/julie-extractors/src/language_spec/specs.rs",
    ])
    .expect("mixed changed plan");
    assert!(
        mixed_change
            .commands
            .iter()
            .flat_map(|command| command.args.iter())
            .any(|arg| arg == "test-certification"),
        "any parser dependency path in a changed set must trigger certification"
    );

    let error = plan_from_args(["test", "changed"]).expect_err("missing changed path");
    assert!(
        error.message().contains("missing changed path"),
        "unexpected error: {error}"
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
            "changed <path>...",
            "real-world-smoke",
            "real-world",
            "real-world-release",
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
