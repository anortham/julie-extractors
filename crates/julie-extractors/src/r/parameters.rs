use crate::base::{Symbol, SymbolKind, SymbolOptions};
use std::collections::HashMap;
use tree_sitter::Node;

use super::RExtractor;
use super::text_args::clean_r_name;

pub(super) fn extract_parameter_symbols(
    extractor: &mut RExtractor,
    func_def: Node,
    parent_id: &str,
) -> Vec<Symbol> {
    let Some(params_node) = func_def.child_by_field_name("parameters") else {
        return Vec::new();
    };

    let mut cursor = params_node.walk();
    params_node
        .children_by_field_name("parameter", &mut cursor)
        .filter_map(|param_node| parameter_symbol(extractor, param_node, parent_id))
        .collect()
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

fn parameter_symbol(
    extractor: &mut RExtractor,
    param_node: Node,
    parent_id: &str,
) -> Option<Symbol> {
    let name_node = param_node.child_by_field_name("name")?;
    let name =
        clean_r_name(&extractor.base.get_node_text(&name_node)).filter(|name| name != "...")?;
    let signature = extractor.base.get_node_text(&param_node);
    let metadata = HashMap::from([(
        "role".to_string(),
        serde_json::Value::String("parameter".to_string()),
    )]);
    Some(extractor.base.create_symbol(
        &param_node,
        name,
        SymbolKind::Variable,
        SymbolOptions {
            signature: Some(signature),
            parent_id: Some(parent_id.to_string()),
            metadata: Some(metadata),
            ..Default::default()
        },
    ))
}
