use std::process::{Command, ExitCode};

const CAPABILITIES_JSON: &str = include_str!("../../fixtures/extraction/capabilities.json");

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TestPlan {
    pub commands: Vec<CommandSpec>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandSpec {
    pub program: String,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
}

impl CommandSpec {
    pub fn new<P, I, A>(program: P, args: I) -> Self
    where
        P: Into<String>,
        I: IntoIterator<Item = A>,
        A: Into<String>,
    {
        Self {
            program: program.into(),
            args: args.into_iter().map(Into::into).collect(),
            env: Vec::new(),
        }
    }

    pub fn with_env<I, K, V>(mut self, env: I) -> Self
    where
        I: IntoIterator<Item = (K, V)>,
        K: Into<String>,
        V: Into<String>,
    {
        self.env = env
            .into_iter()
            .map(|(key, value)| (key.into(), value.into()))
            .collect();
        self.env.sort_unstable_by(|left, right| left.cmp(right));
        self
    }

    pub fn display(&self) -> String {
        let mut parts = self
            .env
            .iter()
            .map(|(key, value)| format!("{}={}", shell_quote(key), shell_quote(value)))
            .collect::<Vec<_>>();
        parts.push(self.program.clone());
        parts.extend(self.args.iter().cloned());
        let command_start = self.env.len();
        parts
            .iter()
            .enumerate()
            .map(|(index, part)| {
                if index < command_start {
                    part.clone()
                } else {
                    shell_quote(part)
                }
            })
            .collect::<Vec<_>>()
            .join(" ")
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CliError {
    message: String,
}

impl CliError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    pub fn message(&self) -> &str {
        &self.message
    }
}

impl std::fmt::Display for CliError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for CliError {}

pub fn tier_names() -> [&'static str; 10] {
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
}

pub fn plan_from_args<I, S>(args: I) -> Result<TestPlan, CliError>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let args = args
        .into_iter()
        .map(|arg| arg.as_ref().to_string())
        .collect::<Vec<_>>();

    let Some(command) = args.first() else {
        return Err(CliError::new(help_text()));
    };
    if command != "test" {
        return Err(CliError::new(format!(
            "unsupported xtask command `{command}`\n\n{}",
            help_text()
        )));
    }

    let Some(tier) = args.get(1) else {
        return Err(CliError::new(help_text()));
    };

    match tier.as_str() {
        "default" => expect_no_extra_args(&args, 1).map(|()| default_plan()),
        "language" => language_plan(&args),
        "golden" => expect_no_extra_args(&args, 1).map(|()| golden_plan()),
        "capability" => expect_no_extra_args(&args, 1).map(|()| capability_plan()),
        "contract" => expect_no_extra_args(&args, 1).map(|()| contract_plan()),
        "certification" => expect_no_extra_args(&args, 1).map(|()| certification_plan()),
        "changed" => changed_plan(&args),
        "real-world-smoke" => expect_no_extra_args(&args, 1).map(|()| real_world_smoke_plan()),
        "real-world" | "real-world-release" => {
            expect_no_extra_args(&args, 1).map(|()| real_world_release_plan())
        }
        "list" => expect_no_extra_args(&args, 1).map(|()| TestPlan {
            commands: Vec::new(),
        }),
        other => Err(CliError::new(format!(
            "unsupported test tier `{other}`\n\n{}",
            help_text()
        ))),
    }
}

pub fn run_plan(plan: TestPlan) -> ExitCode {
    for command in plan.commands {
        println!("+ {}", command.display());
        let mut process = Command::new(&command.program);
        process.args(&command.args);
        for (key, value) in &command.env {
            process.env(key, value);
        }
        let status = process.status();
        match status {
            Ok(status) if status.success() => {}
            Ok(status) => return ExitCode::from(status.code().unwrap_or(1) as u8),
            Err(err) => {
                eprintln!("failed to run `{}`: {err}", command.display());
                return ExitCode::from(1);
            }
        }
    }

    ExitCode::SUCCESS
}

fn default_plan() -> TestPlan {
    TestPlan {
        commands: vec![
            CommandSpec::new("cargo", ["test", "-p", "julie-extractors"]),
            CommandSpec::new("cargo", ["test", "-p", "julie-extract-artifact"]),
            CommandSpec::new("cargo", ["test", "-p", "julie-extract-cli"]),
        ],
    }
}

fn language_plan(args: &[String]) -> Result<TestPlan, CliError> {
    let Some(language) = args.get(2) else {
        return Err(CliError::new(
            "missing language name for `cargo xtask test language <name>`",
        ));
    };
    expect_no_extra_args(args, 2)?;
    if !language
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-')
    {
        return Err(CliError::new(format!(
            "invalid language `{language}`; expected letters, numbers, `_`, or `-`"
        )));
    }
    let Some(test_filter) = language_test_filter(language)? else {
        return Err(CliError::new(format!(
            "unsupported language `{language}`; supported languages: {}",
            supported_languages()?.join(", ")
        )));
    };

    Ok(TestPlan {
        commands: vec![
            CommandSpec::new(
                "cargo",
                ["test", "-p", "julie-extractors", "--lib", &test_filter],
            ),
            CommandSpec::new(
                "cargo",
                [
                    "test",
                    "-p",
                    "julie-extractors",
                    "--features",
                    "test-golden",
                    "--lib",
                    "golden_fixtures_match_canonical_extraction",
                ],
            )
            .with_env([("JULIE_GOLDEN_LANGUAGE", language)]),
        ],
    })
}

fn language_test_filter(language: &str) -> Result<Option<String>, CliError> {
    if !supported_languages()?
        .iter()
        .any(|supported| supported == language)
    {
        return Ok(None);
    }

    Ok(Some(match language {
        "tsx" => "tests::typescript::tsx".to_string(),
        "jsx" => "tests::javascript::jsx".to_string(),
        other => format!("tests::{}::", other.replace('-', "_")),
    }))
}

fn supported_languages() -> Result<Vec<String>, CliError> {
    let snapshot: serde_json::Value = serde_json::from_str(CAPABILITIES_JSON).map_err(|err| {
        CliError::new(format!("failed to parse embedded capabilities.json: {err}"))
    })?;
    let languages = snapshot
        .get("languages")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| CliError::new("embedded capabilities.json is missing languages array"))?
        .iter()
        .map(|row| {
            row.get("language")
                .and_then(serde_json::Value::as_str)
                .map(str::to_string)
                .ok_or_else(|| {
                    CliError::new("embedded capabilities.json has a row missing language")
                })
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(languages)
}

fn golden_plan() -> TestPlan {
    TestPlan {
        commands: vec![CommandSpec::new(
            "cargo",
            [
                "test",
                "-p",
                "julie-extractors",
                "--features",
                "test-golden",
                "--lib",
                "golden",
            ],
        )],
    }
}

fn capability_plan() -> TestPlan {
    TestPlan {
        commands: vec![
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
                ],
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
                ],
            ),
        ],
    }
}

fn contract_plan() -> TestPlan {
    let mut commands = golden_plan().commands;
    commands.extend(capability_plan().commands);
    commands.push(CommandSpec::new(
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
        ],
    ));
    for harness in [
        "store_maintenance_contract",
        "store_maintenance_property",
        "store_generation_equivalence",
    ] {
        commands.push(CommandSpec::new(
            "cargo",
            [
                "test",
                "-p",
                "julie-extract-artifact",
                "--test",
                harness,
                "--",
                "--test-threads=1",
            ],
        ));
    }
    for harness in [
        "store_maintenance_crash_contract",
        "store_generation_crash_contract",
    ] {
        commands.push(CommandSpec::new(
            "cargo",
            [
                "test",
                "-p",
                "julie-extract-artifact",
                "--features",
                "test-store-maintenance-contract",
                "--test",
                harness,
                "--",
                "--test-threads=1",
            ],
        ));
    }
    commands.push(CommandSpec::new(
        "cargo",
        [
            "test",
            "-p",
            "julie-extract-artifact",
            "--test",
            "schema_contract",
        ],
    ));
    commands.push(CommandSpec::new(
        "cargo",
        [
            "test",
            "-p",
            "julie-extract-artifact",
            "--test",
            "report_contract",
        ],
    ));
    commands.push(CommandSpec::new(
        "cargo",
        [
            "test",
            "-p",
            "julie-extract-artifact",
            "--test",
            "jsonl_contract",
        ],
    ));
    commands.push(CommandSpec::new(
        "cargo",
        ["test", "-p", "julie-extract-cli", "--test", "cli_contract"],
    ));
    commands.push(CommandSpec::new(
        "cargo",
        ["test", "-p", "julie-extract-cli", "--test", "path_policy"],
    ));
    commands.push(CommandSpec::new(
        "cargo",
        [
            "test",
            "-p",
            "julie-extract-cli",
            "--test",
            "operations_contract",
        ],
    ));
    commands.push(CommandSpec::new(
        "cargo",
        [
            "test",
            "-p",
            "julie-extract-cli",
            "--test",
            "determinism_contract",
        ],
    ));
    commands.push(CommandSpec::new(
        "cargo",
        [
            "test",
            "-p",
            "julie-extract-cli",
            "--features",
            "test-heavy-contracts",
            "--test",
            "deep_recursion_contract",
        ],
    ));
    commands.push(CommandSpec::new(
        "cargo",
        [
            "test",
            "-p",
            "julie-extract-cli",
            "--features",
            "test-heavy-contracts",
            "--test",
            "reference_site_identity",
        ],
    ));
    commands.push(CommandSpec::new(
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
        ],
    ));
    for harness in [
        "store_equivalence",
        "store_mixed_version",
        "store_import_contract",
        "store_operations_contract",
    ] {
        commands.push(CommandSpec::new(
            "cargo",
            [
                "test",
                "-p",
                "julie-extract-cli",
                "--features",
                "test-store-contract",
                "--test",
                harness,
                "--",
                "--test-threads=1",
            ],
        ));
    }
    for harness in [
        "store_maintenance_equivalence",
        "store_maintenance_mixed_version",
        "store_maintenance_performance",
    ] {
        commands.push(CommandSpec::new(
            "cargo",
            [
                "test",
                "-p",
                "julie-extract-cli",
                "--features",
                "test-store-maintenance-contract",
                "--test",
                harness,
                "--",
                "--test-threads=1",
            ],
        ));
    }
    TestPlan { commands }
}

fn certification_plan() -> TestPlan {
    let mut commands = capability_plan().commands;
    commands.push(CommandSpec::new(
        "cargo",
        [
            "test",
            "-p",
            "julie-extractors",
            "--features",
            "test-certification",
            "--lib",
            "parser_upgrade",
        ],
    ));
    TestPlan { commands }
}

fn changed_plan(args: &[String]) -> Result<TestPlan, CliError> {
    if args.len() < 3 {
        return Err(CliError::new(
            "missing changed path for `cargo xtask test changed <path>...`",
        ));
    }

    let changed_paths = args
        .iter()
        .skip(2)
        .map(|path| normalize_changed_path(path))
        .collect::<Vec<_>>();
    if changed_paths
        .iter()
        .any(|path| is_golden_expected_output_path(path))
        && !changed_paths
            .iter()
            .any(|path| path == "crates/julie-extractors/src/lib.rs")
    {
        return Err(CliError::new(
            "golden expected output changed without extractor contract review; include \
             crates/julie-extractors/src/lib.rs in the changed path set and update \
             EXTRACTION_CONTRACT_VERSION if downstream-visible extraction output changed",
        ));
    }

    let mut commands = default_plan().commands;
    if changed_paths.iter().any(|path| is_xtask_path(path)) {
        commands.push(CommandSpec::new("cargo", ["test", "-p", "xtask"]));
    }
    if changed_paths
        .iter()
        .any(|path| is_parser_dependency_path(path))
    {
        commands.extend(certification_plan().commands);
    }
    Ok(TestPlan { commands })
}

fn is_golden_expected_output_path(path: &str) -> bool {
    path.starts_with("fixtures/extraction/") && path.ends_with("/expected.json")
}

fn normalize_changed_path(path: &str) -> String {
    path.replace('\\', "/")
        .trim_start_matches("./")
        .to_ascii_lowercase()
}

fn is_xtask_path(path: &str) -> bool {
    path == "xtask/cargo.toml" || path.starts_with("xtask/src/") || path.starts_with("xtask/tests/")
}

fn is_parser_dependency_path(path: &str) -> bool {
    path == "cargo.lock"
        || path == "crates/julie-extractors/cargo.toml"
        || path == "fixtures/extraction/capabilities.json"
        || path.starts_with("fixtures/extraction/")
        || path.starts_with("crates/julie-extractors/src/language_spec/")
        || path.starts_with("crates/julie-extractors/src/registry")
        || path.starts_with("crates/julie-extractors/src/tests/capability_matrix")
        || path.starts_with("crates/julie-extractors/src/tests/pending_shape_contract")
}

fn real_world_smoke_plan() -> TestPlan {
    TestPlan {
        commands: vec![CommandSpec::new(
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
        )],
    }
}

fn real_world_release_plan() -> TestPlan {
    TestPlan {
        commands: vec![
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
                ],
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
                ],
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
                ],
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
                ],
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
                ],
            ),
        ],
    }
}

fn expect_no_extra_args(args: &[String], last_index: usize) -> Result<(), CliError> {
    if args.len() > last_index + 1 {
        Err(CliError::new(format!(
            "unexpected argument `{}`\n\n{}",
            args[last_index + 1],
            help_text()
        )))
    } else {
        Ok(())
    }
}

fn help_text() -> String {
    format!(
        "usage: cargo xtask test <tier>\n\navailable tiers:\n{}",
        tier_names()
            .iter()
            .map(|tier| format!("  - {tier}"))
            .collect::<Vec<_>>()
            .join("\n")
    )
}

fn shell_quote(value: &str) -> String {
    if value
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | ':' | '/' | '.'))
    {
        value.to_string()
    } else {
        format!("'{}'", value.replace('\'', "'\\''"))
    }
}
