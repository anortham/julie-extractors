use crate::base::BaseExtractor;
use crate::base::types::TypeNameRules;
use std::collections::HashSet;
use tree_sitter::Node;

pub(super) const DART_TYPE_NAME_RULES: TypeNameRules = TypeNameRules {
    nullable_suffixes: &["?"],
    reference_prefixes: &[],
    generic_open: &['<'],
};

pub(super) fn record_declared_type(base: &mut BaseExtractor, symbol_id: &str, type_node: Node) {
    let declared = base.get_node_text(&type_node);
    if declared.is_empty() || declared == "void" || declared == "var" {
        return;
    }
    base.record_declared_type_fact(symbol_id, &declared, &DART_TYPE_NAME_RULES, false);
}

pub(super) fn record_constructor_fact(base: &mut BaseExtractor, symbol_id: &str, class_name: &str) {
    base.record_declared_type_fact(symbol_id, class_name, &DART_TYPE_NAME_RULES, true);
}

pub(super) fn inferred_constructor_name(
    base: &BaseExtractor,
    value: Node,
    same_file_types: &HashSet<String>,
) -> Option<String> {
    match value.kind() {
        "new_expression" | "const_object_expression" => {
            let type_node = value.child_by_field_name("type")?;
            let name = constructor_type_name(base, type_node)?;
            same_file_types.contains(&name).then_some(name)
        }
        "call_expression" => {
            let function = value.child_by_field_name("function")?;
            let name = match function.kind() {
                "identifier" => base.get_node_text(&function),
                "member_expression" | "null_aware_member_expression" => {
                    let object = function.child_by_field_name("object")?;
                    if object.kind() != "identifier" {
                        return None;
                    }
                    base.get_node_text(&object)
                }
                _ => return None,
            };
            same_file_types.contains(&name).then_some(name)
        }
        _ => None,
    }
}

fn constructor_type_name(base: &BaseExtractor, type_node: Node) -> Option<String> {
    let declared = base.get_node_text(&type_node);
    let stripped = crate::base::types::strip_type_decorations(&declared, &DART_TYPE_NAME_RULES);
    if stripped.is_empty() || stripped.contains('.') {
        None
    } else {
        Some(stripped)
    }
}
