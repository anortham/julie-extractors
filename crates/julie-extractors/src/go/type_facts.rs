//! Declared-type fact recording for Go.

use crate::base::BaseExtractor;
use crate::base::types::TypeNameRules;
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
