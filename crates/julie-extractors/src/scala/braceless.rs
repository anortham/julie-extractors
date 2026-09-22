//! Scala 3 indentation syntax: tree-sitter-scala keeps the comments that
//! follow an indented body inside that body, so the node of `def f` ends
//! after the doc comment of the next definition, and the next definition has
//! no preceding comment sibling. These helpers end a symbol at its last real
//! token and find the doc comment at the tail of the previous definition.

use crate::base::body::body_hash;
use crate::base::{BaseExtractor, Symbol};
use tree_sitter::Node;

fn is_comment(node: &Node) -> bool {
    node.kind().contains("comment")
}

/// The last token of `node` that is not inside a comment.
fn last_code_token<'tree>(node: Node<'tree>) -> Option<Node<'tree>> {
    if node.child_count() == 0 {
        return (!is_comment(&node) && node.end_byte() > node.start_byte()).then_some(node);
    }
    let mut cursor = node.walk();
    let children: Vec<Node> = node.children(&mut cursor).collect();
    children
        .into_iter()
        .rev()
        .filter(|child| !is_comment(child))
        .find_map(last_code_token)
}

/// The `/** */` comment that ends the definition just before `node`, where the
/// grammar nested it. A comment before a closing brace is not at the tail.
pub(super) fn trailing_doc_comment(base: &BaseExtractor, node: &Node) -> Option<String> {
    let mut current = node.prev_named_sibling()?;
    if is_comment(&current) {
        return None;
    }
    loop {
        let last = current.child((current.child_count() as u32).checked_sub(1)?)?;
        if is_comment(&last) {
            let text = base.get_node_text(&last);
            return text.starts_with("/**").then_some(text);
        }
        if last.child_count() == 0 {
            return None;
        }
        current = last;
    }
}

/// End the symbol, and its body, at the last code token of `node`, and add
/// the doc comment the grammar nested in the previous definition.
pub(super) fn apply(base: &mut BaseExtractor, node: &Node, symbol: &mut Symbol) {
    if symbol.doc_comment.is_none() {
        symbol.doc_comment = trailing_doc_comment(base, node);
    }

    let Some(last) = last_code_token(*node) else {
        return;
    };
    let end_byte = last.end_byte() as u32;
    if end_byte >= symbol.end_byte {
        return;
    }
    let end = last.end_position();
    symbol.end_line = end.row as u32 + 1;
    symbol.end_column = end.column as u32;
    symbol.end_byte = end_byte;
    if let Some(body) = symbol.body_span.as_mut()
        && body.end_byte > end_byte
    {
        body.end_line = symbol.end_line;
        body.end_column = symbol.end_column;
        body.end_byte = end_byte;
        symbol.body_hash = body_hash(&base.content, *body, &base.language);
    }

    let span = crate::base::NormalizedSpan {
        start_line: symbol.start_line,
        start_column: symbol.start_column,
        end_line: symbol.end_line,
        end_column: symbol.end_column,
        start_byte: symbol.start_byte,
        end_byte: symbol.end_byte,
    };
    let old_id = std::mem::replace(
        &mut symbol.id,
        base.generate_id_for_span(&symbol.name, &span),
    );
    if let Some(mut type_info) = base.type_info.remove(&old_id) {
        type_info.symbol_id = symbol.id.clone();
        base.type_info.insert(symbol.id.clone(), type_info);
    }
}
