//! Full extraction of a source block embedded in a host file (an HTML
//! `<script>` or `<style>`, a Vue SFC section, an event-handler attribute).
//!
//! The block runs through the native language pipeline on its own text, then
//! every row moves into host-file coordinates: spans, body spans, line
//! numbers, stable ids, and every reference to a remapped id.

use std::collections::HashMap;
use std::path::Path;

use tree_sitter::Tree;

use crate::base::relationship_resolution::{StructuredPendingRelationship, UnresolvedTarget};
use crate::base::{
    ComplexityMetric, EmbeddedSpanOffset, ExtractionLevel, ExtractionResults, Identifier,
    IdentifierKind, Literal, NormalizedSpan, Relationship, RelationshipKind, Symbol,
};

/// Extracts `source` as `language` and moves the rows to the host file, where
/// `source` starts at byte `host_byte_offset` of `host_content`.
pub(crate) fn extract_embedded(
    language: &str,
    source: &str,
    host_content: &str,
    host_byte_offset: usize,
    file_path: &str,
    workspace_root: &Path,
    level: ExtractionLevel,
) -> Option<ExtractionResults> {
    let mut parser = crate::pipeline::configured_parser_for_language(language).ok()?;
    let tree = parser.parse(source, None)?;
    extract_embedded_tree(
        language,
        &tree,
        source,
        host_content,
        host_byte_offset,
        file_path,
        workspace_root,
        level,
    )
}

/// [`extract_embedded`] for a block the caller already parsed.
#[allow(clippy::too_many_arguments)]
pub(crate) fn extract_embedded_tree(
    language: &str,
    tree: &Tree,
    source: &str,
    host_content: &str,
    host_byte_offset: usize,
    file_path: &str,
    workspace_root: &Path,
    level: ExtractionLevel,
) -> Option<ExtractionResults> {
    let offset = EmbeddedSpanOffset::from_host_byte(host_content, host_byte_offset)?;
    let mut results = crate::registry::extract_for_language_at(
        language,
        tree,
        file_path,
        source,
        workspace_root,
        level,
    )
    .ok()?;
    results
        .parse_diagnostics
        .extend(crate::pipeline::parse_diagnostics_for_tree(tree));
    remap_to_host(&mut results, offset);
    Some(results)
}

/// Identifiers and literals of JavaScript held in markup: an attribute value
/// or an interpolation whose text starts at host byte `start`. An object
/// literal is parsed inside parentheses laid over the byte before and after
/// the text, so offsets still map one to one. With `handler`, a value that is
/// only a name or member path (`save`, `app.save`) names the function the
/// event invokes and becomes a call.
pub(crate) fn extract_expression(
    host_content: &str,
    start: usize,
    text: &str,
    file_path: &str,
    handler: bool,
) -> Option<(Vec<Identifier>, Vec<Literal>)> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }
    let (source, source_start) = if trimmed.starts_with('{') && start > 0 {
        (format!("({text})"), start - 1)
    } else {
        (text.to_string(), start)
    };
    let results = extract_embedded(
        "javascript",
        &source,
        host_content,
        source_start,
        file_path,
        Path::new(""),
        ExtractionLevel::Full,
    )?;
    let reference_end =
        (handler && is_member_path(trimmed)).then(|| (start + text.trim_end().len()) as u32);
    let mut identifiers = results.identifiers;
    for identifier in &mut identifiers {
        identifier.containing_symbol_id = None;
        identifier.target_symbol_id = None;
        if reference_end == Some(identifier.end_byte) {
            identifier.kind = IdentifierKind::Call;
        }
    }
    let mut literals = results.literals;
    for literal in &mut literals {
        literal.containing_symbol_id = None;
    }
    Some((identifiers, literals))
}

/// `name` or `a.b.c`: identifier segments joined by dots.
pub(crate) fn is_member_path(value: &str) -> bool {
    !value.is_empty()
        && value.split('.').all(|segment| {
            let mut chars = segment.chars();
            chars
                .next()
                .is_some_and(|first| first.is_alphabetic() || first == '_' || first == '$')
                && chars.all(|ch| ch.is_alphanumeric() || ch == '_' || ch == '$')
        })
}

/// Links expression identifiers owned by `caller` to same-file symbols. A
/// bare call to a unique local callable gives a `calls` edge; any other bare
/// call gives a pending call. With `bindings`, a variable read of a unique
/// local binding gives a `references` edge. Member calls (`a.b()`) stay
/// identifiers only, as in JavaScript files.
pub(crate) fn link_expression_identifiers(
    host_content: &str,
    caller: &Symbol,
    identifiers: &[Identifier],
    callables: &HashMap<&str, Option<&Symbol>>,
    bindings: Option<&HashMap<&str, Option<&Symbol>>>,
    relationships: &mut Vec<Relationship>,
    pending: &mut Vec<StructuredPendingRelationship>,
) {
    for identifier in identifiers {
        let span = identifier_span(identifier);
        let target = match identifier.kind {
            IdentifierKind::Call => {
                let member_call = host_content
                    .get(..identifier.start_byte as usize)
                    .is_some_and(|prefix| prefix.trim_end().ends_with('.'));
                if member_call {
                    continue;
                }
                match callables.get(identifier.name.as_str()) {
                    Some(Some(target)) => Some((*target, RelationshipKind::Calls)),
                    _ => {
                        let mut row = StructuredPendingRelationship::new(
                            caller.id.clone(),
                            UnresolvedTarget::simple(identifier.name.clone()),
                            Some(caller.id.clone()),
                            RelationshipKind::Calls,
                            identifier.file_path.clone(),
                            span.start_line,
                            0.9,
                        );
                        row.span = Some(span);
                        row.reference_site_is_exact = true;
                        pending.push(row);
                        None
                    }
                }
            }
            IdentifierKind::VariableRef => bindings
                .and_then(|bindings| bindings.get(identifier.name.as_str()))
                .and_then(|target| *target)
                .map(|target| (target, RelationshipKind::References)),
            _ => None,
        };
        let Some((target, kind)) = target else {
            continue;
        };
        if target.id == caller.id {
            continue;
        }
        relationships.push(Relationship {
            id: format!(
                "{}_{}_{:?}_{}_{}",
                caller.id, target.id, kind, span.start_line, span.start_byte
            ),
            from_symbol_id: caller.id.clone(),
            to_symbol_id: target.id.clone(),
            kind,
            file_path: identifier.file_path.clone(),
            line_number: span.start_line,
            span: Some(span),
            reference_site_is_exact: true,
            confidence: 1.0,
            metadata: Some(HashMap::from([(
                "referenceName".to_string(),
                serde_json::Value::String(identifier.name.clone()),
            )])),
        });
    }
}

/// Symbols by name, `None` when the name is not unique.
pub(crate) fn unique_by_name<'a>(
    symbols: impl IntoIterator<Item = &'a Symbol>,
) -> HashMap<&'a str, Option<&'a Symbol>> {
    let mut by_name: HashMap<&str, Option<&Symbol>> = HashMap::new();
    for symbol in symbols {
        by_name
            .entry(symbol.name.as_str())
            .and_modify(|existing| *existing = None)
            .or_insert(Some(symbol));
    }
    by_name
}

fn identifier_span(identifier: &Identifier) -> NormalizedSpan {
    NormalizedSpan {
        start_line: identifier.start_line,
        start_column: identifier.start_column,
        end_line: identifier.end_line,
        end_column: identifier.end_column,
        start_byte: identifier.start_byte,
        end_byte: identifier.end_byte,
    }
}

fn remap_to_host(results: &mut ExtractionResults, offset: EmbeddedSpanOffset) {
    let mut symbol_ids = HashMap::new();
    for symbol in &mut results.symbols {
        let old_id = symbol.id.clone();
        symbol.apply_normalized_span(offset.apply(symbol_span(symbol)));
        symbol.body_span = symbol.body_span.map(|span| offset.apply(span));
        symbol.refresh_id();
        symbol_ids.insert(old_id, symbol.id.clone());
    }
    let map = |id: &mut String| {
        if let Some(new_id) = symbol_ids.get(id.as_str()) {
            *id = new_id.clone();
        }
    };
    let map_option = |id: &mut Option<String>| {
        if let Some(id) = id.as_mut()
            && let Some(new_id) = symbol_ids.get(id.as_str())
        {
            *id = new_id.clone();
        }
    };

    for symbol in &mut results.symbols {
        map_option(&mut symbol.parent_id);
    }
    for relationship in &mut results.relationships {
        let (old_from, old_to) = (
            relationship.from_symbol_id.clone(),
            relationship.to_symbol_id.clone(),
        );
        map(&mut relationship.from_symbol_id);
        map(&mut relationship.to_symbol_id);
        relationship.id = relationship
            .id
            .replace(&old_from, &relationship.from_symbol_id)
            .replace(&old_to, &relationship.to_symbol_id);
        relationship.line_number = offset.apply_line(relationship.line_number);
        relationship.span = relationship.span.map(|span| offset.apply(span));
    }
    for pending in &mut results.pending_relationships {
        map(&mut pending.from_symbol_id);
        pending.line_number = offset.apply_line(pending.line_number);
    }
    for structured in &mut results.structured_pending_relationships {
        map(&mut structured.pending.from_symbol_id);
        structured.pending.line_number = offset.apply_line(structured.pending.line_number);
        map_option(&mut structured.caller_scope_symbol_id);
        structured.span = structured.span.map(|span| offset.apply(span));
    }

    let mut identifier_ids = HashMap::new();
    for identifier in &mut results.identifiers {
        let old_id = identifier.id.clone();
        identifier.apply_normalized_span(offset.apply(NormalizedSpan {
            start_line: identifier.start_line,
            start_column: identifier.start_column,
            end_line: identifier.end_line,
            end_column: identifier.end_column,
            start_byte: identifier.start_byte,
            end_byte: identifier.end_byte,
        }));
        identifier.refresh_id();
        map_option(&mut identifier.containing_symbol_id);
        map_option(&mut identifier.target_symbol_id);
        identifier_ids.insert(old_id, identifier.id.clone());
    }
    for usage in &mut results.type_argument_usages {
        if let Some(new_id) = identifier_ids.get(&usage.identifier_id) {
            usage.identifier_id = new_id.clone();
        }
    }
    for literal in &mut results.literals {
        let span = offset.apply(NormalizedSpan {
            start_line: literal.start_line,
            start_column: literal.start_column,
            end_line: literal.end_line,
            end_column: literal.end_column,
            start_byte: literal.start_byte,
            end_byte: literal.end_byte,
        });
        literal.start_line = span.start_line;
        literal.start_column = span.start_column;
        literal.end_line = span.end_line;
        literal.end_column = span.end_column;
        literal.start_byte = span.start_byte;
        literal.end_byte = span.end_byte;
        literal.id =
            crate::base::types::stable_location_id(&literal.file_path, &literal.literal_text, span);
        map_option(&mut literal.containing_symbol_id);
    }
    for region in &mut results.source_regions {
        region.apply_normalized_span(offset.apply(NormalizedSpan {
            start_line: region.start_line,
            start_column: region.start_column,
            end_line: region.end_line,
            end_column: region.end_column,
            start_byte: region.start_byte,
            end_byte: region.end_byte,
        }));
        region.refresh_id();
        map_option(&mut region.containing_symbol_id);
    }
    for fact in &mut results.structural_facts {
        fact.apply_normalized_span(offset.apply(NormalizedSpan {
            start_line: fact.start_line,
            start_column: fact.start_column,
            end_line: fact.end_line,
            end_column: fact.end_column,
            start_byte: fact.start_byte,
            end_byte: fact.end_byte,
        }));
        fact.refresh_id();
        map_option(&mut fact.containing_symbol_id);
    }
    for metric in &mut results.complexity_metrics {
        metric.apply_normalized_span(offset.apply(NormalizedSpan {
            start_line: metric.start_line,
            start_column: metric.start_column,
            end_line: metric.end_line,
            end_column: metric.end_column,
            start_byte: metric.start_byte,
            end_byte: metric.end_byte,
        }));
        map_option(&mut metric.symbol_id);
        metric.refresh_id();
    }
    results.types = std::mem::take(&mut results.types)
        .into_iter()
        .map(|(mut id, mut info)| {
            map(&mut id);
            map(&mut info.symbol_id);
            (id, info)
        })
        .collect();
    for diagnostic in &mut results.parse_diagnostics {
        let span = offset.apply(NormalizedSpan {
            start_line: diagnostic.start_line,
            start_column: diagnostic.start_column,
            end_line: diagnostic.end_line,
            end_column: diagnostic.end_column,
            start_byte: diagnostic.start_byte,
            end_byte: diagnostic.end_byte,
        });
        diagnostic.start_line = span.start_line;
        diagnostic.start_column = span.start_column;
        diagnostic.end_line = span.end_line;
        diagnostic.end_column = span.end_column;
        diagnostic.start_byte = span.start_byte;
        diagnostic.end_byte = span.end_byte;
    }
}

/// Drops the symbols `keep` rejects. Rows that pointed at a dropped symbol
/// move to its nearest kept ancestor; relationships, pending rows, types, and
/// complexity rows owned by a dropped symbol with no kept ancestor go too.
pub(crate) fn retain_symbols(results: &mut ExtractionResults, keep: impl Fn(&Symbol) -> bool) {
    let parents: HashMap<String, Option<String>> = results
        .symbols
        .iter()
        .map(|symbol| (symbol.id.clone(), symbol.parent_id.clone()))
        .collect();
    let dropped: HashMap<String, ()> = results
        .symbols
        .iter()
        .filter(|symbol| !keep(symbol))
        .map(|symbol| (symbol.id.clone(), ()))
        .collect();
    if dropped.is_empty() {
        return;
    }
    let replacement = |id: &str| -> Option<Option<String>> {
        if !dropped.contains_key(id) {
            return None;
        }
        let mut current = parents.get(id).cloned().flatten();
        while let Some(candidate) = current {
            if !dropped.contains_key(&candidate) {
                return Some(Some(candidate));
            }
            current = parents.get(&candidate).cloned().flatten();
        }
        Some(None)
    };
    let rehome = |id: &mut Option<String>| {
        if let Some(current) = id.as_deref()
            && let Some(new_id) = replacement(current)
        {
            *id = new_id;
        }
    };
    let rehome_required = |id: &mut String| -> bool {
        match replacement(id) {
            None => true,
            Some(Some(new_id)) => {
                *id = new_id;
                true
            }
            Some(None) => false,
        }
    };

    results
        .symbols
        .retain(|symbol| !dropped.contains_key(&symbol.id));
    for symbol in &mut results.symbols {
        rehome(&mut symbol.parent_id);
    }
    results.relationships.retain_mut(|relationship| {
        rehome_required(&mut relationship.from_symbol_id)
            && rehome_required(&mut relationship.to_symbol_id)
            && relationship.from_symbol_id != relationship.to_symbol_id
    });
    results
        .pending_relationships
        .retain_mut(|pending| rehome_required(&mut pending.from_symbol_id));
    results
        .structured_pending_relationships
        .retain_mut(|pending| {
            rehome(&mut pending.caller_scope_symbol_id);
            rehome_required(&mut pending.pending.from_symbol_id)
        });
    for identifier in &mut results.identifiers {
        rehome(&mut identifier.containing_symbol_id);
        rehome(&mut identifier.target_symbol_id);
    }
    for literal in &mut results.literals {
        rehome(&mut literal.containing_symbol_id);
    }
    for region in &mut results.source_regions {
        rehome(&mut region.containing_symbol_id);
    }
    for fact in &mut results.structural_facts {
        rehome(&mut fact.containing_symbol_id);
    }
    results.complexity_metrics.retain(|metric| {
        metric
            .symbol_id
            .as_deref()
            .is_none_or(|id| !dropped.contains_key(id))
    });
    results.types.retain(|id, _| !dropped.contains_key(id));
}

/// Appends every row of `embedded` to `host`.
pub(crate) fn merge_into(host: &mut ExtractionResults, embedded: ExtractionResults) {
    host.symbols.extend(embedded.symbols);
    host.relationships.extend(embedded.relationships);
    host.pending_relationships
        .extend(embedded.pending_relationships);
    host.structured_pending_relationships
        .extend(embedded.structured_pending_relationships);
    host.types.extend(embedded.types);
    host.identifiers.extend(embedded.identifiers);
    host.type_argument_usages
        .extend(embedded.type_argument_usages);
    host.literals.extend(embedded.literals);
    host.source_regions.extend(embedded.source_regions);
    host.structural_facts.extend(embedded.structural_facts);
    host.complexity_metrics.extend(embedded.complexity_metrics);
    host.parse_diagnostics.extend(embedded.parse_diagnostics);
}

/// Folds the file-scope complexity rows of several embedded blocks into one
/// file row for the host: counts add up, nesting takes the maximum, and the
/// span runs from the first block to the last.
pub(crate) fn merge_file_complexity(
    metrics: &mut Vec<ComplexityMetric>,
    file_path: &str,
    host_language: &str,
) {
    let (file_rows, symbol_rows): (Vec<_>, Vec<_>) = std::mem::take(metrics)
        .into_iter()
        .partition(|metric| metric.scope == "file");
    *metrics = symbol_rows;
    let Some(mut merged) = file_rows.first().cloned() else {
        return;
    };
    for row in &file_rows[1..] {
        merged.decision_count += row.decision_count;
        merged.loop_count += row.loop_count;
        merged.max_nesting_depth = merged.max_nesting_depth.max(row.max_nesting_depth);
        if row.start_byte < merged.start_byte {
            (merged.start_line, merged.start_column, merged.start_byte) =
                (row.start_line, row.start_column, row.start_byte);
        }
        if row.end_byte > merged.end_byte {
            (merged.end_line, merged.end_column, merged.end_byte) =
                (row.end_line, row.end_column, row.end_byte);
        }
    }
    merged.file_path = file_path.to_string();
    merged.language = host_language.to_string();
    merged.covered_lines = merged.end_line.saturating_sub(merged.start_line) + 1;
    merged.covered_bytes = merged.end_byte.saturating_sub(merged.start_byte);
    merged.refresh_id();
    metrics.insert(0, merged);
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
