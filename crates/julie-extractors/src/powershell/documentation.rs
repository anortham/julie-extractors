//! PowerShell doc comment extraction (comment-based help).

use crate::base::BaseExtractor;
use regex::Regex;
use std::sync::LazyLock;
use tree_sitter::Node;

static HELP_KEYWORD_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?im)^\s*(?:<#)?\s*\.(?:synopsis|description|parameter|example|inputs|outputs|notes|link|component|role|functionality|forwardhelptargetname|forwardhelpcategory|remotehelprunspace|externalhelp)\b",
    )
    .unwrap()
});

/// The doc comment of a declaration: the comments directly above it, or, for a
/// function, a comment-based help block at the start of its body.
///
/// A comment attaches only when no blank line separates it from the
/// declaration. PowerShell allows one blank line after a comment-based help
/// block, so a block with help keywords may sit one blank line above.
pub(super) fn extract_powershell_doc_comment(base: &BaseExtractor, node: &Node) -> Option<String> {
    leading_comments(base, *node).or_else(|| in_body_help(base, *node))
}

fn leading_comments(base: &BaseExtractor, node: Node) -> Option<String> {
    let mut anchor = node;
    while anchor.prev_named_sibling().is_none() {
        match anchor.parent() {
            Some(parent) if parent.start_byte() == anchor.start_byte() => anchor = parent,
            _ => break,
        }
    }

    let mut comments = Vec::new();
    let mut next_row = anchor.start_position().row;
    let mut current = anchor.prev_named_sibling();
    while let Some(sibling) = current {
        if sibling.kind() != "comment" {
            break;
        }
        let text = base.get_node_text(&sibling);
        let blank_lines = next_row.saturating_sub(sibling.end_position().row + 1);
        let allowed = if comments.is_empty() && HELP_KEYWORD_RE.is_match(&text) {
            1
        } else {
            0
        };
        if blank_lines > allowed {
            break;
        }
        next_row = sibling.start_position().row;
        comments.push(text);
        current = sibling.prev_named_sibling();
    }

    if comments.is_empty() {
        return None;
    }
    comments.reverse();
    Some(comments.join("\n"))
}

fn in_body_help(base: &BaseExtractor, node: Node) -> Option<String> {
    if node.kind() != "function_statement" {
        return None;
    }
    let mut cursor = node.walk();
    node.named_children(&mut cursor)
        .take_while(|child| child.kind() != "script_block")
        .filter(|child| child.kind() == "comment")
        .map(|child| base.get_node_text(&child))
        .find(|text| HELP_KEYWORD_RE.is_match(text))
}
