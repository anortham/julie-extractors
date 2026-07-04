use tree_sitter::{Node, Tree};

use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};

pub(super) struct ScriptSyntaxMask {
    flags: Vec<bool>,
}

impl ScriptSyntaxMask {
    pub(super) fn for_js_ranges(content: &str, ranges: &[(usize, usize)]) -> Self {
        let mut flags = vec![false; content.len()];
        for (start, end) in ranges {
            mask_js_range(content, &mut flags, *start, *end);
        }
        Self { flags }
    }

    pub(super) fn is_ignored(&self, index: usize) -> bool {
        self.flags.get(index).copied().unwrap_or(false)
    }
}

fn mask_js_range(content: &str, flags: &mut [bool], start: usize, end: usize) {
    let bytes = content.as_bytes();
    let mut cursor = start;
    let mut line_comment = false;
    let mut block_comment = false;
    let mut quote: Option<u8> = None;

    while cursor < end {
        let byte = bytes[cursor];
        let next = bytes.get(cursor + 1).copied();
        flags[cursor] = line_comment || block_comment || quote.is_some();

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
                if cursor + 1 < end {
                    flags[cursor + 1] = true;
                }
                cursor += 2;
            } else {
                cursor += 1;
            }
            continue;
        }
        if let Some(active) = quote {
            if byte == b'\\' {
                if cursor + 1 < end {
                    flags[cursor + 1] = true;
                }
                cursor += 2;
                continue;
            }
            if byte == active {
                quote = None;
            }
            cursor += 1;
            continue;
        }

        if byte == b'/' && next == Some(b'/') {
            line_comment = true;
            if cursor + 1 < end {
                flags[cursor + 1] = true;
            }
            cursor += 2;
            continue;
        }
        if byte == b'/' && next == Some(b'*') {
            block_comment = true;
            if cursor + 1 < end {
                flags[cursor + 1] = true;
            }
            cursor += 2;
            continue;
        }
        if byte == b'/'
            && next != Some(b'/')
            && next != Some(b'*')
            && is_regex_literal_context(bytes, cursor)
            && let Some(regex_end) = regex_literal_end(bytes, cursor, end)
        {
            for flag in flags.iter_mut().take(regex_end + 1).skip(cursor + 1) {
                *flag = true;
            }
            cursor = regex_end + 1;
            while cursor < end && bytes[cursor].is_ascii_alphabetic() {
                flags[cursor] = true;
                cursor += 1;
            }
            continue;
        }
        if matches!(byte, b'\'' | b'"' | b'`') {
            quote = Some(byte);
        }
        cursor += 1;
    }
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

fn regex_literal_end(bytes: &[u8], slash: usize, end: usize) -> Option<usize> {
    let mut cursor = slash + 1;
    let mut in_class = false;
    while cursor < end {
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

pub(super) fn parse_object_string_property(
    content: &str,
    start: usize,
    end: usize,
    property_name: &str,
) -> Option<String> {
    let value_start = find_object_property_value_start(content, start, end, property_name)?;
    let (value, value_end) = parse_js_string_literal(content, value_start)?;
    (value_end <= end).then_some(value)
}

pub(super) fn parse_object_identifier_property(
    content: &str,
    start: usize,
    end: usize,
    property_name: &str,
) -> Option<String> {
    let value_start = find_object_property_value_start(content, start, end, property_name)?;
    let (identifier, _) = parse_js_identifier(content, value_start, end)?;
    Some(identifier)
}

pub(super) fn find_object_property_value_start(
    content: &str,
    start: usize,
    end: usize,
    property_name: &str,
) -> Option<usize> {
    let mut cursor = start;
    while cursor < end {
        let Some(relative_start) = content[cursor..end].find(property_name) else {
            break;
        };
        let property_start = cursor + relative_start;
        cursor = property_start + property_name.len();
        if !is_identifier_boundary(content, property_start, property_name.len()) {
            continue;
        }
        let colon = skip_ascii_whitespace_until(content, cursor, end);
        if content.as_bytes().get(colon) != Some(&b':') {
            continue;
        }
        return Some(skip_ascii_whitespace_until(content, colon + 1, end));
    }
    None
}

pub(in crate::base) fn parse_js_string_literal(
    content: &str,
    start: usize,
) -> Option<(String, usize)> {
    let bytes = content.as_bytes();
    let quote = bytes
        .get(start)
        .copied()
        .filter(|byte| matches!(*byte, b'\'' | b'"'))?;
    let mut cursor = start + 1;
    let mut value = String::new();

    while cursor < content.len() {
        let byte = bytes[cursor];
        if byte == b'\\' {
            let escaped_start = cursor + 1;
            let escaped = *bytes.get(escaped_start)?;
            match escaped {
                b'n' => value.push('\n'),
                b't' => value.push('\t'),
                b'r' => value.push('\r'),
                b'b' => value.push('\u{0008}'),
                b'f' => value.push('\u{000c}'),
                b'v' => value.push('\u{000b}'),
                b'0' => value.push('\0'),
                b'\'' => value.push('\''),
                b'"' => value.push('"'),
                b'\\' => value.push('\\'),
                b'/' => value.push('/'),
                b'u' => {
                    let hex_start = escaped_start + 1;
                    let hex = content.get(hex_start..hex_start + 4)?;
                    let codepoint = parse_four_hex_digits(hex)?;
                    value.push(char::from_u32(codepoint)?);
                    cursor = hex_start + 4;
                    continue;
                }
                _ => {
                    let escaped = content.get(escaped_start..)?.chars().next()?;
                    value.push(escaped);
                    cursor = escaped_start + escaped.len_utf8();
                    continue;
                }
            }
            cursor = escaped_start + 1;
        } else if byte == quote {
            return Some((value, cursor + 1));
        } else if matches!(byte, b'\n' | b'\r') {
            return None;
        } else {
            let ch = content.get(cursor..)?.chars().next()?;
            value.push(ch);
            cursor += ch.len_utf8();
        }
    }

    None
}

fn parse_four_hex_digits(hex: &str) -> Option<u32> {
    if hex.len() != 4 {
        return None;
    }
    hex.bytes().try_fold(0u32, |value, byte| {
        let digit = match byte {
            b'0'..=b'9' => u32::from(byte - b'0'),
            b'a'..=b'f' => u32::from(byte - b'a' + 10),
            b'A'..=b'F' => u32::from(byte - b'A' + 10),
            _ => return None,
        };
        Some((value << 4) | digit)
    })
}

pub(in crate::base) fn parse_js_identifier(
    content: &str,
    start: usize,
    end: usize,
) -> Option<(String, usize)> {
    let bytes = content.as_bytes();
    let first = *bytes.get(start)?;
    if !is_js_identifier_start_byte(first) {
        return None;
    }
    let mut cursor = start + 1;
    while cursor < end
        && bytes
            .get(cursor)
            .is_some_and(|byte| is_js_identifier_byte(*byte))
    {
        cursor += 1;
    }
    Some((content.get(start..cursor)?.to_string(), cursor))
}

pub(in crate::base) fn is_js_identifier(value: &str) -> bool {
    let mut bytes = value.bytes();
    let Some(first) = bytes.next() else {
        return false;
    };
    is_js_identifier_start_byte(first) && bytes.all(is_js_identifier_byte)
}

pub(super) fn is_identifier_boundary(content: &str, start: usize, len: usize) -> bool {
    let bytes = content.as_bytes();
    let before = start.checked_sub(1).and_then(|index| bytes.get(index));
    let after = bytes.get(start + len);
    !before.is_some_and(|byte| is_js_identifier_byte(*byte))
        && !after.is_some_and(|byte| is_js_identifier_byte(*byte))
}

fn is_js_identifier_start_byte(byte: u8) -> bool {
    byte == b'_' || byte == b'$' || byte.is_ascii_alphabetic()
}

fn is_js_identifier_byte(byte: u8) -> bool {
    is_js_identifier_start_byte(byte) || byte.is_ascii_digit()
}

pub(super) fn is_ignored_syntax_range(tree: &Tree, start_byte: usize, end_byte: usize) -> bool {
    smallest_node_covering_range(tree.root_node(), start_byte, end_byte)
        .is_some_and(|node| node_or_parent_is_comment_or_string(node))
}

fn smallest_node_covering_range<'tree>(
    node: Node<'tree>,
    start_byte: usize,
    end_byte: usize,
) -> Option<Node<'tree>> {
    smallest_node_covering_range_at_depth(node, start_byte, end_byte, 0)
}

fn smallest_node_covering_range_at_depth<'tree>(
    node: Node<'tree>,
    start_byte: usize,
    end_byte: usize,
    depth: u32,
) -> Option<Node<'tree>> {
    if !should_visit_tree_depth(depth) {
        return None;
    }
    if node.start_byte() > start_byte || node.end_byte() < end_byte {
        return None;
    }

    let Some(child_depth) = child_tree_depth(depth) else {
        return Some(node);
    };
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if let Some(found) =
            smallest_node_covering_range_at_depth(child, start_byte, end_byte, child_depth)
        {
            return Some(found);
        }
    }

    Some(node)
}

fn node_or_parent_is_comment_or_string(mut node: Node<'_>) -> bool {
    loop {
        if is_comment_or_string_node(node.kind()) {
            return true;
        }
        let Some(parent) = node.parent() else {
            return false;
        };
        node = parent;
    }
}

fn is_comment_or_string_node(node_kind: &str) -> bool {
    node_kind.contains("comment") || node_kind.contains("string")
}

pub(super) fn parent_route_path_for_object(
    tree: &Tree,
    content: &str,
    range_start: usize,
    range_end: usize,
    object_start: usize,
    object_end: usize,
) -> Option<String> {
    let mut best: Option<(usize, String)> = None;
    let mut cursor = range_start;
    while cursor < range_end {
        let Some(relative_path_start) = content[cursor..range_end].find("path") else {
            break;
        };
        let path_start = cursor + relative_path_start;
        cursor = path_start + "path".len();
        if !is_identifier_boundary(content, path_start, "path".len()) {
            continue;
        }
        if is_ignored_syntax_range(tree, path_start, cursor) {
            continue;
        }
        let colon = skip_ascii_whitespace_until(content, cursor, range_end);
        if content.as_bytes().get(colon) != Some(&b':') {
            continue;
        }
        let value_start = skip_ascii_whitespace_until(content, colon + 1, range_end);
        let Some((route_path, _)) = parse_js_string_literal(content, value_start) else {
            continue;
        };
        let Some((candidate_start, candidate_end)) =
            find_enclosing_object_range(content, range_start, range_end, path_start)
        else {
            continue;
        };
        if candidate_start >= object_start || candidate_end < object_end {
            continue;
        }
        let candidate_len = candidate_end - candidate_start;
        if best
            .as_ref()
            .map(|(best_len, _)| candidate_len < *best_len)
            .unwrap_or(true)
        {
            best = Some((candidate_len, route_path));
        }
    }
    best.map(|(_, route_path)| route_path)
}

pub(super) fn join_frontend_route_paths(parent: &str, child: &str) -> String {
    if child.starts_with('/') {
        return child.to_string();
    }
    let parent = parent.trim_end_matches('/');
    let child = child.trim_start_matches('/');
    if parent.is_empty() {
        format!("/{child}")
    } else if child.is_empty() {
        parent.to_string()
    } else {
        format!("{parent}/{child}")
    }
}

pub(super) fn find_enclosing_object_range(
    content: &str,
    start: usize,
    end: usize,
    position: usize,
) -> Option<(usize, usize)> {
    let mut cursor = start;
    let mut candidate = None;
    while cursor < position {
        let Some(relative_open) = content[cursor..position].find('{') else {
            break;
        };
        let object_start = cursor + relative_open;
        cursor = object_start + 1;
        let Some(object_end) = find_matching_brace(content, object_start, end) else {
            continue;
        };
        if object_end >= position {
            candidate = Some((object_start, object_end + 1));
        }
    }
    candidate
}

pub(super) fn object_value_property_name(
    content: &str,
    object_start: usize,
    search_start: usize,
) -> Option<&str> {
    let bytes = content.as_bytes();
    let before_object = bytes
        .get(search_start..object_start)?
        .iter()
        .rposition(|byte| !byte.is_ascii_whitespace())
        .map(|index| search_start + index + 1)?;
    if before_object == 0 || bytes.get(before_object - 1) != Some(&b':') {
        return None;
    }

    let before_colon = bytes
        .get(search_start..before_object - 1)?
        .iter()
        .rposition(|byte| !byte.is_ascii_whitespace())
        .map(|index| search_start + index + 1)?;
    if before_colon == 0 {
        return None;
    }

    let key_end = before_colon;
    let key_last = key_end - 1;
    if matches!(bytes.get(key_last), Some(b'\'') | Some(b'"')) {
        let quote = bytes[key_last];
        let key_start = bytes
            .get(search_start..key_last)?
            .iter()
            .rposition(|byte| *byte == quote)
            .map(|index| search_start + index + 1)?;
        return content.get(key_start..key_last);
    }

    let key_start = bytes
        .get(search_start..key_end)?
        .iter()
        .rposition(|byte| !is_js_identifier_byte(*byte))
        .map_or(search_start, |index| search_start + index + 1);
    (key_start < key_end).then(|| content.get(key_start..key_end))?
}

pub(super) fn object_or_ancestor_value_property_matches(
    content: &str,
    range_start: usize,
    range_end: usize,
    mut object_start: usize,
    mut object_end: usize,
    property_names: &[&str],
) -> bool {
    loop {
        if object_value_property_name(content, object_start, range_start)
            .is_some_and(|name| property_names.contains(&name))
        {
            return true;
        }

        let Some(parent_position) = object_start.checked_sub(1) else {
            return false;
        };
        if parent_position < range_start {
            return false;
        }
        let Some((parent_start, parent_end)) =
            find_enclosing_object_range(content, range_start, range_end, parent_position)
        else {
            return false;
        };
        if parent_start >= object_start || parent_end < object_end {
            return false;
        }

        object_start = parent_start;
        object_end = parent_end;
    }
}

pub(super) fn find_matching_brace(content: &str, open_brace: usize, end: usize) -> Option<usize> {
    if content.as_bytes().get(open_brace) != Some(&b'{') {
        return None;
    }
    let bytes = content.as_bytes();
    let mut depth = 0usize;
    let mut cursor = open_brace;
    let mut quote = None;
    let mut escaped = false;

    while cursor < end {
        let byte = bytes[cursor];
        if let Some(active_quote) = quote {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == active_quote {
                quote = None;
            }
        } else if byte == b'\'' || byte == b'"' {
            quote = Some(byte);
        } else if byte == b'{' {
            depth += 1;
        } else if byte == b'}' {
            depth = depth.saturating_sub(1);
            if depth == 0 {
                return Some(cursor);
            }
        }
        cursor += 1;
    }

    None
}

pub(super) fn find_matching_paren(content: &str, open_paren: usize, end: usize) -> Option<usize> {
    if content.as_bytes().get(open_paren) != Some(&b'(') {
        return None;
    }
    let bytes = content.as_bytes();
    let mut paren_depth = 0usize;
    let mut brace_depth = 0usize;
    let mut bracket_depth = 0usize;
    let mut cursor = open_paren;
    let mut quote = None;
    let mut escaped = false;

    while cursor < end {
        let byte = bytes[cursor];
        if let Some(active_quote) = quote {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == active_quote {
                quote = None;
            }
        } else if byte == b'\'' || byte == b'"' || byte == b'`' {
            quote = Some(byte);
        } else if byte == b'{' {
            brace_depth += 1;
        } else if byte == b'}' {
            brace_depth = brace_depth.saturating_sub(1);
        } else if byte == b'[' {
            bracket_depth += 1;
        } else if byte == b']' {
            bracket_depth = bracket_depth.saturating_sub(1);
        } else if byte == b'(' {
            paren_depth += 1;
        } else if byte == b')' {
            paren_depth = paren_depth.saturating_sub(1);
            if paren_depth == 0 && brace_depth == 0 && bracket_depth == 0 {
                return Some(cursor);
            }
        }
        cursor += 1;
    }

    None
}

fn find_matching_bracket(content: &str, open_bracket: usize, end: usize) -> Option<usize> {
    if content.as_bytes().get(open_bracket) != Some(&b'[') {
        return None;
    }
    let bytes = content.as_bytes();
    let mut depth = 0usize;
    let mut cursor = open_bracket;
    let mut quote = None;
    let mut escaped = false;

    while cursor < end {
        let byte = bytes[cursor];
        if let Some(active_quote) = quote {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == active_quote {
                quote = None;
            }
        } else if byte == b'\'' || byte == b'"' || byte == b'`' {
            quote = Some(byte);
        } else if byte == b'[' {
            depth += 1;
        } else if byte == b']' {
            depth = depth.saturating_sub(1);
            if depth == 0 {
                return Some(cursor);
            }
        }
        cursor += 1;
    }

    None
}

pub(super) fn find_top_level_comma_or_end(content: &str, start: usize, end: usize) -> usize {
    let mut cursor = start;
    let mut paren_depth = 0usize;
    let mut brace_depth = 0usize;
    let mut bracket_depth = 0usize;
    let mut quote = None;
    let mut escaped = false;

    while cursor < end {
        let byte = content.as_bytes()[cursor];
        if let Some(active_quote) = quote {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == active_quote {
                quote = None;
            }
        } else if byte == b'\'' || byte == b'"' || byte == b'`' {
            quote = Some(byte);
        } else {
            match byte {
                b'(' => paren_depth += 1,
                b')' => paren_depth = paren_depth.saturating_sub(1),
                b'{' => brace_depth += 1,
                b'}' => brace_depth = brace_depth.saturating_sub(1),
                b'[' => bracket_depth += 1,
                b']' => bracket_depth = bracket_depth.saturating_sub(1),
                b',' if paren_depth == 0 && brace_depth == 0 && bracket_depth == 0 => {
                    return cursor;
                }
                _ => {}
            }
        }
        cursor += 1;
    }

    end
}

pub(super) fn find_js_array_initializer_range(
    content: &str,
    identifier: &str,
) -> Option<(usize, usize)> {
    find_js_array_initializer_range_in(content, identifier, 0, content.len())
}

pub(super) fn find_js_array_initializer_range_in(
    content: &str,
    identifier: &str,
    start: usize,
    end: usize,
) -> Option<(usize, usize)> {
    let mut cursor = start;
    while cursor < end {
        let Some(relative_start) = content[cursor..end].find(identifier) else {
            break;
        };
        let identifier_start = cursor + relative_start;
        cursor = identifier_start + identifier.len();
        if !is_identifier_boundary(content, identifier_start, identifier.len()) {
            continue;
        }
        let equals = skip_ascii_whitespace_until(content, cursor, end);
        if content.as_bytes().get(equals) != Some(&b'=') {
            continue;
        }
        let array_start = skip_ascii_whitespace_until(content, equals + 1, end);
        if content.as_bytes().get(array_start) != Some(&b'[') {
            continue;
        }
        let array_end = find_matching_bracket(content, array_start, end)?;
        return Some((array_start, array_end + 1));
    }
    None
}

pub(super) fn skip_ascii_whitespace_until(content: &str, mut cursor: usize, end: usize) -> usize {
    let bytes = content.as_bytes();
    while cursor < end && bytes[cursor].is_ascii_whitespace() {
        cursor += 1;
    }
    cursor
}

#[cfg(test)]
mod tests {
    use super::parse_js_string_literal;

    #[test]
    fn parse_js_string_literal_decodes_common_escapes() {
        assert_eq!(
            parse_js_string_literal(r#""/line\nnext""#, 0),
            Some(("/line\nnext".to_string(), 13))
        );
        assert_eq!(
            parse_js_string_literal(r#""/col\tvalue""#, 0),
            Some(("/col\tvalue".to_string(), 13))
        );
        assert_eq!(
            parse_js_string_literal(r#""/caf\u00e9""#, 0),
            Some(("/café".to_string(), 12))
        );
        assert_eq!(
            parse_js_string_literal(r#""\/api\/users""#, 0),
            Some(("/api/users".to_string(), 14))
        );
    }

    #[test]
    fn parse_js_string_literal_rejects_malformed_escapes() {
        assert_eq!(parse_js_string_literal(r#""/bad\u12xz""#, 0), None);
        assert_eq!(parse_js_string_literal("\"/bad\nline\"", 0), None);
        assert_eq!(parse_js_string_literal(r#""/bad\"#, 0), None);
    }
}
