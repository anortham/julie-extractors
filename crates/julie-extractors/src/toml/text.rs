//! TOML string, key, and comment text.
//!
//! - Strings and quoted keys decode by the TOML rules: basic strings unescape,
//!   literal strings stay raw, and a multi-line string drops the newline right
//!   after its opening delimiter. In a multi-line basic string a backslash at
//!   the end of a line removes that line break and the whitespace after it.
//! - A table or key is documented by the contiguous `#` comment lines directly
//!   above it. The grammar can place those lines inside the previous table, so
//!   the rule reads source lines, not sibling nodes.

use tree_sitter::Node;

/// The value and style (`basic`, `literal`, `multiline_basic`,
/// `multiline_literal`) of a TOML string token.
pub(crate) fn decode_toml_string(raw: &str) -> Option<(String, &'static str)> {
    let raw = raw.trim();
    if let Some(inner) = strip_delimiters(raw, "\"\"\"") {
        return Some((
            unescape_basic(strip_first_newline(inner)),
            "multiline_basic",
        ));
    }
    if let Some(inner) = strip_delimiters(raw, "'''") {
        return Some((strip_first_newline(inner).to_string(), "multiline_literal"));
    }
    if let Some(inner) = strip_delimiters(raw, "\"") {
        return Some((unescape_basic(inner), "basic"));
    }
    strip_delimiters(raw, "'").map(|inner| (inner.to_string(), "literal"))
}

/// A key segment: a bare key as written, a quoted key decoded.
pub(crate) fn decode_toml_key(raw: &str) -> String {
    let raw = raw.trim();
    decode_toml_string(raw).map_or_else(|| raw.to_string(), |(value, _)| value)
}

fn strip_delimiters<'a>(raw: &'a str, delimiter: &str) -> Option<&'a str> {
    (raw.len() >= 2 * delimiter.len())
        .then(|| raw.strip_prefix(delimiter)?.strip_suffix(delimiter))
        .flatten()
}

fn strip_first_newline(inner: &str) -> &str {
    inner
        .strip_prefix("\r\n")
        .or_else(|| inner.strip_prefix('\n'))
        .unwrap_or(inner)
}

fn unescape_basic(inner: &str) -> String {
    let mut out = String::with_capacity(inner.len());
    let mut chars = inner.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            out.push(ch);
            continue;
        }
        match chars.next() {
            Some('b') => out.push('\u{8}'),
            Some('t') => out.push('\t'),
            Some('n') => out.push('\n'),
            Some('f') => out.push('\u{c}'),
            Some('r') => out.push('\r'),
            Some('e') => out.push('\u{1b}'),
            Some('"') => out.push('"'),
            Some('\\') => out.push('\\'),
            Some(kind @ ('u' | 'U' | 'x')) => {
                let width = match kind {
                    'u' => 4,
                    'U' => 8,
                    _ => 2,
                };
                let hex: String = (0..width).filter_map(|_| chars.next()).collect();
                match u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32) {
                    Some(decoded) => out.push(decoded),
                    None => {
                        out.push('\\');
                        out.push(kind);
                        out.push_str(&hex);
                    }
                }
            }
            Some(space) if space.is_whitespace() => {
                while chars.peek().is_some_and(|next| next.is_whitespace()) {
                    chars.next();
                }
            }
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            None => out.push('\\'),
        }
    }
    out
}

/// The `#` comment lines directly above the item that starts at `item_start`,
/// markers kept, or `None` when the item does not start its line.
pub(crate) fn leading_comment_doc(content: &str, item_start: usize) -> Option<String> {
    let line_start = content[..item_start]
        .rfind('\n')
        .map_or(0, |index| index + 1);
    if !content[line_start..item_start].trim().is_empty() {
        return None;
    }
    let mut lines: Vec<&str> = content[..line_start]
        .lines()
        .rev()
        .map(str::trim)
        .take_while(|line| line.starts_with('#'))
        .collect();
    lines.reverse();
    (!lines.is_empty()).then(|| lines.join("\n"))
}

/// True when `comment` belongs to the leading doc block of the table or key on
/// the first non-comment line after it, by the rule of [`leading_comment_doc`].
pub(crate) fn comment_documents_following_item(content: &str, comment: Node) -> bool {
    let start = comment.start_byte();
    let line_start = content[..start].rfind('\n').map_or(0, |index| index + 1);
    if !content[line_start..start].trim().is_empty() {
        return false;
    }
    let mut offset = content[start..]
        .find('\n')
        .map_or(content.len(), |index| start + index + 1);
    while offset < content.len() {
        let line_end = content[offset..]
            .find('\n')
            .map_or(content.len(), |index| offset + index);
        let line = &content[offset..line_end];
        let trimmed = line.trim_start();
        if trimmed.trim_end().is_empty() {
            return false;
        }
        if !trimmed.starts_with('#') {
            let item_start = offset + (line.len() - trimmed.len());
            return starts_item(comment, item_start);
        }
        offset = line_end + 1;
    }
    false
}

fn starts_item(anchor: Node, byte: usize) -> bool {
    let mut root = anchor;
    while let Some(parent) = root.parent() {
        root = parent;
    }
    let mut node = root.descendant_for_byte_range(byte, byte);
    while let Some(current) = node.filter(|current| current.start_byte() == byte) {
        if matches!(current.kind(), "pair" | "table" | "table_array_element") {
            return true;
        }
        node = current.parent();
    }
    false
}
