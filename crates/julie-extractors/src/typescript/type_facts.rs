//! Declared-type fact recording for TypeScript.

use crate::base::BaseExtractor;
use crate::base::types::TypeNameRules;
use crate::javascript::type_facts::record_new_expression_fact;
use tree_sitter::Node;

pub(super) const TYPE_NAME_RULES: TypeNameRules = TypeNameRules {
    nullable_suffixes: &[],
    reference_prefixes: &[],
    generic_open: &['<'],
};

/// Record facts for a `variable_declarator` that names a plain identifier:
/// the annotation when present, then a plain `new Foo()` initializer.
/// Destructuring declarators record nothing.
pub(super) fn record_variable_type_facts(
    base: &mut BaseExtractor,
    symbol_id: &str,
    declarator_node: Node,
) {
    let names_identifier = declarator_node
        .child_by_field_name("name")
        .is_some_and(|name| name.kind() == "identifier");
    if !names_identifier {
        return;
    }
    record_annotation_fact(base, symbol_id, declarator_node);
    if let Some(value_node) = declarator_node.child_by_field_name("value") {
        record_new_expression_fact(base, symbol_id, value_node, &TYPE_NAME_RULES);
    }
}

/// Record a declared fact from a node's `type` field when the annotation names
/// a single type plainly. Unions, intersections, object/mapped/conditional
/// types, function types, and literal types record nothing.
pub(super) fn record_annotation_fact(
    base: &mut BaseExtractor,
    symbol_id: &str,
    annotated_node: Node,
) {
    let Some(type_node) = annotation_type_node(annotated_node) else {
        return;
    };
    let Some(declared) = declared_type_text(base, type_node) else {
        return;
    };
    base.record_declared_type_fact(symbol_id, &declared, &TYPE_NAME_RULES, false);
}

fn annotation_type_node(annotated_node: Node<'_>) -> Option<Node<'_>> {
    let annotation = annotated_node.child_by_field_name("type")?;
    let mut cursor = annotation.walk();
    annotation.named_children(&mut cursor).last()
}

fn declared_type_text(base: &BaseExtractor, type_node: Node) -> Option<String> {
    if is_plain_named_type(type_node.kind()) {
        return Some(base.get_node_text(&type_node));
    }
    if type_node.kind() == "array_type" {
        let mut cursor = type_node.walk();
        let element = type_node.named_children(&mut cursor).next()?;
        if is_plain_named_type(element.kind()) {
            return Some(base.get_node_text(&type_node));
        }
    }
    None
}

fn is_plain_named_type(node_kind: &str) -> bool {
    matches!(
        node_kind,
        "type_identifier" | "nested_type_identifier" | "predefined_type" | "generic_type"
    )
}
