use crate::base::{
    BaseExtractor, NormalizedSpan, Relationship, RelationshipKind, Symbol, SymbolKind,
};
use serde_json::Value;
use std::collections::{HashMap, HashSet};

/// `References` edges from the heading that holds an inline `[text](#anchor)`
/// link to the heading with that slug. Links come from the inline-link
/// symbols, so text in code blocks, code spans, and HTML never links.
pub(super) fn extract_relationships(base: &BaseExtractor, symbols: &[Symbol]) -> Vec<Relationship> {
    let headings = heading_symbols_by_slug(symbols);
    let mut relationships = Vec::new();
    let mut seen = HashSet::new();

    for link in symbols {
        let Some(metadata) = link.metadata.as_ref() else {
            continue;
        };
        if metadata.get("markdown_kind").and_then(Value::as_str) != Some("inline_link") {
            continue;
        }
        let Some(raw_anchor) = metadata
            .get("destination")
            .and_then(Value::as_str)
            .and_then(|destination| destination.strip_prefix('#'))
        else {
            continue;
        };
        let Some(target) = headings.get(&normalize_anchor(raw_anchor)) else {
            continue;
        };
        let Some(source) = containing_heading_at_line(symbols, link.start_line) else {
            continue;
        };
        push_relationship(
            base,
            source,
            target,
            link,
            raw_anchor,
            &mut seen,
            &mut relationships,
        );
    }

    relationships
}

fn containing_heading_at_line(symbols: &[Symbol], line_number: u32) -> Option<&Symbol> {
    symbols
        .iter()
        .filter(|symbol| {
            symbol.kind == SymbolKind::Module
                && symbol.start_line <= line_number
                && symbol.end_line >= line_number
        })
        .min_by_key(|symbol| symbol.end_line.saturating_sub(symbol.start_line))
}

fn heading_symbols_by_slug(symbols: &[Symbol]) -> HashMap<String, &Symbol> {
    let mut headings = HashMap::new();
    for symbol in symbols {
        if symbol.kind != SymbolKind::Module {
            continue;
        }
        headings
            .entry(slugify_heading(&symbol.name))
            .or_insert(symbol);
    }
    headings
}

fn push_relationship(
    base: &BaseExtractor,
    source: &Symbol,
    target: &Symbol,
    link: &Symbol,
    anchor: &str,
    seen: &mut HashSet<(String, String, u32, String)>,
    relationships: &mut Vec<Relationship>,
) {
    if source.id == target.id {
        return;
    }
    let line_number = link.start_line;

    let key = (
        source.id.clone(),
        target.id.clone(),
        line_number,
        anchor.to_string(),
    );
    if !seen.insert(key) {
        return;
    }

    let mut metadata = HashMap::new();
    metadata.insert("anchor".to_string(), Value::String(anchor.to_string()));

    relationships.push(Relationship {
        id: format!(
            "{}_{}_{:?}_{}_{}",
            source.id,
            target.id,
            RelationshipKind::References,
            line_number,
            anchor
        ),
        from_symbol_id: source.id.clone(),
        to_symbol_id: target.id.clone(),
        kind: RelationshipKind::References,
        file_path: base.file_path.clone(),
        line_number,
        span: Some(NormalizedSpan {
            start_line: link.start_line,
            start_column: link.start_column,
            end_line: link.end_line,
            end_column: link.end_column,
            start_byte: link.start_byte,
            end_byte: link.end_byte,
        }),
        reference_site_is_exact: true,
        confidence: 1.0,
        metadata: Some(metadata),
    });
}

fn normalize_anchor(anchor: &str) -> String {
    slugify_heading(&anchor.replace('-', " "))
}

fn slugify_heading(text: &str) -> String {
    let mut slug = String::new();
    let mut last_was_dash = false;

    for ch in text.chars().flat_map(char::to_lowercase) {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch);
            last_was_dash = false;
        } else if (ch.is_whitespace() || ch == '-') && !last_was_dash && !slug.is_empty() {
            slug.push('-');
            last_was_dash = true;
        }
    }

    slug.trim_matches('-').to_string()
}
