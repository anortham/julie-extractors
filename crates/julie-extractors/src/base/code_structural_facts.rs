use std::collections::HashMap;

use serde_json::{Number, Value};
use tree_sitter::{Node, Tree};

use super::span::NormalizedSpan;
use super::structural_facts::sort_structural_facts;
use super::types::{StructuralFact, Symbol, stable_location_id};

#[derive(Debug, Clone, Copy)]
struct CodeStructuralPattern {
    pattern_id: &'static str,
    capture_name: &'static str,
    node_kinds: &'static [&'static str],
    query_family: &'static str,
}

const JAVA_PATTERNS: &[CodeStructuralPattern] = &[
    CodeStructuralPattern {
        pattern_id: "java.synchronized_statement.v1",
        capture_name: "synchronized_statement",
        node_kinds: &["synchronized_statement"],
        query_family: "concurrency",
    },
    CodeStructuralPattern {
        pattern_id: "java.try_with_resources_statement.v1",
        capture_name: "try_with_resources_statement",
        node_kinds: &["try_with_resources_statement"],
        query_family: "resources",
    },
    CodeStructuralPattern {
        pattern_id: "java.lambda_expression.v1",
        capture_name: "lambda_expression",
        node_kinds: &["lambda_expression"],
        query_family: "functional",
    },
    CodeStructuralPattern {
        pattern_id: "java.marker_annotation.v1",
        capture_name: "marker_annotation",
        node_kinds: &["marker_annotation"],
        query_family: "metadata",
    },
    CodeStructuralPattern {
        pattern_id: "java.annotation.v1",
        capture_name: "annotation",
        node_kinds: &["annotation"],
        query_family: "metadata",
    },
];

const KOTLIN_PATTERNS: &[CodeStructuralPattern] = &[
    CodeStructuralPattern {
        pattern_id: "kotlin.suspend_modifier.v1",
        capture_name: "suspend_modifier",
        node_kinds: &["suspend"],
        query_family: "async",
    },
    CodeStructuralPattern {
        pattern_id: "kotlin.property_delegate.v1",
        capture_name: "property_delegate",
        node_kinds: &["property_delegate"],
        query_family: "delegation",
    },
    CodeStructuralPattern {
        pattern_id: "kotlin.annotation.v1",
        capture_name: "annotation",
        node_kinds: &["annotation"],
        query_family: "metadata",
    },
];

const SCALA_PATTERNS: &[CodeStructuralPattern] = &[
    CodeStructuralPattern {
        pattern_id: "scala.extension_definition.v1",
        capture_name: "extension_definition",
        node_kinds: &["extension_definition"],
        query_family: "metaprogramming",
    },
    CodeStructuralPattern {
        pattern_id: "scala.given_definition.v1",
        capture_name: "given_definition",
        node_kinds: &["given_definition"],
        query_family: "typeclass",
    },
    CodeStructuralPattern {
        pattern_id: "scala.for_expression.v1",
        capture_name: "for_expression",
        node_kinds: &["for_expression"],
        query_family: "comprehension",
    },
    CodeStructuralPattern {
        pattern_id: "scala.annotation.v1",
        capture_name: "annotation",
        node_kinds: &["annotation"],
        query_family: "metadata",
    },
];

const SWIFT_PATTERNS: &[CodeStructuralPattern] = &[
    CodeStructuralPattern {
        pattern_id: "swift.await_expression.v1",
        capture_name: "await_expression",
        node_kinds: &["await_expression"],
        query_family: "async",
    },
    CodeStructuralPattern {
        pattern_id: "swift.actor_declaration.v1",
        capture_name: "actor_declaration",
        node_kinds: &["class_declaration"],
        query_family: "concurrency",
    },
    CodeStructuralPattern {
        pattern_id: "swift.attribute.v1",
        capture_name: "attribute",
        node_kinds: &["attribute"],
        query_family: "metadata",
    },
];

const DART_PATTERNS: &[CodeStructuralPattern] = &[
    CodeStructuralPattern {
        pattern_id: "dart.await_expression.v1",
        capture_name: "await_expression",
        node_kinds: &["await_expression"],
        query_family: "async",
    },
    CodeStructuralPattern {
        pattern_id: "dart.async_modifier.v1",
        capture_name: "async_modifier",
        node_kinds: &["async"],
        query_family: "async",
    },
    CodeStructuralPattern {
        pattern_id: "dart.annotation.v1",
        capture_name: "annotation",
        node_kinds: &["annotation", "marker_annotation"],
        query_family: "metadata",
    },
];

const VBNET_PATTERNS: &[CodeStructuralPattern] = &[
    CodeStructuralPattern {
        pattern_id: "vbnet.handles_clause.v1",
        capture_name: "handles_clause",
        node_kinds: &["handles_clause"],
        query_family: "events",
    },
    CodeStructuralPattern {
        pattern_id: "vbnet.implements_clause.v1",
        capture_name: "implements_clause",
        node_kinds: &["implements_clause"],
        query_family: "interface",
    },
    CodeStructuralPattern {
        pattern_id: "vbnet.event_declaration.v1",
        capture_name: "event_declaration",
        node_kinds: &["event_declaration"],
        query_family: "events",
    },
    CodeStructuralPattern {
        pattern_id: "vbnet.attribute.v1",
        capture_name: "attribute",
        node_kinds: &["attribute"],
        query_family: "metadata",
    },
];

#[cfg(all(test, feature = "test-capability-matrix"))]
const JAVA_PATTERN_IDS: &[&str] = &[
    "java.synchronized_statement.v1",
    "java.try_with_resources_statement.v1",
    "java.lambda_expression.v1",
    "java.marker_annotation.v1",
    "java.annotation.v1",
];
#[cfg(all(test, feature = "test-capability-matrix"))]
const KOTLIN_PATTERN_IDS: &[&str] = &[
    "kotlin.suspend_modifier.v1",
    "kotlin.property_delegate.v1",
    "kotlin.annotation.v1",
];
#[cfg(all(test, feature = "test-capability-matrix"))]
const SCALA_PATTERN_IDS: &[&str] = &[
    "scala.extension_definition.v1",
    "scala.given_definition.v1",
    "scala.for_expression.v1",
    "scala.annotation.v1",
];
#[cfg(all(test, feature = "test-capability-matrix"))]
const SWIFT_PATTERN_IDS: &[&str] = &[
    "swift.await_expression.v1",
    "swift.actor_declaration.v1",
    "swift.attribute.v1",
];
#[cfg(all(test, feature = "test-capability-matrix"))]
const DART_PATTERN_IDS: &[&str] = &[
    "dart.await_expression.v1",
    "dart.async_modifier.v1",
    "dart.annotation.v1",
];
#[cfg(all(test, feature = "test-capability-matrix"))]
const VBNET_PATTERN_IDS: &[&str] = &[
    "vbnet.handles_clause.v1",
    "vbnet.implements_clause.v1",
    "vbnet.event_declaration.v1",
    "vbnet.attribute.v1",
];

pub fn collect_code_structural_facts(
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
    symbols: &[Symbol],
) -> Vec<StructuralFact> {
    let patterns = patterns_for_language(language);
    if patterns.is_empty() {
        return Vec::new();
    }

    let mut facts = Vec::new();
    collect_node(
        tree.root_node(),
        language,
        file_path,
        content,
        patterns,
        &mut facts,
    );
    attach_containing_symbols(&mut facts, symbols);
    sort_structural_facts(&mut facts);
    facts
}

#[cfg(all(test, feature = "test-capability-matrix"))]
pub(crate) fn code_structural_fact_pattern_ids_for_language(
    language: &str,
) -> &'static [&'static str] {
    match language {
        "java" => JAVA_PATTERN_IDS,
        "kotlin" => KOTLIN_PATTERN_IDS,
        "scala" => SCALA_PATTERN_IDS,
        "swift" => SWIFT_PATTERN_IDS,
        "dart" => DART_PATTERN_IDS,
        "vbnet" => VBNET_PATTERN_IDS,
        _ => &[],
    }
}

fn patterns_for_language(language: &str) -> &'static [CodeStructuralPattern] {
    match language {
        "java" => JAVA_PATTERNS,
        "kotlin" => KOTLIN_PATTERNS,
        "scala" => SCALA_PATTERNS,
        "swift" => SWIFT_PATTERNS,
        "dart" => DART_PATTERNS,
        "vbnet" => VBNET_PATTERNS,
        _ => &[],
    }
}

fn collect_node(
    node: Node<'_>,
    language: &str,
    file_path: &str,
    content: &str,
    patterns: &[CodeStructuralPattern],
    facts: &mut Vec<StructuralFact>,
) {
    for pattern in patterns {
        if pattern.node_kinds.contains(&node.kind()) {
            if pattern.pattern_id == "swift.actor_declaration.v1"
                && !node_text(content, node).trim_start().starts_with("actor")
            {
                continue;
            }
            facts.push(fact_for_node(file_path, language, content, node, *pattern));
        }
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_node(child, language, file_path, content, patterns, facts);
    }
}

fn fact_for_node(
    file_path: &str,
    language: &str,
    content: &str,
    node: Node<'_>,
    pattern: CodeStructuralPattern,
) -> StructuralFact {
    let span = NormalizedSpan::from_node(&node);
    let mut metadata = base_metadata(pattern.query_family);
    enrich_metadata(language, content, node, pattern.pattern_id, &mut metadata);

    StructuralFact {
        id: stable_location_id(
            file_path,
            &format!("{}:{}", pattern.pattern_id, pattern.capture_name),
            span,
        ),
        file_path: file_path.to_string(),
        language: language.to_string(),
        pattern_id: pattern.pattern_id.to_string(),
        capture_name: pattern.capture_name.to_string(),
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

fn base_metadata(query_family: &str) -> HashMap<String, Value> {
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

fn enrich_metadata(
    language: &str,
    content: &str,
    node: Node<'_>,
    pattern_id: &str,
    metadata: &mut HashMap<String, Value>,
) {
    match pattern_id {
        "java.marker_annotation.v1"
        | "java.annotation.v1"
        | "kotlin.annotation.v1"
        | "scala.annotation.v1"
        | "dart.annotation.v1" => {
            if let Some(name) = annotation_name(content, node) {
                insert_string(metadata, "annotation_name", &name);
            }
        }
        "swift.attribute.v1" | "vbnet.attribute.v1" => {
            if let Some(name) = attribute_name(content, node) {
                insert_string(metadata, "attribute_name", &name);
            }
        }
        "kotlin.property_delegate.v1" => {
            if let Some(delegate) = delegate_name(content, node) {
                insert_string(metadata, "delegate_name", &delegate);
            }
        }
        "vbnet.handles_clause.v1" => {
            if let Some(target) = handles_target(content, node) {
                insert_string(metadata, "handles_target", &target);
            }
        }
        "vbnet.implements_clause.v1" => {
            if let Some(target) = implements_target(content, node) {
                insert_string(metadata, "implements_target", &target);
            }
        }
        "scala.extension_definition.v1" => {
            if let Some(extended_type) = scala_extended_type(content, node) {
                insert_string(metadata, "extended_type", &extended_type);
            }
        }
        "scala.given_definition.v1" => {
            if let Some(name) = scala_given_name(content, node) {
                insert_string(metadata, "given_name", &name);
            } else if let Some(given_type) = scala_given_type(content, node) {
                insert_string(metadata, "given_type", &given_type);
            }
        }
        _ if language == "swift" && pattern_id == "swift.actor_declaration.v1" => {
            if let Some(name) = swift_actor_name(content, node) {
                insert_string(metadata, "actor_name", &name);
            }
        }
        _ => {}
    }
}

fn annotation_name(content: &str, node: Node<'_>) -> Option<String> {
    first_named_identifier(
        content,
        node,
        &[
            "identifier",
            "type_identifier",
            "simple_identifier",
            "scoped_identifier",
        ],
    )
}

fn attribute_name(content: &str, node: Node<'_>) -> Option<String> {
    first_named_identifier(
        content,
        node,
        &["type_identifier", "identifier", "simple_identifier"],
    )
}

fn delegate_name(content: &str, node: Node<'_>) -> Option<String> {
    if let Some(name) = first_named_identifier(
        content,
        node,
        &["identifier", "simple_identifier", "type_identifier"],
    ) {
        return Some(name);
    }

    find_descendant(node, "call_expression").and_then(|call| {
        first_named_identifier(content, call, &["identifier", "simple_identifier"])
    })
}

fn implements_target(content: &str, node: Node<'_>) -> Option<String> {
    let text = node_text(content, node);
    text.trim()
        .strip_prefix("Implements")
        .or_else(|| text.trim().strip_prefix("implements"))
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .map(str::to_string)
}

fn scala_extended_type(content: &str, node: Node<'_>) -> Option<String> {
    extension_parameter_type(content, node)
}

fn extension_parameter_type(content: &str, node: Node<'_>) -> Option<String> {
    let parameters = node.child_by_field_name("parameters")?;
    let parameter = first_direct_child(parameters, "parameter")?;
    parameter
        .child_by_field_name("type")
        .map(|type_node| node_text(content, type_node))
}

fn scala_given_name(content: &str, node: Node<'_>) -> Option<String> {
    let mut cursor = node.walk();
    let children: Vec<_> = node.children(&mut cursor).collect();
    for (index, child) in children.iter().enumerate() {
        if child.kind() != "identifier" {
            continue;
        }
        if children
            .get(index + 1)
            .is_some_and(|next| next.kind() == ":")
        {
            return Some(node_text(content, *child));
        }
    }
    None
}

fn scala_given_type(content: &str, node: Node<'_>) -> Option<String> {
    node.child_by_field_name("return_type")
        .map(|return_type| {
            node_text(content, return_type)
                .split('=')
                .next()
                .unwrap_or_default()
                .trim()
                .to_string()
        })
        .filter(|given_type| !given_type.is_empty())
}

fn handles_target(content: &str, node: Node<'_>) -> Option<String> {
    let text = node_text(content, node);
    text.trim()
        .strip_prefix("Handles")
        .or_else(|| text.trim().strip_prefix("handles"))
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .map(str::to_string)
}

fn first_direct_child<'tree>(node: Node<'tree>, kind: &str) -> Option<Node<'tree>> {
    let mut cursor = node.walk();
    node.children(&mut cursor)
        .find(|child| child.kind() == kind)
}

fn swift_actor_name(content: &str, node: Node<'_>) -> Option<String> {
    first_named_identifier(content, node, &["type_identifier", "simple_identifier"])
}

fn first_named_identifier(content: &str, node: Node<'_>, kinds: &[&str]) -> Option<String> {
    if kinds.contains(&node.kind()) {
        return Some(node_text(content, node));
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if let Some(name) = first_named_identifier(content, child, kinds) {
            return Some(name);
        }
    }
    None
}

fn find_descendant<'tree>(node: Node<'tree>, kind: &str) -> Option<Node<'tree>> {
    if node.kind() == kind {
        return Some(node);
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if let Some(found) = find_descendant(child, kind) {
            return Some(found);
        }
    }
    None
}

fn node_text(content: &str, node: Node<'_>) -> String {
    node.utf8_text(content.as_bytes())
        .unwrap_or_default()
        .to_string()
}

fn insert_string(metadata: &mut HashMap<String, Value>, key: &str, value: &str) {
    metadata.insert(key.to_string(), Value::String(value.to_string()));
}

fn attach_containing_symbols(facts: &mut [StructuralFact], symbols: &[Symbol]) {
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
