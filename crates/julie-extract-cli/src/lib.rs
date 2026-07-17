//! Library surface for `julie-extract-cli`.
//!
//! The CLI is primarily a binary (`src/main.rs`), but a handful of internals are
//! also reachable through this thin library target so that **integration tests in
//! `tests/`** (which can only see a crate's public library API, never a binary's
//! private modules) can exercise them directly.
//!
//! Today the only re-export is [`resolution`], whose [`resolution::resolve_workspace`]
//! is the DB-bound workspace reference-resolution pass. The performance gate
//! (`tests/resolution_perf.rs`, behind the `test-perf` feature) times that pass at
//! synthetic scale, which requires calling it in-process against a seeded SQLite
//! artifact — not through the built binary. The module is fully self-contained (it
//! references no sibling CLI module via `crate::`), so exporting it here compiles
//! identically to its use inside the binary and adds no new coupling.

pub mod limits;
pub mod resolution;
