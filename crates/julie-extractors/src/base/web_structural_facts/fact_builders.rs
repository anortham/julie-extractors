use std::collections::HashMap;

use serde_json::{Number, Value};
use tree_sitter::Node;

use crate::base::span::NormalizedSpan;
use crate::base::types::{StructuralFact, Symbol, stable_location_id};

pub(super) fn fact_for_node(
    file_path: &str,
    language: &str,
    pattern_id: &str,
    capture_name: &str,
    node: Node<'_>,
    metadata: HashMap<String, Value>,
) -> StructuralFact {
    fact_for_span(
        file_path,
        language,
        pattern_id,
        capture_name,
        node.kind(),
        NormalizedSpan::from_node(&node),
        metadata,
    )
}

pub(super) fn fact_for_span(
    file_path: &str,
    language: &str,
    pattern_id: &str,
    capture_name: &str,
    node_kind: &str,
    span: NormalizedSpan,
    metadata: HashMap<String, Value>,
) -> StructuralFact {
    StructuralFact {
        id: stable_location_id(file_path, &format!("{pattern_id}:{capture_name}"), span),
        file_path: file_path.to_string(),
        language: language.to_string(),
        pattern_id: pattern_id.to_string(),
        capture_name: capture_name.to_string(),
        node_kind: node_kind.to_string(),
        containing_symbol_id: None,
        start_line: span.start_line,
        start_column: span.start_column,
        end_line: span.end_line,
        end_column: span.end_column,
        start_byte: span.start_byte,
        end_byte: span.end_byte,
        confidence: 1.0,
        metadata: Some(metadata),
    }
}

pub(super) fn base_metadata(query_family: &str) -> HashMap<String, Value> {
    HashMap::from([
        (
            "pattern_version".to_string(),
            Value::Number(Number::from(1)),
        ),
        (
            "query_family".to_string(),
            Value::String(query_family.to_string()),
        ),
    ])
}

pub(super) fn insert_string(metadata: &mut HashMap<String, Value>, key: &str, value: &str) {
    metadata.insert(key.to_string(), Value::String(value.to_string()));
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

pub(super) fn attach_containing_symbols(facts: &mut [StructuralFact], symbols: &[Symbol]) {
    for fact in facts {
        fact.containing_symbol_id = symbols
            .iter()
            .filter(|symbol| {
                symbol.start_byte <= fact.start_byte && symbol.end_byte >= fact.end_byte
            })
            .min_by_key(|symbol| symbol.end_byte.saturating_sub(symbol.start_byte))
            .map(|symbol| symbol.id.clone());
    }
}

pub(super) fn child_by_kind<'a>(node: Node<'a>, kind: &str) -> Option<Node<'a>> {
    let mut cursor = node.walk();
    node.children(&mut cursor)
        .find(|child| child.kind() == kind)
}

pub(super) fn node_text<'a>(content: &'a str, node: Node<'_>) -> Option<&'a str> {
    content.get(node.start_byte()..node.end_byte())
}
