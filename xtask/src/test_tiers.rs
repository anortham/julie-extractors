use std::ffi::OsString;
use std::process::{Command, ExitCode};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TestPlan {
    pub commands: Vec<CommandSpec>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandSpec {
    pub program: String,
    pub args: Vec<String>,
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
        }
    }

    pub fn display(&self) -> String {
        std::iter::once(self.program.as_str())
            .chain(self.args.iter().map(String::as_str))
            .map(shell_quote)
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

pub fn tier_names() -> [&'static str; 7] {
    [
        "default",
        "language <name>",
        "golden",
        "capability",
        "contract",
        "certification",
        "real-world",
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
        "real-world" => expect_no_extra_args(&args, 1).map(|()| real_world_plan()),
        "list" => expect_no_extra_args(&args, 1).map(|()| TestPlan {
            commands: Vec::new(),
        }),
        other => Err(CliError::new(format!(
            "unsupported test tier `{other}`\n\n{}",
            help_text()
        ))),
    }
}

pub fn run_from_env_args(args: impl IntoIterator<Item = OsString>) -> ExitCode {
    let args = args
        .into_iter()
        .skip(1)
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect::<Vec<_>>();

    if args == ["test", "list"] {
        for name in tier_names() {
            println!("{name}");
        }
        return ExitCode::SUCCESS;
    }

    match plan_from_args(args) {
        Ok(plan) => run_plan(plan),
        Err(err) => {
            eprintln!("{err}");
            ExitCode::from(2)
        }
    }
}

pub fn run_plan(plan: TestPlan) -> ExitCode {
    for command in plan.commands {
        println!("+ {}", command.display());
        let status = Command::new(&command.program).args(&command.args).status();
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

    Ok(TestPlan {
        commands: vec![CommandSpec::new(
            "cargo",
            [
                "test",
                "-p",
                "julie-extractors",
                "--lib",
                &format!("tests::{}::", language.replace('-', "_")),
            ],
        )],
    })
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
        commands: vec![CommandSpec::new(
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
        )],
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
    TestPlan { commands }
}

fn certification_plan() -> TestPlan {
    TestPlan {
        commands: vec![CommandSpec::new(
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
        )],
    }
}

fn real_world_plan() -> TestPlan {
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
