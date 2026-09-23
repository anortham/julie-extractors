//! JSONC comments as doc comments.
//!
//! A pair or array element is documented by the comments directly above it:
//! each comment starts its own line, and no blank line separates the block
//! from the value. Without such a block, a comment that starts on the line
//! where the value ends (`"strict": true, // why`) documents it instead.

use tree_sitter::Node;

pub(super) fn leading_doc(content: &str, holder: Node) -> Option<String> {
    let block = leading_block(content, holder);
    (!block.is_empty()).then(|| {
        block
            .iter()
            .rev()
            .filter_map(|comment| text(content, *comment))
            .collect::<Vec<_>>()
            .join("\n")
    })
}

pub(super) fn trailing_doc(content: &str, holder: Node) -> Option<String> {
    let mut sibling = holder.next_sibling();
    while let Some(node) = sibling.filter(|node| node.kind() == ",") {
        sibling = node.next_sibling();
    }
    let comment = sibling.filter(|node| node.kind() == "comment")?;
    (comment.start_position().row == holder.end_position().row)
        .then(|| text(content, comment).map(str::to_string))
        .flatten()
}

/// True when `comment` belongs to the leading doc block of the pair or array
/// element that follows it, by the rule of [`leading_doc`].
pub(crate) fn comment_documents_following_value(content: &str, comment: Node) -> bool {
    let mut sibling = comment.next_sibling();
    while let Some(node) = sibling {
        match node.kind() {
            "comment" => sibling = node.next_sibling(),
            "pair" | "object" if node.kind() == "pair" || is_array_element(node) => {
                return leading_block(content, node)
                    .iter()
                    .any(|documented| documented.id() == comment.id());
            }
            _ => return false,
        }
    }
    false
}

/// The comments of the leading doc block, nearest first.
fn leading_block<'tree>(content: &str, holder: Node<'tree>) -> Vec<Node<'tree>> {
    let mut block = Vec::new();
    let mut next_row = holder.start_position().row;
    let mut sibling = holder.prev_sibling();
    while let Some(comment) = sibling.filter(|node| node.kind() == "comment") {
        if comment.end_position().row + 1 != next_row || !starts_line(content, comment) {
            break;
        }
        block.push(comment);
        next_row = comment.start_position().row;
        sibling = comment.prev_sibling();
    }
    block
}

fn is_array_element(node: Node) -> bool {
    node.parent().is_some_and(|parent| parent.kind() == "array")
}

fn starts_line(content: &str, node: Node) -> bool {
    let start = node.start_byte();
    let line_start = content[..start].rfind('\n').map_or(0, |index| index + 1);
    content[line_start..start].trim().is_empty()
}

fn text<'a>(content: &'a str, node: Node) -> Option<&'a str> {
    content
        .get(node.start_byte()..node.end_byte())
        .map(str::trim)
}
