//! Declared-type fact recording for Go.

use crate::base::BaseExtractor;
use crate::base::types::TypeNameRules;
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};
use tree_sitter::Node;

const TYPE_NAME_RULES: TypeNameRules = TypeNameRules {
    nullable_suffixes: &[],
    reference_prefixes: &["*"],
    generic_open: &[],
};

/// `[` opens both generic argument lists and array types in Go, so this rules
/// set applies only to nodes already proven to be `generic_type` with a
/// `type_identifier` base; array, slice, and map types never reach it.
const GENERIC_INSTANTIATION_RULES: TypeNameRules = TypeNameRules {
    nullable_suffixes: &[],
    reference_prefixes: &["*"],
    generic_open: &['['],
};

/// Record a declared-type fact for `symbol_id` when `type_node` names a base
/// type Miller can bind: a plain or qualified type name, a generic
/// instantiation with an identifier base, or a pointer to one of those.
pub(super) fn record_type_node_fact(
    base: &mut BaseExtractor,
    symbol_id: &str,
    type_node: Node,
    is_inferred: bool,
) {
    let Some(rules) = binding_rules(type_node) else {
        return;
    };
    let declared = base.get_node_text(&type_node);
    base.record_declared_type_fact(symbol_id, &declared, rules, is_inferred);
}

pub(super) fn binds_base_type(type_node: Node) -> bool {
    binding_rules(type_node).is_some()
}

fn binding_rules(type_node: Node) -> Option<&'static TypeNameRules> {
    match type_node.kind() {
        "type_identifier" | "qualified_type" => Some(&TYPE_NAME_RULES),
        "generic_type" => generic_rules(type_node),
        "pointer_type" => match type_node.named_child(0)?.kind() {
            "type_identifier" | "qualified_type" => Some(&TYPE_NAME_RULES),
            "generic_type" => generic_rules(type_node.named_child(0)?),
            _ => None,
        },
        _ => None,
    }
}

fn generic_rules(generic_node: Node) -> Option<&'static TypeNameRules> {
    let base_node = generic_node.child_by_field_name("type")?;
    (base_node.kind() == "type_identifier").then_some(&GENERIC_INSTANTIATION_RULES)
}

/// The type node of a `Foo{...}` or `&Foo{...}` initializer, or of a same-file
/// `NewFoo(...)` call whose result is a single named type or pointer to one.
pub(super) fn inferred_rhs_type_node<'a>(
    base: &BaseExtractor,
    value_node: Node<'a>,
) -> Option<Node<'a>> {
    composite_literal_type_node(value_node)
        .or_else(|| constructor_result_type_node(base, value_node))
}

/// The type node of a `Foo{...}` or `&Foo{...}` initializer, when present.
pub(super) fn composite_literal_type_node(value_node: Node) -> Option<Node> {
    let literal = match value_node.kind() {
        "composite_literal" => value_node,
        "unary_expression" => {
            let operator = value_node.child_by_field_name("operator")?;
            let operand = value_node.child_by_field_name("operand")?;
            (operator.kind() == "&" && operand.kind() == "composite_literal").then_some(operand)?
        }
        _ => return None,
    };
    literal.child_by_field_name("type")
}

fn constructor_result_type_node<'a>(
    base: &BaseExtractor,
    value_node: Node<'a>,
) -> Option<Node<'a>> {
    if value_node.kind() != "call_expression" {
        return None;
    }
    let function = value_node.child_by_field_name("function")?;
    if function.kind() != "identifier" {
        return None;
    }
    let name = base.get_node_text(&function);
    let declaration = same_file_function_declaration(value_node, &name, base)?;
    let result = declaration.child_by_field_name("result")?;
    match result.kind() {
        "type_identifier" => named_constructor_result(base, result, result),
        "pointer_type" => {
            let inner = result.named_child(0)?;
            named_constructor_result(base, inner, result)
        }
        _ => None,
    }
}

fn named_constructor_result<'a>(
    base: &BaseExtractor,
    type_id: Node,
    result: Node<'a>,
) -> Option<Node<'a>> {
    if type_id.kind() != "type_identifier" {
        return None;
    }
    let name = base.get_node_text(&type_id);
    if is_predeclared_type(&name) {
        return None;
    }
    Some(result)
}

fn same_file_function_declaration<'a>(
    node: Node<'a>,
    name: &str,
    base: &BaseExtractor,
) -> Option<Node<'a>> {
    find_function_declaration(file_root(node), name, base, 0)
}

fn file_root(mut node: Node) -> Node {
    while let Some(parent) = node.parent() {
        node = parent;
    }
    node
}

fn find_function_declaration<'a>(
    node: Node<'a>,
    name: &str,
    base: &BaseExtractor,
    depth: u32,
) -> Option<Node<'a>> {
    if !should_visit_tree_depth(depth) {
        return None;
    }
    if node.kind() == "function_declaration"
        && let Some(name_node) = node.child_by_field_name("name")
        && base.get_node_text(&name_node) == name
    {
        return Some(node);
    }
    let Some(child_depth) = child_tree_depth(depth) else {
        return None;
    };
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if let Some(found) = find_function_declaration(child, name, base, child_depth) {
            return Some(found);
        }
    }
    None
}

fn is_predeclared_type(name: &str) -> bool {
    matches!(
        name,
        "any"
            | "bool"
            | "byte"
            | "comparable"
            | "complex64"
            | "complex128"
            | "error"
            | "float32"
            | "float64"
            | "int"
            | "int8"
            | "int16"
            | "int32"
            | "int64"
            | "rune"
            | "string"
            | "uint"
            | "uint8"
            | "uint16"
            | "uint32"
            | "uint64"
            | "uintptr"
    )
}
