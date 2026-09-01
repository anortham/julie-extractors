use super::helpers::{find_child_by_type, get_node_text};
use super::type_facts;
use super::DartExtractor;
use crate::base::{Symbol, SymbolKind, SymbolOptions};
use tree_sitter::Node;

pub(super) fn extract_locals(
    extractor: &mut DartExtractor,
    node: Node,
    parent_id: Option<&str>,
) -> Vec<Symbol> {
    let Some(definition) = find_child_by_type(&node, "initialized_variable_definition") else {
        return Vec::new();
    };
    let Some(name_node) = definition
        .child_by_field_name("name")
        .filter(|name| name.kind() == "identifier")
        .or_else(|| find_child_by_type(&definition, "identifier"))
    else {
        return Vec::new();
    };
    let name = get_node_text(&name_node);
    if name.is_empty() {
        return Vec::new();
    }
    let signature = extractor.base.get_node_text(&definition);
    let type_node = definition
        .child_by_field_name("type")
        .or_else(|| find_child_by_type(&definition, "type"));
    let value = definition.child_by_field_name("value");
    let symbol = extractor.base.create_symbol(
        &definition,
        name,
        SymbolKind::Variable,
        SymbolOptions {
            signature: Some(signature),
            parent_id: parent_id.map(|parent| parent.to_string()),
            ..Default::default()
        },
    );
    if let Some(type_node) = type_node {
        type_facts::record_declared_type(&mut extractor.base, &symbol.id, type_node);
    } else if let Some(value) = value
        && let Some(class_name) = type_facts::inferred_constructor_name(
            &extractor.base,
            value,
            &extractor.same_file_type_names,
        )
    {
        type_facts::record_constructor_fact(&mut extractor.base, &symbol.id, &class_name);
    }
    vec![symbol]
}
