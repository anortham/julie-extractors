//! Declared-type fact recording for Java.

use crate::base::BaseExtractor;
use crate::base::types::TypeNameRules;
use tree_sitter::Node;

pub(super) const JAVA_TYPE_NAME_RULES: TypeNameRules = TypeNameRules {
    nullable_suffixes: &[],
    reference_prefixes: &[],
    generic_open: &['<'],
};

/// Record a syntactically stated type for a symbol (`is_inferred=false`).
pub(super) fn record_declared_type(base: &mut BaseExtractor, symbol_id: &str, type_node: Node) {
    record_type_node(base, symbol_id, type_node, false);
}

/// Record the constructed type of a `var x = new Foo(...)` initializer
/// (`is_inferred=true`). Any other initializer records nothing.
pub(super) fn record_new_expression_type(
    base: &mut BaseExtractor,
    symbol_id: &str,
    value_node: Node,
) {
    if value_node.kind() != "object_creation_expression" {
        return;
    }
    let Some(type_node) = value_node.child_by_field_name("type") else {
        return;
    };
    record_type_node(base, symbol_id, type_node, true);
}

/// Record a method's declared return type (`is_inferred=false`). `void`
/// is not a type fact and records nothing.
pub(super) fn record_return_type(base: &mut BaseExtractor, symbol_id: &str, type_node: Node) {
    if type_node.kind() == "void_type" {
        return;
    }
    record_type_node(base, symbol_id, type_node, false);
}

/// True when a stated type is the `var` keyword, which tree-sitter-java
/// parses as a `type_identifier` named `var`. `var` is a reserved type name
/// in Java, so no real type can collide with it.
pub(super) fn is_var_type(base: &BaseExtractor, type_node: Node) -> bool {
    type_node.kind() == "type_identifier" && base.get_node_text(&type_node) == "var"
}

fn record_type_node(base: &mut BaseExtractor, symbol_id: &str, type_node: Node, is_inferred: bool) {
    if !names_single_base_type(type_node) || is_var_type(base, type_node) {
        return;
    }
    let declared = base.get_node_text(&type_node);
    base.record_declared_type_fact(symbol_id, &declared, &JAVA_TYPE_NAME_RULES, is_inferred);
}

/// True for type nodes whose text reduces to one base type name. Generics
/// over dotted bases, wildcards, `void`, and annotated types do not, so they
/// record nothing. Array types record their full `Foo[]` text unstripped.
fn names_single_base_type(node: Node) -> bool {
    match node.kind() {
        "type_identifier"
        | "scoped_type_identifier"
        | "integral_type"
        | "floating_point_type"
        | "boolean_type" => true,
        "generic_type" => {
            let mut cursor = node.walk();
            node.children(&mut cursor)
                .any(|child| child.kind() == "type_identifier")
        }
        "array_type" => node
            .child_by_field_name("element")
            .is_some_and(names_single_base_type),
        _ => false,
    }
}
