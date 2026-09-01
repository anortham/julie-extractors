use crate::base::BaseExtractor;
use crate::base::types::TypeNameRules;
use serde_json::Value;
use std::collections::HashMap;
use tree_sitter::Node;

pub(super) const RUST_TYPE_NAME_RULES: TypeNameRules = TypeNameRules {
    nullable_suffixes: &[],
    reference_prefixes: &["&", "*const", "*mut", "*", "mut", "dyn", "impl"],
    generic_open: &['<'],
};

/// Record a syntactically stated type for a symbol (`is_inferred=false`).
pub(super) fn record_declared_type(base: &mut BaseExtractor, symbol_id: &str, type_node: Node) {
    record_type_node(base, symbol_id, type_node, false);
}

/// Record the constructed type of a `Type::new(...)` or `Type { .. }`
/// initializer (`is_inferred=true`). Any other initializer records nothing.
pub(super) fn record_initializer_type(base: &mut BaseExtractor, symbol_id: &str, value: Node) {
    match value.kind() {
        "call_expression" => {
            let Some(function) = value.child_by_field_name("function") else {
                return;
            };
            let function = if function.kind() == "generic_function" {
                function.child_by_field_name("function").unwrap_or(function)
            } else {
                function
            };
            if function.kind() != "scoped_identifier" {
                return;
            }
            let Some(name) = function.child_by_field_name("name") else {
                return;
            };
            if base.get_node_text(&name) != "new" {
                return;
            }
            let Some(path) = function.child_by_field_name("path") else {
                return;
            };
            record_type_node(base, symbol_id, path, true);
        }
        "struct_expression" => {
            if let Some(name) = value.child_by_field_name("name") {
                record_type_node(base, symbol_id, name, true);
            }
        }
        _ => {}
    }
}

/// Record the impl target type for a `self` parameter (`is_inferred=false`).
pub(super) fn record_impl_self_type(
    base: &mut BaseExtractor,
    symbol_id: &str,
    impl_type_name: &str,
) {
    base.record_declared_type_fact(symbol_id, impl_type_name, &RUST_TYPE_NAME_RULES, false);
}

fn record_type_node(base: &mut BaseExtractor, symbol_id: &str, type_node: Node, is_inferred: bool) {
    let Some(name_node) = base_type_name_node(type_node) else {
        return;
    };
    if base.type_info.contains_key(symbol_id) {
        return;
    }
    let base_name = base.get_node_text(&name_node);
    base.record_declared_type_fact(symbol_id, &base_name, &RUST_TYPE_NAME_RULES, is_inferred);
    let declared = base.get_node_text(&type_node).trim().to_string();
    if let Some(fact) = base.type_info.get_mut(symbol_id) {
        if fact.resolved_type == declared {
            fact.metadata = None;
        } else {
            fact.metadata
                .get_or_insert_with(HashMap::new)
                .insert("declared".to_string(), Value::String(declared));
        }
    }
}

/// Structurally reduce a type-position node to the single node naming its base
/// type: the final path segment, with generics, turbofish, reference, pointer,
/// `dyn`, and `impl` wrappers dropped. Shapes without one base name (tuples,
/// arrays, function types) yield nothing.
fn base_type_name_node(node: Node) -> Option<Node> {
    let mut node = node;
    loop {
        match node.kind() {
            "type_identifier" | "identifier" | "primitive_type" => return Some(node),
            "scoped_type_identifier" | "scoped_identifier" => {
                return node.child_by_field_name("name");
            }
            "generic_type" | "generic_type_with_turbofish" | "reference_type" | "pointer_type" => {
                node = node.child_by_field_name("type")?;
            }
            "dynamic_type" | "abstract_type" => {
                node = node.child_by_field_name("trait")?;
            }
            _ => return None,
        }
    }
}
