//! CSS custom functions (`@function --name(...)`) and their dashed-function
//! calls (`--name(...)`).
//!
//! tree-sitter-css has no grammar for these, and it splits a dashed name that
//! holds a non-ASCII code point or an escape. So both are read from the source
//! with the CSS Syntax tokenizer rules: an ident code point is an ASCII letter,
//! digit, `-`, `_`, or any code point at or above U+0080 (as browsers parse
//! it), and a name compares after its escapes are decoded.

/// A dashed function token: a call, or the name in an `@function` prelude.
pub(super) struct DashedFunction {
    /// The name with its escapes decoded (`--foo\31 ` is `--foo1`).
    pub(super) name: String,
    /// The byte range of the raw name, without the `(`.
    pub(super) start_byte: usize,
    pub(super) end_byte: usize,
    /// The number of comma-separated arguments, or `None` when the argument
    /// list has no closing `)`.
    pub(super) arg_count: Option<usize>,
    pub(super) is_declaration: bool,
}

/// The token in `functions` (sorted by position) whose name overlaps the byte
/// range, such as the fragment of a dashed name that tree-sitter parsed as a
/// call of its own.
pub(super) fn overlapping(
    functions: &[DashedFunction],
    start_byte: usize,
    end_byte: usize,
) -> Option<&DashedFunction> {
    let index = functions.partition_point(|function| function.end_byte <= start_byte);
    functions
        .get(index)
        .filter(|function| function.start_byte < end_byte)
}

/// Every dashed function token in the stylesheet, in source order. Comments,
/// strings, and unquoted `url()` contents hold none.
pub(super) fn dashed_functions(content: &str) -> Vec<DashedFunction> {
    let bytes = content.as_bytes();
    let mut functions: Vec<DashedFunction> = Vec::new();
    let mut blocks: Vec<Block> = Vec::new();
    let mut after_function_keyword = false;
    let mut offset = 0;
    while offset < bytes.len() {
        let byte = bytes[offset];
        if byte == b'/' && bytes.get(offset + 1) == Some(&b'*') {
            offset = comment_end(bytes, offset);
            continue;
        }
        if is_css_whitespace(byte) {
            offset += 1;
            continue;
        }
        let at_function_name = std::mem::take(&mut after_function_keyword);
        if byte == b',' && blocks.last().is_some_and(|block| block.call.is_some()) {
            if let Some(call) = blocks.last_mut().and_then(|block| block.call.as_mut()) {
                call.commas += 1;
            }
            offset += 1;
            continue;
        }
        if matches!(byte, b')' | b']' | b'}')
            && blocks.last().is_some_and(|block| block.closer == byte)
        {
            if let Some(call) = blocks.pop().and_then(|block| block.call) {
                functions[call.function].arg_count = Some(call.count());
            }
            offset += 1;
            continue;
        }
        if let Some(call) = blocks.last_mut().and_then(|block| block.call.as_mut()) {
            call.has_content = true;
        }
        if byte == b'"' || byte == b'\'' {
            offset = string_end(bytes, offset);
            continue;
        }
        if byte == b'@' && starts_ident(content, offset + 1) {
            let end = ident_end(content, offset + 1);
            after_function_keyword =
                decode_css_escapes(&content[offset + 1..end]).eq_ignore_ascii_case("function");
            offset = end;
            continue;
        }
        if byte == b'#' && ident_code_point_at(content, offset + 1) {
            offset = ident_end(content, offset + 1);
            continue;
        }
        if starts_number(bytes, offset) {
            offset = number_end(content, offset);
            continue;
        }
        if starts_ident(content, offset) {
            let end = ident_end(content, offset);
            if bytes.get(end) != Some(&b'(') {
                offset = end;
                continue;
            }
            let name = decode_css_escapes(&content[offset..end]);
            if name.eq_ignore_ascii_case("url") && is_unquoted_url(bytes, end + 1) {
                offset = url_end(bytes, end + 1);
                continue;
            }
            let call = name.starts_with("--").then(|| {
                functions.push(DashedFunction {
                    name,
                    start_byte: offset,
                    end_byte: end,
                    arg_count: None,
                    is_declaration: at_function_name,
                });
                ArgumentCounter {
                    function: functions.len() - 1,
                    commas: 0,
                    has_content: false,
                }
            });
            blocks.push(Block { closer: b')', call });
            offset = end + 1;
            continue;
        }
        if let Some(closer) = closer_of(byte) {
            blocks.push(Block { closer, call: None });
        }
        offset += content[offset..].chars().next().map_or(1, char::len_utf8);
    }
    functions
}

/// An open `()`, `[]`, or `{}` block. Per CSS Syntax, only its own closer ends
/// it; any other closer inside it is an ordinary token.
struct Block {
    closer: u8,
    call: Option<ArgumentCounter>,
}

/// The top-level commas and content of a dashed function's argument list.
/// Commas inside nested blocks, strings, and comments do not separate
/// arguments, and an empty list has none.
struct ArgumentCounter {
    function: usize,
    commas: usize,
    has_content: bool,
}

impl ArgumentCounter {
    fn count(&self) -> usize {
        if self.commas == 0 && !self.has_content {
            0
        } else {
            self.commas + 1
        }
    }
}

fn closer_of(opener: u8) -> Option<u8> {
    match opener {
        b'(' => Some(b')'),
        b'[' => Some(b']'),
        b'{' => Some(b'}'),
        _ => None,
    }
}

/// The decoded name that an `@function` prelude declares, when the prelude
/// starting at `prelude_start` opens with a dashed function token.
pub(super) fn declared_function_name(content: &str, prelude_start: usize) -> Option<String> {
    let bytes = content.as_bytes();
    let mut offset = prelude_start;
    loop {
        match bytes.get(offset) {
            Some(byte) if is_css_whitespace(*byte) => offset += 1,
            Some(b'/') if bytes.get(offset + 1) == Some(&b'*') => {
                offset = comment_end(bytes, offset);
            }
            _ => break,
        }
    }
    if !starts_ident(content, offset) {
        return None;
    }
    let end = ident_end(content, offset);
    let name = decode_css_escapes(&content[offset..end]);
    (bytes.get(end) == Some(&b'(') && name.starts_with("--")).then_some(name)
}

/// Splits on CSS whitespace (space, tab, and newlines) only, so a non-ASCII
/// space such as U+2000 stays part of the text it is in.
pub(super) fn collapse_css_whitespace(text: &str) -> String {
    text.split(|character: char| character.is_ascii() && is_css_whitespace(character as u8))
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

/// Decodes CSS escapes: `\:` is `:`, and `\31 ` (1 to 6 hex digits plus one
/// optional whitespace) is the code point. Zero, a surrogate, a value above
/// U+10FFFF, and an escape at the end of the text decode to U+FFFD.
pub(super) fn decode_css_escapes(text: &str) -> String {
    let mut decoded = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            decoded.push(ch);
            continue;
        }
        let mut hex = String::new();
        while hex.len() < 6 && chars.peek().is_some_and(char::is_ascii_hexdigit) {
            hex.extend(chars.next());
        }
        if hex.is_empty() {
            decoded.push(chars.next().unwrap_or('\u{FFFD}'));
            continue;
        }
        if chars.next_if_eq(&'\r').is_some() {
            chars.next_if_eq(&'\n');
        } else {
            chars.next_if(|next| matches!(next, ' ' | '\t' | '\n' | '\u{c}'));
        }
        let code_point = u32::from_str_radix(&hex, 16).unwrap_or(0);
        decoded.push(
            char::from_u32(code_point)
                .filter(|character| *character != '\0')
                .unwrap_or('\u{FFFD}'),
        );
    }
    decoded
}

fn is_css_whitespace(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | b'\n' | b'\r' | 0x0C)
}

fn is_newline(byte: u8) -> bool {
    matches!(byte, b'\n' | b'\r' | 0x0C)
}

fn char_at(content: &str, offset: usize) -> Option<char> {
    content.get(offset..)?.chars().next()
}

fn ident_start_char(character: char) -> bool {
    character.is_ascii_alphabetic() || character == '_' || !character.is_ascii()
}

fn ident_char(character: char) -> bool {
    ident_start_char(character) || character.is_ascii_digit() || character == '-'
}

fn ident_code_point_at(content: &str, offset: usize) -> bool {
    char_at(content, offset).is_some_and(ident_char) || valid_escape(content.as_bytes(), offset)
}

fn valid_escape(bytes: &[u8], offset: usize) -> bool {
    bytes.get(offset) == Some(&b'\\') && !bytes.get(offset + 1).is_some_and(|b| is_newline(*b))
}

fn starts_ident(content: &str, offset: usize) -> bool {
    let bytes = content.as_bytes();
    match char_at(content, offset) {
        Some('-') => {
            char_at(content, offset + 1).is_some_and(|next| next == '-' || ident_start_char(next))
                || valid_escape(bytes, offset + 1)
        }
        Some('\\') => valid_escape(bytes, offset),
        Some(character) => ident_start_char(character),
        None => false,
    }
}

fn ident_end(content: &str, start: usize) -> usize {
    let bytes = content.as_bytes();
    let mut offset = start;
    loop {
        if valid_escape(bytes, offset) {
            offset = escape_end(content, offset);
            continue;
        }
        match char_at(content, offset) {
            Some(character) if ident_char(character) => offset += character.len_utf8(),
            _ => return offset,
        }
    }
}

fn escape_end(content: &str, slash: usize) -> usize {
    let bytes = content.as_bytes();
    let mut offset = slash + 1;
    let digits = bytes[offset..]
        .iter()
        .take(6)
        .take_while(|byte| byte.is_ascii_hexdigit())
        .count();
    if digits == 0 {
        return offset + char_at(content, offset).map_or(0, char::len_utf8);
    }
    offset += digits;
    match bytes.get(offset) {
        Some(b'\r') if bytes.get(offset + 1) == Some(&b'\n') => offset + 2,
        Some(byte) if is_css_whitespace(*byte) => offset + 1,
        _ => offset,
    }
}

fn starts_number(bytes: &[u8], offset: usize) -> bool {
    let digit_at = |index: usize| bytes.get(index).is_some_and(u8::is_ascii_digit);
    match bytes.get(offset) {
        Some(b'+' | b'-') => {
            digit_at(offset + 1) || (bytes.get(offset + 1) == Some(&b'.') && digit_at(offset + 2))
        }
        Some(b'.') => digit_at(offset + 1),
        Some(byte) => byte.is_ascii_digit(),
        None => false,
    }
}

fn number_end(content: &str, start: usize) -> usize {
    let bytes = content.as_bytes();
    let digits_from = |mut offset: usize| {
        while bytes.get(offset).is_some_and(u8::is_ascii_digit) {
            offset += 1;
        }
        offset
    };
    let mut offset = start;
    if matches!(bytes.get(offset), Some(b'+' | b'-')) {
        offset += 1;
    }
    offset = digits_from(offset);
    if bytes.get(offset) == Some(&b'.') && bytes.get(offset + 1).is_some_and(u8::is_ascii_digit) {
        offset = digits_from(offset + 1);
    }
    if matches!(bytes.get(offset), Some(b'e' | b'E')) {
        let sign = usize::from(matches!(bytes.get(offset + 1), Some(b'+' | b'-')));
        if bytes.get(offset + 1 + sign).is_some_and(u8::is_ascii_digit) {
            offset = digits_from(offset + 1 + sign);
        }
    }
    if starts_ident(content, offset) {
        ident_end(content, offset)
    } else if bytes.get(offset) == Some(&b'%') {
        offset + 1
    } else {
        offset
    }
}

fn comment_end(bytes: &[u8], start: usize) -> usize {
    bytes[start + 2..]
        .windows(2)
        .position(|pair| pair == b"*/")
        .map_or(bytes.len(), |position| start + 2 + position + 2)
}

fn string_end(bytes: &[u8], start: usize) -> usize {
    let quote = bytes[start];
    let mut offset = start + 1;
    while let Some(&byte) = bytes.get(offset) {
        match byte {
            b'\\' => offset += 2,
            byte if byte == quote => return offset + 1,
            byte if is_newline(byte) => return offset,
            _ => offset += 1,
        }
    }
    bytes.len()
}

fn is_unquoted_url(bytes: &[u8], after_paren: usize) -> bool {
    let first = bytes[after_paren.min(bytes.len())..]
        .iter()
        .find(|byte| !is_css_whitespace(**byte));
    !matches!(first, Some(b'"' | b'\''))
}

fn url_end(bytes: &[u8], after_paren: usize) -> usize {
    let mut offset = after_paren;
    while let Some(&byte) = bytes.get(offset) {
        match byte {
            b'\\' => offset += 2,
            b')' => return offset + 1,
            _ => offset += 1,
        }
    }
    bytes.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(content: &str) -> Vec<(String, bool, Option<usize>)> {
        dashed_functions(content)
            .into_iter()
            .map(|function| (function.name, function.is_declaration, function.arg_count))
            .collect()
    }

    #[test]
    fn escapes_decode_to_their_code_points() {
        assert_eq!(decode_css_escapes("--foo\\31 "), "--foo1");
        assert_eq!(decode_css_escapes("--a\\31\r\nb"), "--a1b");
        assert_eq!(decode_css_escapes("--escaped\\ name"), "--escaped name");
        assert_eq!(decode_css_escapes("--z\\0 "), "--z\u{FFFD}");
        assert_eq!(decode_css_escapes("--s\\D800 "), "--s\u{FFFD}");
        assert_eq!(decode_css_escapes("--end\\"), "--end\u{FFFD}");
    }

    #[test]
    fn declarations_and_calls_are_told_apart() {
        assert_eq!(
            names("@function --a(--x) { result: --b(1, --c()); }"),
            [
                ("--a".to_string(), true, Some(1)),
                ("--b".to_string(), false, Some(2)),
                ("--c".to_string(), false, Some(0)),
            ]
        );
        assert_eq!(
            names("@FUNCTION /* c */ --a() {}"),
            [("--a".to_string(), true, Some(0))]
        );
    }

    #[test]
    fn units_hashes_urls_and_unterminated_lists_are_handled() {
        assert_eq!(
            names("a { b: 2--x(1); c: #--y(1); e: url( --u(1).png); d: --z(1, 2"),
            [("--z".to_string(), false, None)]
        );
    }

    #[test]
    fn many_unclosed_calls_are_read_in_one_pass() {
        let functions = dashed_functions(&"a { b: --x(1, ".repeat(50_000));
        assert_eq!(functions.len(), 50_000);
        assert!(
            functions
                .iter()
                .all(|function| function.arg_count.is_none())
        );
    }

    #[test]
    fn whitespace_collapses_on_css_whitespace_only() {
        assert_eq!(
            collapse_css_whitespace("--\u{2000}_css()  \n returns"),
            "--\u{2000}_css() returns"
        );
    }

    #[test]
    fn a_declared_name_needs_an_open_paren() {
        assert_eq!(
            declared_function_name(" --foo\\31 ()", 0).as_deref(),
            Some("--foo1")
        );
        assert_eq!(declared_function_name(" --foo {", 0), None);
        assert_eq!(declared_function_name(" foo()", 0), None);
    }
}
