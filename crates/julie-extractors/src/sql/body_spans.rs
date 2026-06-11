//! SQL body-span helpers for views, triggers, and stored procedures.
//!
//! Clean AST nodes may still produce weak spans when tree-sitter only captures a
//! fragment. Recovery-path symbols are tagged with `extractedFromError` and get
//! statement-level body spans derived from the full source text when possible.

use crate::base::body::body_hash;
use crate::base::{BaseExtractor, NormalizedSpan, Symbol};
use serde_json::Value;
use std::collections::HashMap;

pub(super) fn finalize_sql_callable_symbol(base: &BaseExtractor, symbol: &mut Symbol) {
    if !is_sql_callable(symbol) {
        return;
    }

    let is_recovery = metadata_bool(symbol, "extractedFromError");
    let statement = statement_text_from(base, symbol);

    match infer_body_span_from_statement(base, symbol, &statement) {
        Some(body_span) if should_replace_body_span(symbol, &body_span, is_recovery) => {
            symbol.body_span = Some(body_span);
            symbol.body_hash = body_hash(&base.content, body_span, &base.language);
            set_metadata_str(
                symbol,
                "bodySpanSource",
                if is_recovery {
                    "recovery_heuristic"
                } else {
                    "statement_text"
                },
            );
        }
        None if is_recovery => {
            symbol.body_span = None;
            symbol.body_hash = None;
            set_metadata_str(symbol, "bodySpanSource", "unavailable");
        }
        _ => {}
    }
}

fn is_sql_callable(symbol: &Symbol) -> bool {
    let Some(metadata) = symbol.metadata.as_ref() else {
        return false;
    };
    metadata_bool_key(metadata, "isView")
        || metadata_bool_key(metadata, "isTrigger")
        || metadata_bool_key(metadata, "isStoredProcedure")
        || metadata_bool_key(metadata, "isFunction")
}

fn should_replace_body_span(symbol: &Symbol, improved: &NormalizedSpan, is_recovery: bool) -> bool {
    if is_recovery || symbol.body_span.is_none() {
        return true;
    }

    let Some(current) = symbol.body_span else {
        return true;
    };

    let current_len = current.end_byte.saturating_sub(current.start_byte);
    let improved_len = improved.end_byte.saturating_sub(improved.start_byte);
    improved_len > current_len.saturating_mul(2)
}

fn infer_body_span_from_statement(
    base: &BaseExtractor,
    symbol: &Symbol,
    statement: &str,
) -> Option<NormalizedSpan> {
    let metadata = symbol.metadata.as_ref()?;
    if metadata_bool_key(metadata, "isView") {
        return view_body_span(base, symbol.start_byte as usize, statement);
    }
    if metadata_bool_key(metadata, "isTrigger")
        || metadata_bool_key(metadata, "isStoredProcedure")
        || metadata_bool_key(metadata, "isFunction")
    {
        return begin_end_or_as_body_span(base, symbol.start_byte as usize, statement);
    }
    None
}

fn view_body_span(
    base: &BaseExtractor,
    declaration_start: usize,
    statement: &str,
) -> Option<NormalizedSpan> {
    let lower = statement.to_ascii_lowercase();
    let as_index = find_sql_keyword(&lower, "as")?;
    let body_start = first_non_whitespace_after(statement, as_index + "as".len())?;
    if body_start >= statement.len() {
        return None;
    }
    span_for_statement_range(base, declaration_start, body_start, statement.len())
}

fn begin_end_or_as_body_span(
    base: &BaseExtractor,
    declaration_start: usize,
    statement: &str,
) -> Option<NormalizedSpan> {
    if let Some(span) = begin_end_body_span(base, declaration_start, statement) {
        return Some(span);
    }

    let lower = statement.to_ascii_lowercase();
    let as_index = find_sql_keyword(&lower, "as")?;
    let body_start = first_non_whitespace_after(statement, as_index + "as".len())?;
    if body_start >= statement.len() {
        return None;
    }
    span_for_statement_range(base, declaration_start, body_start, statement.len())
}

fn begin_end_body_span(
    base: &BaseExtractor,
    declaration_start: usize,
    statement: &str,
) -> Option<NormalizedSpan> {
    let lower = statement.to_ascii_lowercase();
    let begin_index = find_sql_keyword(&lower, "begin")?;

    let body_start = begin_index + "begin".len();
    let end_index = rfind_sql_keyword(&lower, "end")?;

    let body_end = end_index + "end".len();
    if body_start >= body_end {
        return None;
    }
    span_for_statement_range(base, declaration_start, body_start, body_end)
}

fn find_sql_keyword(lower: &str, keyword: &str) -> Option<usize> {
    lower
        .match_indices(keyword)
        .find_map(|(index, _)| sql_keyword_at(lower, keyword, index).then_some(index))
}

fn rfind_sql_keyword(lower: &str, keyword: &str) -> Option<usize> {
    lower
        .match_indices(keyword)
        .filter_map(|(index, _)| sql_keyword_at(lower, keyword, index).then_some(index))
        .last()
}

fn sql_keyword_at(lower: &str, keyword: &str, index: usize) -> bool {
    let bytes = lower.as_bytes();
    let before_ok = index == 0 || !is_sql_ident_char(bytes[index - 1]);
    let after_index = index + keyword.len();
    let after_ok = after_index >= lower.len() || !is_sql_ident_char(bytes[after_index]);
    before_ok && after_ok
}

fn is_sql_ident_char(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

fn first_non_whitespace_after(statement: &str, index: usize) -> Option<usize> {
    statement
        .get(index..)?
        .char_indices()
        .find_map(|(offset, ch)| (!ch.is_whitespace()).then_some(index + offset))
}

fn span_for_statement_range(
    base: &BaseExtractor,
    declaration_start: usize,
    relative_start: usize,
    relative_end: usize,
) -> Option<NormalizedSpan> {
    let absolute_start = declaration_start + relative_start;
    let absolute_end = declaration_start + relative_end;
    NormalizedSpan::from_content_range_with_line_starts(
        &base.content,
        base.line_starts(),
        absolute_start,
        absolute_end,
    )
}

fn statement_text_from(base: &BaseExtractor, symbol: &Symbol) -> String {
    let start = symbol.start_byte as usize;
    let end = symbol.end_byte as usize;
    if end > start
        && let Some(text) = base.content.get(start..end)
    {
        return text.to_string();
    }

    let tail = base.content.get(start..).unwrap_or("");
    if let Some(semi) = tail.find(';') {
        tail[..=semi].to_string()
    } else {
        tail.to_string()
    }
}

fn metadata_bool(symbol: &Symbol, key: &str) -> bool {
    symbol
        .metadata
        .as_ref()
        .is_some_and(|metadata| metadata_bool_key(metadata, key))
}

fn metadata_bool_key(metadata: &HashMap<String, Value>, key: &str) -> bool {
    metadata.get(key).and_then(Value::as_bool).unwrap_or(false)
}

fn set_metadata_str(symbol: &mut Symbol, key: &str, value: &str) {
    let metadata = symbol.metadata.get_or_insert_with(HashMap::new);
    metadata.insert(key.to_string(), Value::String(value.to_string()));
}
