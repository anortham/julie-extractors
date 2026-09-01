//! Local variable symbol extraction.

use crate::base::{Symbol, SymbolKind, SymbolOptions};
use crate::java::JavaExtractor;
use tree_sitter::Node;

use super::type_facts;

/// Create one `variable` symbol per declarator of a `local_variable_declaration`,
/// parented to the enclosing symbol. Stated types record a declared-type fact;
/// `var` locals record the constructed type of a `new Foo(...)` initializer
/// (`is_inferred=true`) and record nothing for any other initializer.
pub(super) fn extract_locals(
    extractor: &mut JavaExtractor,
    node: Node,
    parent_id: Option<&str>,
) -> Vec<Symbol> {
    let type_node = node.child_by_field_name("type");
    let type_text = type_node.map(|type_node| extractor.base().get_node_text(&type_node));

    let declarators: Vec<Node> = {
        let mut cursor = node.walk();
        node.children_by_field_name("declarator", &mut cursor)
            .collect()
    };

    let mut symbols = Vec::new();
    for declarator in declarators {
        let Some(name_node) = declarator
            .child_by_field_name("name")
            .filter(|name| name.kind() == "identifier")
        else {
            continue;
        };
        let name = extractor.base().get_node_text(&name_node);
        let declarator_text = extractor.base().get_node_text(&declarator);
        let signature = match &type_text {
            Some(type_text) => format!("{type_text} {declarator_text}"),
            None => declarator_text,
        };
        let symbol = extractor.base_mut().create_symbol(
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
            if type_facts::is_var_type(extractor.base(), type_node) {
                if let Some(value) = declarator.child_by_field_name("value") {
                    type_facts::record_new_expression_type(extractor.base_mut(), &symbol.id, value);
                }
            } else if declarator.child_by_field_name("dimensions").is_none() {
                type_facts::record_declared_type(extractor.base_mut(), &symbol.id, type_node);
            }
        }
        symbols.push(symbol);
    }
    symbols
}

pub(super) fn extract_bindings(
    extractor: &mut JavaExtractor,
    node: Node,
    parent_id: Option<&str>,
) -> Vec<Symbol> {
    match node.kind() {
        "resource" => extract_resource(extractor, node, parent_id)
            .into_iter()
            .collect(),
        "enhanced_for_statement" => extract_enhanced_for(extractor, node, parent_id)
            .into_iter()
            .collect(),
        "catch_formal_parameter" => extract_catch_parameter(extractor, node, parent_id)
            .into_iter()
            .collect(),
        "instanceof_expression" => extract_instanceof_binding(extractor, node, parent_id)
            .into_iter()
            .collect(),
        _ => Vec::new(),
    }
}

fn extract_resource(
    extractor: &mut JavaExtractor,
    node: Node,
    parent_id: Option<&str>,
) -> Option<Symbol> {
    let name_node = node
        .child_by_field_name("name")
        .filter(|name| name.kind() == "identifier")?;
    let type_node = node.child_by_field_name("type");
    let record_type = type_node
        .is_some_and(|type_node| !type_facts::is_var_type(extractor.base(), type_node))
        && node.child_by_field_name("dimensions").is_none();
    Some(binding_symbol(
        extractor,
        node,
        name_node,
        type_node,
        parent_id,
        record_type,
    ))
}

fn extract_enhanced_for(
    extractor: &mut JavaExtractor,
    node: Node,
    parent_id: Option<&str>,
) -> Option<Symbol> {
    let name_node = node
        .child_by_field_name("name")
        .filter(|name| name.kind() == "identifier")?;
    let type_node = node.child_by_field_name("type");
    let record_type = type_node
        .is_some_and(|type_node| !type_facts::is_var_type(extractor.base(), type_node))
        && node.child_by_field_name("dimensions").is_none();
    Some(binding_symbol(
        extractor,
        name_node,
        name_node,
        type_node,
        parent_id,
        record_type,
    ))
}

fn extract_catch_parameter(
    extractor: &mut JavaExtractor,
    node: Node,
    parent_id: Option<&str>,
) -> Option<Symbol> {
    let name_node = node
        .child_by_field_name("name")
        .filter(|name| name.kind() == "identifier")?;
    let catch_type = {
        let mut cursor = node.walk();
        node.named_children(&mut cursor)
            .find(|child| child.kind() == "catch_type")
    };
    let single_type = catch_type.and_then(|catch_type| {
        let mut cursor = catch_type.walk();
        let types: Vec<Node> = catch_type.named_children(&mut cursor).collect();
        if types.len() == 1 {
            Some(types[0])
        } else {
            None
        }
    });
    Some(binding_symbol(
        extractor,
        node,
        name_node,
        single_type,
        parent_id,
        single_type.is_some() && node.child_by_field_name("dimensions").is_none(),
    ))
}

fn extract_instanceof_binding(
    extractor: &mut JavaExtractor,
    node: Node,
    parent_id: Option<&str>,
) -> Option<Symbol> {
    let name_node = node
        .child_by_field_name("name")
        .filter(|name| name.kind() == "identifier")?;
    let type_node = node.child_by_field_name("right");
    Some(binding_symbol(
        extractor,
        name_node,
        name_node,
        type_node,
        parent_id,
        type_node.is_some(),
    ))
}

fn binding_symbol(
    extractor: &mut JavaExtractor,
    span_node: Node,
    name_node: Node,
    type_node: Option<Node>,
    parent_id: Option<&str>,
    record_type: bool,
) -> Symbol {
    let name = extractor.base().get_node_text(&name_node);
    let signature = match type_node {
        Some(type_node) => format!("{} {name}", extractor.base().get_node_text(&type_node)),
        None => name.clone(),
    };
    let symbol = extractor.base_mut().create_symbol(
        &span_node,
        name,
        SymbolKind::Variable,
        SymbolOptions {
            signature: Some(signature),
            parent_id: parent_id.map(|parent| parent.to_string()),
            ..Default::default()
        },
    );
    if record_type {
        if let Some(type_node) = type_node {
            type_facts::record_declared_type(extractor.base_mut(), &symbol.id, type_node);
        }
    }
    symbol
}
