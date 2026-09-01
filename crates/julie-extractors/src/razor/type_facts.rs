//! Declared-type fact recording for C# nodes inside Razor code blocks.

use crate::base::types::TypeNameRules;
use crate::base::BaseExtractor;
use tree_sitter::Node;

pub(super) const RAZOR_TYPE_NAME_RULES: TypeNameRules = TypeNameRules {
    nullable_suffixes: &["?"],
    reference_prefixes: &["ref", "out", "in", "scoped"],
    generic_open: &['<'],
};

/// Record a syntactically stated type for a symbol (`is_inferred=false`).
pub(super) fn record_declared_type(base: &mut BaseExtractor, symbol_id: &str, type_node: Node) {
    record_type_node(base, symbol_id, type_node, false);
}

/// Record the constructed type of a `var x = new Foo(...)` initializer
/// (`is_inferred=true`). Target-typed `new()` carries no type node and
/// records nothing.
pub(super) fn record_new_expression_type(
    base: &mut BaseExtractor,
    symbol_id: &str,
    initializer: Node,
) {
    if initializer.kind() != "object_creation_expression" {
        return;
    }
    let Some(type_node) = initializer.child_by_field_name("type") else {
        return;
    };
    record_type_node(base, symbol_id, type_node, true);
}

/// Record a callable's declared return type (`is_inferred=false`). `void`
/// is not a type fact and records nothing.
pub(super) fn record_return_type(base: &mut BaseExtractor, symbol_id: &str, type_node: Node) {
    if base.get_node_text(&type_node).trim() == "void" {
        return;
    }
    record_type_node(base, symbol_id, type_node, false);
}

fn is_var_type(base: &BaseExtractor, type_node: Node) -> bool {
    type_node.kind() == "implicit_type" || base.get_node_text(&type_node) == "var"
}

fn record_type_node(base: &mut BaseExtractor, symbol_id: &str, type_node: Node, is_inferred: bool) {
    if is_var_type(base, type_node) || !names_single_base_type(type_node) {
        return;
    }
    let declared = base.get_node_text(&type_node);
    base.record_declared_type_fact(symbol_id, &declared, &RAZOR_TYPE_NAME_RULES, is_inferred);
}

/// True for type nodes whose text reduces to one base type name. Tuple,
/// pointer, function-pointer, and implicit (`var`) types do not, so they
/// record nothing.
fn names_single_base_type(node: Node) -> bool {
    match node.kind() {
        "predefined_type"
        | "identifier"
        | "generic_name"
        | "qualified_name"
        | "alias_qualified_name" => true,
        "nullable_type" | "ref_type" | "scoped_type" | "array_type" => node
            .child_by_field_name("type")
            .is_some_and(names_single_base_type),
        _ => false,
    }
}
