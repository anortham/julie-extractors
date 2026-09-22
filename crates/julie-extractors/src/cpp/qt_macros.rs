//! Qt's C++ macros (`Q_OBJECT`, `Q_PROPERTY`, `Q_SIGNALS:`, export macros) are
//! preprocessor names the C++ grammar cannot parse, so both parse paths blank
//! them to same-length spaces while the original text stays the source of record.
//!
//! The scan is token-level and never uses a tree: string, character and raw
//! string literals, `//` and `/* */` comments and preprocessor lines are left
//! alone.

pub(crate) const SIGNALS: &str = "signals";
pub(crate) const SLOTS: &str = "slots";

const PUBLIC_LABEL: &[u8] = b"public:";

/// A macro in a declaration prefix position may follow a run of these, so
/// `virtual Q_INVOKABLE QIcon icon() const;` is rewritten like a line-leading one.
const DECLARATION_SPECIFIERS: &[&str] = &[
    "virtual",
    "static",
    "inline",
    "explicit",
    "constexpr",
    "extern",
    "friend",
    "const",
    "volatile",
    "mutable",
    "template",
];

const TYPE_INTRODUCERS: &[&str] = &["class", "struct", "union", "enum"];

/// These stand in front of one member declaration, so they are prefix sites
/// whatever follows them on the line.
const MEMBER_PREFIX_MACROS: &[&str] = &[
    "Q_INVOKABLE",
    "Q_NOREPLY",
    "Q_REQUIRED_RESULT",
    "Q_SCRIPTABLE",
    "Q_SIGNAL",
    "Q_SLOT",
];

const RUNTIME_MACROS: &[&str] = &[
    "Q_ASSERT",
    "Q_ASSERT_X",
    "Q_CHECK_PTR",
    "Q_ASSUME",
    "Q_LIKELY",
    "Q_UNLIKELY",
    "Q_UNREACHABLE",
];

/// Vendor macros outside the Qt vocabulary are rewritten only by name, because a
/// line-leading all-caps identifier is also how Catch2 and gtest declare a test.
const VENDOR_STATEMENT_MACROS: &[&str] = &[
    "DBUS_QML_TYPE",
    "K_PLUGIN_CLASS",
    "K_PLUGIN_CLASS_WITH_JSON",
    "K_PLUGIN_FACTORY",
    "K_PLUGIN_FACTORY_WITH_JSON",
    "QTEST_APPLESS_MAIN",
    "QTEST_GUILESS_MAIN",
    "QTEST_MAIN",
    "QUICK_TEST_MAIN",
    "QUICK_TEST_MAIN_WITH_SETUP",
];

const RAW_STRING_PREFIXES: &[&[u8]] = &[b"R", b"LR", b"uR", b"UR", b"u8R"];
const ENCODING_PREFIXES: &[&[u8]] = &[b"", b"L", b"u", b"U", b"u8"];
const MAX_RAW_DELIMITER: usize = 16;

const DEPRECATION_MARKER: &str = "_DEPRECATED";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MacroKind {
    Statement,
    Prefix,
    Section,
    Export,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MacroSite {
    pub(crate) kind: MacroKind,
    pub(crate) name: String,
    pub(crate) arguments: Option<String>,
    pub(crate) start_byte: usize,
    pub(crate) end_byte: usize,
    pub(crate) line: usize,
}

/// `None` when the source carries no macro site, so the ordinary C++ path
/// parses the original text without an extra allocation.
pub(crate) fn blank_macros(content: &str) -> Option<String> {
    let sites = scan(content);
    if sites.is_empty() {
        return None;
    }

    let mut bytes = content.as_bytes().to_vec();
    for site in &sites {
        if site.name == "Q_UNUSED" && site.arguments.is_some() {
            let name_end = site.start_byte + site.name.len();
            let open = argument_list_start(content.as_bytes(), name_end)
                .expect("Q_UNUSED site has an argument list");
            for byte in &mut bytes[site.start_byte..=open] {
                if *byte != b'\n' && *byte != b'\r' {
                    *byte = b' ';
                }
            }
            bytes[site.end_byte - 1] = b';';
            continue;
        }
        for byte in &mut bytes[site.start_byte..site.end_byte] {
            if *byte != b'\n' && *byte != b'\r' {
                *byte = b' ';
            }
        }
        if writes_public_label(site) {
            bytes[site.start_byte..site.start_byte + PUBLIC_LABEL.len()]
                .copy_from_slice(PUBLIC_LABEL);
        }
    }
    String::from_utf8(bytes).ok()
}

pub(crate) fn scan(content: &str) -> Vec<MacroSite> {
    let bytes = content.as_bytes();
    let line_starts = line_starts(bytes);
    let mut sites = Vec::new();
    let mut cursor = 0;
    let mut consumed = 0;
    let mut brace_depth = 0usize;
    let mut class_brace_depths = Vec::new();
    let mut class_pending = false;

    while cursor < bytes.len() {
        match bytes[cursor] {
            b'/' if bytes.get(cursor + 1) == Some(&b'/') => {
                cursor = end_of_logical_line(bytes, cursor);
                consumed = cursor;
            }
            b'/' if bytes.get(cursor + 1) == Some(&b'*') => {
                cursor = end_of_block_comment(bytes, cursor + 2);
                consumed = cursor;
            }
            b'"' | b'\'' => {
                cursor = end_of_literal_token(bytes, cursor).unwrap_or(cursor + 1);
            }
            byte if byte.is_ascii_digit() => cursor = end_of_numeric_literal(bytes, cursor),
            b'{' => {
                brace_depth += 1;
                if class_pending {
                    class_brace_depths.push(brace_depth);
                }
                class_pending = false;
                cursor += 1;
            }
            b'}' => {
                if class_brace_depths.last() == Some(&brace_depth) {
                    class_brace_depths.pop();
                }
                brace_depth = brace_depth.saturating_sub(1);
                cursor += 1;
            }
            b';' => {
                class_pending = false;
                cursor += 1;
            }
            b'#' if line_prefix(
                content,
                line_starts[line_of(&line_starts, cursor) - 1],
                consumed,
                cursor,
            )
            .is_empty() =>
            {
                cursor = end_of_logical_line(bytes, cursor);
            }
            byte if is_identifier_start(byte) => {
                let line = line_of(&line_starts, cursor);
                let prefix = line_prefix(content, line_starts[line - 1], consumed, cursor);
                let (next, site) = identifier(
                    content,
                    bytes,
                    &line_starts,
                    consumed,
                    cursor,
                    class_brace_depths.last() == Some(&brace_depth),
                );
                if let Some(site) = site {
                    consumed = site.end_byte;
                    sites.push(site);
                }
                if class_body_can_follow(prefix)
                    && matches!(&content[cursor..next], "class" | "struct" | "union")
                {
                    class_pending = true;
                }
                cursor = next;
            }
            _ => cursor += 1,
        }
    }

    sites
}

/// A bare `Q_SIGNALS:` or `signals:` label is replaced by `public:` because
/// everything after it is public in Qt; a bare `Q_SLOTS:` expands to nothing.
fn writes_public_label(site: &MacroSite) -> bool {
    site.kind == MacroKind::Section
        && site.name == SIGNALS
        && site.arguments.is_none()
        && site.end_byte - site.start_byte >= PUBLIC_LABEL.len()
}

fn identifier(
    content: &str,
    bytes: &[u8],
    line_starts: &[usize],
    consumed: usize,
    start: usize,
    inside_class_member: bool,
) -> (usize, Option<MacroSite>) {
    if let Some(after_literal) = end_of_literal_token(bytes, start) {
        return (after_literal, None);
    }

    let end = end_of_identifier(bytes, start);
    let word = &content[start..end];

    let line = line_of(line_starts, start);
    let prefix = line_prefix(content, line_starts[line - 1], consumed, start);
    let site = |kind, name: &str, arguments, end_byte| {
        Some(MacroSite {
            kind,
            name: name.to_string(),
            arguments,
            start_byte: start,
            end_byte,
            line,
        })
    };

    if let Some(section) = section_name(word) {
        if matches!(word, "signals" | "slots") && !inside_class_member {
            return (end, None);
        }
        let access = match prefix {
            "" => None,
            "public" | "private" | "protected" => Some(prefix.to_string()),
            _ => return (end, None),
        };
        if let Some(colon) = section_colon(bytes, end) {
            let end_byte = if access.is_some() { end } else { colon + 1 };
            return (
                colon + 1,
                site(MacroKind::Section, section, access, end_byte),
            );
        }
        return (end, None);
    }

    if is_declaration_attribute_macro(word) {
        let (end_byte, arguments, kind) = macro_extent(content, bytes, end);
        return (end_byte, site(kind, word, arguments, end_byte));
    }

    if word == "Q_UNUSED" {
        let (end_byte, arguments, kind) = macro_extent(content, bytes, end);
        return (end_byte, site(kind, word, arguments, end_byte));
    }

    if word == "Q_NULLPTR" {
        return (end, None);
    }

    if RUNTIME_MACROS.contains(&word) {
        return (end, None);
    }

    if is_specifier_prefix(prefix) && is_qt_macro_name(word) {
        let (end_byte, arguments, kind) = macro_extent(content, bytes, end);
        if MEMBER_PREFIX_MACROS.contains(&word) {
            return (end_byte, site(MacroKind::Prefix, word, arguments, end_byte));
        }
        if kind == MacroKind::Prefix && !declaration_follows(bytes, end_byte) {
            return (end, None);
        }
        return (end_byte, site(kind, word, arguments, end_byte));
    }

    if is_specifier_prefix(prefix) && VENDOR_STATEMENT_MACROS.contains(&word) {
        let (end_byte, arguments, kind) = macro_extent(content, bytes, end);
        if kind == MacroKind::Statement {
            return (end_byte, site(kind, word, arguments, end_byte));
        }
        return (end, None);
    }

    if is_specifier_prefix(prefix) && is_deprecation_macro_name(word) {
        let (end_byte, arguments, kind) = macro_extent(content, bytes, end);
        if declaration_follows(bytes, end_byte) {
            return (end_byte, site(kind, word, arguments, end_byte));
        }
        return (end, None);
    }

    if prefix.is_empty() && word == "emit" && precedes_identifier(bytes, end) {
        return (end, site(MacroKind::Prefix, word, None, end));
    }

    if is_declaration_prefix(prefix)
        && is_export_macro_name(word)
        && precedes_identifier(bytes, end)
    {
        return (end, site(MacroKind::Export, word, None, end));
    }

    if is_class_key_prefix(prefix)
        && is_visibility_macro_name(word)
        && precedes_class_head(bytes, end)
    {
        return (end, site(MacroKind::Export, word, None, end));
    }

    (end, None)
}

/// `class ENGINE_API Renderer : Base {` and `struct DLL_PUBLIC Point {`: a
/// visibility macro sits between the class key and the class name.
fn is_class_key_prefix(prefix: &str) -> bool {
    prefix
        .split_whitespace()
        .last()
        .is_some_and(|word| matches!(word, "class" | "struct" | "union"))
}

fn is_visibility_macro_name(word: &str) -> bool {
    word.len() > 1
        && word.as_bytes().first().is_some_and(u8::is_ascii_uppercase)
        && word.bytes().all(is_macro_body_byte)
}

/// An identifier, then the `:` of a base clause or the `{` of the body.
fn precedes_class_head(bytes: &[u8], from: usize) -> bool {
    if !precedes_identifier(bytes, from) {
        return false;
    }
    let name_start = skip_blanks(bytes, from);
    let name_end = name_start
        + bytes[name_start..]
            .iter()
            .take_while(|byte| byte.is_ascii_alphanumeric() || **byte == b'_')
            .count();
    let next = name_end
        + bytes[name_end..]
            .iter()
            .take_while(|byte| byte.is_ascii_whitespace())
            .count();
    match bytes.get(next) {
        Some(b'{') => true,
        Some(b':') => bytes.get(next + 1) != Some(&b':'),
        _ => false,
    }
}

fn section_name(word: &str) -> Option<&'static str> {
    match word {
        "Q_SIGNALS" | "signals" => Some(SIGNALS),
        "Q_SLOTS" | "slots" => Some(SLOTS),
        _ => None,
    }
}

/// A section label ends in a single `:`; `Q_SLOTS::x` is not a label.
fn section_colon(bytes: &[u8], from: usize) -> Option<usize> {
    let colon = skip_blanks(bytes, from);
    (bytes.get(colon) == Some(&b':') && bytes.get(colon + 1) != Some(&b':')).then_some(colon)
}

fn is_qt_macro_name(word: &str) -> bool {
    word.strip_prefix("QML_")
        .or_else(|| word.strip_prefix("QT_"))
        .or_else(|| word.strip_prefix("Q_"))
        .is_some_and(|rest| !rest.is_empty() && rest.bytes().all(is_macro_body_byte))
}

/// `Q_DECL_*` expands to a keyword, an attribute, `= default`, `= delete` or
/// nothing, so it is blanked wherever it stands.
fn is_declaration_attribute_macro(word: &str) -> bool {
    word.strip_prefix("Q_DECL_")
        .is_some_and(|rest| !rest.is_empty() && rest.bytes().all(is_macro_body_byte))
}

fn is_deprecation_macro_name(word: &str) -> bool {
    word.contains(DEPRECATION_MARKER)
        && word.as_bytes().first().is_some_and(u8::is_ascii_uppercase)
        && word.bytes().all(is_macro_body_byte)
}

/// The line prefix a `Prefix` or `Statement` macro may carry: nothing, or a run
/// of declaration specifiers.
fn is_specifier_prefix(prefix: &str) -> bool {
    prefix
        .split_whitespace()
        .all(|word| DECLARATION_SPECIFIERS.contains(&word))
}

/// The line prefix an export macro may carry: a specifier run, optionally ending
/// in the `class`, `struct`, `union` or `enum class` that introduces the type.
fn is_declaration_prefix(prefix: &str) -> bool {
    prefix
        .split_whitespace()
        .all(|word| DECLARATION_SPECIFIERS.contains(&word) || TYPE_INTRODUCERS.contains(&word))
}

fn is_export_macro_name(word: &str) -> bool {
    let Some(head) = word.strip_suffix("_EXPORT") else {
        return false;
    };
    let head = head.strip_suffix("_NO").unwrap_or(head);
    head.as_bytes().first().is_some_and(u8::is_ascii_uppercase)
        && head.bytes().all(is_macro_body_byte)
}

fn is_macro_body_byte(byte: u8) -> bool {
    byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_'
}

fn macro_extent(content: &str, bytes: &[u8], end: usize) -> (usize, Option<String>, MacroKind) {
    let (end_byte, arguments) = match argument_list_start(bytes, end)
        .and_then(|open| matching_paren(bytes, open).map(|close| (open, close)))
    {
        Some((open, close)) => (close + 1, Some(content[open + 1..close].to_string())),
        None => (end, None),
    };
    let kind = if line_tail_is_terminal(bytes, end_byte) {
        MacroKind::Statement
    } else {
        MacroKind::Prefix
    };
    (end_byte, arguments, kind)
}

fn argument_list_start(bytes: &[u8], from: usize) -> Option<usize> {
    let open = skip_trivia(bytes, from);
    (bytes.get(open) == Some(&b'(')).then_some(open)
}

/// Whitespace, newlines and comments between a macro name and its argument list.
fn skip_trivia(bytes: &[u8], from: usize) -> usize {
    let mut cursor = from;
    loop {
        match bytes.get(cursor) {
            Some(b' ' | b'\t' | b'\n' | b'\r') => cursor += 1,
            Some(b'/') if bytes.get(cursor + 1) == Some(&b'/') => {
                cursor = end_of_logical_line(bytes, cursor);
            }
            Some(b'/') if bytes.get(cursor + 1) == Some(&b'*') => {
                cursor = end_of_block_comment(bytes, cursor + 2);
            }
            _ => return cursor,
        }
    }
}

fn matching_paren(bytes: &[u8], open: usize) -> Option<usize> {
    let mut depth = 0usize;
    let mut cursor = open;
    while cursor < bytes.len() {
        match bytes[cursor] {
            b'(' => {
                depth += 1;
                cursor += 1;
            }
            b')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(cursor);
                }
                cursor += 1;
            }
            b'/' if bytes.get(cursor + 1) == Some(&b'/') => {
                cursor = end_of_logical_line(bytes, cursor);
            }
            b'/' if bytes.get(cursor + 1) == Some(&b'*') => {
                cursor = end_of_block_comment(bytes, cursor + 2);
            }
            byte if byte.is_ascii_digit() => cursor = end_of_numeric_literal(bytes, cursor),
            byte if byte == b'"' || byte == b'\'' || is_identifier_start(byte) => {
                cursor = end_of_literal_token(bytes, cursor)
                    .unwrap_or_else(|| end_of_identifier(bytes, cursor).max(cursor + 1));
            }
            _ => cursor += 1,
        }
    }
    None
}

/// A statement macro owns its whole line: nothing follows it but a `;` or a comment.
fn line_tail_is_terminal(bytes: &[u8], from: usize) -> bool {
    let mut cursor = skip_blanks(bytes, from);
    if bytes.get(cursor) == Some(&b';') {
        cursor = skip_blanks(bytes, cursor + 1);
    }
    match bytes.get(cursor) {
        None | Some(b'\n') | Some(b'\r') => true,
        Some(b'/') => matches!(bytes.get(cursor + 1), Some(b'/' | b'*')),
        _ => false,
    }
}

/// A vendor deprecation macro prefixes a declaration; an enumerator that ends in
/// `_DEPRECATED` is followed by `=`, `,` or `}` instead.
fn declaration_follows(bytes: &[u8], from: usize) -> bool {
    line_tail_is_terminal(bytes, from)
        || bytes
            .get(skip_blanks(bytes, from))
            .copied()
            .is_some_and(is_identifier_start)
}

fn precedes_identifier(bytes: &[u8], from: usize) -> bool {
    matches!(bytes.get(from), Some(b' ' | b'\t'))
        && bytes
            .get(skip_blanks(bytes, from))
            .copied()
            .is_some_and(is_identifier_start)
}

/// The text before `index` on its line that the scan has not consumed yet. A
/// blanked macro site and a comment count as blanks, so any run of specifiers
/// and recognized macros is still a declaration prefix.
fn line_prefix(content: &str, line_start: usize, consumed: usize, index: usize) -> &str {
    content[line_start.max(consumed.min(index))..index].trim()
}

fn class_body_can_follow(prefix: &str) -> bool {
    prefix.is_empty()
        || matches!(prefix.as_bytes().last(), Some(b'{' | b'}' | b';' | b'>'))
        || matches!(prefix, "export" | "typedef")
}

fn skip_blanks(bytes: &[u8], from: usize) -> usize {
    let mut cursor = from;
    while matches!(bytes.get(cursor), Some(b' ' | b'\t')) {
        cursor += 1;
    }
    cursor
}

fn end_of_line(bytes: &[u8], from: usize) -> usize {
    bytes[from..]
        .iter()
        .position(|byte| *byte == b'\n')
        .map_or(bytes.len(), |at| from + at)
}

fn end_of_block_comment(bytes: &[u8], from: usize) -> usize {
    let mut cursor = from;
    while cursor + 1 < bytes.len() {
        if bytes[cursor] == b'*' && bytes[cursor + 1] == b'/' {
            return cursor + 2;
        }
        cursor += 1;
    }
    bytes.len()
}

/// The end of a string, character or raw-string literal that starts at `start`,
/// its encoding prefix included, or `None` when no literal starts there. The
/// outer scan and the macro argument balancing share it, so a raw string never
/// closes a macro early.
fn end_of_literal_token(bytes: &[u8], start: usize) -> Option<usize> {
    let quote = end_of_identifier(bytes, start);
    let prefix = &bytes[start..quote];
    match bytes.get(quote) {
        Some(b'"') if RAW_STRING_PREFIXES.contains(&prefix) => {
            Some(end_of_raw_string(bytes, quote))
        }
        Some(b'"' | b'\'') if ENCODING_PREFIXES.contains(&prefix) => {
            Some(end_of_literal(bytes, quote))
        }
        _ => None,
    }
}

fn end_of_identifier(bytes: &[u8], from: usize) -> usize {
    let mut cursor = from;
    while cursor < bytes.len() && is_identifier_byte(bytes[cursor]) {
        cursor += 1;
    }
    cursor
}

fn end_of_literal(bytes: &[u8], start: usize) -> usize {
    let quote = bytes[start];
    let mut cursor = start + 1;
    while cursor < bytes.len() {
        match bytes[cursor] {
            b'\\' => cursor += 2,
            b'\n' => return cursor,
            byte if byte == quote => return cursor + 1,
            _ => cursor += 1,
        }
    }
    bytes.len()
}

/// A raw string delimiter is at most 16 characters, so a `R"` with no `(` after
/// it is a malformed literal and the scan treats it as an ordinary one.
fn end_of_raw_string(bytes: &[u8], quote: usize) -> usize {
    let open = quote + 1;
    let limit = bytes.len().min(open + MAX_RAW_DELIMITER + 1);
    let Some(paren) = bytes[open.min(limit)..limit]
        .iter()
        .position(|byte| *byte == b'(')
        .map(|at| open + at)
    else {
        return end_of_literal(bytes, quote);
    };
    let mut terminator = Vec::with_capacity(paren - open + 2);
    terminator.push(b')');
    terminator.extend_from_slice(&bytes[open..paren]);
    terminator.push(b'"');
    let mut cursor = paren + 1;
    while cursor + terminator.len() <= bytes.len() {
        if bytes[cursor..cursor + terminator.len()] == terminator[..] {
            return cursor + terminator.len();
        }
        cursor += 1;
    }
    bytes.len()
}

/// A preprocessor line and a `//` comment both continue onto the next physical
/// line when the line ends in a backslash.
fn end_of_logical_line(bytes: &[u8], from: usize) -> usize {
    let mut cursor = from;
    loop {
        let line_end = end_of_line(bytes, cursor);
        let mut tail = line_end;
        while tail > cursor && matches!(bytes[tail - 1], b' ' | b'\t' | b'\r') {
            tail -= 1;
        }
        if line_end >= bytes.len() || tail == cursor || bytes[tail - 1] != b'\\' {
            return line_end;
        }
        cursor = line_end + 1;
    }
}

fn line_starts(bytes: &[u8]) -> Vec<usize> {
    let mut starts = vec![0];
    starts.extend(
        bytes
            .iter()
            .enumerate()
            .filter(|(_, byte)| **byte == b'\n')
            .map(|(at, _)| at + 1),
    );
    starts
}

fn line_of(line_starts: &[usize], byte: usize) -> usize {
    line_starts.partition_point(|start| *start <= byte)
}

fn is_identifier_start(byte: u8) -> bool {
    byte.is_ascii_alphabetic() || byte == b'_'
}

fn is_identifier_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

fn end_of_numeric_literal(bytes: &[u8], from: usize) -> usize {
    let mut cursor = from;
    while bytes
        .get(cursor)
        .is_some_and(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'\'' | b'.'))
    {
        cursor += 1;
    }
    cursor
}
