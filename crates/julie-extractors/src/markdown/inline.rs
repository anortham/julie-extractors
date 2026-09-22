//! Inline syntax trees for Markdown.
//!
//! The block grammar leaves each paragraph, heading, and table cell as one
//! opaque `inline` node. Links, images, autolinks, code spans, and escapes
//! exist only in the inline grammar, so each `inline` node is parsed again
//! with it, restricted to that node's byte ranges the way
//! `tree_sitter_md::MarkdownParser` does. Code blocks, HTML blocks, and
//! frontmatter have no `inline` node, so no link is ever read from them.

use tree_sitter::{Node, Parser, Range, Tree};

use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};

/// One inline tree per `inline` or `pipe_table_cell` node, in document order.
/// Node positions in each tree are absolute offsets into `content`.
pub(crate) fn parse_inline_trees(block_tree: &Tree, content: &str) -> Vec<Tree> {
    let mut parser = Parser::new();
    if parser
        .set_language(&tree_sitter_md::INLINE_LANGUAGE.into())
        .is_err()
    {
        return Vec::new();
    }
    let mut trees = Vec::new();
    collect(block_tree.root_node(), content, &mut parser, &mut trees, 0);
    trees
}

fn collect(node: Node, content: &str, parser: &mut Parser, trees: &mut Vec<Tree>, depth: u32) {
    if !should_visit_tree_depth(depth) {
        return;
    }
    if matches!(node.kind(), "inline" | "pipe_table_cell") {
        if parser.set_included_ranges(&inline_ranges(node)).is_ok()
            && let Some(tree) = parser.parse(content, None)
        {
            trees.push(tree);
        }
        return;
    }
    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect(child, content, parser, trees, child_depth);
    }
}

/// The inline tree of one `inline` node.
pub(crate) fn parse_inline_node(node: Node, content: &str) -> Option<Tree> {
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_md::INLINE_LANGUAGE.into())
        .ok()?;
    parser.set_included_ranges(&inline_ranges(node)).ok()?;
    parser.parse(content, None)
}

/// The node's range minus its named children (block continuation markers
/// such as a `> ` quote prefix inside a multi-line paragraph).
fn inline_ranges(node: Node) -> Vec<Range> {
    let mut range = node.range();
    let mut ranges = Vec::new();
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        let child_range = child.range();
        ranges.push(Range {
            start_byte: range.start_byte,
            start_point: range.start_point,
            end_byte: child_range.start_byte,
            end_point: child_range.start_point,
        });
        range.start_byte = child_range.end_byte;
        range.start_point = child_range.end_point;
    }
    ranges.push(range);
    ranges
}

/// The readable text of a link label or image description: nested images
/// read as their description, code spans lose their backticks, escapes lose
/// their backslash, and emphasis delimiters drop out.
pub(crate) fn plain_text(content: &str, node: Node) -> String {
    plain_text_at(content, node, 0)
}

fn plain_text_at(content: &str, node: Node, depth: u32) -> String {
    let Some(child_depth) = crate::tree_traversal::child_tree_depth(depth) else {
        return String::new();
    };
    let mut text = String::new();
    let mut offset = node.start_byte();
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        text.push_str(content.get(offset..child.start_byte()).unwrap_or_default());
        offset = child.end_byte();
        let raw = content.get(child.byte_range()).unwrap_or_default();
        match child.kind() {
            "image" => push_labelled(content, child, "image_description", &mut text, child_depth),
            "inline_link" => push_labelled(content, child, "link_text", &mut text, child_depth),
            "code_span" => text.push_str(raw.trim_matches('`')),
            "backslash_escape" => text.push_str(raw.get(1..).unwrap_or_default()),
            "emphasis_delimiter" | "code_span_delimiter" => {}
            _ => text.push_str(&plain_text_at(content, child, child_depth)),
        }
    }
    text.push_str(content.get(offset..node.end_byte()).unwrap_or_default());
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn push_labelled(content: &str, node: Node, label_kind: &str, text: &mut String, depth: u32) {
    let mut cursor = node.walk();
    if let Some(label) = node
        .named_children(&mut cursor)
        .find(|child| child.kind() == label_kind)
    {
        text.push_str(&plain_text_at(content, label, depth));
    }
}

/// An inline link whose text holds nested brackets.
pub(crate) struct NestedBracketLink {
    pub start: usize,
    pub end: usize,
    pub label: String,
    pub destination: String,
    pub destination_start: usize,
}

/// The inline grammar cannot nest brackets inside link text: it reads
/// `[API [v2] docs](url)` as a `[v2]` shortcut link inside plain text. This
/// finds such links in the text the grammar left plain, skipping code spans,
/// escapes, autolinks, HTML, and links the grammar did parse.
pub(crate) fn nested_bracket_links(tree: &Tree, content: &str) -> Vec<NestedBracketLink> {
    let root = tree.root_node();
    let mut opaque = Vec::new();
    collect_opaque(root, &mut opaque);
    let skip_to = |index: usize| {
        opaque
            .iter()
            .find(|(start, end)| *start <= index && index < *end)
            .map(|(_, end)| *end)
    };
    let bytes = content.as_bytes();
    let (mut index, end) = (root.start_byte(), root.end_byte().min(bytes.len()));
    let mut links = Vec::new();
    while index < end {
        if let Some(next) = skip_to(index) {
            index = next;
            continue;
        }
        if bytes[index] == b'['
            && (index == 0 || bytes[index - 1] != b'!')
            && let Some(link) = nested_link_at(content, index, end, &skip_to)
        {
            index = link.end;
            links.push(link);
            continue;
        }
        index += 1;
    }
    links
}

fn nested_link_at(
    content: &str,
    open: usize,
    end: usize,
    skip_to: &impl Fn(usize) -> Option<usize>,
) -> Option<NestedBracketLink> {
    let bytes = content.as_bytes();
    let (mut depth, mut nested, mut index) = (0usize, false, open);
    let close = loop {
        if index >= end {
            return None;
        }
        if let Some(next) = skip_to(index) {
            index = next;
            continue;
        }
        match bytes[index] {
            b'[' => {
                nested |= depth > 0;
                depth += 1;
            }
            b']' => {
                depth -= 1;
                if depth == 0 {
                    break index;
                }
            }
            _ => {}
        }
        index += 1;
    };
    if !nested || bytes.get(close + 1) != Some(&b'(') {
        return None;
    }
    let (mut parens, mut index) = (0usize, close + 1);
    let destination_end = loop {
        match bytes.get(index)? {
            b'(' => parens += 1,
            b')' => {
                parens -= 1;
                if parens == 0 {
                    break index;
                }
            }
            b'\n' => return None,
            _ => {}
        }
        index += 1;
    };
    let target = content.get(close + 2..destination_end)?;
    let token = target.split_whitespace().next()?;
    let mut destination_start = close + 2 + (target.len() - target.trim_start().len());
    let destination = match token
        .strip_prefix('<')
        .and_then(|inner| inner.strip_suffix('>'))
    {
        Some(inner) => {
            destination_start += 1;
            inner
        }
        None => token,
    };
    Some(NestedBracketLink {
        start: open,
        end: destination_end + 1,
        label: content
            .get(open + 1..close)?
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" "),
        destination: destination.to_string(),
        destination_start,
    })
}

fn collect_opaque(node: Node, ranges: &mut Vec<(usize, usize)>) {
    collect_opaque_at(node, ranges, 0);
}

fn collect_opaque_at(node: Node, ranges: &mut Vec<(usize, usize)>, depth: u32) {
    let Some(child_depth) = crate::tree_traversal::child_tree_depth(depth) else {
        return;
    };
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        match child.kind() {
            "code_span" | "inline_link" | "image" | "uri_autolink" | "email_autolink"
            | "html_tag" | "backslash_escape" | "latex_block" => {
                ranges.push((child.start_byte(), child.end_byte()));
            }
            _ => collect_opaque_at(child, ranges, child_depth),
        }
    }
}
