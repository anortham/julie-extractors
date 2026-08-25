//! Shared source-discovery and extraction limits.

/// Directory names and file suffixes discovery refuses before any ignore file is
/// consulted. Defined by [`crate::discovery`] and re-exported here so the
/// published discovery limits have one import path.
pub use crate::discovery::{HARD_EXCLUDE_DIRS, HARD_EXCLUDE_SUFFIXES};

/// Maximum byte size of a source file that `julie-extract` will parse. Files
/// larger than this are skipped with a typed `slow_file_skipped` warning instead
/// of being parsed, bounding worst-case extraction time. Exposed through the
/// crate's thin library target so integration tests derive fixture sizes from the
/// real limit instead of hard-coding it.
pub const MAX_SOURCE_FILE_BYTES: usize = 1024 * 1024;

/// The `slow_file_skipped` diagnostic message for a source file that exceeds
/// [`MAX_SOURCE_FILE_BYTES`]. Single-sourced so the `scan` and `update` paths
/// emit byte-identical warning text.
pub fn slow_file_skip_message() -> String {
    format!("source file exceeds the {MAX_SOURCE_FILE_BYTES}-byte extraction limit and was skipped")
}
