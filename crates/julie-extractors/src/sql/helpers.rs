//! Helper utilities and regex patterns for SQL extraction.
//!
//! This module contains shared regex patterns compiled once for performance,
//! and utility constants used across the SQL extractor.

use regex::Regex;
use std::sync::LazyLock;

/// Regex for matching SQL data types (INT, VARCHAR, TEXT, etc.)
pub(super) static SQL_TYPE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"\b(INT|INTEGER|VARCHAR|TEXT|DECIMAL|FLOAT|BOOLEAN|DATE|TIMESTAMP|CHAR|BIGINT|SMALLINT)\b",
    )
    .unwrap()
});

/// Regex for extracting CREATE VIEW statements
pub(super) static CREATE_VIEW_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"CREATE\s+VIEW\s+([a-zA-Z_][a-zA-Z0-9_]*)\s+AS").unwrap());

/// Regex for extracting index column definitions
pub(super) static INDEX_COLUMN_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?:ON\s+[a-zA-Z_][a-zA-Z0-9_]*(?:\s+USING\s+[A-Z]+)?\s*)?(\([^)]+\))").unwrap()
});

/// Regex for extracting INCLUDE clauses (PostgreSQL indexes)
pub(super) static INCLUDE_CLAUSE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"INCLUDE\s*(\([^)]+\))").unwrap());

/// Regex for extracting variable declarations (PostgreSQL style)
pub(super) static VAR_DECL_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^([a-zA-Z_][a-zA-Z0-9_]*)\s+([A-Z0-9(),\s]+)").unwrap());

/// Regex for extracting DECLARE variable statements (MySQL style)
pub(super) static DECLARE_VAR_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"DECLARE\s+([a-zA-Z_][a-zA-Z0-9_]*)\s+(DECIMAL\([^)]+\)|INT|BIGINT|VARCHAR\([^)]+\)|TEXT|BOOLEAN)").unwrap()
});

pub(crate) fn normalize_sql_identifier(raw: &str) -> String {
    let trimmed = raw.trim();
    if let Some(inner) = trimmed
        .strip_prefix('[')
        .and_then(|value| value.strip_suffix(']'))
    {
        return inner.replace("]]", "]");
    }
    if let Some(inner) = trimmed
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
    {
        return inner.to_string();
    }
    if let Some(inner) = trimmed
        .strip_prefix('`')
        .and_then(|value| value.strip_suffix('`'))
    {
        return inner.to_string();
    }
    trimmed.to_string()
}

#[cfg(test)]
mod tests {
    use super::normalize_sql_identifier;

    #[test]
    fn normalizes_sql_identifier_delimiters() {
        for (raw, expected) in [
            ("plain_name", "plain_name"),
            ("[edr]", "edr"),
            ("[a]]b]", "a]b"),
            ("\"quoted\"", "quoted"),
            ("`quoted`", "quoted"),
            (" [spaced] ", "spaced"),
            ("", ""),
            ("[mismatched", "[mismatched"),
        ] {
            assert_eq!(normalize_sql_identifier(raw), expected);
        }
    }
}
