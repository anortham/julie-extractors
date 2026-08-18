//! Library surface for `julie-extract-cli`.
//!
//! The CLI is primarily a binary (`src/main.rs`), but a handful of internals are
//! also reachable through this thin library target so that **integration tests in
//! `tests/`** (which can only see a crate's public library API, never a binary's
//! private modules) can exercise them directly.
//!
//! The [`store`] module owns the production family-store parser, request and
//! maintenance executors, and their separate report contracts.

pub mod limits;
pub mod store;

#[allow(dead_code)]
mod artifact_access;
#[allow(dead_code)]
mod capability_snapshot;
#[allow(dead_code)]
mod discovery;
#[allow(dead_code)]
mod extraction;
#[allow(dead_code)]
mod paths;
#[allow(dead_code)]
mod progress;
#[allow(dead_code)]
mod reports;
#[allow(dead_code)]
mod spool;
#[allow(dead_code)]
mod watchdog;
