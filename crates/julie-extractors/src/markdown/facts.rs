//! Markdown facts for inline and extended constructs: reference-link usages,
//! footnote references and definitions, autolinks, task-list items, and
//! definition-list items.
//!
//! Link and footnote facts come from the symbols, so each fact matches the
//! symbol the extractor already reads from the inline grammar. Task-list and
//! definition-list facts come from the block tree.

use std::collections::HashMap;

use serde_json::Value;
use tree_sitter::{Node, Tree};

use super::blocks;
use crate::base::structural_fact_builders::{base_metadata, fact_for_span, insert_string};
use crate::base::{NormalizedSpan, StructuralFact, Symbol};
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};

pub(crate) const REFERENCE_LINK_PATTERN_ID: &str = "markdown.reference_link.v1";
pub(crate) const FOOTNOTE_REFERENCE_PATTERN_ID: &str = "markdown.footnote_reference.v1";
pub(crate) const FOOTNOTE_DEFINITION_PATTERN_ID: &str = "markdown.footnote_definition.v1";
pub(crate) const AUTOLINK_PATTERN_ID: &str = "markdown.autolink.v1";
pub(crate) const TASK_LIST_ITEM_PATTERN_ID: &str = "markdown.task_list_item.v1";
pub(crate) const DEFINITION_LIST_ITEM_PATTERN_ID: &str = "markdown.definition_list_item.v1";

pub(crate) fn markdown_facts(
    tree: &Tree,
    file_path: &str,
    content: &str,
    symbols: &[Symbol],
) -> Vec<StructuralFact> {
    let text = |symbol: &Symbol, key: &str| {
        symbol
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.get(key))
            .and_then(Value::as_str)
            .map(str::to_string)
    };
    let destinations: HashMap<String, String> = symbols
        .iter()
        .filter(|symbol| {
            text(symbol, "markdown_kind").as_deref() == Some("link_reference_definition")
        })
        .filter_map(|symbol| {
            Some((
                normalize_label(&text(symbol, "reference_label")?),
                text(symbol, "destination")?,
            ))
        })
        .collect();
    let mut facts = Vec::new();
    for symbol in symbols {
        let Some(kind) = text(symbol, "markdown_kind") else {
            continue;
        };
        let label = text(symbol, "reference_label");
        let (pattern_id, capture, metadata) = match kind.as_str() {
            "reference_link" => {
                let Some(label) = label else { continue };
                let mut metadata = base_metadata("document_links");
                insert_string(&mut metadata, "label", &label);
                if let Some(reference_kind) = text(symbol, "reference_kind") {
                    insert_string(&mut metadata, "reference_kind", &reference_kind);
                }
                if let Some(destination) = destinations.get(&normalize_label(&label)) {
                    insert_string(&mut metadata, "destination", destination);
                }
                (REFERENCE_LINK_PATTERN_ID, "reference_link", metadata)
            }
            "footnote_reference" => {
                let Some(label) = label else { continue };
                let mut metadata = base_metadata("document_links");
                insert_string(&mut metadata, "label", label.trim_start_matches('^'));
                (
                    FOOTNOTE_REFERENCE_PATTERN_ID,
                    "footnote_reference",
                    metadata,
                )
            }
            "footnote_definition" => {
                let Some(label) = label else { continue };
                let mut metadata = base_metadata("document_links");
                insert_string(&mut metadata, "label", label.trim_start_matches('^'));
                if let Some(body) = text(symbol, "destination") {
                    insert_string(&mut metadata, "text", &body);
                }
                (
                    FOOTNOTE_DEFINITION_PATTERN_ID,
                    "footnote_definition",
                    metadata,
                )
            }
            "autolink" => {
                let Some(destination) = text(symbol, "destination") else {
                    continue;
                };
                let mut metadata = base_metadata("document_links");
                insert_string(&mut metadata, "destination", &destination);
                if let Some(autolink_kind) = text(symbol, "autolink_kind") {
                    insert_string(&mut metadata, "autolink_kind", &autolink_kind);
                }
                (AUTOLINK_PATTERN_ID, "autolink", metadata)
            }
            _ => continue,
        };
        facts.push(fact_for_span(
            file_path,
            "markdown",
            pattern_id,
            capture,
            &kind,
            symbol_span(symbol),
            metadata,
        ));
    }
    collect_blocks(tree.root_node(), file_path, content, &mut facts, 0);
    facts
}

fn normalize_label(label: &str) -> String {
    label
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

fn symbol_span(symbol: &Symbol) -> NormalizedSpan {
    NormalizedSpan {
        start_line: symbol.start_line,
        start_column: symbol.start_column,
        end_line: symbol.end_line,
        end_column: symbol.end_column,
        start_byte: symbol.start_byte,
        end_byte: symbol.end_byte,
    }
}

fn collect_blocks(
    node: Node<'_>,
    file_path: &str,
    content: &str,
    facts: &mut Vec<StructuralFact>,
    depth: u32,
) {
    if !should_visit_tree_depth(depth) {
        return;
    }
    match node.kind() {
        "list_item" => facts.extend(task_list_item_fact(node, file_path, content)),
        "paragraph" => {
            for item in blocks::definition_items(content, node) {
                let Some(span) = NormalizedSpan::from_content_range(content, item.start, item.end)
                else {
                    continue;
                };
                let mut metadata = base_metadata("document_structure");
                insert_string(&mut metadata, "term", &item.term);
                insert_string(&mut metadata, "definition", &item.definition);
                facts.push(fact_for_span(
                    file_path,
                    "markdown",
                    DEFINITION_LIST_ITEM_PATTERN_ID,
                    "definition_list_item",
                    "paragraph",
                    span,
                    metadata,
                ));
            }
        }
        _ => {}
    }
    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_blocks(child, file_path, content, facts, child_depth);
    }
}

fn task_list_item_fact(item: Node<'_>, file_path: &str, content: &str) -> Option<StructuralFact> {
    let mut cursor = item.walk();
    let children: Vec<Node> = item.children(&mut cursor).collect();
    let checked = children.iter().find_map(|child| match child.kind() {
        "task_list_marker_checked" => Some(true),
        "task_list_marker_unchecked" => Some(false),
        _ => None,
    })?;
    let text = children
        .iter()
        .find(|child| child.kind() == "paragraph")
        .and_then(|paragraph| content.get(paragraph.byte_range()))
        .map(|text| text.split_whitespace().collect::<Vec<_>>().join(" "))
        .unwrap_or_default();
    let mut metadata = base_metadata("document_structure");
    metadata.insert("checked".to_string(), Value::Bool(checked));
    insert_string(&mut metadata, "text", &text);
    let end = item.start_byte()
        + content
            .get(item.byte_range())?
            .trim_end_matches(['\n', '\r'])
            .len();
    let span = NormalizedSpan::from_content_range(content, item.start_byte(), end)?;
    Some(fact_for_span(
        file_path,
        "markdown",
        TASK_LIST_ITEM_PATTERN_ID,
        "task_list_item",
        "list_item",
        span,
        metadata,
    ))
}
