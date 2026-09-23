//! Item-position macros whose bodies define items.
//!
//! Tree-sitter leaves a macro body as flat tokens. `lazy_static!`,
//! `thread_local!`, and `cfg_if!` bodies are Rust items (after dropping the
//! `ref` in `static ref`), so they are re-parsed in place with included
//! ranges and keep their source positions. `bitflags!` and `proptest!` bodies
//! are not valid items, so their declarations are read from the tokens.

use crate::base::{
    BaseExtractor, Symbol, SymbolKind, SymbolOptions, TestRole, Visibility, normalize_annotations,
};
use crate::test_detection::apply_test_role;
use std::collections::HashMap;
use tree_sitter::{Node, Parser, Range, Tree};

pub(super) enum ItemMacro<'tree> {
    Reparse(Vec<Range>),
    Bitflags(Node<'tree>),
    Proptest(Node<'tree>),
}

pub(super) fn classify<'tree>(base: &BaseExtractor, node: Node<'tree>) -> Option<ItemMacro<'tree>> {
    let mut name = node.child_by_field_name("macro")?;
    if name.kind() == "scoped_identifier" {
        name = name.child_by_field_name("name")?;
    }
    let body = node
        .children(&mut node.walk())
        .find(|child| child.kind() == "token_tree")?;
    match base.get_node_text(&name).as_str() {
        "lazy_static" => Some(ItemMacro::Reparse(interior_ranges(body, |token| {
            base.get_node_text(&token) == "ref"
                && token
                    .prev_sibling()
                    .is_some_and(|previous| base.get_node_text(&previous) == "static")
        }))),
        "thread_local" => Some(ItemMacro::Reparse(interior_ranges(body, |_| false))),
        "cfg_if" => Some(ItemMacro::Reparse(
            children(body)
                .into_iter()
                .filter(|child| is_brace_tree(*child))
                .flat_map(|branch| interior_ranges(branch, |_| false))
                .collect(),
        )),
        "bitflags" => Some(ItemMacro::Bitflags(body)),
        "proptest" => Some(ItemMacro::Proptest(body)),
        _ => None,
    }
}

/// Parse the included ranges of the file as Rust items.
pub(super) fn reparse(content: &str, ranges: &[Range]) -> Option<Tree> {
    if ranges.is_empty() {
        return None;
    }
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_rust::LANGUAGE.into())
        .ok()?;
    parser.set_included_ranges(ranges).ok()?;
    parser.parse(content, None)
}

/// `bitflags! { pub struct Flags: u32 { const A = 1; } }`: a struct per
/// `struct` declaration and a constant per flag, parented to the struct.
pub(super) fn bitflags_symbols(
    base: &mut BaseExtractor,
    body: Node,
    parent_id: Option<String>,
) -> Vec<Symbol> {
    let tokens = children(body);
    let mut symbols = Vec::new();
    for (index, token) in tokens.iter().enumerate() {
        if base.get_node_text(token) != "struct" {
            continue;
        }
        let Some(name) = tokens.get(index + 1).filter(|n| n.kind() == "identifier") else {
            continue;
        };
        let Some(flags) = tokens[index + 2..].iter().find(|t| is_brace_tree(**t)) else {
            continue;
        };
        let (start, visibility) = item_start(base, &tokens, index);
        let signature = text_between(base, tokens[start].start_byte(), flags.start_byte());
        let Some(span) = base.span_for_byte_range(tokens[start].start_byte(), flags.end_byte())
        else {
            continue;
        };
        let doc = token_docs(base, &tokens, start);
        let mut symbol = base.create_symbol_from_span(
            name,
            span,
            base.get_node_text(name),
            SymbolKind::Struct,
            SymbolOptions {
                signature: Some(signature),
                visibility: Some(visibility.clone()),
                parent_id: parent_id.clone(),
                doc_comment: doc.clone(),
                metadata: Some(HashMap::new()),
                annotations: Vec::new(),
            },
        );
        symbol.doc_comment = doc;
        let body_span = base.span_for_byte_range(flags.start_byte(), flags.end_byte());
        base.set_body_span(&mut symbol, body_span);
        let struct_id = symbol.id.clone();
        symbols.push(symbol);
        symbols.extend(flag_constants(base, *flags, &struct_id, &visibility));
    }
    symbols
}

fn flag_constants(
    base: &mut BaseExtractor,
    flags: Node,
    struct_id: &str,
    visibility: &Visibility,
) -> Vec<Symbol> {
    let tokens = children(flags);
    let mut symbols = Vec::new();
    for (index, token) in tokens.iter().enumerate() {
        if base.get_node_text(token) != "const" {
            continue;
        }
        let Some(name) = tokens.get(index + 1).filter(|n| n.kind() == "identifier") else {
            continue;
        };
        let end = tokens[index..]
            .iter()
            .find(|t| t.kind() == ";")
            .map(|semicolon| semicolon.start_byte())
            .unwrap_or_else(|| tokens[tokens.len() - 1].start_byte());
        let Some(span) = base.span_for_byte_range(token.start_byte(), end) else {
            continue;
        };
        let doc = token_docs(base, &tokens, index);
        let mut symbol = base.create_symbol_from_span(
            name,
            span,
            base.get_node_text(name),
            SymbolKind::Constant,
            SymbolOptions {
                signature: Some(text_between(base, token.start_byte(), end)),
                visibility: Some(visibility.clone()),
                parent_id: Some(struct_id.to_string()),
                doc_comment: doc.clone(),
                metadata: Some(HashMap::new()),
                annotations: Vec::new(),
            },
        );
        symbol.doc_comment = doc;
        symbols.push(symbol);
    }
    symbols
}

/// `proptest! { #[test] fn prop_add(a in 0u32..10) { .. } }`: a function per
/// `fn` declaration; the `#[test]` ones are test cases.
pub(super) fn proptest_symbols(
    base: &mut BaseExtractor,
    body: Node,
    parent_id: Option<String>,
) -> Vec<Symbol> {
    let tokens = children(body);
    let mut symbols = Vec::new();
    for (index, token) in tokens.iter().enumerate() {
        if base.get_node_text(token) != "fn" {
            continue;
        }
        let Some(name) = tokens.get(index + 1).filter(|n| n.kind() == "identifier") else {
            continue;
        };
        let Some(block) = tokens[index + 2..].iter().find(|t| is_brace_tree(**t)) else {
            continue;
        };
        let attributes = token_attributes(base, &tokens, index);
        let start = index - attributes.len() * 2;
        let Some(span) = base.span_for_byte_range(tokens[start].start_byte(), block.end_byte())
        else {
            continue;
        };
        let annotations = normalize_annotations(&attributes, "rust");
        let mut metadata = HashMap::new();
        if annotations
            .iter()
            .any(|marker| marker.annotation_key == "test")
        {
            apply_test_role(&mut metadata, TestRole::TestCase);
        }
        let doc = token_docs(base, &tokens, start);
        let mut symbol = base.create_symbol_from_span(
            name,
            span,
            base.get_node_text(name),
            SymbolKind::Function,
            SymbolOptions {
                signature: Some(text_between(base, token.start_byte(), block.start_byte())),
                visibility: Some(Visibility::Private),
                parent_id: parent_id.clone(),
                doc_comment: doc.clone(),
                metadata: Some(metadata),
                annotations,
            },
        );
        symbol.doc_comment = doc;
        let body_span = base.span_for_byte_range(block.start_byte(), block.end_byte());
        base.set_body_span(&mut symbol, body_span);
        symbols.push(symbol);
    }
    symbols
}

fn children(node: Node) -> Vec<Node> {
    node.children(&mut node.walk()).collect()
}

fn is_brace_tree(node: Node) -> bool {
    node.kind() == "token_tree" && node.child(0).is_some_and(|open| open.kind() == "{")
}

/// The ranges between a token tree's delimiters, minus the excluded tokens.
fn interior_ranges(tree: Node, mut exclude: impl FnMut(Node) -> bool) -> Vec<Range> {
    let tokens = children(tree);
    let (Some(open), Some(close)) = (tokens.first(), tokens.last()) else {
        return Vec::new();
    };
    if tokens.len() < 2 {
        return Vec::new();
    }
    let mut ranges = Vec::new();
    let mut start = (open.end_byte(), open.end_position());
    for token in &tokens[1..tokens.len() - 1] {
        if exclude(*token) {
            push_range(
                &mut ranges,
                start,
                (token.start_byte(), token.start_position()),
            );
            start = (token.end_byte(), token.end_position());
        }
    }
    push_range(
        &mut ranges,
        start,
        (close.start_byte(), close.start_position()),
    );
    ranges
}

fn push_range(
    ranges: &mut Vec<Range>,
    (start_byte, start_point): (usize, tree_sitter::Point),
    (end_byte, end_point): (usize, tree_sitter::Point),
) {
    if start_byte < end_byte {
        ranges.push(Range {
            start_byte,
            end_byte,
            start_point,
            end_point,
        });
    }
}

/// The first token of the item whose keyword is at `keyword`, and the
/// visibility its `pub` / `pub(..)` prefix states.
fn item_start(base: &BaseExtractor, tokens: &[Node], keyword: usize) -> (usize, Visibility) {
    if keyword >= 1 && base.get_node_text(&tokens[keyword - 1]) == "pub" {
        return (keyword - 1, Visibility::Public);
    }
    if keyword >= 2
        && tokens[keyword - 1].kind() == "token_tree"
        && base.get_node_text(&tokens[keyword - 2]) == "pub"
    {
        return (keyword - 2, Visibility::Internal);
    }
    (keyword, Visibility::Private)
}

/// The `#[..]` attribute texts directly before the token at `index`.
fn token_attributes(base: &BaseExtractor, tokens: &[Node], index: usize) -> Vec<String> {
    let mut attributes = Vec::new();
    let mut cursor = index;
    while cursor >= 2
        && tokens[cursor - 1].kind() == "token_tree"
        && base.get_node_text(&tokens[cursor - 2]) == "#"
    {
        attributes.push(text_between(
            base,
            tokens[cursor - 2].start_byte(),
            tokens[cursor - 1].end_byte(),
        ));
        cursor -= 2;
    }
    attributes.reverse();
    attributes
}

/// The `///` doc comment tokens directly before the token at `index`.
fn token_docs(base: &BaseExtractor, tokens: &[Node], index: usize) -> Option<String> {
    let mut lines = Vec::new();
    let mut cursor = index;
    while cursor >= 1 && tokens[cursor - 1].kind() == "line_comment" {
        let text = base.get_node_text(&tokens[cursor - 1]);
        let Some(doc) = text.trim().strip_prefix("///") else {
            break;
        };
        lines.push(doc.trim().to_string());
        cursor -= 1;
    }
    lines.reverse();
    let doc = lines.join("\n");
    (!doc.is_empty()).then_some(doc)
}

fn text_between(base: &BaseExtractor, start: usize, end: usize) -> String {
    base.content
        .get(start..end)
        .unwrap_or_default()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}
