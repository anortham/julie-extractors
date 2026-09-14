use std::collections::HashMap;

use serde_json::Value;
use tree_sitter::Node;

use crate::base::span::NormalizedSpan;
pub(super) use crate::base::structural_fact_builders::{
    base_metadata, fact_for_node, fact_for_span, insert_string,
};
use crate::base::types::{StructuralFact, stable_location_id};

/// Like [`fact_for_node`], but folds an extra discriminator into the hashed
/// identity so several facts sharing one node/pattern/capture/span (e.g. every
/// `data-*` attribute on the same element) receive distinct, deterministic ids
/// instead of colliding and being dropped by the writer's id-dedup.
pub(super) fn fact_for_node_with_identity(
    file_path: &str,
    language: &str,
    pattern_id: &str,
    capture_name: &str,
    identity_discriminator: &str,
    node: Node<'_>,
    metadata: HashMap<String, Value>,
) -> StructuralFact {
    let span = NormalizedSpan::from_node(&node);
    let mut fact = fact_for_span(
        file_path,
        language,
        pattern_id,
        capture_name,
        node.kind(),
        span,
        metadata,
    );
    fact.id = stable_location_id(
        file_path,
        &format!("{pattern_id}:{capture_name}:{identity_discriminator}"),
        span,
    );
    fact
}

pub(super) fn insert_string_array(
    metadata: &mut HashMap<String, Value>,
    key: &str,
    values: Vec<String>,
) {
    metadata.insert(
        key.to_string(),
        Value::Array(values.into_iter().map(Value::String).collect()),
    );
}

pub(super) fn child_by_kind<'a>(node: Node<'a>, kind: &str) -> Option<Node<'a>> {
    let mut cursor = node.walk();
    node.children(&mut cursor)
        .find(|child| child.kind() == kind)
}

pub(super) fn node_text<'a>(content: &'a str, node: Node<'_>) -> Option<&'a str> {
    content.get(node.start_byte()..node.end_byte())
}
