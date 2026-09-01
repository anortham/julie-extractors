//! Declared-type fact recording for JavaScript.

use crate::base::BaseExtractor;
use crate::base::types::TypeNameRules;
use tree_sitter::Node;

pub(crate) const TYPE_NAME_RULES: TypeNameRules = TypeNameRules {
    nullable_suffixes: &[],
    reference_prefixes: &[],
    generic_open: &['<'],
};

/// Record an inferred type fact for `symbol_id` when `value_node` is a plain
/// `new Identifier(...)` expression. Qualified constructors such as
/// `new ns.Foo()` record nothing.
pub(crate) fn record_new_expression_fact(
    base: &mut BaseExtractor,
    symbol_id: &str,
    value_node: Node,
    rules: &TypeNameRules,
) {
    if value_node.kind() != "new_expression" {
        return;
    }
    let Some(constructor) = value_node.child_by_field_name("constructor") else {
        return;
    };
    if constructor.kind() != "identifier" {
        return;
    }
    let declared = base.get_node_text(&constructor);
    base.record_declared_type_fact(symbol_id, &declared, rules, true);
}
