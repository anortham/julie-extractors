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
mod resolution;
mod spool;
mod watchdog;

fn main() -> std::process::ExitCode {
    let outcome = commands::run_from_env();
    // A shadow-mode mismatch has to surface AFTER the write commits, so it cannot
    // ride the resolution hook's error type: the writer catches that error, rolls
    // the overlay writes back and still exits zero.
    match resolution::shadow_mismatch_exit_code() {
        Some(code) => std::process::ExitCode::from(code),
        None => outcome,
    }
}
