//! Shared lexical scanning core for the framework structural-fact collectors.
//!
//! Every backend collector scans raw source for needles like `app.get` or
//! `http.NewRequest`, then parses the call arguments around each hit. The
//! string/comment awareness for that scanning lives here, once per language,
//! as a per-byte [`SourceMask`] computed in a single pass. Collectors build
//! the mask once per file and consult it for needle filtering, delimiter
//! matching, and argument splitting.

use std::collections::HashMap;

use serde_json::Value;
use tree_sitter::Tree;

use super::helpers::{
    base_metadata, fact_for_span, insert_string, insert_string_array, is_comment_or_string_node,
    smallest_node_covering_range,
};
use crate::base::http_boundary::{ParamFlavor, join_route_templates, normalize_route_template};
use crate::base::span::NormalizedSpan;
use crate::base::types::StructuralFact;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum MaskLanguage {
    Js,
    Go,
    Java,
    CSharp,
    Python,
    Ruby,
}

/// Per-byte string/comment classification for one source file.
///
/// `is_string_or_comment(i)` mirrors the historical `is_in_<lang>_string_or_comment(content, i)`
/// semantics: an opening delimiter byte is still code, everything after it up
/// to and including the closing delimiter is string/comment.
pub(super) struct SourceMask {
    flags: Vec<bool>,
}

impl SourceMask {
    pub(super) fn new(content: &str, language: MaskLanguage) -> Self {
        Self {
            flags: build_flags(content, language),
        }
    }

    pub(super) fn is_string_or_comment(&self, index: usize) -> bool {
        self.flags.get(index).copied().unwrap_or(false)
    }
}

fn build_flags(content: &str, language: MaskLanguage) -> Vec<bool> {
    use MaskLanguage::*;
    let bytes = content.as_bytes();
    let mut flags = vec![false; bytes.len()];

    let slash_comments = matches!(language, Js | Go | Java | CSharp);
    let hash_comments = matches!(language, Python | Ruby);
    let single_quotes = matches!(language, Js | Go | Java | CSharp | Python | Ruby);
    let backtick_quotes = matches!(language, Js | Go);
    let raw_backtick = matches!(language, Go);
    let triple_quotes = matches!(language, CSharp | Python);
    let verbatim_strings = matches!(language, CSharp);
    let regex_literals = matches!(language, Js | Ruby);

    let mut cursor = 0;
    let mut line_comment = false;
    let mut block_comment = false;
    let mut quote: Option<u8> = None;
    let mut triple = false;
    let mut verbatim = false;

    while cursor < bytes.len() {
        let byte = bytes[cursor];
        let next = bytes.get(cursor + 1).copied();
        flags[cursor] = line_comment || block_comment || verbatim || quote.is_some();

        if line_comment {
            if byte == b'\n' {
                line_comment = false;
            }
            cursor += 1;
            continue;
        }
        if block_comment {
            if byte == b'*' && next == Some(b'/') {
                block_comment = false;
                flags[cursor + 1] = true;
                cursor += 2;
            } else {
                cursor += 1;
            }
            continue;
        }
        if verbatim {
            if byte == b'"' {
                if next == Some(b'"') {
                    flags[cursor + 1] = true;
                    cursor += 2;
                } else {
                    verbatim = false;
                    cursor += 1;
                }
            } else {
                cursor += 1;
            }
            continue;
        }
        if let Some(active) = quote {
            let raw = raw_backtick && active == b'`';
            if !raw && byte == b'\\' {
                if cursor + 1 < bytes.len() {
                    flags[cursor + 1] = true;
                }
                cursor += 2;
                continue;
            }
            if byte == active {
                if triple {
                    if next == Some(active) && bytes.get(cursor + 2) == Some(&active) {
                        flags[cursor + 1] = true;
                        flags[cursor + 2] = true;
                        quote = None;
                        triple = false;
                        cursor += 3;
                        continue;
                    }
                } else {
                    quote = None;
                }
            }
            cursor += 1;
            continue;
        }

        if hash_comments && byte == b'#' {
            line_comment = true;
            cursor += 1;
            continue;
        }
        if slash_comments && byte == b'/' && next == Some(b'/') {
            line_comment = true;
            flags[cursor + 1] = true;
            cursor += 2;
            continue;
        }
        if slash_comments && byte == b'/' && next == Some(b'*') {
            block_comment = true;
            flags[cursor + 1] = true;
            cursor += 2;
            continue;
        }
        if regex_literals
            && byte == b'/'
            && next != Some(b'/')
            && next != Some(b'*')
            && is_regex_literal_context(bytes, cursor)
            && let Some(end) = regex_literal_end(bytes, cursor)
        {
            for flag in flags.iter_mut().take(end + 1).skip(cursor + 1) {
                *flag = true;
            }
            cursor = end + 1;
            while cursor < bytes.len() && bytes[cursor].is_ascii_alphabetic() {
                flags[cursor] = true;
                cursor += 1;
            }
            continue;
        }
        if verbatim_strings
            && (byte == b'@' && (next == Some(b'"') || next == Some(b'$')))
            && (next == Some(b'"') || bytes.get(cursor + 2) == Some(&b'"'))
        {
            verbatim = true;
            let quote_offset = if next == Some(b'$') { 2 } else { 1 };
            for flag in flags
                .iter_mut()
                .take(cursor + quote_offset + 1)
                .skip(cursor + 1)
            {
                *flag = true;
            }
            cursor += quote_offset + 1;
            continue;
        }
        if verbatim_strings
            && byte == b'$'
            && next == Some(b'@')
            && bytes.get(cursor + 2) == Some(&b'"')
        {
            verbatim = true;
            flags[cursor + 1] = true;
            flags[cursor + 2] = true;
            cursor += 3;
            continue;
        }
        let is_quote_byte =
            byte == b'"' || (single_quotes && byte == b'\'') || (backtick_quotes && byte == b'`');
        if is_quote_byte {
            quote = Some(byte);
            if triple_quotes && next == Some(byte) && bytes.get(cursor + 2) == Some(&byte) {
                triple = true;
                flags[cursor + 1] = true;
                flags[cursor + 2] = true;
                cursor += 3;
                continue;
            }
            triple = false;
        }
        cursor += 1;
    }
    flags
}

fn is_regex_literal_context(bytes: &[u8], slash: usize) -> bool {
    let Some(previous) = bytes[..slash]
        .iter()
        .rposition(|byte| !byte.is_ascii_whitespace())
        .map(|index| bytes[index])
    else {
        return true;
    };
    !matches!(
        previous,
        b')' | b']' | b'}' | b'"' | b'\'' | b'`' | b'_' | b'$' | b'0'..=b'9' | b'a'..=b'z' | b'A'..=b'Z'
    )
}

fn regex_literal_end(bytes: &[u8], slash: usize) -> Option<usize> {
    let mut cursor = slash + 1;
    let mut in_class = false;
    while cursor < bytes.len() {
        match bytes[cursor] {
            b'\\' => cursor += 2,
            b'[' if !in_class => {
                in_class = true;
                cursor += 1;
            }
            b']' if in_class => {
                in_class = false;
                cursor += 1;
            }
            b'/' if !in_class => return Some(cursor),
            b'\n' | b'\r' => return None,
            _ => cursor += 1,
        }
    }
    None
}

pub(super) fn find_matching_paren(content: &str, mask: &SourceMask, open: usize) -> Option<usize> {
    find_matching_delimiter(content, mask, open, content.len(), b'(', b')')
}

pub(super) fn find_matching_paren_within(
    content: &str,
    mask: &SourceMask,
    open: usize,
    end: usize,
) -> Option<usize> {
    find_matching_delimiter(content, mask, open, end, b'(', b')')
}

pub(super) fn find_matching_brace_within(
    content: &str,
    mask: &SourceMask,
    open: usize,
    end: usize,
) -> Option<usize> {
    find_matching_delimiter(content, mask, open, end, b'{', b'}')
}

pub(super) fn find_matching_bracket_within(
    content: &str,
    mask: &SourceMask,
    open: usize,
    end: usize,
) -> Option<usize> {
    find_matching_delimiter(content, mask, open, end, b'[', b']')
}

pub(super) fn find_matching_angle_within(
    content: &str,
    mask: &SourceMask,
    open: usize,
    end: usize,
) -> Option<usize> {
    find_matching_delimiter(content, mask, open, end, b'<', b'>')
}

fn find_matching_delimiter(
    content: &str,
    mask: &SourceMask,
    open: usize,
    end: usize,
    left: u8,
    right: u8,
) -> Option<usize> {
    let bytes = content.as_bytes();
    if bytes.get(open) != Some(&left) {
        return None;
    }
    let mut depth = 0usize;
    let mut cursor = open;
    while cursor < end.min(bytes.len()) {
        if mask.is_string_or_comment(cursor) {
            cursor += 1;
            continue;
        }
        let byte = bytes[cursor];
        if byte == left {
            depth += 1;
        } else if byte == right {
            depth = depth.saturating_sub(1);
            if depth == 0 {
                return Some(cursor);
            }
        }
        cursor += 1;
    }
    None
}

pub(super) fn find_top_level_comma_or_end(
    content: &str,
    mask: &SourceMask,
    start: usize,
    end: usize,
) -> usize {
    find_top_level_comma_or_end_impl(content, mask, start, end, false)
}

/// C# argument splitting must also skip commas inside generic type arguments.
pub(super) fn find_top_level_comma_or_end_with_angles(
    content: &str,
    mask: &SourceMask,
    start: usize,
    end: usize,
) -> usize {
    find_top_level_comma_or_end_impl(content, mask, start, end, true)
}

fn find_top_level_comma_or_end_impl(
    content: &str,
    mask: &SourceMask,
    start: usize,
    end: usize,
    track_angles: bool,
) -> usize {
    let bytes = content.as_bytes();
    let mut cursor = start;
    let mut paren_depth = 0usize;
    let mut brace_depth = 0usize;
    let mut bracket_depth = 0usize;
    let mut angle_depth = 0usize;
    while cursor < end.min(bytes.len()) {
        if mask.is_string_or_comment(cursor) {
            cursor += 1;
            continue;
        }
        match bytes[cursor] {
            b'(' => paren_depth += 1,
            b')' => paren_depth = paren_depth.saturating_sub(1),
            b'{' => brace_depth += 1,
            b'}' => brace_depth = brace_depth.saturating_sub(1),
            b'[' => bracket_depth += 1,
            b']' => bracket_depth = bracket_depth.saturating_sub(1),
            b'<' if track_angles => angle_depth += 1,
            b'>' if track_angles => angle_depth = angle_depth.saturating_sub(1),
            b',' if paren_depth == 0
                && brace_depth == 0
                && bracket_depth == 0
                && angle_depth == 0 =>
            {
                return cursor;
            }
            _ => {}
        }
        cursor += 1;
    }
    end
}

/// End of the statement starting at `start`: the first top-level `;` (or, when
/// `newline_terminates`, newline) outside strings, comments, and brackets.
pub(super) fn statement_end(
    content: &str,
    mask: &SourceMask,
    start: usize,
    newline_terminates: bool,
) -> usize {
    let bytes = content.as_bytes();
    let mut cursor = start;
    let mut paren_depth = 0usize;
    let mut brace_depth = 0usize;
    let mut bracket_depth = 0usize;
    while cursor < bytes.len() {
        if mask.is_string_or_comment(cursor) {
            cursor += 1;
            continue;
        }
        match bytes[cursor] {
            b'(' => paren_depth += 1,
            b')' => paren_depth = paren_depth.saturating_sub(1),
            b'{' => brace_depth += 1,
            b'}' => brace_depth = brace_depth.saturating_sub(1),
            b'[' => bracket_depth += 1,
            b']' => bracket_depth = bracket_depth.saturating_sub(1),
            byte if (byte == b';' || (newline_terminates && byte == b'\n'))
                && paren_depth == 0
                && brace_depth == 0
                && bracket_depth == 0 =>
            {
                return cursor + 1;
            }
            _ => {}
        }
        cursor += 1;
    }
    content.len()
}

pub(super) fn parse_python_string_literal(content: &str, start: usize) -> Option<(String, usize)> {
    let bytes = content.as_bytes();
    let mut cursor = start;
    while matches!(bytes.get(cursor).copied(), Some(b'r' | b'R' | b'u' | b'U')) {
        cursor += 1;
    }
    if matches!(bytes.get(cursor).copied(), Some(b'f' | b'F' | b'b' | b'B')) {
        return None;
    }
    let quote = bytes
        .get(cursor)
        .copied()
        .filter(|byte| matches!(*byte, b'\'' | b'"'))?;
    let triple = bytes.get(cursor + 1) == Some(&quote) && bytes.get(cursor + 2) == Some(&quote);
    let content_start = if triple { cursor + 3 } else { cursor + 1 };
    let mut value = String::new();
    let mut index = content_start;
    while index < content.len() {
        let byte = bytes[index];
        if byte == b'\\' {
            let escaped_start = index + 1;
            let escaped = content.get(escaped_start..)?.chars().next()?;
            value.push(escaped);
            index = escaped_start + escaped.len_utf8();
        } else if triple
            && byte == quote
            && bytes.get(index + 1) == Some(&quote)
            && bytes.get(index + 2) == Some(&quote)
        {
            return Some((value, index + 3));
        } else if !triple && byte == quote {
            return Some((value, index + 1));
        } else {
            let ch = content.get(index..)?.chars().next()?;
            value.push(ch);
            index += ch.len_utf8();
        }
    }
    None
}

pub(super) fn parse_go_string_literal(content: &str, start: usize) -> Option<(String, usize)> {
    let quote = content.as_bytes().get(start).copied()?;
    if quote == b'`' {
        let end = content[start + 1..].find('`')? + start + 1;
        return Some((content[start + 1..end].to_string(), end + 1));
    }
    if quote != b'"' {
        return None;
    }
    parse_escaped_string_literal(content, start, b'"')
}

pub(super) fn parse_java_string_literal(content: &str, start: usize) -> Option<(String, usize)> {
    if content.as_bytes().get(start) != Some(&b'"') {
        return None;
    }
    parse_escaped_string_literal(content, start, b'"')
}

pub(super) fn parse_ruby_string_literal(content: &str, start: usize) -> Option<(String, usize)> {
    let quote = content.as_bytes().get(start).copied()?;
    if !matches!(quote, b'\'' | b'"') {
        return None;
    }
    parse_escaped_string_literal(content, start, quote)
}

fn parse_escaped_string_literal(content: &str, start: usize, quote: u8) -> Option<(String, usize)> {
    let mut cursor = start + 1;
    let mut value = String::new();
    while cursor < content.len() {
        let byte = content.as_bytes()[cursor];
        if byte == b'\\' {
            let escaped_start = cursor + 1;
            let escaped = content.get(escaped_start..)?.chars().next()?;
            value.push(escaped);
            cursor = escaped_start + escaped.len_utf8();
        } else if byte == quote {
            return Some((value, cursor + 1));
        } else {
            let ch = content.get(cursor..)?.chars().next()?;
            value.push(ch);
            cursor += ch.len_utf8();
        }
    }
    None
}

/// Shared builder for route-handler definition facts (Express/Fastify, the
/// Python decorator families, and the Go call families all emit this shape).
pub(super) struct RouteFactSpec<'a> {
    pub framework: &'a str,
    pub pattern_id: &'a str,
    pub capture_name: &'a str,
    pub api_style: &'a str,
    pub route_template: &'a str,
    pub verb: Option<&'a str>,
    pub verb_source: Option<&'a str>,
    pub flavor: ParamFlavor,
    pub prefix: Option<&'a str>,
    /// Metadata key recording the raw prefix (e.g. `route_group_prefix`).
    /// `None` leaves prefix recording to the `enrich` callback while still
    /// joining the prefix into `effective_route_template`.
    pub prefix_key: Option<&'a str>,
}

#[allow(clippy::too_many_arguments)]
pub(super) fn route_fact(
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
    start: usize,
    end: usize,
    spec: RouteFactSpec<'_>,
    enrich: impl FnOnce(&mut HashMap<String, Value>),
) -> Option<StructuralFact> {
    let node = smallest_node_covering_range(tree.root_node(), start, end)?;
    if is_comment_or_string_node(node.kind()) {
        return None;
    }
    let span = NormalizedSpan::from_content_range(content, start, end)?;
    let mut metadata = base_metadata("framework", spec.framework);
    insert_string(&mut metadata, "api_style", spec.api_style);
    insert_string(&mut metadata, "route_template", spec.route_template);
    let mut normalized_source = spec.route_template.to_string();
    if let Some(prefix) = spec.prefix {
        if let Some(prefix_key) = spec.prefix_key {
            insert_string(&mut metadata, prefix_key, prefix);
        }
        let effective = join_route_templates(prefix, spec.route_template);
        insert_string(&mut metadata, "effective_route_template", &effective);
        normalized_source = effective;
    }
    let normalized = normalize_route_template(&normalized_source, spec.flavor);
    insert_string(
        &mut metadata,
        "normalized_route_template",
        &normalized.template,
    );
    if !normalized.dynamic_segments.is_empty() {
        insert_string_array(
            &mut metadata,
            "dynamic_segments",
            normalized.dynamic_segments,
        );
    }
    if let Some(verb) = spec.verb {
        insert_string(&mut metadata, "verb", verb);
    }
    if let Some(verb_source) = spec.verb_source {
        insert_string(&mut metadata, "verb_source", verb_source);
    }
    enrich(&mut metadata);
    Some(fact_for_span(
        file_path,
        language,
        spec.pattern_id,
        spec.capture_name,
        node.kind(),
        span,
        metadata,
    ))
}

#[cfg(test)]
mod tests {
    use super::{MaskLanguage, SourceMask};

    fn masked_positions(content: &str, language: MaskLanguage) -> Vec<usize> {
        let mask = SourceMask::new(content, language);
        (0..content.len())
            .filter(|index| mask.is_string_or_comment(*index))
            .collect()
    }

    #[test]
    fn python_mask_survives_triple_quoted_docstrings_with_apostrophes() {
        let content = "'''Bob's routes'''\napp.route(\"/x\")";
        let mask = SourceMask::new(content, MaskLanguage::Python);
        let call = content.find("app.route").unwrap();
        assert!(!mask.is_string_or_comment(call));
        assert!(mask.is_string_or_comment(content.find("Bob").unwrap()));
    }

    #[test]
    fn python_mask_flags_comments_and_plain_strings() {
        let content = "# hidden app.get\nx = 'app.get'\napp.get";
        let mask = SourceMask::new(content, MaskLanguage::Python);
        let hidden = content.find("hidden").unwrap();
        let quoted = content.rfind("'app.get'").unwrap() + 1;
        let live = content.rfind("app.get").unwrap();
        assert!(mask.is_string_or_comment(hidden));
        assert!(mask.is_string_or_comment(quoted));
        assert!(!mask.is_string_or_comment(live));
    }

    #[test]
    fn go_mask_covers_backtick_raw_strings_without_escape_processing() {
        let content = "s := `raw \\ string`\nhttp.Get(\"/x\")";
        let mask = SourceMask::new(content, MaskLanguage::Go);
        assert!(mask.is_string_or_comment(content.find("raw").unwrap()));
        assert!(!mask.is_string_or_comment(content.find("http.Get").unwrap()));
    }

    #[test]
    fn js_mask_covers_template_literals_and_comments() {
        let content = "const t = `app.get`; // app.get\n/* app.get */\napp.get('/x')";
        let mask = SourceMask::new(content, MaskLanguage::Js);
        let live = content.rfind("app.get").unwrap();
        assert!(!mask.is_string_or_comment(live));
        let occurrences: Vec<usize> = content
            .match_indices("app.get")
            .map(|(index, _)| index)
            .collect();
        for index in &occurrences[..occurrences.len() - 1] {
            assert!(mask.is_string_or_comment(*index), "offset {index}");
        }
    }

    #[test]
    fn csharp_mask_covers_verbatim_strings_with_quote_escapes() {
        let content = "var s = @\"say \"\"hi\"\" now\"; client.GetAsync(\"/x\")";
        let mask = SourceMask::new(content, MaskLanguage::CSharp);
        assert!(mask.is_string_or_comment(content.find("say").unwrap()));
        assert!(!mask.is_string_or_comment(content.find("client.GetAsync").unwrap()));
    }

    #[test]
    fn ruby_mask_flags_hash_comments() {
        let content = "# get '/hidden'\nget '/live'";
        let mask = SourceMask::new(content, MaskLanguage::Ruby);
        assert!(mask.is_string_or_comment(content.find("hidden").unwrap() - 6));
        assert!(!mask.is_string_or_comment(content.rfind("get").unwrap()));
        assert!(masked_positions(content, MaskLanguage::Ruby).contains(&2));
    }
}
