//! Tree-independent lexical scan for Erlang literal text.
//!
//! Recovery cannot ask the parse tree where the literals are: the files it runs
//! on are the ones whose parse failed, and an unclosed `"` is exactly the case
//! the grammar does not materialize as a `string` node — it lands under an
//! `ERROR`, its interior lines look like top-level forms, and resuming there
//! mints declarations out of literal text.
//!
//! This scan reads the source the way the lexer does instead, from byte 0, so
//! there is no earlier state to be wrong about: comments, strings, quoted
//! atoms, `$` char literals, sigil strings, and triple-quoted strings are
//! recognised by the same token rules `tree-sitter-erlang` 0.20.0 uses
//! (`grammar.js` `comment`/`_sq_string`/`atom`/`char`/sigil tokens, plus the
//! `TQ_STRING` external scanner in `src/scanner.c`).
//!
//! A literal left open at end of input is bounded at the next blank line rather
//! than run to EOF. An unterminated literal only occurs in a file that is
//! already broken, and letting one swallow the remainder would trade phantom
//! declarations for lost real ones — the blank line is the same paragraph
//! boundary a reader uses, and it also keeps a scan that mis-pairs a quote on
//! exotic syntax from silently suppressing recovery for the whole file.

use std::ops::Range;

/// Byte ranges of the literal tokens in a source file.
#[derive(Debug, Default)]
pub(super) struct LiteralSpans {
    spans: Vec<Range<usize>>,
}

impl LiteralSpans {
    pub(super) fn scan(content: &str) -> Self {
        Self {
            spans: scan_spans(content),
        }
    }

    /// Whether `offset` sits STRICTLY inside a literal.
    ///
    /// A literal that begins at the offset is that form's own head: `'quoted
    /// name'(X) -> X.` is a legal Erlang function, so its opening quote must
    /// stay a usable resume point.
    pub(super) fn contains_strictly(&self, offset: usize) -> bool {
        self.spans
            .iter()
            .any(|span| span.start < offset && offset < span.end)
    }
}

fn scan_spans(content: &str) -> Vec<Range<usize>> {
    let bytes = content.as_bytes();
    let mut spans = Vec::new();
    let mut offset = 0;

    while offset < bytes.len() {
        let start = offset;
        match bytes[offset] {
            b'%' => {
                offset = line_end(bytes, offset);
                spans.push(start..offset);
            }
            b'$' => {
                offset = char_literal_end(bytes, offset);
                spans.push(start..offset);
            }
            b'\'' => {
                offset = quoted_end(content, offset, b'\'');
                spans.push(start..offset);
            }
            b'"' => {
                offset = string_end(content, offset);
                spans.push(start..offset);
            }
            b'~' => match sigil_body(bytes, offset) {
                Some(body) => {
                    offset = if bytes[body] == b'"' {
                        string_end(content, body)
                    } else {
                        quoted_end(content, body, closing_delimiter(bytes[body]))
                    };
                    spans.push(start..offset);
                }
                None => offset += 1,
            },
            _ => offset += 1,
        }
    }

    spans
}

/// Offset of the newline that ends the line containing `offset`, or the end of
/// input. A comment runs to there and is never unterminated.
fn line_end(bytes: &[u8], offset: usize) -> usize {
    bytes[offset..]
        .iter()
        .position(|byte| *byte == b'\n')
        .map_or(bytes.len(), |found| offset + found)
}

/// End of `$…`. The escape forms come from the `char` token regex: `\^X`,
/// `\x41`, `\x{1F600}`, octal, and `\` plus any single character.
fn char_literal_end(bytes: &[u8], offset: usize) -> usize {
    let mut offset = offset + 1;
    if offset >= bytes.len() {
        return offset;
    }
    if bytes[offset] != b'\\' {
        return offset + 1;
    }

    offset += 1;
    match bytes.get(offset) {
        Some(b'^') => (offset + 2).min(bytes.len()),
        Some(b'x') => hex_escape_end(bytes, offset + 1),
        Some(b'0'..=b'7') => octal_escape_end(bytes, offset),
        Some(_) => offset + 1,
        None => offset,
    }
}

fn hex_escape_end(bytes: &[u8], offset: usize) -> usize {
    if bytes.get(offset) == Some(&b'{') {
        return closing_brace_end(bytes, offset).min(line_end(bytes, offset));
    }

    let mut offset = offset;
    while bytes.get(offset).is_some_and(u8::is_ascii_hexdigit) {
        offset += 1;
    }
    offset
}

fn closing_brace_end(bytes: &[u8], offset: usize) -> usize {
    bytes[offset..]
        .iter()
        .position(|byte| *byte == b'}')
        .map_or(bytes.len(), |found| offset + found + 1)
}

fn octal_escape_end(bytes: &[u8], offset: usize) -> usize {
    let digits = bytes[offset..]
        .iter()
        .take(3)
        .take_while(|byte| matches!(**byte, b'0'..=b'7'))
        .count();
    offset + digits
}

/// End of a `"`-delimited token: a triple-quoted string when the opening run is
/// three or more quotes alone on its line, an ordinary string otherwise.
fn string_end(content: &str, offset: usize) -> usize {
    triple_quoted_end(content, offset).unwrap_or_else(|| quoted_end(content, offset, b'"'))
}

/// End of `open`-delimited text, honouring `\` escapes. `\^X` consumes the
/// character after the caret so a `\^"` control escape does not close a string.
fn quoted_end(content: &str, offset: usize, close: u8) -> usize {
    let bytes = content.as_bytes();
    let mut scan = offset + 1;

    while scan < bytes.len() {
        match bytes[scan] {
            b'\\' if bytes.get(scan + 1) == Some(&b'^') => scan += 3,
            b'\\' => scan += 2,
            byte if byte == close => return scan + 1,
            _ => scan += 1,
        }
    }

    open_literal_end(content, offset)
}

/// End of a triple-quoted string, or `None` when the run at `offset` does not
/// open one. The opening run is three or more quotes followed by nothing but
/// whitespace to end of line; the closing run is the same count of quotes
/// preceded by nothing but whitespace from start of line.
fn triple_quoted_end(content: &str, offset: usize) -> Option<usize> {
    let bytes = content.as_bytes();
    let quotes = bytes[offset..]
        .iter()
        .take_while(|byte| **byte == b'"')
        .count();
    if quotes < 3 {
        return None;
    }

    let rest_of_line = offset + quotes..line_end(bytes, offset + quotes);
    if !content[rest_of_line.clone()]
        .chars()
        .all(char::is_whitespace)
        || rest_of_line.end >= bytes.len()
    {
        return None;
    }

    let mut line_start = rest_of_line.end + 1;
    while line_start < bytes.len() {
        let end_of_line = line_end(bytes, line_start);
        let line = &content[line_start..end_of_line];
        let closing = line_start + line.len() - line.trim_start().len();
        if content[closing..end_of_line].starts_with(&"\"".repeat(quotes)) {
            return Some(closing + quotes);
        }
        line_start = end_of_line + 1;
    }

    Some(open_literal_end(content, offset))
}

/// Where a literal left open at end of input stops counting as literal text:
/// the next blank line after the one it opened on, or end of input.
fn open_literal_end(content: &str, offset: usize) -> usize {
    let bytes = content.as_bytes();
    let mut line_start = line_end(bytes, offset) + 1;

    while line_start < bytes.len() {
        let end_of_line = line_end(bytes, line_start);
        if content[line_start..end_of_line].trim().is_empty() {
            return line_start;
        }
        line_start = end_of_line + 1;
    }

    bytes.len()
}

/// Offset of a sigil string's opening delimiter, for `~`, `~s`, `~S`, `~b` or
/// `~B` followed by one of the EEP-66 delimiters.
fn sigil_body(bytes: &[u8], offset: usize) -> Option<usize> {
    let body = match bytes.get(offset + 1) {
        Some(b's' | b'S' | b'b' | b'B') => offset + 2,
        _ => offset + 1,
    };

    matches!(
        bytes.get(body),
        Some(b'(' | b'[' | b'{' | b'<' | b'/' | b'|' | b'\'' | b'"' | b'`' | b'#')
    )
    .then_some(body)
}

fn closing_delimiter(open: u8) -> u8 {
    match open {
        b'(' => b')',
        b'[' => b']',
        b'{' => b'}',
        b'<' => b'>',
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spans(content: &str) -> Vec<&str> {
        scan_spans(content)
            .into_iter()
            .map(|span| &content[span])
            .collect()
    }

    #[test]
    fn a_comment_runs_to_end_of_line() {
        assert_eq!(spans("f() -> ok. % why\ng() -> ok.\n"), vec!["% why"]);
    }

    #[test]
    fn a_string_spans_lines_and_hides_its_interior() {
        let code = "f() -> \"one\ntwo\".\n";
        let literals = LiteralSpans::scan(code);

        assert_eq!(spans(code), vec!["\"one\ntwo\""]);
        assert!(literals.contains_strictly(code.find("two").unwrap()));
    }

    #[test]
    fn an_escaped_quote_does_not_close_a_string() {
        assert_eq!(spans("f() -> \"a\\\"b\".\n"), vec!["\"a\\\"b\""]);
    }

    #[test]
    fn a_caret_escape_does_not_close_a_string() {
        assert_eq!(spans("f() -> \"a\\^\"b\".\n"), vec!["\"a\\^\"b\""]);
    }

    #[test]
    fn a_char_literal_quote_does_not_open_a_string() {
        assert_eq!(spans("f() -> $\". g() -> $'.\n"), vec!["$\"", "$'"]);
    }

    #[test]
    fn an_escaped_backslash_char_literal_consumes_both_bytes() {
        assert_eq!(spans("f() -> [$\\\\, $\\n, $\\x41, $\\^C].\n"), {
            vec!["$\\\\", "$\\n", "$\\x41", "$\\^C"]
        });
    }

    #[test]
    fn a_percent_inside_a_string_is_not_a_comment() {
        assert_eq!(spans("f() -> \"100%\".\n% real\n"), {
            vec!["\"100%\"", "% real"]
        });
    }

    #[test]
    fn a_quote_inside_a_comment_does_not_open_a_string() {
        assert_eq!(spans("% it's fine\nf() -> ok.\n"), vec!["% it's fine"]);
    }

    #[test]
    fn a_triple_quoted_string_ends_at_its_matching_run() {
        let code = "-doc \"\"\"\nghost() -> x.\n\"\"\".\nreal() -> ok.\n";

        assert_eq!(spans(code), vec!["\"\"\"\nghost() -> x.\n\"\"\""]);
    }

    #[test]
    fn a_longer_triple_quote_run_needs_the_same_count_to_close() {
        let code = "-doc \"\"\"\"\n\"\"\"\n\"\"\"\".\n";

        assert_eq!(spans(code), vec!["\"\"\"\"\n\"\"\"\n\"\"\"\""]);
    }

    #[test]
    fn a_sigil_string_is_literal_text() {
        assert_eq!(spans("f() -> ~S/a\"b/.\n"), vec!["~S/a\"b/"]);
        assert_eq!(spans("f() -> ~b[a\"b].\n"), vec!["~b[a\"b]"]);
    }

    #[test]
    fn a_literal_left_open_at_eof_stops_at_the_next_blank_line() {
        let code = "f() ->\n    \"unclosed\nghost() -> x.\n\nreal() -> ok.\n";
        let literals = LiteralSpans::scan(code);

        assert!(literals.contains_strictly(code.find("ghost").unwrap()));
        assert!(!literals.contains_strictly(code.find("real").unwrap()));
    }

    #[test]
    fn a_literal_left_open_with_no_blank_line_runs_to_end_of_input() {
        let code = "f() ->\n    'unclosed\nghost() -> x.\n";
        let literals = LiteralSpans::scan(code);

        assert!(literals.contains_strictly(code.find("ghost").unwrap()));
    }

    #[test]
    fn a_literal_that_begins_at_the_offset_is_not_strictly_inside_it() {
        let code = "'quoted name'(X) -> X.\n";
        let literals = LiteralSpans::scan(code);

        assert!(!literals.contains_strictly(0));
    }

    #[test]
    fn multibyte_content_does_not_shift_literal_boundaries() {
        let code = "%% ☃ snowman\nf() -> \"☃\".\n";

        assert_eq!(spans(code), vec!["%% ☃ snowman", "\"☃\""]);
    }
}
