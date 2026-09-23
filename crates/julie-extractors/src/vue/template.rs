use super::manual_symbols::create_symbol_manual;
use super::parsing::VueSection;
use crate::base::{BaseExtractor, Symbol, SymbolKind};
use serde_json::Value;
use std::collections::HashMap;

pub(super) fn extract_template_symbols(base: &BaseExtractor, section: &VueSection) -> Vec<Symbol> {
    let section_offset = section.content_start;

    let mut symbols = Vec::new();
    for (line_start, line) in template_lines(&section.content) {
        extract_attribute_symbol(
            base,
            section_offset + line_start,
            line,
            "ref",
            SymbolKind::Variable,
            &mut symbols,
        );
        extract_attribute_symbol(
            base,
            section_offset + line_start,
            line,
            "v-model",
            SymbolKind::Property,
            &mut symbols,
        );
        if line.contains("<slot") {
            extract_attribute_symbol(
                base,
                section_offset + line_start,
                line,
                "name",
                SymbolKind::Event,
                &mut symbols,
            );
        }
    }
    symbols
}

fn template_lines(content: &str) -> impl Iterator<Item = (usize, &str)> {
    content.lines().scan(0usize, |offset, line| {
        let current = *offset;
        *offset += line.len() + 1;
        Some((current, line))
    })
}

fn extract_attribute_symbol(
    base: &BaseExtractor,
    absolute_line_start: usize,
    line: &str,
    attribute: &str,
    kind: SymbolKind,
    symbols: &mut Vec<Symbol>,
) {
    let pattern = format!("{}=\"", attribute);
    let mut search_start = 0usize;

    while let Some(relative) = line[search_start..].find(&pattern) {
        let value_start = search_start + relative + pattern.len();
        let Some(value_end_relative) = line[value_start..].find('"') else {
            break;
        };
        let value_end = value_start + value_end_relative;
        let name = &line[value_start..value_end];
        if is_declared_name(attribute, name) {
            let start_byte = absolute_line_start + value_start;
            let end_byte = absolute_line_start + value_end;
            let metadata = HashMap::from([(
                "type".to_string(),
                Value::String(format!("template-{}", attribute)),
            )]);
            if let Some(mut symbol) = create_symbol_manual(
                base,
                name,
                kind.clone(),
                start_byte,
                end_byte,
                Some(format!("{}=\"{}\"", attribute, name)),
                None,
                Some(metadata),
            ) {
                symbol.body_span = None;
                symbol.body_hash = None;
                symbols.push(symbol);
            }
        }
        search_start = value_end.saturating_add(1);
    }
}

/// A template `ref` or `v-model` declares a plain identifier; a slot name may
/// also hold dashes. Expressions such as `form.email` declare nothing.
fn is_declared_name(attribute: &str, name: &str) -> bool {
    let mut chars = name.chars();
    chars
        .next()
        .is_some_and(|first| first.is_ascii_alphabetic() || first == '_' || first == '$')
        && chars.all(|c| {
            c.is_ascii_alphanumeric() || c == '_' || c == '$' || (attribute == "name" && c == '-')
        })
}

/// Whether a template symbol declares a name that a script binding owns.
pub(super) fn declares_script_binding(symbol: &Symbol) -> bool {
    symbol
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get("type"))
        .and_then(Value::as_str)
        .is_some_and(|kind| matches!(kind, "template-ref" | "template-v-model"))
}
