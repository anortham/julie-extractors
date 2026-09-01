use super::type_facts;
use crate::base::{BaseExtractor, Symbol, SymbolKind, SymbolOptions, Visibility};
use std::collections::HashSet;
use tree_sitter::Node;

pub(super) fn extract_dim_statement(
    base: &mut BaseExtractor,
    node: Node,
    parent_id: Option<String>,
    symbols: &[Symbol],
) -> Vec<Symbol> {
    let same_file = same_file_type_names(symbols);
    let mut out = Vec::new();
    let mut pending_name: Option<Node> = None;
    let mut pending_type: Option<Node> = None;
    let mut pending_init: Option<Node> = None;

    for i in 0..node.child_count() {
        let Some(child) = node.child(i as u32) else {
            continue;
        };
        let field = node.field_name_for_child(i as u32);
        match field {
            Some("name") => {
                flush_dim_binding(
                    base,
                    parent_id.clone(),
                    &same_file,
                    &mut out,
                    pending_name,
                    pending_type,
                    pending_init,
                );
                pending_name = Some(child);
                pending_type = None;
                pending_init = None;
            }
            Some("value") if child.kind() == "new_expression" => {
                pending_type = child.child_by_field_name("type");
            }
            Some("initializer") => {
                pending_init = Some(child);
            }
            _ if child.kind() == "as_clause" => {
                pending_type = child.child_by_field_name("type");
            }
            _ => {}
        }
    }

    flush_dim_binding(
        base,
        parent_id,
        &same_file,
        &mut out,
        pending_name,
        pending_type,
        pending_init,
    );
    out
}

fn flush_dim_binding(
    base: &mut BaseExtractor,
    parent_id: Option<String>,
    same_file: &HashSet<String>,
    out: &mut Vec<Symbol>,
    name_node: Option<Node>,
    type_node: Option<Node>,
    initializer: Option<Node>,
) {
    let Some(name_node) = name_node else {
        return;
    };
    let name = base.get_node_text(&name_node);
    let signature = format!("Dim {}", name);
    let symbol = base.create_symbol(
        &name_node,
        name,
        SymbolKind::Variable,
        SymbolOptions {
            signature: Some(signature),
            visibility: Some(Visibility::Private),
            parent_id,
            ..Default::default()
        },
    );
    if let Some(type_node) = type_node {
        type_facts::record_declared_type(base, &symbol.id, type_node);
    } else if let Some(initializer) = initializer
        && let Some(constructed) = type_facts::constructor_type_node(initializer)
        && let Some(class_name) = type_facts::simple_unqualified_name(base, constructed)
        && same_file.contains(&class_name)
    {
        type_facts::record_constructor_fact(base, &symbol.id, &class_name);
    }
    out.push(symbol);
}

fn same_file_type_names(symbols: &[Symbol]) -> HashSet<String> {
    symbols
        .iter()
        .filter(|symbol| matches!(symbol.kind, SymbolKind::Class | SymbolKind::Struct))
        .map(|symbol| symbol.name.clone())
        .collect()
}
