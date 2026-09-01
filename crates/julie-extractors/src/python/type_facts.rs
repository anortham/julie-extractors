/// Declared-type fact recording for receiver-typed call resolution.
/// Records only plainly named annotations: a bare identifier, a plain dotted
/// name, or a subscript whose base is one of those. Unions, string
/// annotations, and inline callables record nothing.
use crate::base::BaseExtractor;
use crate::base::types::TypeNameRules;
use tree_sitter::Node;

pub(super) const PYTHON_TYPE_NAME_RULES: TypeNameRules = TypeNameRules {
    nullable_suffixes: &[],
    reference_prefixes: &[],
    generic_open: &['['],
};

/// Record a syntactically stated annotation for a symbol (`is_inferred=false`).
pub(super) fn record_annotation_fact(base: &mut BaseExtractor, symbol_id: &str, type_node: Node) {
    if let Some(declared) = plainly_named_annotation(base, type_node) {
        base.record_declared_type_fact(symbol_id, &declared, &PYTHON_TYPE_NAME_RULES, false);
    }
}

/// Record the class constructed by an `x = Foo()` initializer when `Foo` is a
/// class defined in the same file (`is_inferred=true`).
pub(super) fn record_constructor_fact(base: &mut BaseExtractor, symbol_id: &str, class_name: &str) {
    base.record_declared_type_fact(symbol_id, class_name, &PYTHON_TYPE_NAME_RULES, true);
}

fn plainly_named_annotation(base: &BaseExtractor, node: Node) -> Option<String> {
    match node.kind() {
        "type" => plainly_named_annotation(base, node.named_child(0)?),
        "identifier" => Some(base.get_node_text(&node)),
        "attribute" | "member_type" => is_plain_name(node).then(|| base.get_node_text(&node)),
        "generic_type" => Some(base.get_node_text(&node)),
        "subscript" => {
            let value = node.child_by_field_name("value")?;
            is_plain_name(value).then(|| base.get_node_text(&node))
        }
        _ => None,
    }
}

fn is_plain_name(node: Node) -> bool {
    match node.kind() {
        "type" | "member_type" => node.named_child(0).is_some_and(is_plain_name),
        "identifier" => true,
        "attribute" => node
            .child_by_field_name("object")
            .is_some_and(is_plain_name),
        _ => false,
    }
}
