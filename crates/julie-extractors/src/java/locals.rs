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
