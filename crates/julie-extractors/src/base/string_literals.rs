use tree_sitter::Node;

use super::extractor::BaseExtractor;
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};

/// Decode a string-literal node's contents for capture.
pub(crate) fn decode_string_literal(base: &BaseExtractor, node: &Node) -> Option<String> {
    let kind = node.kind();
    if !(kind.contains("string") || kind.contains("char")) {
        return None;
    }
    let mut out = String::new();
    decode_string_children(base, node, &mut out, 0);
    if out.is_empty() {
        out = strip_string_delimiters(&base.get_node_text(node));
    }
    Some(out)
}

/// The text a string literal writes, for a name such as a test label: the
/// text between its quotes, with each escape of a punctuation character
/// (`\"`, `\\`, `\$`) and each Unicode or hex escape of a printable
/// character replaced by that character. Other escapes (`\n`, `\t`) stay as
/// written, so the name stays on one line. An interpolation hole is `{}`, as
/// in [`decode_string_literal`], or its text as written with
/// [`Holes::AsWritten`]. A literal without plain quotes (a raw string, a
/// sigil, a long bracket string, a here-string) decodes as
/// [`decode_string_literal`] does, or is its text as written with
/// [`Holes::AsWritten`].
pub(crate) fn decode_string_value(
    base: &BaseExtractor,
    node: &Node,
    holes: Holes,
) -> Option<String> {
    let kind = node.kind();
    if !(kind.contains("string") || kind.contains("char")) {
        return None;
    }
    let raw = base.get_node_text(node);
    let Some((open, close)) = quote_bounds(&raw).filter(|_| !kind.contains("raw")) else {
        return match holes {
            Holes::Braces => decode_string_literal(base, node),
            Holes::AsWritten => Some(raw.trim().to_string()),
        };
    };
    let mut value = LiteralValue {
        source: &base.content,
        holes,
        text_escapes: TextEscapes::of(&base.language, &raw[..open]),
        start: node.start_byte() + open,
        end: node.start_byte() + close,
        at: node.start_byte() + open,
        out: String::new(),
    };
    value.read(node, 0);
    value.push_text(value.end);
    Some(value.out)
}

/// How [`decode_string_value`] writes an interpolation hole.
#[derive(Clone, Copy)]
pub(crate) enum Holes {
    /// As `{}`.
    Braces,
    /// As the source writes it (`${name}`, `#{name}`).
    AsWritten,
}

/// The byte offsets of the start and the end of a literal's body: after an
/// optional prefix (`s`, `r`, `@`) and its opening quote, and before the
/// same quote at its end.
fn quote_bounds(raw: &str) -> Option<(usize, usize)> {
    let quote_at = raw.find(['"', '\'', '`'])?;
    if !raw[..quote_at]
        .chars()
        .all(|character| character.is_ascii_alphanumeric() || character == '@')
    {
        return None;
    }
    let rest = &raw[quote_at..];
    let quote = ["\"\"\"", "'''", "\"", "'", "`"]
        .into_iter()
        .find(|quote| rest.starts_with(quote))?;
    (rest.len() >= 2 * quote.len() && rest.ends_with(quote))
        .then(|| (quote_at + quote.len(), raw.len() - quote.len()))
}

/// The escapes that a grammar leaves inside plain text, with no escape node.
#[derive(Clone, Copy)]
enum TextEscapes {
    None,
    /// A Bash double-quoted string: `\` before `$`, `` ` ``, `"`, or `\`.
    BashDouble,
    /// A PowerShell expandable string: a backtick before a punctuation
    /// character, and `""`.
    PowerShellDouble,
    /// A PowerShell verbatim string: `''`.
    PowerShellSingle,
    /// A Ruby single-quoted string: `\'` and `\\`.
    RubySingle,
}

impl TextEscapes {
    fn of(language: &str, opening: &str) -> Self {
        let double = opening.ends_with('"');
        match language {
            "bash" if double => Self::BashDouble,
            "powershell" if double => Self::PowerShellDouble,
            "powershell" => Self::PowerShellSingle,
            "ruby" if opening.ends_with('\'') => Self::RubySingle,
            _ => Self::None,
        }
    }

    fn decode(self, text: &str, out: &mut String) {
        let mut characters = text.chars().peekable();
        while let Some(character) = characters.next() {
            let escaped = match (self, character, characters.peek().copied()) {
                (Self::BashDouble, '\\', Some(next @ ('$' | '`' | '"' | '\\')))
                | (Self::RubySingle, '\\', Some(next @ ('\'' | '\\')))
                | (Self::PowerShellDouble, '"', Some(next @ '"'))
                | (Self::PowerShellSingle, '\'', Some(next @ '\'')) => Some(next),
                (Self::PowerShellDouble, '`', Some(next)) if next.is_ascii_punctuation() => {
                    Some(next)
                }
                _ => None,
            };
            match escaped {
                Some(next) => {
                    characters.next();
                    out.push(next);
                }
                None => out.push(character),
            }
        }
    }
}

struct LiteralValue<'a> {
    source: &'a str,
    holes: Holes,
    text_escapes: TextEscapes,
    start: usize,
    end: usize,
    at: usize,
    out: String,
}

impl LiteralValue<'_> {
    fn read(&mut self, node: &Node, depth: u32) {
        if !should_visit_tree_depth(depth) {
            return;
        }
        let Some(child_depth) = child_tree_depth(depth) else {
            return;
        };
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            if child.end_byte() <= self.start || child.start_byte() >= self.end {
                continue;
            }
            let kind = child.kind();
            if matches!(kind, "escape_sequence" | "str_escaped_char") {
                self.push_text(child.start_byte());
                let text = self.source.get(child.byte_range()).unwrap_or_default();
                match decode_escape(text) {
                    Some(character) => self.out.push(character),
                    None => self.out.push_str(text),
                }
                self.at = child.end_byte();
            } else if is_interpolation_hole(kind) {
                let (start, end) = self.hole_range(&child);
                self.push_text(start);
                match self.holes {
                    Holes::Braces => self.out.push_str("{}"),
                    Holes::AsWritten => self
                        .out
                        .push_str(self.source.get(start..end).unwrap_or_default()),
                }
                self.at = end;
            } else {
                self.read(&child, child_depth);
            }
        }
    }

    /// The bytes of an interpolation hole with the delimiter tokens around it
    /// when the grammar leaves them outside the hole node (Swift `\(` and `)`).
    fn hole_range(&self, hole: &Node) -> (usize, usize) {
        let text = |node: Node| self.source.get(node.byte_range()).unwrap_or_default();
        let start = hole
            .prev_sibling()
            .filter(|token| !token.is_named() && token.end_byte() == hole.start_byte())
            .filter(|token| {
                let text = text(*token);
                text.starts_with(['\\', '$', '#']) && text.ends_with(['(', '{'])
            })
            .map_or(hole.start_byte(), |token| token.start_byte());
        let end = hole
            .next_sibling()
            .filter(|token| !token.is_named() && token.start_byte() == hole.end_byte())
            .filter(|token| matches!(text(*token), ")" | "}"))
            .filter(|_| start < hole.start_byte())
            .map_or(hole.end_byte(), |token| token.end_byte());
        (start.max(self.at), end.min(self.end))
    }

    fn push_text(&mut self, until: usize) {
        let until = until.min(self.end);
        if until > self.at {
            let text = self.source.get(self.at..until).unwrap_or_default();
            self.text_escapes.decode(text, &mut self.out);
            self.at = until;
        }
    }
}

/// The character a backslash escape writes, when it is a punctuation
/// character or a printable character given by its code.
fn decode_escape(text: &str) -> Option<char> {
    let body = text.strip_prefix('\\')?;
    let mut characters = body.chars();
    let first = characters.next()?;
    if first.is_ascii_punctuation() && characters.next().is_none() {
        return Some(first);
    }
    if !matches!(first, 'u' | 'U' | 'x') {
        return None;
    }
    let digits = body[1..].trim_start_matches('{').trim_end_matches('}');
    if digits.is_empty() || !digits.chars().all(|digit| digit.is_ascii_hexdigit()) {
        return None;
    }
    u32::from_str_radix(digits, 16)
        .ok()
        .and_then(char::from_u32)
        .filter(|character| !character.is_control())
}

fn strip_string_delimiters(raw: &str) -> String {
    let s = raw.trim();
    let Some(qpos) = s.find(['"', '\'', '`']) else {
        return s.to_string();
    };
    let s = &s[qpos..];
    for q in ["\"\"\"", "'''"] {
        if s.len() >= 2 * q.len() && s.starts_with(q) && s.ends_with(q) {
            return s[q.len()..s.len() - q.len()].to_string();
        }
    }
    let count = s.chars().count();
    if count >= 2 {
        let first = s.chars().next();
        let last = s.chars().last();
        if let (Some(f), Some(l)) = (first, last)
            && f == l
            && matches!(f, '"' | '\'' | '`')
        {
            return s.chars().skip(1).take(count - 2).collect();
        }
    }
    s.to_string()
}

fn decode_string_children(base: &BaseExtractor, node: &Node, out: &mut String, depth: u32) {
    if !should_visit_tree_depth(depth) {
        return;
    }
    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        let ck = child.kind();
        if ck.contains("content")
            || ck.contains("fragment")
            || ck.contains("text")
            || ck.contains("template_chars")
            || ck == "escape_sequence"
        {
            out.push_str(&base.get_node_text(&child));
        } else if is_interpolation_hole(ck) {
            out.push_str("{}");
        } else if child.named_child_count() > 0 {
            decode_string_children(base, &child, out, child_depth);
        }
    }
}

fn is_interpolation_hole(ck: &str) -> bool {
    if matches!(
        ck,
        "simple_expansion" | "expansion" | "arithmetic_expansion"
    ) {
        return true;
    }
    (ck.contains("interpolat") || ck.contains("substitution"))
        && !ck.ends_with("_start")
        && !ck.ends_with("_end")
        && !ck.ends_with("_quote")
        && !ck.ends_with("_brace")
        && !ck.ends_with("_open")
        && !ck.ends_with("_close")
}
