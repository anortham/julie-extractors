/// Table field extraction: `{ field = value, method = function() end }`
use super::core::ValueOwners;
use super::variables;
use crate::base::{BaseExtractor, Symbol, SymbolKind, Visibility};
use tree_sitter::Node;

/// Extract the named fields of a table constructor as child symbols of `parent_id`.
pub(super) fn extract_table_fields(
    symbols: &mut Vec<Symbol>,
    base: &mut BaseExtractor,
    owners: &mut ValueOwners,
    node: Node,
    parent_id: Option<&str>,
) {
    let mut cursor = node.walk();
    for field in node.children(&mut cursor) {
        if field.kind() != "field" {
            continue;
        }
        let (Some(name_node), Some(value)) = (
            field.child_by_field_name("name"),
            field.child_by_field_name("value"),
        ) else {
            continue;
        };
        if name_node.kind() != "identifier" {
            continue;
        }
        let (kind, data_type) = variables::infer_kind_and_type(base, value, true);
        let kind = if kind == SymbolKind::Import {
            SymbolKind::Field
        } else {
            kind
        };
        variables::push_variable_symbol(
            symbols,
            base,
            owners,
            &name_node,
            base.get_node_text(&name_node),
            kind,
            data_type,
            base.get_node_text(&field),
            parent_id.map(str::to_string),
            Visibility::Public,
            None,
            Some(value),
        );
    }
}
