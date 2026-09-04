//! Library surface for `julie-extract-cli`.
//!
//! The CLI binary (`src/main.rs`) is a thin shim over this library target, and a
//! handful of internals are also reachable through this surface so that
//! **integration tests in `tests/`** can exercise them directly.
//!
//! The [`store`] module owns the production family-store parser, request and
//! maintenance executors, and their separate report contracts.

pub mod limits;
pub mod store;

mod args;
mod artifact_access;
mod capability_snapshot;
mod commands;
mod discovery;
mod extraction;
mod paths;
mod progress;
mod reports;
mod spool;
mod watchdog;

pub fn run_from_env() -> std::process::ExitCode {
    commands::run_from_env()
}
