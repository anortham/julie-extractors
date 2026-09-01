use crate::base::{Symbol, SymbolKind, SymbolOptions};
use std::collections::HashMap;
use tree_sitter::Node;

use super::text_args::clean_r_name;
use super::RExtractor;

pub(super) fn extract_parameter_symbols(
    extractor: &mut RExtractor,
    func_def: Node,
    parent_id: &str,
) -> Vec<Symbol> {
    let Some(params_node) = parameters_node(func_def) else {
        return Vec::new();
    };

    let mut symbols = Vec::new();
    let mut cursor = params_node.walk();
    for child in params_node.children(&mut cursor) {
        match child.kind() {
            "parameter" | "default_parameter" => {
                if let Some(symbol) = parameter_symbol(extractor, child, parent_id) {
                    symbols.push(symbol);
                }
            }
            "identifier" => {
                if let Some(symbol) = named_parameter_symbol(extractor, child, parent_id) {
                    symbols.push(symbol);
                }
            }
            _ => {}
        }
    }
    symbols
}

pub(super) fn extract_class_method_parameters(extractor: &mut RExtractor, func_def: Node) {
    let Some(parent) = func_def.parent() else {
        return;
    };
    let Some(method_name) = super::type_facts::argument_name(extractor, parent) else {
        return;
    };
    let Some(class_name) = super::type_facts::enclosing_r6_class_name(extractor, func_def) else {
        return;
    };
    let Some(class_id) = extractor
        .symbols
        .iter()
        .find(|symbol| symbol.name == class_name && symbol.kind == SymbolKind::Class)
        .map(|symbol| symbol.id.clone())
    else {
        return;
    };
    let Some(parent_id) = extractor
        .symbols
        .iter()
        .find(|symbol| {
            symbol.name == method_name
                && symbol.parent_id.as_deref() == Some(class_id.as_str())
                && matches!(symbol.kind, SymbolKind::Method | SymbolKind::Function)
        })
        .map(|symbol| symbol.id.clone())
    else {
        return;
    };
    let parameter_symbols = extract_parameter_symbols(extractor, func_def, &parent_id);
    extractor.symbols.extend(parameter_symbols);
}

fn parameters_node(func_def: Node) -> Option<Node> {
    if let Some(node) = func_def.child_by_field_name("parameters") {
        return Some(node);
    }
    let mut cursor = func_def.walk();
    func_def.children(&mut cursor).find(|child| {
        child.kind() == "formal_parameters" || child.kind() == "parameters"
    })
}

fn parameter_symbol(
    extractor: &mut RExtractor,
    param_node: Node,
    parent_id: &str,
) -> Option<Symbol> {
    let name = parameter_name(extractor, param_node)?;
    Some(named_parameter_symbol_on(extractor, param_node, name, parent_id))
}

fn named_parameter_symbol(
    extractor: &mut RExtractor,
    name_node: Node,
    parent_id: &str,
) -> Option<Symbol> {
    let name = clean_r_name(&extractor.base.get_node_text(&name_node))?;
    if name == "..." {
        return None;
    }
    Some(named_parameter_symbol_on(
        extractor, name_node, name, parent_id,
    ))
}

fn parameter_name(extractor: &RExtractor, param_node: Node) -> Option<String> {
    if let Some(name_node) = param_node.child_by_field_name("name") {
        return clean_r_name(&extractor.base.get_node_text(&name_node)).filter(|name| name != "...");
    }
    let mut cursor = param_node.walk();
    for child in param_node.named_children(&mut cursor) {
        if child.kind() == "identifier" {
            return clean_r_name(&extractor.base.get_node_text(&child)).filter(|name| name != "...");
        }
    }
    let text = extractor.base.get_node_text(&param_node);
    let name = text.split('=').next()?.trim();
    clean_r_name(name).filter(|name| name != "...")
}

fn named_parameter_symbol_on(
    extractor: &mut RExtractor,
    node: Node,
    name: String,
    parent_id: &str,
) -> Symbol {
    let signature = extractor.base.get_node_text(&node);
    let metadata = HashMap::from([(
        "role".to_string(),
        serde_json::Value::String("parameter".to_string()),
    )]);
    extractor.base.create_symbol(
        &node,
        name,
        SymbolKind::Variable,
        SymbolOptions {
            signature: Some(signature),
            parent_id: Some(parent_id.to_string()),
            metadata: Some(metadata),
            ..Default::default()
        },
    )
}
