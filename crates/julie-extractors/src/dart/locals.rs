use super::DartExtractor;
use super::helpers::{find_child_by_type, get_node_text};
use super::type_facts;
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
    let type_node = definition
        .child_by_field_name("type")
        .or_else(|| find_child_by_type(&definition, "type"));
    let extra_declarators: Vec<Node> = {
        let mut cursor = definition.walk();
        definition
            .named_children(&mut cursor)
            .filter(|child| child.kind() == "initialized_identifier")
            .collect()
    };
    let first_signature = match extra_declarators.first() {
        Some(next) => extractor.base.content[definition.start_byte()..next.start_byte()]
            .trim_end()
            .trim_end_matches(',')
            .trim_end()
            .to_string(),
        None => extractor.base.get_node_text(&definition),
    };
    let mut symbols = Vec::new();
    symbols.extend(extract_local(
        extractor,
        definition,
        first_signature,
        type_node,
        parent_id,
    ));
    for declarator in extra_declarators {
        let signature = extractor.base.get_node_text(&declarator);
        symbols.extend(extract_local(
            extractor, declarator, signature, type_node, parent_id,
        ));
    }
    symbols
}

fn extract_local(
    extractor: &mut DartExtractor,
    declarator: Node,
    signature: String,
    type_node: Option<Node>,
    parent_id: Option<&str>,
) -> Option<Symbol> {
    let name_node = declarator
        .child_by_field_name("name")
        .filter(|name| name.kind() == "identifier")
        .or_else(|| find_child_by_type(&declarator, "identifier"))?;
    let name = get_node_text(&name_node);
    if name.is_empty() {
        return None;
    }
    let value = declarator.child_by_field_name("value");
    let symbol = extractor.base.create_symbol(
        &declarator,
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
    Some(symbol)
}
