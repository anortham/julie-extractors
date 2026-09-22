//! JSON Schema `$ref` relationship extraction (Phase 3.2).
//!
//! The `$ref` value is a decoded JSON string. Its fragment is a JSON Pointer
//! (RFC 6901): percent-decoded, then `~1` -> `/` and `~0` -> `~`.
//!
//! - **Local pointer** (`#/$defs/Address`) → concrete `References`
//!   relationship from the symbol that owns the containing object (its pair,
//!   or its array-element container) to the target. The pointer is resolved
//!   from the document root. A `$ref` in the root object uses its own `$ref`
//!   symbol as the source. An unresolved pointer is malformed, not "deferred
//!   to another file", so it emits nothing.
//! - **Local anchor** (`#person`) → `References` relationship to the object
//!   that declares `"$anchor": "person"` (or the legacy `"$id": "#person"`).
//! - **External reference** (`<uri>#/$defs/Address`, `<uri>#`, `<uri>`) →
//!   `StructuredPendingRelationship` carrying
//!   `target.import_context = Some("<uri>")`,
//!   `target.terminal_name` = last pointer segment, or the target
//!   document's file stem when the fragment is empty,
//!   `target.namespace_path` = preceding pointer segments,
//!   `target.display_name` = the `$ref` value,
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
    root: Node,
    symbols: &[Symbol],
    relationships: &mut Vec<Relationship>,
) {
    let anchors = anchor_targets(base, root, symbols);
    walk_refs(base, root, symbols, &anchors, relationships, 0);
    super::manifest::extract_manifest_relationships(base, root, symbols, relationships);
}

fn walk_refs(
    base: &mut BaseExtractor,
    node: Node,
    symbols: &[Symbol],
    anchors: &HashMap<String, usize>,
    relationships: &mut Vec<Relationship>,
    depth: u32,
) {
    if !should_visit_tree_depth(depth) {
        return;
    }

    if node.kind() == "pair"
        && pair_key(base, node).as_deref() == Some("$ref")
        && let Some((value_text, value_node)) = string_value(base, node)
    {
        handle_ref_pair(
            base,
            node,
            value_node,
            &value_text,
            symbols,
            anchors,
            relationships,
        );
    }
    if node.kind() == "pair"
        && pair_key(base, node).as_deref() == Some("$schema")
        && let Some((value_text, value_node)) = string_value(base, node)
        && is_local_file_reference(&value_text)
        && let Some(from_symbol) = symbol_for_node(symbols, node)
    {
        let line_number = value_node.start_position().row as u32 + 1;
        let from_symbol = from_symbol.clone();
        emit_external_pending(
            base,
            &from_symbol,
            &value_text,
            &value_text,
            "",
            line_number,
        );
    }
    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };
    for child in node.children(&mut node.walk()) {
        walk_refs(base, child, symbols, anchors, relationships, child_depth);
    }
}

fn pair_key(base: &BaseExtractor, pair: Node) -> Option<String> {
    let key = pair.child_by_field_name("key")?;
    Some(super::decode_json_string(&base.get_node_text(&key)))
}

/// The decoded string value of a pair plus the value node.
fn string_value<'a>(base: &BaseExtractor, pair: Node<'a>) -> Option<(String, Node<'a>)> {
    let value_node = pair.child_by_field_name("value")?;
    if value_node.kind() != "string" {
        return None;
    }
    let value = super::decode_json_string(&base.get_node_text(&value_node));
    Some((value, value_node))
}

/// Anchor name -> index of the symbol that owns the object declaring it via
/// `"$anchor": "<name>"` or the legacy `"$id": "#<name>"`.
fn anchor_targets(base: &BaseExtractor, root: Node, symbols: &[Symbol]) -> HashMap<String, usize> {
    let mut anchors = HashMap::new();
    for symbol in symbols
        .iter()
        .filter(|s| matches!(s.name.as_str(), "$anchor" | "$id"))
    {
        let Some(pair) = root
            .descendant_for_byte_range(symbol.start_byte as usize, symbol.end_byte as usize)
            .filter(|node| node.kind() == "pair")
        else {
            continue;
        };
        let Some((value, _)) = string_value(base, pair) else {
            continue;
        };
        let name = if symbol.name == "$id" {
            match value.strip_prefix('#') {
                Some(name) => name.to_string(),
                None => continue,
            }
        } else {
            value
        };
        let owner = symbol
            .parent_id
            .as_deref()
            .and_then(|parent| symbols.iter().position(|s| s.id == parent));
        if let Some(owner) = owner.filter(|_| !name.is_empty()) {
            anchors.entry(name).or_insert(owner);
        }
    }
    anchors
}

fn handle_ref_pair(
    base: &mut BaseExtractor,
    pair: Node,
    value_node: Node,
    value_text: &str,
    symbols: &[Symbol],
    anchors: &HashMap<String, usize>,
    relationships: &mut Vec<Relationship>,
) {
    let Some(from_symbol) = ref_source_symbol(symbols, pair) else {
        return;
    };
    if let Some(anchor) = value_text.strip_prefix('#').filter(|f| !f.starts_with('/'))
        && let Some(&target) = anchors.get(anchor)
    {
        emit_local_relationship(
            base,
            from_symbol,
            &symbols[target],
            value_node,
            value_node.start_position().row as u32 + 1,
            relationships,
        );
        return;
    }
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

    match value_text.split_once('#') {
        Some(("", fragment)) => {
            if let Some(pointer) = fragment.strip_prefix('/')
                && let Some(target_symbol) = resolve_local_pointer(symbols, pointer)
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
        }
        Some((document, fragment)) => {
            emit_external_pending(
                base,
                from_symbol,
                value_text,
                document,
                fragment,
                line_number,
            );
        }
        None if !value_text.trim().is_empty() => {
            emit_external_pending(base, from_symbol, value_text, value_text, "", line_number);
        }
        None => {}
    }
}

/// A `$schema` value that names a file in the repository, not a URL.
fn is_local_file_reference(value: &str) -> bool {
    !value.is_empty() && !value.contains("://") && !value.starts_with('#')
}

/// RFC 6901 reference tokens of a URI-fragment JSON Pointer (without its
/// leading `/`): percent-decoded, then `~1` -> `/` and `~0` -> `~`.
fn pointer_tokens(pointer: &str) -> Vec<String> {
    pointer
        .split('/')
        .map(|token| percent_decode(token).replace("~1", "/").replace("~0", "~"))
        .collect()
}

fn percent_decode(token: &str) -> String {
    let bytes = token.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        let hex = bytes
            .get(index + 1..index + 3)
            .and_then(|pair| std::str::from_utf8(pair).ok())
            .and_then(|pair| u8::from_str_radix(pair, 16).ok());
        match (bytes[index], hex) {
            (b'%', Some(byte)) => {
                out.push(byte);
                index += 3;
            }
            (byte, _) => {
                out.push(byte);
                index += 1;
            }
        }
    }
    String::from_utf8(out).unwrap_or_else(|_| token.to_string())
}

/// The file stem of the last path segment of a URI: `./a/orders.json` ->
/// `orders`, `../core` -> `core`, `x/customer.schema.json` -> `customer.schema`.
pub(crate) fn document_stem(uri: &str) -> String {
    let path = uri
        .split(['?', '#'])
        .next()
        .unwrap_or(uri)
        .trim_end_matches('/');
    let file = path.rsplit('/').next().unwrap_or(path);
    match file.rsplit_once('.') {
        Some((stem, _)) if !stem.is_empty() => stem.to_string(),
        _ => file.to_string(),
    }
}

fn emit_external_pending(
    base: &mut BaseExtractor,
    from_symbol: &Symbol,
    raw_ref: &str,
    document: &str,
    fragment: &str,
    line_number: u32,
) {
    let mut segments = match fragment.strip_prefix('/') {
        Some(pointer) => pointer_tokens(pointer),
        None if fragment.is_empty() => Vec::new(),
        None => vec![fragment.to_string()],
    };
    let terminal_name = match segments.pop() {
        Some(name) => name,
        None => document_stem(document),
    };
    if terminal_name.is_empty() {
        return;
    }
    let target = UnresolvedTarget {
        display_name: raw_ref.to_string(),
        terminal_name,
        receiver: None,
        namespace_path: segments,
        import_context: Some(document.to_string()),
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

/// Resolve a JSON Pointer (without its leading `/`) against the extracted
/// symbol tree, starting at the document root: the first token names a
/// top-level symbol and each later token names a child of the previous one.
fn resolve_local_pointer<'a>(symbols: &'a [Symbol], pointer: &str) -> Option<&'a Symbol> {
    let mut current: Option<&Symbol> = None;
    for token in pointer_tokens(pointer) {
        let parent_id = current.map(|symbol| symbol.id.as_str());
        let element = format!("[{token}]");
        current = Some(symbols.iter().find(|symbol| {
            symbol.parent_id.as_deref() == parent_id
                && (symbol.name == token || symbol.name == element)
        })?);
    }
    current
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
