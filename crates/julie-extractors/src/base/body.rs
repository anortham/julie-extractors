use super::span::NormalizedSpan;
use tree_sitter::Node;

pub type BodySpan = NormalizedSpan;

const BODY_FIELD_NAMES: &[&str] = &["body"];

const BODY_NODE_KINDS: &[&str] = &[
    "block",
    "statement_block",
    "class_body",
    "declaration_list",
    "field_declaration_list",
    "enum_body",
    "trait_body",
    "interface_body",
    "object_body",
    "suite",
    "do_block",
];

pub(crate) fn infer_body_span(
    node: &Node,
    content: &str,
    line_starts: &[usize],
    declaration_span: NormalizedSpan,
) -> Option<BodySpan> {
    for field_name in BODY_FIELD_NAMES {
        if let Some(body) = node.child_by_field_name(field_name) {
            return Some(NormalizedSpan::from_node(&body));
        }
    }

    let mut cursor = node.walk();
    node.children(&mut cursor)
        .find(|child| BODY_NODE_KINDS.contains(&child.kind()))
        .map(|child| NormalizedSpan::from_node(&child))
        .or_else(|| {
            infer_body_span_from_span_with_line_starts(content, line_starts, declaration_span)
        })
}

pub(crate) fn body_hash(content: &str, span: BodySpan, language: &str) -> Option<String> {
    let source = content.get(span.start_byte as usize..span.end_byte as usize)?;
    let tokens = normalized_body_tokens(source, comment_syntax(language));
    let input = tokens.join("\u{1f}");
    Some(format!("{:x}", md5::compute(input.as_bytes())))
}

fn normalized_body_tokens(source: &str, comments: CommentSyntax) -> Vec<String> {
    let mut tokens = Vec::new();
    let chars: Vec<char> = source.chars().collect();
    let mut index = 0;

    while index < chars.len() {
        let ch = chars[index];
        if ch.is_whitespace() {
            index += 1;
            continue;
        }

        if let Some(next_index) = comment_end(&chars, index, comments) {
            index = next_index;
            continue;
        }

        if ch == '"' || ch == '\'' || ch == '`' {
            let (token, next_index) = quoted_token(&chars, index, ch);
            tokens.push(token);
            index = next_index;
            continue;
        }

        if is_word_char(ch) {
            let start = index;
            index += 1;
            while index < chars.len() && is_word_char(chars[index]) {
                index += 1;
            }
            tokens.push(chars[start..index].iter().collect());
            continue;
        }

        tokens.push(ch.to_string());
        index += 1;
    }

    tokens
}

#[derive(Clone, Copy)]
struct CommentSyntax {
    line: &'static [&'static str],
    block: &'static [(&'static str, &'static str)],
    vbnet_rem: bool,
}

fn comment_syntax(language: &str) -> CommentSyntax {
    match language {
        "bash" | "elixir" | "gdscript" | "powershell" | "python" | "r" | "ruby" | "toml"
        | "yaml" => CommentSyntax {
            line: &["#"],
            block: &[],
            vbnet_rem: false,
        },
        "css" => CommentSyntax {
            line: &[],
            block: &[("/*", "*/")],
            vbnet_rem: false,
        },
        "html" | "markdown" | "razor" => CommentSyntax {
            line: &[],
            block: &[("<!--", "-->")],
            vbnet_rem: false,
        },
        "lua" => CommentSyntax {
            line: &["--"],
            block: &[("--[[", "]]")],
            vbnet_rem: false,
        },
        "php" => CommentSyntax {
            line: &["//", "#"],
            block: &[("/*", "*/")],
            vbnet_rem: false,
        },
        "sql" => CommentSyntax {
            line: &["--"],
            block: &[("/*", "*/")],
            vbnet_rem: false,
        },
        "vbnet" => CommentSyntax {
            line: &["'"],
            block: &[],
            vbnet_rem: true,
        },
        "c" | "cpp" | "csharp" | "dart" | "go" | "java" | "javascript" | "json" | "jsonc"
        | "jsx" | "kotlin" | "qml" | "rust" | "scala" | "swift" | "tsx" | "typescript" | "vue"
        | "zig" => CommentSyntax {
            line: &["//"],
            block: &[("/*", "*/")],
            vbnet_rem: false,
        },
        _ => CommentSyntax {
            line: &[],
            block: &[],
            vbnet_rem: false,
        },
    }
}

fn comment_end(chars: &[char], index: usize, comments: CommentSyntax) -> Option<usize> {
    for (start, end) in comments.block {
        if starts_with(chars, index, start) {
            let mut next = index + start.chars().count();
            while next < chars.len() {
                if starts_with(chars, next, end) {
                    return Some(next + end.chars().count());
                }
                next += 1;
            }
            return Some(chars.len());
        }
    }

    if comments.vbnet_rem {
        if let Some(next) = vbnet_rem_comment_end(chars, index) {
            return Some(next);
        }
        if let Some(next) = vbnet_colon_comment_end(chars, index) {
            return Some(next);
        }
    }

    for start in comments.line {
        if starts_with(chars, index, start) {
            return Some(line_end(chars, index + start.chars().count()));
        }
    }

    None
}

fn vbnet_colon_comment_end(chars: &[char], index: usize) -> Option<usize> {
    if chars.get(index).copied() != Some(':') {
        return None;
    }

    let mut next = index + 1;
    while next < chars.len() && matches!(chars[next], ' ' | '\t') {
        next += 1;
    }

    if starts_with(chars, next, "'") || starts_vbnet_rem_comment(chars, next) {
        return Some(line_end(chars, next));
    }
    None
}

fn vbnet_rem_comment_end(chars: &[char], index: usize) -> Option<usize> {
    starts_vbnet_rem_comment(chars, index).then(|| line_end(chars, index + 3))
}

fn starts_vbnet_rem_comment(chars: &[char], index: usize) -> bool {
    let Some(candidate) = chars.get(index..index + 3) else {
        return false;
    };
    if !candidate
        .iter()
        .copied()
        .zip(['R', 'E', 'M'])
        .all(|(actual, expected)| actual.eq_ignore_ascii_case(&expected))
    {
        return false;
    }

    chars.get(index + 3).is_none_or(|next| !is_word_char(*next))
}

fn line_end(chars: &[char], mut index: usize) -> usize {
    while index < chars.len() && chars[index] != '\n' && chars[index] != '\r' {
        index += 1;
    }
    index
}

fn starts_with(chars: &[char], index: usize, pattern: &str) -> bool {
    let pattern_len = pattern.chars().count();
    chars
        .get(index..index + pattern_len)
        .is_some_and(|candidate| candidate.iter().copied().eq(pattern.chars()))
}

fn quoted_token(chars: &[char], start: usize, quote: char) -> (String, usize) {
    let mut index = start + 1;
    let mut escaped = false;

    while index < chars.len() {
        let ch = chars[index];
        index += 1;

        if escaped {
            escaped = false;
            continue;
        }
        if ch == '\\' {
            escaped = true;
            continue;
        }
        if ch == quote {
            break;
        }
    }

    (chars[start..index].iter().collect(), index)
}

fn is_word_char(ch: char) -> bool {
    ch == '_' || ch.is_alphanumeric()
}

pub(crate) fn infer_body_span_from_span(
    content: &str,
    declaration_span: NormalizedSpan,
) -> Option<BodySpan> {
    infer_body_span_from_span_with_line_starts(content, &[], declaration_span)
}

fn infer_body_span_from_span_with_line_starts(
    content: &str,
    line_starts: &[usize],
    declaration_span: NormalizedSpan,
) -> Option<BodySpan> {
    let start = declaration_span.start_byte as usize;
    let end = declaration_span.end_byte as usize;
    let source = content.get(start..end)?;

    brace_body_span(content, line_starts, declaration_span, source)
        .or_else(|| html_body_span(content, line_starts, declaration_span, source))
        .or_else(|| parenthesized_body_span(content, line_starts, declaration_span, source))
        .or_else(|| sql_as_body_span(content, line_starts, declaration_span, source))
        .or_else(|| keyword_body_span(content, line_starts, declaration_span, source))
}

fn brace_body_span(
    content: &str,
    line_starts: &[usize],
    declaration_span: NormalizedSpan,
    source: &str,
) -> Option<BodySpan> {
    let open = source.find('{')?;
    let close = source.rfind('}')?;
    if open >= close {
        return None;
    }
    span_for_relative_range(content, line_starts, declaration_span, open, close + 1)
}

fn html_body_span(
    content: &str,
    line_starts: &[usize],
    declaration_span: NormalizedSpan,
    source: &str,
) -> Option<BodySpan> {
    if !source.trim_start().starts_with('<') || !source.contains("</") {
        return None;
    }
    let open_end = source.find('>')? + 1;
    let close_start = source.rfind("</")?;
    if open_end > close_start {
        return None;
    }
    span_for_relative_range(
        content,
        line_starts,
        declaration_span,
        open_end,
        close_start,
    )
}

fn parenthesized_body_span(
    content: &str,
    line_starts: &[usize],
    declaration_span: NormalizedSpan,
    source: &str,
) -> Option<BodySpan> {
    let open = source.find('(')?;
    let close = source.rfind(')')?;
    if open >= close {
        return None;
    }
    span_for_relative_range(content, line_starts, declaration_span, open, close + 1)
}

fn sql_as_body_span(
    content: &str,
    line_starts: &[usize],
    declaration_span: NormalizedSpan,
    source: &str,
) -> Option<BodySpan> {
    let lower = source.to_ascii_lowercase();
    let as_index = lower.find(" as ")?;
    let body_start = as_index + " as ".len();
    if body_start >= source.len() {
        return None;
    }
    span_for_relative_range(
        content,
        line_starts,
        declaration_span,
        body_start,
        source.len(),
    )
}

fn keyword_body_span(
    content: &str,
    line_starts: &[usize],
    declaration_span: NormalizedSpan,
    source: &str,
) -> Option<BodySpan> {
    let lower = source.to_ascii_lowercase();
    if !lower.contains(" end")
        && !lower.ends_with("end")
        && !lower.contains("end ")
        && !lower.contains("end;")
    {
        return None;
    }

    let body_start = source.find('\n').map(|index| index + 1)?;
    if body_start >= source.len() {
        return None;
    }
    span_for_relative_range(
        content,
        line_starts,
        declaration_span,
        body_start,
        source.len(),
    )
}

fn span_for_relative_range(
    content: &str,
    line_starts: &[usize],
    declaration_span: NormalizedSpan,
    relative_start: usize,
    relative_end: usize,
) -> Option<BodySpan> {
    let absolute_start = declaration_span.start_byte as usize + relative_start;
    let absolute_end = declaration_span.start_byte as usize + relative_end;
    if absolute_start > absolute_end {
        return None;
    }
    NormalizedSpan::from_content_range_with_line_starts(
        content,
        line_starts,
        absolute_start,
        absolute_end,
    )
}
