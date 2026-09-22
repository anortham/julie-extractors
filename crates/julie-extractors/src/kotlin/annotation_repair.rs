//! Recovery for top-level annotations that tree-sitter-kotlin-ng detaches.
//!
//! A `.kt` file may hold script-style statements, so at the top level the
//! grammar often reads `@Profile("dev")` as an `annotated_expression` and
//! either leaves the declaration as its next sibling or swallows it
//! (`@Keep enum class Mode` becomes an infix expression of `enum`, `class`,
//! `Mode`). The extractor blanks those annotations with spaces, which keeps
//! every byte offset and line, parses the result again, and attaches the
//! annotations to the declaration that follows them.

use crate::base::{BaseExtractor, NormalizedSpan, Symbol, normalize_annotations};
use tree_sitter::{Node, Parser, Tree};

/// Annotations blanked out of the source, in front of one declaration.
pub(super) struct DetachedAnnotations {
    start: tree_sitter::Point,
    start_byte: usize,
    end_byte: usize,
    texts: Vec<String>,
}

pub(super) struct AnnotationRepair {
    pub(super) tree: Tree,
    pub(super) detached: Vec<DetachedAnnotations>,
}

const DECLARATION_WORDS: &[&str] = &[
    "abstract",
    "actual",
    "annotation",
    "class",
    "companion",
    "const",
    "data",
    "enum",
    "expect",
    "external",
    "final",
    "fun",
    "infix",
    "inline",
    "inner",
    "interface",
    "internal",
    "lateinit",
    "object",
    "open",
    "operator",
    "override",
    "private",
    "protected",
    "public",
    "sealed",
    "suspend",
    "tailrec",
    "typealias",
    "val",
    "value",
    "var",
];

const DECLARATION_KINDS: &[&str] = &[
    "class_declaration",
    "object_declaration",
    "function_declaration",
    "property_declaration",
    "type_alias",
    "ERROR",
];

/// Reparse `content` with detached top-level annotations blanked, or `None`
/// when the tree has no such annotations.
pub(super) fn repair(content: &str, tree: &Tree) -> Option<AnnotationRepair> {
    let root = tree.root_node();
    let mut cursor = root.walk();
    let children: Vec<Node> = root.named_children(&mut cursor).collect();
    let mut detached: Vec<DetachedAnnotations> = Vec::new();
    let mut next_is_region = false;
    for node in children.iter().rev() {
        let region = (node.kind() == "annotated_expression")
            .then(|| detached_annotations(content, *node, next_is_region))
            .flatten();
        next_is_region = region.is_some();
        if let Some(mut region) = region {
            if let Some(next) = detached.last_mut()
                && content[region.end_byte..next.start_byte].trim().is_empty()
            {
                region.texts.append(&mut next.texts);
                region.end_byte = next.end_byte;
                detached.pop();
            }
            detached.push(region);
        }
    }
    if detached.is_empty() {
        return None;
    }

    let mut blanked = content.as_bytes().to_vec();
    for region in &detached {
        for byte in &mut blanked[region.start_byte..region.end_byte] {
            if !matches!(*byte, b'\n' | b'\r') {
                *byte = b' ';
            }
        }
    }
    let blanked = String::from_utf8(blanked).ok()?;
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_kotlin_ng::LANGUAGE.into())
        .ok()?;
    let tree = parser.parse(blanked, None)?;
    Some(AnnotationRepair { tree, detached })
}

fn detached_annotations(
    content: &str,
    node: Node,
    next_is_region: bool,
) -> Option<DetachedAnnotations> {
    let mut annotation_starts = Vec::new();
    let mut current = node;
    let mut tail = None;
    loop {
        let mut cursor = current.walk();
        let children: Vec<Node> = current
            .named_children(&mut cursor)
            .filter(|child| !child.kind().contains("comment"))
            .collect();
        annotation_starts.extend(
            children
                .iter()
                .filter(|child| child.kind() == "annotation")
                .map(|child| child.start_byte()),
        );
        match children.last() {
            Some(last) if last.kind() == "annotated_expression" => current = *last,
            Some(last) if last.kind() != "annotation" => {
                tail = Some(*last);
                break;
            }
            _ => break,
        }
    }

    let end_byte = match tail {
        Some(tail) if starts_with_declaration_word(content, tail) => tail.start_byte(),
        Some(tail) if tail.kind() != "parenthesized_expression" => return None,
        _ => {
            let next = node.next_named_sibling()?;
            if !(DECLARATION_KINDS.contains(&next.kind()) || next_is_region) {
                return None;
            }
            node.end_byte()
        }
    };

    let texts = annotation_starts
        .iter()
        .enumerate()
        .map(|(index, &start)| {
            let end = annotation_starts
                .get(index + 1)
                .copied()
                .unwrap_or(end_byte);
            content[start..end].trim().to_string()
        })
        .filter(|text| !text.is_empty())
        .collect();
    Some(DetachedAnnotations {
        start: node.start_position(),
        start_byte: node.start_byte(),
        end_byte,
        texts,
    })
}

fn starts_with_declaration_word(content: &str, node: Node) -> bool {
    let word: String = content[node.start_byte()..node.end_byte()]
        .chars()
        .take_while(|c| c.is_ascii_alphabetic())
        .collect();
    DECLARATION_WORDS.contains(&word.as_str())
}

impl AnnotationRepair {
    /// The blanked annotation nodes of the original tree, for identifier
    /// extraction: every node that lies wholly inside a blanked region.
    pub(super) fn original_annotation_nodes<'tree>(
        &self,
        original: &'tree Tree,
    ) -> Vec<Node<'tree>> {
        let mut nodes = Vec::new();
        for region in &self.detached {
            collect_inside(original.root_node(), region, &mut nodes);
        }
        nodes
    }

    /// Attach the annotations blanked in front of `node` to its symbol: the
    /// annotation rows, the signature and modifiers text, and a span that
    /// starts at the first annotation, as a member declaration's does.
    pub(super) fn attach(&self, base: &mut BaseExtractor, node: &Node, symbol: &mut Symbol) {
        let Some(region) = self.detached.iter().find(|region| {
            region.end_byte <= node.start_byte()
                && base.content[region.end_byte..node.start_byte()]
                    .trim()
                    .is_empty()
        }) else {
            return;
        };

        let mut annotations = normalize_annotations(&region.texts, "kotlin");
        annotations.append(&mut symbol.annotations);
        symbol.annotations = annotations;

        let prefix = region.texts.join(" ");
        if let Some(signature) = symbol.signature.as_mut() {
            *signature = format!("{prefix} {signature}");
        }
        if let Some(serde_json::Value::String(modifiers)) = symbol
            .metadata
            .as_mut()
            .and_then(|metadata| metadata.get_mut("modifiers"))
        {
            let mut all = region.texts.clone();
            all.extend(
                modifiers
                    .split(',')
                    .filter(|m| !m.is_empty())
                    .map(String::from),
            );
            *modifiers = all.join(",");
        }

        let span = NormalizedSpan {
            start_line: region.start.row as u32 + 1,
            start_column: region.start.column as u32,
            end_line: symbol.end_line,
            end_column: symbol.end_column,
            start_byte: region.start_byte as u32,
            end_byte: symbol.end_byte,
        };
        let old_id = std::mem::replace(
            &mut symbol.id,
            base.generate_id_for_span(&symbol.name, &span),
        );
        symbol.start_line = span.start_line;
        symbol.start_column = span.start_column;
        symbol.start_byte = span.start_byte;
        if let Some(mut type_info) = base.type_info.remove(&old_id) {
            type_info.symbol_id = symbol.id.clone();
            base.type_info.insert(symbol.id.clone(), type_info);
        }
    }
}

fn collect_inside<'tree>(
    node: Node<'tree>,
    region: &DetachedAnnotations,
    out: &mut Vec<Node<'tree>>,
) {
    collect_inside_at(node, region, out, 0);
}

fn collect_inside_at<'tree>(
    node: Node<'tree>,
    region: &DetachedAnnotations,
    out: &mut Vec<Node<'tree>>,
    depth: u32,
) {
    if node.end_byte() <= region.start_byte || node.start_byte() >= region.end_byte {
        return;
    }
    if node.start_byte() >= region.start_byte && node.end_byte() <= region.end_byte {
        out.push(node);
        return;
    }
    let Some(child_depth) = crate::tree_traversal::child_tree_depth(depth) else {
        return;
    };
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_inside_at(child, region, out, child_depth);
    }
}
