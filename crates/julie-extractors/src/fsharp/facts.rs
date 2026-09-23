//! F# domain-native structural facts: computation expressions, active
//! pattern definitions, and code quotations.

use crate::base::types::{StructuralFact, Symbol, stable_location_id};
use crate::base::{NormalizedSpan, attach_containing_symbols};
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};
use serde_json::Value;
use std::collections::HashMap;
use tree_sitter::{Node, Tree};

pub(crate) const COMPUTATION_EXPRESSION_PATTERN_ID: &str = "fsharp.computation_expression.v1";
pub(crate) const ACTIVE_PATTERN_PATTERN_ID: &str = "fsharp.active_pattern.v1";
pub(crate) const QUOTATION_PATTERN_ID: &str = "fsharp.quotation.v1";

pub(crate) const PATTERN_IDS: &[&str] = &[
    ACTIVE_PATTERN_PATTERN_ID,
    COMPUTATION_EXPRESSION_PATTERN_ID,
    QUOTATION_PATTERN_ID,
];

pub(crate) fn collect_domain_facts(
    tree: &Tree,
    file_path: &str,
    content: &str,
    symbols: &[Symbol],
) -> Vec<StructuralFact> {
    let mut facts = Vec::new();
    visit(tree.root_node(), file_path, content, &mut facts, 0);
    attach_containing_symbols(&mut facts, symbols);
    facts
}

fn visit(node: Node, file_path: &str, content: &str, facts: &mut Vec<StructuralFact>, depth: u32) {
    if !should_visit_tree_depth(depth) {
        return;
    }
    let text = |node: Node| content.get(node.byte_range()).unwrap_or_default().trim();
    match node.kind() {
        "ce_expression" => {
            if let Some(builder) = first_named_child(node) {
                let mut metadata = base_metadata("computation_expression");
                metadata.insert("builder".to_string(), Value::String(text(builder).into()));
                facts.push(fact(
                    file_path,
                    node,
                    COMPUTATION_EXPRESSION_PATTERN_ID,
                    metadata,
                ));
            }
        }
        "active_pattern" => {
            let mut cursor = node.walk();
            let children: Vec<Node> = node.named_children(&mut cursor).collect();
            let cases: Vec<Value> = children
                .iter()
                .filter(|child| child.kind() == "active_pattern_op_name")
                .map(|case| Value::String(text(*case).into()))
                .collect();
            let partial = children
                .iter()
                .any(|child| child.kind() == "wildcard_active_pattern_op");
            let mut metadata = base_metadata("pattern_matching");
            metadata.insert("name".to_string(), Value::String(text(node).into()));
            metadata.insert("cases".to_string(), Value::Array(cases));
            metadata.insert("partial".to_string(), Value::Bool(partial));
            facts.push(fact(file_path, node, ACTIVE_PATTERN_PATTERN_ID, metadata));
        }
        "literal_expression" => {
            let quotation_kind = if text(node).starts_with("<@@") {
                "untyped"
            } else {
                "typed"
            };
            let mut metadata = base_metadata("metaprogramming");
            metadata.insert(
                "quotation_kind".to_string(),
                Value::String(quotation_kind.into()),
            );
            facts.push(fact(file_path, node, QUOTATION_PATTERN_ID, metadata));
        }
        _ => {}
    }
    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        visit(child, file_path, content, facts, child_depth);
    }
}

fn base_metadata(query_family: &str) -> HashMap<String, Value> {
    HashMap::from([
        ("pattern_version".to_string(), Value::Number(1.into())),
        (
            "query_family".to_string(),
            Value::String(query_family.to_string()),
        ),
    ])
}

fn fact(
    file_path: &str,
    node: Node,
    pattern_id: &str,
    metadata: HashMap<String, Value>,
) -> StructuralFact {
    let span = NormalizedSpan::from_node(&node);
    let capture_name = pattern_id
        .strip_prefix("fsharp.")
        .and_then(|rest| rest.strip_suffix(".v1"))
        .unwrap_or(pattern_id);
    StructuralFact {
        id: stable_location_id(file_path, &format!("{pattern_id}:{capture_name}"), span),
        file_path: file_path.to_string(),
        language: "fsharp".to_string(),
        pattern_id: pattern_id.to_string(),
        capture_name: capture_name.to_string(),
        node_kind: node.kind().to_string(),
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

fn first_named_child(node: Node) -> Option<Node> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor).next()
}
