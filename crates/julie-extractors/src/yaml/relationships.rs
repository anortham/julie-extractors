use super::resolve_alias_anchor_target;
use crate::base::{
    BaseExtractor, Relationship, RelationshipKind, StructuredPendingRelationship, Symbol,
    UnresolvedTarget,
};
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use tree_sitter::{Node, Tree};

pub(super) fn extract_relationships(
    base: &mut BaseExtractor,
    tree: &Tree,
    symbols: &[Symbol],
) -> Vec<Relationship> {
    let mut relationships = Vec::new();
    let mut seen = HashSet::new();
    walk_tree(
        base,
        tree.root_node(),
        symbols,
        &mut relationships,
        &mut seen,
        0,
    );
    super::cloudformation::extract_relationships(base, tree, symbols, &mut relationships);
    super::ci::extract_relationships(base, tree, symbols, &mut relationships);
    super::compose::extract_relationships(base, tree, symbols, &mut relationships);
    super::kubernetes::extract_relationships(base, tree, symbols, &mut relationships);
    super::ansible::extract_relationships(base, tree, symbols, &mut relationships);
    relationships
}

fn walk_tree(
    base: &mut BaseExtractor,
    node: Node,
    symbols: &[Symbol],
    relationships: &mut Vec<Relationship>,
    seen: &mut HashSet<(String, String, u32, String)>,
    depth: u32,
) {
    if !should_visit_tree_depth(depth) {
        return;
    }

    if node.kind() == "alias" {
        extract_alias_relationship(base, node, symbols, relationships, seen);
    }
    if matches!(node.kind(), "block_mapping_pair" | "flow_pair")
        && pair_key(&base.content, node).as_deref() == Some("$ref")
    {
        extract_schema_ref(base, node, symbols, relationships);
    }

    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk_tree(base, child, symbols, relationships, seen, child_depth);
    }
}

/// `$ref` in YAML OpenAPI, AsyncAPI, and JSON Schema documents, with the JSON
/// model: the edge starts at the symbol that owns the mapping holding the
/// `$ref` (a key or a `[i]` sequence item), or at the `$ref` key at the root.
fn extract_schema_ref(
    base: &mut BaseExtractor,
    pair: Node,
    symbols: &[Symbol],
    relationships: &mut Vec<Relationship>,
) {
    let Some(value) = pair.child_by_field_name("value") else {
        return;
    };
    let Some((scalar, text)) = scalar_value(&base.content, value) else {
        return;
    };
    let from_symbol = ancestors(pair)
        .filter(|node| matches!(node.kind(), "block_mapping_pair" | "block_sequence_item"))
        .find_map(|node| symbol_for_node(symbols, node))
        .or_else(|| symbol_for_node(symbols, pair));
    let Some(from_symbol) = from_symbol else {
        return;
    };
    crate::json::relationships::emit_schema_ref(
        base,
        from_symbol,
        scalar,
        &text,
        symbols,
        relationships,
    );
}

fn extract_alias_relationship(
    base: &BaseExtractor,
    node: Node,
    symbols: &[Symbol],
    relationships: &mut Vec<Relationship>,
    seen: &mut HashSet<(String, String, u32, String)>,
) {
    let Some(alias_name) = alias_name(base, node) else {
        return;
    };
    let Some(target) = resolve_alias_anchor_target(symbols, node, &alias_name) else {
        return;
    };
    let Some(source) = super::innermost_symbol(symbols, node) else {
        return;
    };

    let line_number = (node.start_position().row + 1) as u32;
    let key = (
        source.id.clone(),
        target.id.clone(),
        line_number,
        alias_name.clone(),
    );
    if !seen.insert(key) {
        return;
    }

    let mut metadata = HashMap::new();
    metadata.insert("alias".to_string(), Value::String(alias_name));

    relationships.push(base.create_relationship(
        source.id.clone(),
        target.id.clone(),
        RelationshipKind::References,
        &node,
        Some(1.0),
        Some(metadata),
    ));
}

fn alias_name(base: &BaseExtractor, node: Node) -> Option<String> {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "alias_name" {
            return Some(base.get_node_text(&child));
        }
    }
    None
}

/// The strict ancestors of `node`, innermost first.
pub(super) fn ancestors(node: Node) -> impl Iterator<Item = Node> {
    std::iter::successors(node.parent(), |node| node.parent())
}

/// The symbol whose span is exactly this node (a mapping pair or sequence item).
pub(super) fn symbol_for_node<'a>(symbols: &'a [Symbol], node: Node) -> Option<&'a Symbol> {
    let (start, end) = (node.start_byte() as u32, node.end_byte() as u32);
    symbols
        .iter()
        .find(|symbol| symbol.start_byte == start && symbol.end_byte == end)
}

/// The unquoted key text of a mapping pair.
pub(super) fn pair_key(content: &str, pair: Node) -> Option<String> {
    let key = pair.child_by_field_name("key")?;
    scalar_value(content, key).map(|(_, text)| text)
}

/// The scalar node and its unquoted text inside a `flow_node` (after any tag
/// or anchor), or `None` for containers.
pub(super) fn scalar_value<'tree>(
    content: &str,
    value: Node<'tree>,
) -> Option<(Node<'tree>, String)> {
    let scalar = if is_scalar(value.kind()) {
        value
    } else {
        let mut cursor = value.walk();
        value
            .named_children(&mut cursor)
            .find(|child| is_scalar(child.kind()))?
    };
    let text = content.get(scalar.byte_range())?;
    let text = match scalar.kind() {
        "double_quote_scalar" | "single_quote_scalar" => text.get(1..text.len() - 1)?.to_string(),
        _ => text.trim().to_string(),
    };
    Some((scalar, text))
}

fn is_scalar(kind: &str) -> bool {
    matches!(
        kind,
        "plain_scalar" | "double_quote_scalar" | "single_quote_scalar"
    )
}

/// Scalars of a value that is one scalar or a flow/block sequence of scalars.
pub(super) fn scalar_list<'tree>(content: &str, value: Node<'tree>) -> Vec<(Node<'tree>, String)> {
    if let Some(scalar) = scalar_value(content, value) {
        return vec![scalar];
    }
    let mut cursor = value.walk();
    let Some(sequence) = value
        .named_children(&mut cursor)
        .find(|child| matches!(child.kind(), "flow_sequence" | "block_sequence"))
    else {
        return Vec::new();
    };
    let mut cursor = sequence.walk();
    sequence
        .named_children(&mut cursor)
        .filter_map(|item| {
            let node = if item.kind() == "block_sequence_item" {
                item.named_child(0)?
            } else {
                item
            };
            scalar_value(content, node)
        })
        .collect()
}

/// Structured pending `References` row for a reference into another file.
pub(super) fn push_file_pending(
    base: &mut BaseExtractor,
    from_symbol: &Symbol,
    path: &str,
    display_name: &str,
    node: Node,
) {
    let terminal_name = path
        .rsplit('/')
        .find(|segment| !segment.is_empty())
        .unwrap_or(path)
        .to_string();
    let target = UnresolvedTarget {
        display_name: display_name.to_string(),
        terminal_name,
        receiver: None,
        namespace_path: Vec::new(),
        import_context: Some(path.to_string()),
    };
    let pending = StructuredPendingRelationship::new(
        from_symbol.id.clone(),
        target,
        Some(from_symbol.id.clone()),
        RelationshipKind::References,
        base.file_path.clone(),
        node.start_position().row as u32 + 1,
        1.0,
    );
    base.add_structured_pending_relationship(pending);
}

/// A `References` edge with a one-key metadata map, deduplicated per site.
pub(super) fn push_reference(
    base: &BaseExtractor,
    from: &Symbol,
    to: &Symbol,
    node: &Node,
    metadata: (&str, &str),
    relationships: &mut Vec<Relationship>,
) {
    let line = node.start_position().row as u32 + 1;
    if relationships
        .iter()
        .any(|r| r.from_symbol_id == from.id && r.to_symbol_id == to.id && r.line_number == line)
    {
        return;
    }
    let metadata = HashMap::from([(
        metadata.0.to_string(),
        Value::String(metadata.1.to_string()),
    )]);
    relationships.push(base.create_relationship(
        from.id.clone(),
        to.id.clone(),
        RelationshipKind::References,
        node,
        Some(1.0),
        Some(metadata),
    ));
}

/// The pairs of the mapping held by a value node (block or flow).
pub(super) fn mapping_pairs(value: Node) -> Vec<Node> {
    let mapping = if matches!(value.kind(), "block_mapping" | "flow_mapping") {
        Some(value)
    } else {
        let mut cursor = value.walk();
        value
            .named_children(&mut cursor)
            .find(|child| matches!(child.kind(), "block_mapping" | "flow_mapping"))
    };
    let Some(mapping) = mapping else {
        return Vec::new();
    };
    let mut cursor = mapping.walk();
    mapping
        .named_children(&mut cursor)
        .filter(|child| matches!(child.kind(), "block_mapping_pair" | "flow_pair"))
        .collect()
}

/// The item value nodes of the sequence held by a value node.
pub(super) fn sequence_items(value: Node) -> Vec<Node> {
    let mut cursor = value.walk();
    let Some(sequence) = value
        .named_children(&mut cursor)
        .find(|child| matches!(child.kind(), "block_sequence" | "flow_sequence"))
    else {
        return Vec::new();
    };
    let mut cursor = sequence.walk();
    sequence
        .named_children(&mut cursor)
        .filter_map(|item| match item.kind() {
            "block_sequence_item" => item.named_child(0),
            "flow_node" => Some(item),
            _ => None,
        })
        .collect()
}

/// The value node of the pair keyed `key` in a mapping value.
pub(super) fn field<'tree>(content: &str, value: Node<'tree>, key: &str) -> Option<Node<'tree>> {
    mapping_pairs(value)
        .into_iter()
        .find(|pair| pair_key(content, *pair).as_deref() == Some(key))
        .and_then(|pair| pair.child_by_field_name("value"))
}

/// The root value node of each document in the stream.
pub(super) fn document_roots(tree: &Tree) -> Vec<Node<'_>> {
    let root = tree.root_node();
    let mut cursor = root.walk();
    root.named_children(&mut cursor)
        .filter(|child| child.kind() == "document")
        .filter_map(|document| {
            let mut cursor = document.walk();
            document
                .named_children(&mut cursor)
                .find(|child| matches!(child.kind(), "block_node" | "flow_node"))
        })
        .collect()
}

/// Structured pending `References` row with an explicit target.
pub(super) fn push_pending(
    base: &mut BaseExtractor,
    from_symbol: &Symbol,
    target: UnresolvedTarget,
    node: Node,
) {
    let pending = StructuredPendingRelationship::new(
        from_symbol.id.clone(),
        target,
        Some(from_symbol.id.clone()),
        RelationshipKind::References,
        base.file_path.clone(),
        node.start_position().row as u32 + 1,
        1.0,
    );
    base.add_structured_pending_relationship(pending);
}

/// The file name of a path, for gating domain collectors.
pub(super) fn file_name(file_path: &str) -> &str {
    file_path.rsplit(['/', '\\']).next().unwrap_or(file_path)
}
