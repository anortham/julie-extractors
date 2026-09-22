//! Ruby doc comments: the `#` comment block directly above a declaration.
//!
//! A comment before the first statement of a class, module, or singleton
//! class body sits in the declaration node, before its `body_statement`, so
//! the first member finds its doc there. A blank line ends the block. Magic
//! comments (`frozen_string_literal:`, `encoding:`), a shebang, and
//! `rubocop:` directives are never docs and also end the block.

use crate::base::BaseExtractor;
use crate::base::extractor::select_doc_comment_block;
use tree_sitter::Node;

pub(crate) fn find_ruby_doc_comment(base: &BaseExtractor, node: Node) -> Option<String> {
    let mut comments = Vec::new();
    let mut next_row = node.start_position().row;
    let mut current = node.prev_named_sibling().or_else(|| {
        node.parent()
            .filter(|parent| parent.kind() == "body_statement")
            .and_then(|body| body.prev_named_sibling())
    });
    while let Some(comment) = current {
        if comment.kind() != "comment" || last_row(comment) + 1 != next_row {
            break;
        }
        if !starts_its_line(&base.content, comment) {
            break;
        }
        let text = base.get_node_text(&comment);
        if is_ruby_directive_comment(&text) {
            break;
        }
        comments.push(text);
        next_row = comment.start_position().row;
        current = comment.prev_named_sibling();
    }
    select_doc_comment_block(&base.language, &comments)
}

/// The last line a comment covers; a comment token can end at column 0 of
/// the next line when it includes its newline.
fn last_row(comment: Node) -> usize {
    let end = comment.end_position();
    if end.column == 0 && end.row > comment.start_position().row {
        end.row - 1
    } else {
        end.row
    }
}

fn starts_its_line(content: &str, comment: Node) -> bool {
    let start = comment.start_byte();
    let line_start = content[..start].rfind('\n').map_or(0, |index| index + 1);
    content[line_start..start].trim().is_empty()
}

/// A shebang, a magic comment, or a RuboCop directive.
pub(crate) fn is_ruby_directive_comment(text: &str) -> bool {
    if text.starts_with("#!") {
        return true;
    }
    let body = text.trim_start_matches('#').trim();
    let body = body
        .strip_prefix("-*-")
        .and_then(|rest| rest.strip_suffix("-*-"))
        .map_or(body, str::trim);
    if body.starts_with("rubocop:") {
        return true;
    }
    let Some((key, _)) = body.split_once(':') else {
        return false;
    };
    matches!(
        key.trim().to_ascii_lowercase().replace('-', "_").as_str(),
        "frozen_string_literal"
            | "encoding"
            | "coding"
            | "warn_indent"
            | "shareable_constant_value"
            | "typed"
    )
}
