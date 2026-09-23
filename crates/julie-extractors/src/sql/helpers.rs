//! Helper utilities and regex patterns for SQL extraction.
//!
//! This module contains shared regex patterns compiled once for performance,
//! and utility constants used across the SQL extractor.

use regex::Regex;
use std::sync::LazyLock;

/// Regex for matching SQL data types (INT, VARCHAR, TEXT, etc.)
pub(super) static SQL_TYPE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)\b(INT|INTEGER|VARCHAR|TEXT|DECIMAL|FLOAT|BOOLEAN|DATE|TIMESTAMP|CHAR|BIGINT|SMALLINT)\b",
    )
    .unwrap()
});

/// Regex for extracting CREATE VIEW statements
pub(super) static CREATE_VIEW_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"CREATE\s+VIEW\s+([a-zA-Z_][a-zA-Z0-9_]*)\s+AS").unwrap());

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

/// The text of a quoted SQL string literal (`'a''b'`, `N'x'`, `E'x'`), or
/// `None` when the text is not a quoted string or the value is blank.
pub(crate) fn sql_string_literal_text(raw: &str) -> Option<String> {
    let raw = raw.trim();
    let raw = raw
        .strip_prefix(['N', 'n', 'E', 'e'])
        .filter(|rest| rest.starts_with('\''))
        .unwrap_or(raw);
    let inner = raw.strip_prefix('\'')?.strip_suffix('\'')?;
    let text = inner.replace("''", "'");
    (!text.trim().is_empty()).then_some(text)
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
