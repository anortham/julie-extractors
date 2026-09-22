//! Links between parts of a Markdown document, and to headings of other
//! documents.
//!
//! - `[text](#anchor)` (and `<a href="#anchor">` in an HTML block) is a
//!   `References` edge to the heading with that anchor. A heading's anchor is
//!   its explicit `{#id}`, else its GitHub slug, with `-1`, `-2`, ... on
//!   repeated slugs.
//! - A reference link (`[text][label]`, `[label][]`, `[label]`) is a
//!   `References` edge to its link definition, and a footnote reference
//!   (`[^label]`) is one to its footnote definition.
//! - `[text](other.md#anchor)` is a structured pending `References` row whose
//!   terminal name is the anchor and whose import context is the path.
//!
//! The source of each row is the heading whose section holds the link, else
//! the link symbol itself.

use crate::base::{
    BaseExtractor, NormalizedSpan, Relationship, RelationshipKind, StructuredPendingRelationship,
    Symbol, SymbolKind, UnresolvedTarget,
};
use regex::Regex;
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

pub(super) fn extract_relationships(
    base: &mut BaseExtractor,
    symbols: &[Symbol],
) -> Vec<Relationship> {
    let anchors = heading_anchors(symbols);
    let definitions = definitions_by_label(symbols);
    let mut relationships = Vec::new();
    let mut seen = HashSet::new();

    for link in symbols {
        let Some(metadata) = link.metadata.as_ref() else {
            continue;
        };
        let kind = metadata.get("markdown_kind").and_then(Value::as_str);
        let destination = metadata.get("destination").and_then(Value::as_str);
        let source = link_owner(symbols, link);
        match (kind, destination) {
            (Some("inline_link" | "link_reference_definition"), Some(destination)) => {
                let Some((path, anchor)) = destination.split_once('#') else {
                    continue;
                };
                if anchor.is_empty() {
                    continue;
                }
                let anchor = percent_decode(anchor);
                if path.is_empty() {
                    if let Some(target) = find_anchor(&anchors, &anchor) {
                        push_relationship(
                            base,
                            source,
                            target,
                            link,
                            ("anchor", &anchor),
                            &mut seen,
                            &mut relationships,
                        );
                    }
                } else if is_local_path(path) {
                    push_pending(base, source, link, path, &anchor);
                }
            }
            (Some("reference_link" | "footnote_reference"), _) => {
                let Some(label) = metadata.get("reference_label").and_then(Value::as_str) else {
                    continue;
                };
                if let Some(target) = definitions.get(&normalize_label(label)) {
                    push_relationship(
                        base,
                        source,
                        target,
                        link,
                        ("reference_label", label),
                        &mut seen,
                        &mut relationships,
                    );
                }
            }
            _ => {}
        }
    }

    relationships
}

/// The heading whose section holds the link, else the link itself.
fn link_owner<'a>(symbols: &'a [Symbol], link: &'a Symbol) -> &'a Symbol {
    super::containing_heading(symbols, link.start_byte).unwrap_or(link)
}

fn definitions_by_label(symbols: &[Symbol]) -> HashMap<String, &Symbol> {
    let mut definitions = HashMap::new();
    for symbol in symbols {
        let Some(metadata) = symbol.metadata.as_ref() else {
            continue;
        };
        if !matches!(
            metadata.get("markdown_kind").and_then(Value::as_str),
            Some("link_reference_definition" | "footnote_definition")
        ) {
            continue;
        }
        if let Some(label) = metadata.get("reference_label").and_then(Value::as_str) {
            definitions.entry(normalize_label(label)).or_insert(symbol);
        }
    }
    definitions
}

/// CommonMark label matching: case-insensitive, whitespace runs collapsed.
fn normalize_label(label: &str) -> String {
    label
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// Every heading's anchor, in document order: its explicit id, else its
/// GitHub slug with a `-N` suffix from the second use of a slug on.
fn heading_anchors(symbols: &[Symbol]) -> HashMap<String, &Symbol> {
    let mut headings: Vec<&Symbol> = symbols
        .iter()
        .filter(|symbol| symbol.kind == SymbolKind::Module)
        .collect();
    headings.sort_by_key(|symbol| symbol.start_byte);
    let mut anchors = HashMap::new();
    let mut used: HashMap<String, usize> = HashMap::new();
    for heading in headings {
        if let Some(explicit) = heading
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.get("anchor"))
            .and_then(Value::as_str)
        {
            anchors.entry(explicit.to_string()).or_insert(heading);
        }
        let base_slug = github_slug(&heading.name);
        let count = used.entry(base_slug.clone()).or_insert(0);
        let slug = if *count == 0 {
            base_slug.clone()
        } else {
            format!("{base_slug}-{count}")
        };
        *count += 1;
        anchors.entry(slug).or_insert(heading);
    }
    anchors
}

fn find_anchor<'a>(anchors: &HashMap<String, &'a Symbol>, anchor: &str) -> Option<&'a Symbol> {
    anchors
        .get(anchor)
        .or_else(|| anchors.get(&anchor.to_lowercase()))
        .copied()
}

/// GitHub's heading slug: lowercase, drop every character that is not a
/// letter, mark, number, connector punctuation, space, or hyphen, then turn
/// each space into a hyphen.
fn github_slug(text: &str) -> String {
    static DROPPED: OnceLock<Regex> = OnceLock::new();
    let dropped =
        DROPPED.get_or_init(|| Regex::new(r"[^\p{L}\p{M}\p{N}\p{Pc} -]").expect("slug pattern"));
    dropped
        .replace_all(&text.trim().to_lowercase(), "")
        .replace(' ', "-")
}

fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%'
            && let Some(hex) = text.get(index + 1..index + 3)
            && let Ok(value) = u8::from_str_radix(hex, 16)
        {
            out.push(value);
            index += 3;
            continue;
        }
        out.push(bytes[index]);
        index += 1;
    }
    String::from_utf8(out).unwrap_or_else(|_| text.to_string())
}

fn is_local_path(path: &str) -> bool {
    !path.contains("://")
        && !path.starts_with("mailto:")
        && !path.starts_with("//")
        && !path.contains(char::is_whitespace)
}

fn link_span(link: &Symbol) -> NormalizedSpan {
    NormalizedSpan {
        start_line: link.start_line,
        start_column: link.start_column,
        end_line: link.end_line,
        end_column: link.end_column,
        start_byte: link.start_byte,
        end_byte: link.end_byte,
    }
}

fn push_relationship(
    base: &BaseExtractor,
    source: &Symbol,
    target: &Symbol,
    link: &Symbol,
    (key, value): (&str, &str),
    seen: &mut HashSet<(String, String, u32)>,
    relationships: &mut Vec<Relationship>,
) {
    if source.id == target.id
        || !seen.insert((source.id.clone(), target.id.clone(), link.start_byte))
    {
        return;
    }
    relationships.push(Relationship {
        id: format!(
            "{}_{}_{:?}_{}",
            source.id,
            target.id,
            RelationshipKind::References,
            link.start_byte
        ),
        from_symbol_id: source.id.clone(),
        to_symbol_id: target.id.clone(),
        kind: RelationshipKind::References,
        file_path: base.file_path.clone(),
        line_number: link.start_line,
        span: Some(link_span(link)),
        reference_site_is_exact: true,
        confidence: 1.0,
        metadata: Some(HashMap::from([(
            key.to_string(),
            Value::String(value.to_string()),
        )])),
    });
}

fn push_pending(
    base: &mut BaseExtractor,
    source: &Symbol,
    link: &Symbol,
    path: &str,
    anchor: &str,
) {
    let pending = StructuredPendingRelationship::new(
        source.id.clone(),
        UnresolvedTarget {
            display_name: format!("{path}#{anchor}"),
            terminal_name: anchor.to_string(),
            receiver: None,
            namespace_path: Vec::new(),
            import_context: Some(path.to_string()),
        },
        Some(source.id.clone()),
        RelationshipKind::References,
        base.file_path.clone(),
        link.start_line,
        1.0,
    )
    .with_target_span(link_span(link));
    base.add_structured_pending_relationship(pending);
}
