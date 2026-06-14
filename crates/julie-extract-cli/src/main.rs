mod args;
mod artifact_access;
mod capability_snapshot;
mod commands;
mod discovery;
mod extraction;
mod paths;
mod reports;

fn main() -> std::process::ExitCode {
    commands::run_from_env()
}
