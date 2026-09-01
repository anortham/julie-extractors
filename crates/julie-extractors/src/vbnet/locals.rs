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
    let mut pending = DimBinding::default();

    for i in 0..node.child_count() {
        let Some(child) = node.child(i as u32) else {
            continue;
        };
        let field = node.field_name_for_child(i as u32);
        match field {
            Some("name") => {
                flush_dim_binding(base, parent_id.clone(), &same_file, &mut out, pending);
                pending = DimBinding {
                    name: Some(child),
                    ..Default::default()
                };
            }
            Some("value") if child.kind() == "new_expression" => {
                pending.type_node = child.child_by_field_name("type");
            }
            Some("initializer") => {
                pending.initializer = Some(child);
            }
            _ if child.kind() == "as_clause" => {
                pending.type_node = child.child_by_field_name("type");
            }
            _ if child.kind() == "array_rank_specifier" => {
                pending.rank = Some(child);
            }
            _ => {}
        }
    }

    flush_dim_binding(base, parent_id, &same_file, &mut out, pending);
    out
}

#[derive(Default, Clone, Copy)]
struct DimBinding<'a> {
    name: Option<Node<'a>>,
    type_node: Option<Node<'a>>,
    rank: Option<Node<'a>>,
    initializer: Option<Node<'a>>,
}

fn flush_dim_binding(
    base: &mut BaseExtractor,
    parent_id: Option<String>,
    same_file: &HashSet<String>,
    out: &mut Vec<Symbol>,
    binding: DimBinding,
) {
    let DimBinding {
        name: name_node,
        type_node,
        rank,
        initializer,
    } = binding;
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
        type_facts::record_declared_type(base, &symbol.id, type_node, rank);
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
