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

/// Record the declared type of a binding destructured from an annotated
/// parameter: `({ page }: { page: Page })` gives `page` the type `Page`.
pub(super) fn record_destructured_binding_fact(
    base: &mut BaseExtractor,
    symbol_id: &str,
    binding_node: Node,
) {
    let (key, pattern) = match binding_node.parent() {
        Some(parent) if parent.kind() == "object_pattern" => {
            (base.get_node_text(&binding_node), parent)
        }
        Some(parent) if parent.kind() == "pair_pattern" => {
            let Some(key) = parent.child_by_field_name("key") else {
                return;
            };
            let Some(pattern) = parent.parent().filter(|p| p.kind() == "object_pattern") else {
                return;
            };
            (base.get_node_text(&key), pattern)
        }
        Some(parent) if parent.kind() == "object_assignment_pattern" => {
            let Some(pattern) = parent.parent().filter(|p| p.kind() == "object_pattern") else {
                return;
            };
            (base.get_node_text(&binding_node), pattern)
        }
        _ => return,
    };
    let Some(parameter) = pattern.parent().filter(|parameter| {
        matches!(
            parameter.kind(),
            "required_parameter" | "optional_parameter"
        )
    }) else {
        return;
    };
    let Some(object_type) = annotation_type_node(parameter).filter(|t| t.kind() == "object_type")
    else {
        return;
    };
    let mut cursor = object_type.walk();
    let member = object_type.named_children(&mut cursor).find(|member| {
        member.kind() == "property_signature"
            && member
                .child_by_field_name("name")
                .is_some_and(|name| base.get_node_text(&name) == key)
    });
    if let Some(member) = member {
        record_annotation_fact(base, symbol_id, member);
    }
}

/// Record a callable's declared return type from its `return_type`
/// annotation: `getUser(): User` records `User`, and `Promise<User[]>`
/// records `Promise` with the full text as `declared`.
pub(super) fn record_return_type_fact(
    base: &mut BaseExtractor,
    symbol_id: &str,
    callable_node: Node,
) {
    let Some(type_node) = field_type_node(callable_node, "return_type")
        .filter(|type_node| !matches!(base.get_node_text(type_node).as_str(), "void" | "never"))
    else {
        return;
    };
    let Some(declared) = declared_type_text(base, type_node) else {
        return;
    };
    base.record_declared_type_fact(symbol_id, &declared, &TYPE_NAME_RULES, false);
}

fn annotation_type_node(annotated_node: Node<'_>) -> Option<Node<'_>> {
    field_type_node(annotated_node, "type")
}

fn field_type_node<'t>(annotated_node: Node<'t>, field: &str) -> Option<Node<'t>> {
    let annotation = annotated_node.child_by_field_name(field)?;
    let mut cursor = annotation.walk();
    annotation.named_children(&mut cursor).last()
}

fn declared_type_text(base: &BaseExtractor, type_node: Node) -> Option<String> {
    if is_plain_named_type(type_node) {
        return Some(base.get_node_text(&type_node));
    }
    if type_node.kind() == "array_type" {
        let mut cursor = type_node.walk();
        let element = type_node.named_children(&mut cursor).next()?;
        if is_plain_named_type(element) {
            return Some(base.get_node_text(&type_node));
        }
    }
    None
}

fn is_plain_named_type(type_node: Node) -> bool {
    match type_node.kind() {
        "type_identifier" | "nested_type_identifier" | "generic_type" => true,
        "predefined_type" => !is_unique_symbol_operator(type_node),
        _ => false,
    }
}

fn is_unique_symbol_operator(predefined_type_node: Node) -> bool {
    predefined_type_node
        .child(0)
        .is_some_and(|child| child.kind() == "unique symbol")
}
