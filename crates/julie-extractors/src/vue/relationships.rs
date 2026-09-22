//! Rows the component itself owns: template bindings, component tags, and
//! calls made at the top level of a script block.

use super::parsing::ParsedVueSfc;
use crate::base::markup_scan::scan_markup_attributes;
use crate::base::relationship_resolution::{StructuredPendingRelationship, UnresolvedTarget};
use crate::base::{
    BaseExtractor, Identifier, Literal, NormalizedSpan, Relationship, RelationshipKind, Symbol,
    SymbolKind,
};
use crate::embedded::{extract_expression, link_expression_identifiers, unique_by_name};
use regex::Regex;
use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;

static COMPONENT_TAG_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"<([A-Za-z][A-Za-z0-9_-]*)\b").unwrap());

const OPTION_GROUPS: [&str; 5] = ["props", "computed", "methods", "inject", "data"];

#[derive(Default)]
pub(super) struct ComponentRows {
    pub(super) identifiers: Vec<Identifier>,
    pub(super) literals: Vec<Literal>,
    pub(super) relationships: Vec<Relationship>,
    pub(super) pending: Vec<StructuredPendingRelationship>,
}

/// `script_identifiers` are the identifiers of every script block. Code
/// outside any function or class runs as the component's setup. Its caller is
/// the innermost declaration around it, as in a standalone script, and the
/// component only when no declaration encloses it.
pub(super) fn collect_component_rows(
    base: &BaseExtractor,
    sfc: &ParsedVueSfc,
    symbols: &[Symbol],
    script_symbol_ids: &HashSet<String>,
    script_identifiers: &mut [Identifier],
) -> ComponentRows {
    let mut rows = ComponentRows::default();
    let Some(component) = symbols.iter().find(|symbol| is_component(symbol)) else {
        return rows;
    };
    let bindings = component_bindings(symbols, script_symbol_ids);
    let binding_map = unique_by_name(bindings.iter().copied());
    let callables = unique_by_name(bindings.iter().copied().filter(|symbol| {
        matches!(
            symbol.kind,
            SymbolKind::Function | SymbolKind::Method | SymbolKind::Variable
        )
    }));

    let by_id: HashMap<&str, &Symbol> = symbols
        .iter()
        .map(|symbol| (symbol.id.as_str(), symbol))
        .collect();
    let mut top_level: Vec<(&Symbol, Identifier)> = Vec::new();
    for identifier in script_identifiers.iter_mut() {
        if identifier.containing_symbol_id.is_none() {
            identifier.containing_symbol_id = Some(component.id.clone());
            top_level.push((component, identifier.clone()));
        } else if !inside_callable(&by_id, identifier.containing_symbol_id.as_deref()) {
            let owner = identifier
                .containing_symbol_id
                .as_deref()
                .and_then(|id| by_id.get(id).copied())
                .unwrap_or(component);
            top_level.push((owner, identifier.clone()));
        }
    }
    for (owner, identifier) in &top_level {
        link_expression_identifiers(
            &base.content,
            owner,
            std::slice::from_ref(identifier),
            &callables,
            None,
            &mut rows.relationships,
            &mut rows.pending,
        );
    }

    for section in sfc
        .sections
        .iter()
        .filter(|section| section.section_type == "template")
    {
        let start = section.content_start;
        let end = start + section.content.len();
        let mut expressions = template_attribute_expressions(&base.content, start, end);
        expressions.extend(interpolations(&base.content, start, end));
        for (value_start, text, handler) in expressions {
            let Some((mut identifiers, literals)) =
                extract_expression(&base.content, value_start, text, &base.file_path, handler)
            else {
                continue;
            };
            for identifier in &mut identifiers {
                identifier.language = base.language.clone();
                identifier.containing_symbol_id = Some(component.id.clone());
            }
            link_expression_identifiers(
                &base.content,
                component,
                &identifiers,
                &callables,
                Some(&binding_map),
                &mut rows.relationships,
                &mut rows.pending,
            );
            rows.identifiers.extend(identifiers);
            rows.literals
                .extend(literals.into_iter().map(|mut literal| {
                    literal.language = base.language.clone();
                    literal.containing_symbol_id = Some(component.id.clone());
                    literal
                }));
        }
        component_tag_pending(base, component, symbols, start, end, &mut rows.pending);
    }
    rows
}

fn inside_callable(by_id: &HashMap<&str, &Symbol>, id: Option<&str>) -> bool {
    let mut current = id.and_then(|id| by_id.get(id).copied());
    while let Some(symbol) = current {
        if matches!(
            symbol.kind,
            SymbolKind::Function | SymbolKind::Method | SymbolKind::Constructor | SymbolKind::Class
        ) {
            return true;
        }
        current = symbol
            .parent_id
            .as_deref()
            .and_then(|parent| by_id.get(parent).copied());
    }
    false
}

pub(super) fn is_component(symbol: &Symbol) -> bool {
    symbol.kind == SymbolKind::Class
        && symbol.metadata.as_ref().is_some_and(|metadata| {
            metadata.get("type").and_then(|value| value.as_str()) == Some("vue-sfc")
        })
}

/// Script names the template can see: top-level value declarations and the
/// members of the Options API groups of `export default`. Type-literal members
/// such as the fields of `defineProps<{ title: string }>()` have no parent but
/// are not bindings.
fn component_bindings<'a>(
    symbols: &'a [Symbol],
    script_symbol_ids: &HashSet<String>,
) -> Vec<&'a Symbol> {
    let script: Vec<&Symbol> = symbols
        .iter()
        .filter(|symbol| script_symbol_ids.contains(&symbol.id))
        .collect();
    let default_exports: HashSet<&str> = script
        .iter()
        .filter(|symbol| symbol.kind == SymbolKind::Export && symbol.name == "default")
        .map(|symbol| symbol.id.as_str())
        .collect();
    let groups: HashSet<&str> = script
        .iter()
        .filter(|symbol| {
            OPTION_GROUPS.contains(&symbol.name.as_str())
                && symbol
                    .parent_id
                    .as_deref()
                    .is_some_and(|parent| default_exports.contains(parent))
        })
        .map(|symbol| symbol.id.as_str())
        .collect();
    script
        .into_iter()
        .filter(|symbol| {
            let is_parameter = symbol.metadata.as_ref().is_some_and(|metadata| {
                metadata.get("role").and_then(|value| value.as_str()) == Some("parameter")
            });
            if is_parameter {
                return false;
            }
            match symbol.parent_id.as_deref() {
                Some(parent) => groups.contains(parent),
                None => matches!(
                    symbol.kind,
                    SymbolKind::Variable
                        | SymbolKind::Constant
                        | SymbolKind::Function
                        | SymbolKind::Class
                        | SymbolKind::Enum
                        | SymbolKind::Import
                ),
            }
        })
        .collect()
}

/// `(value start, value text, is event handler)` for directive attributes.
/// `v-for` contributes only its source expression; slot props declare names
/// and contribute nothing.
fn template_attribute_expressions(
    content: &str,
    start: usize,
    end: usize,
) -> Vec<(usize, &str, bool)> {
    let mut expressions = Vec::new();
    for attribute in scan_markup_attributes(content, start, end) {
        let name = attribute.name.as_str();
        let handler = name.starts_with('@') || name.starts_with("v-on:");
        let expression = handler
            || name.starts_with(':')
            || name.starts_with("v-bind")
            || name.starts_with("v-model")
            || matches!(
                name,
                "v-if" | "v-else-if" | "v-show" | "v-html" | "v-text" | "v-memo" | "v-for"
            );
        if !expression {
            continue;
        }
        let Some((value_start, value)) =
            attribute_value_range(content, attribute.start_byte, attribute.end_byte)
        else {
            continue;
        };
        if name == "v-for" {
            if let Some((source_start, source)) = v_for_source(value) {
                expressions.push((value_start + source_start, source, false));
            }
        } else {
            expressions.push((value_start, value, handler));
        }
    }
    expressions
}

fn attribute_value_range(content: &str, start: usize, end: usize) -> Option<(usize, &str)> {
    let segment = content.get(start..end)?;
    let equals = segment.find('=')?;
    let after = &segment[equals + 1..];
    let leading = after.len() - after.trim_start().len();
    let value_start = start + equals + 1 + leading;
    let rest = content.get(value_start..end)?;
    match rest.chars().next()? {
        quote @ ('"' | '\'') => {
            let inner = &rest[1..];
            let close = inner.find(quote).unwrap_or(inner.len());
            Some((value_start + 1, &inner[..close]))
        }
        _ => Some((value_start, rest.trim_end())),
    }
}

/// `item in items` or `(item, index) of items`: the part after `in`/`of`.
fn v_for_source(value: &str) -> Option<(usize, &str)> {
    [" in ", " of "]
        .iter()
        .filter_map(|separator| value.find(separator).map(|index| index + separator.len()))
        .min()
        .map(|start| (start, &value[start..]))
}

/// `{{ expression }}` text interpolations.
fn interpolations(content: &str, start: usize, end: usize) -> Vec<(usize, &str, bool)> {
    let mut expressions = Vec::new();
    let mut cursor = start;
    while let Some(open) = content.get(cursor..end).and_then(|rest| rest.find("{{")) {
        let expression_start = cursor + open + 2;
        let Some(close) = content
            .get(expression_start..end)
            .and_then(|rest| rest.find("}}"))
        else {
            break;
        };
        let expression_end = expression_start + close;
        expressions.push((
            expression_start,
            &content[expression_start..expression_end],
            false,
        ));
        cursor = expression_end + 2;
    }
    expressions
}

/// A PascalCase or kebab-case tag with no local definition is a pending
/// reference to a component defined elsewhere.
fn component_tag_pending(
    base: &BaseExtractor,
    component: &Symbol,
    symbols: &[Symbol],
    start: usize,
    end: usize,
    pending: &mut Vec<StructuredPendingRelationship>,
) {
    let local: HashMap<&str, ()> = symbols
        .iter()
        .filter(|symbol| symbol.kind != SymbolKind::Import)
        .map(|symbol| (symbol.name.as_str(), ()))
        .collect();
    let mut seen = HashSet::new();
    let Some(template) = base.content.get(start..end) else {
        return;
    };
    for captures in COMPONENT_TAG_RE.captures_iter(template) {
        let Some(tag) = captures.get(1) else {
            continue;
        };
        let name = tag.as_str();
        let is_component_tag = name
            .chars()
            .next()
            .is_some_and(|ch| ch.is_ascii_uppercase())
            || name.contains('-');
        if !is_component_tag || name == "template" || local.contains_key(name) {
            continue;
        }
        let Some(span) = NormalizedSpan::from_content_range(
            &base.content,
            start + tag.start(),
            start + tag.end(),
        ) else {
            continue;
        };
        if !seen.insert((name.to_string(), span.start_line)) {
            continue;
        }
        let mut row = StructuredPendingRelationship::new(
            component.id.clone(),
            UnresolvedTarget::simple(name),
            Some(component.id.clone()),
            RelationshipKind::References,
            base.file_path.clone(),
            span.start_line,
            1.0,
        );
        row.span = Some(span);
        row.reference_site_is_exact = true;
        pending.push(row);
    }
}
