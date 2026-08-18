mod args;
mod artifact_access;
mod capability_snapshot;
mod commands;
mod discovery;
mod extraction;
mod limits;
mod paths;
mod progress;
mod reports;
mod spool;
mod watchdog;

fn main() -> std::process::ExitCode {
    commands::run_from_env()
}
