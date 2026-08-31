use std::collections::HashMap;

use tree_sitter::{Node, Tree};

use super::attach_containing_symbols;
use super::span::NormalizedSpan;
use super::types::{StructuralFact, Symbol, stable_location_id};

const PATTERN_ID: &str = "rust.doc_test.v1";
const CAPTURE_NAME: &str = "doc_test";
const NODE_KIND: &str = "rustdoc_fence";

#[derive(Debug, Clone)]
struct DocCommentLine {
    parent_id: usize,
    is_inner: bool,
    start_byte: usize,
    end_byte: usize,
    text_start_byte: usize,
    text: String,
}

#[derive(Debug, Clone, Copy)]
struct PendingFence {
    start_byte: usize,
    marker_len: usize,
    mode: Option<&'static str>,
}

pub(crate) fn collect_rust_doc_test_facts(
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
    symbols: &[Symbol],
) -> Vec<StructuralFact> {
    if language != "rust" {
        return Vec::new();
    }

    let mut lines = Vec::new();
    collect_doc_comment_lines(tree.root_node(), content, &mut lines);
    lines.sort_by_key(|line| line.start_byte);

    let mut facts = Vec::new();
    let mut previous: Option<&DocCommentLine> = None;
    let mut pending: Option<PendingFence> = None;

    for line in &lines {
        if previous.is_none_or(|previous| !same_doc_block(previous, line, content)) {
            pending = None;
        }

        if let Some(opening) = pending {
            if let Some(end_byte) = closing_fence_end(line, opening.marker_len) {
                if let Some(mode) = opening.mode
                    && let Some(fact) =
                        make_fact(file_path, content, opening.start_byte, end_byte, mode)
                {
                    facts.push(fact);
                }
                pending = None;
            }
        } else if let Some((start_byte, marker_len, mode)) = opening_fence(line) {
            pending = Some(PendingFence {
                start_byte,
                marker_len,
                mode,
            });
        }

        previous = Some(line);
    }

    attach_containing_symbols(&mut facts, symbols);
    attach_outer_doc_symbols(&mut facts, &lines, symbols);
    super::structural_facts::sort_structural_facts(&mut facts);
    facts
}

fn collect_doc_comment_lines(node: Node<'_>, content: &str, lines: &mut Vec<DocCommentLine>) {
    if node.kind() == "line_comment"
        && let Some(line) = doc_comment_line(node, content)
    {
        lines.push(line);
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_doc_comment_lines(child, content, lines);
    }
}

fn doc_comment_line(node: Node<'_>, content: &str) -> Option<DocCommentLine> {
    let raw = content.get(node.start_byte()..node.end_byte())?;
    let (marker, is_inner) = if raw.starts_with("///") {
        ("///", false)
    } else if raw.starts_with("//!") {
        ("//!", true)
    } else {
        return None;
    };

    let mut text_start = marker.len();
    if raw
        .as_bytes()
        .get(text_start)
        .is_some_and(u8::is_ascii_whitespace)
    {
        text_start += 1;
    }

    Some(DocCommentLine {
        parent_id: node.parent().map_or(0, |parent| parent.id()),
        is_inner,
        start_byte: node.start_byte(),
        end_byte: node.end_byte(),
        text_start_byte: node.start_byte() + text_start,
        text: raw[text_start..].to_string(),
    })
}

fn same_doc_block(left: &DocCommentLine, right: &DocCommentLine, content: &str) -> bool {
    left.parent_id == right.parent_id
        && content
            .get(left.end_byte..right.start_byte)
            .is_some_and(|gap| {
                gap.is_empty() || gap.bytes().all(|byte| matches!(byte, b' ' | b'\t' | b'\r'))
            })
}

fn opening_fence(line: &DocCommentLine) -> Option<(usize, usize, Option<&'static str>)> {
    let leading = line.text.len() - line.text.trim_start_matches(char::is_whitespace).len();
    let text = line.text.get(leading..)?;
    let marker_len = text.bytes().take_while(|byte| *byte == b'`').count();
    if marker_len < 3 {
        return None;
    }

    let info = text.get(marker_len..)?.trim();
    let mode = rustdoc_mode(info);
    Some((line.text_start_byte + leading, marker_len, mode))
}

fn closing_fence_end(line: &DocCommentLine, opening_marker_len: usize) -> Option<usize> {
    let leading = line.text.len() - line.text.trim_start_matches(char::is_whitespace).len();
    let text = line.text.get(leading..)?;
    let marker_len = text.bytes().take_while(|byte| *byte == b'`').count();
    if marker_len < opening_marker_len || !text.get(marker_len..)?.trim().is_empty() {
        return None;
    }
    Some(line.text_start_byte + leading + marker_len)
}

fn rustdoc_mode(info: &str) -> Option<&'static str> {
    let tokens = info
        .split(|character: char| character == ',' || character.is_whitespace())
        .filter(|token| !token.is_empty())
        .collect::<Vec<_>>();
    let Some(first) = tokens.first().copied() else {
        return Some("run");
    };

    if first != "rust" && !matches!(first, "ignore" | "no_run" | "compile_fail") {
        return None;
    }

    if tokens.contains(&"ignore") {
        Some("ignore")
    } else if tokens.contains(&"compile_fail") {
        Some("compile_fail")
    } else if tokens.contains(&"no_run") {
        Some("no_run")
    } else {
        Some("run")
    }
}

fn make_fact(
    file_path: &str,
    content: &str,
    start_byte: usize,
    end_byte: usize,
    mode: &str,
) -> Option<StructuralFact> {
    let span = NormalizedSpan::from_content_range(content, start_byte, end_byte)?;
    let metadata = HashMap::from([
        (
            "pattern_version".to_string(),
            serde_json::Value::Number(serde_json::Number::from(1)),
        ),
        (
            "query_family".to_string(),
            serde_json::Value::String("testing".to_string()),
        ),
        (
            "mode".to_string(),
            serde_json::Value::String(mode.to_string()),
        ),
    ]);

    Some(StructuralFact {
        id: stable_location_id(file_path, &format!("{PATTERN_ID}:{CAPTURE_NAME}"), span),
        file_path: file_path.to_string(),
        language: "rust".to_string(),
        pattern_id: PATTERN_ID.to_string(),
        capture_name: CAPTURE_NAME.to_string(),
        node_kind: NODE_KIND.to_string(),
        containing_symbol_id: None,
        start_line: span.start_line,
        start_column: span.start_column,
        end_line: span.end_line,
        end_column: span.end_column,
        start_byte: span.start_byte,
        end_byte: span.end_byte,
        confidence: 1.0,
        metadata: Some(metadata),
    })
}

fn attach_outer_doc_symbols(
    facts: &mut [StructuralFact],
    lines: &[DocCommentLine],
    symbols: &[Symbol],
) {
    for fact in facts {
        let fact_start_byte = fact.start_byte as usize;
        let Some(line) = lines.iter().find(|line| {
            !line.is_inner
                && line.text_start_byte <= fact_start_byte
                && fact_start_byte <= line.end_byte
        }) else {
            continue;
        };
        let Some(symbol) = symbols
            .iter()
            .filter(|symbol| {
                symbol.doc_comment.is_some() && (symbol.start_byte as usize) >= line.start_byte
            })
            .min_by_key(|symbol| (symbol.start_byte as usize).saturating_sub(line.start_byte))
        else {
            continue;
        };
        fact.containing_symbol_id = Some(symbol.id.clone());
    }
}
