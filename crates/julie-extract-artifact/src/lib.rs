//! Durable extraction artifact support.
//!
//! This crate owns the standalone product artifact boundary: SQLite schema,
//! artifact metadata, report row domains, and writer setup. It intentionally
//! does not contain parser, search, embedding, MCP, watcher, dashboard, or
//! editing behavior.

pub mod jsonl;
pub mod metadata;
pub mod model;
pub mod reports;
pub mod resolution_store;
pub mod schema;
pub mod writer;
