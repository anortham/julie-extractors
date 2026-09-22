//! JSON Schema `$ref` relationship extraction (Phase 3.2).
//!
//! - **Local `$ref`** (`#/$defs/Address`) → concrete `Relationship` from the
//!   symbol that owns the containing object (its pair, or its array-element
//!   container) to the resolved target symbol, kind `References`. A `$ref` in
//!   the root object uses its own `$ref` symbol as the source. If the JSON pointer cannot be resolved, no edge is emitted
//!   (the pointer is malformed, not "deferred to another file").
//! - **External `$ref`** (`<file>#/$defs/Address`) →
//!   `StructuredPendingRelationship` carrying
//!   `target.import_context = Some("<file>")`,
//!   `target.terminal_name` = last fragment segment,
//!   `target.namespace_path` = preceding fragment segments,
//!   `target.display_name` = original `$ref` text,
//!   `caller_scope_symbol_id` = the same source symbol id.
//!
//! AST shape under tree-sitter-json: a `pair` whose first child is a string
//! key `"$ref"` and whose last child is a string value carrying the pointer.

use crate::base::{
    BaseExtractor, Relationship, RelationshipKind, StructuredPendingRelationship, Symbol,
    UnresolvedTarget,
};
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};
use std::collections::HashMap;
use tree_sitter::Node;

pub(super) fn extract_relationships_internal(
    base: &mut BaseExtractor,
    node: Node,
    symbols: &[Symbol],
    relationships: &mut Vec<Relationship>,
    depth: u32,
) {
    if !should_visit_tree_depth(depth) {
        return;
    }

    if node.kind() == "pair"
        && let Some((value_text, value_node)) = ref_pair_value(base, node)
    {
        handle_ref_pair(base, node, value_node, &value_text, symbols, relationships);
    }
    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };
    for child in node.children(&mut node.walk()) {
        extract_relationships_internal(base, child, symbols, relationships, child_depth);
    }
}

/// If the pair's key is the literal `"$ref"` and its value is a string,
/// return the unquoted value text plus the value node.
fn ref_pair_value<'a>(base: &BaseExtractor, pair: Node<'a>) -> Option<(String, Node<'a>)> {
    let mut cursor = pair.walk();
    let children: Vec<Node<'a>> = pair.children(&mut cursor).collect();
    if children.len() < 3 {
        return None;
    }
    let key_text = base.get_node_text(&children[0]);
    if key_text.trim_matches('"') != "$ref" {
        return None;
    }
    let value_node = *children.last()?;
    if value_node.kind() != "string" {
        return None;
    }
    let raw = base.get_node_text(&value_node);
    Some((raw.trim_matches('"').to_string(), value_node))
}

fn handle_ref_pair(
    base: &mut BaseExtractor,
    pair: Node,
    value_node: Node,
    value_text: &str,
    symbols: &[Symbol],
    relationships: &mut Vec<Relationship>,
) {
    let Some(from_symbol) = ref_source_symbol(symbols, pair) else {
        return;
    };
    emit_schema_ref(
        base,
        from_symbol,
        value_node,
        value_text,
        symbols,
        relationships,
    );
}

/// Emit the edge for one JSON Schema `$ref` value: a `References` relationship
/// for a resolvable local pointer, or a structured pending row for a pointer
/// into another file. Shared by the JSON and YAML extractors.
pub(crate) fn emit_schema_ref(
    base: &mut BaseExtractor,
    from_symbol: &Symbol,
    value_node: Node,
    value_text: &str,
    symbols: &[Symbol],
    relationships: &mut Vec<Relationship>,
) {
    let line_number = value_node.start_position().row as u32 + 1;

    if let Some((file_part, fragment)) = split_external_ref(value_text) {
        emit_external_pending(
            base,
            from_symbol,
            value_text,
            file_part,
            fragment,
            line_number,
        );
    } else if let Some(fragment) = value_text.strip_prefix("#/")
        && let Some(target_symbol) = resolve_local_pointer(symbols, fragment)
    {
        emit_local_relationship(
            base,
            from_symbol,
            target_symbol,
            value_node,
            line_number,
            relationships,
        );
    }
    // Unresolved local pointer = malformed; emit nothing.
    // Other shapes (e.g., bare relative URIs, missing `#`) are out of scope.
}

fn split_external_ref(value: &str) -> Option<(&str, &str)> {
    let idx = value.find('#')?;
    if idx == 0 {
        return None; // pure local pointer
    }
    let (file, rest) = value.split_at(idx);
    // Skip the `#` itself; tolerate either `#/...` or `#...` shape.
    let fragment = rest.trim_start_matches('#').trim_start_matches('/');
    Some((file, fragment))
}

fn emit_external_pending(
    base: &mut BaseExtractor,
    from_symbol: &Symbol,
    raw_ref: &str,
    file_part: &str,
    fragment: &str,
    line_number: u32,
) {
    let segments: Vec<&str> = if fragment.is_empty() {
        Vec::new()
    } else {
        fragment.split('/').collect()
    };
    let (terminal_name, namespace_path) = match segments.as_slice() {
        [] => return,
        [name] => ((*name).to_string(), Vec::new()),
        _ => (
            segments.last().expect("non-empty after match").to_string(),
            segments[..segments.len() - 1]
                .iter()
                .map(|s| (*s).to_string())
                .collect(),
        ),
    };
    let target = UnresolvedTarget {
        display_name: raw_ref.to_string(),
        terminal_name,
        receiver: None,
        namespace_path,
        import_context: Some(file_part.to_string()),
    };
    let pending = StructuredPendingRelationship::new(
        from_symbol.id.clone(),
        target,
        Some(from_symbol.id.clone()),
        RelationshipKind::References,
        base.file_path.clone(),
        line_number,
        1.0,
    );
    base.add_structured_pending_relationship(pending);
}

fn emit_local_relationship(
    base: &BaseExtractor,
    from_symbol: &Symbol,
    target_symbol: &Symbol,
    value_node: Node,
    line_number: u32,
    relationships: &mut Vec<Relationship>,
) {
    let mut metadata = HashMap::new();
    metadata.insert(
        "refKind".to_string(),
        serde_json::Value::String("json_schema_ref".to_string()),
    );
    relationships.push(Relationship {
        id: format!(
            "{}_{}_{:?}_{}",
            from_symbol.id,
            target_symbol.id,
            RelationshipKind::References,
            value_node.start_position().row
        ),
        from_symbol_id: from_symbol.id.clone(),
        to_symbol_id: target_symbol.id.clone(),
        kind: RelationshipKind::References,
        file_path: base.file_path.clone(),
        line_number,
        span: Some(crate::base::NormalizedSpan::from_node(&value_node)),
        reference_site_is_exact: false,
        confidence: 1.0,
        metadata: Some(metadata),
    });
}

/// Resolve a JSON-pointer fragment (segments separated by `/`) against the
/// extracted symbol tree. Returns the symbol whose name matches the terminal
/// segment AND whose ancestor chain matches the preceding segments in order.
fn resolve_local_pointer<'a>(symbols: &'a [Symbol], fragment: &str) -> Option<&'a Symbol> {
    let segments: Vec<&str> = fragment.split('/').filter(|s| !s.is_empty()).collect();
    if segments.is_empty() {
        return None;
    }
    'outer: for symbol in symbols {
        if symbol.name != segments[segments.len() - 1] {
            continue;
        }
        // Walk up the parent chain matching preceding segments in reverse.
        let mut current_parent = symbol.parent_id.as_deref();
        for expected in segments[..segments.len() - 1].iter().rev() {
            let parent = match current_parent.and_then(|pid| symbols.iter().find(|s| s.id == pid)) {
                Some(p) => p,
                None => continue 'outer,
            };
            if parent.name != *expected && parent.name != format!("[{expected}]") {
                continue 'outer;
            }
            current_parent = parent.parent_id.as_deref();
        }
        return Some(symbol);
    }
    None
}

/// The symbol that owns the object holding the `$ref`: the enclosing pair
/// (`"billing": { "$ref": ... }`) or array-element object (`allOf: [ { "$ref": ... } ]`).
/// A `$ref` in the root object has no owner, so its own pair symbol is the source.
fn ref_source_symbol<'a>(symbols: &'a [Symbol], ref_pair: Node) -> Option<&'a Symbol> {
    let mut current = ref_pair.parent();
    while let Some(node) = current {
        if matches!(node.kind(), "pair" | "object")
            && let Some(symbol) = symbol_for_node(symbols, node)
        {
            return Some(symbol);
        }
        current = node.parent();
    }
    symbol_for_node(symbols, ref_pair)
}

fn symbol_for_node<'a>(symbols: &'a [Symbol], node: Node) -> Option<&'a Symbol> {
    let start = node.start_byte() as u32;
    let end = node.end_byte() as u32;
    symbols
        .iter()
        .find(|s| s.start_byte == start && s.end_byte == end)
}
