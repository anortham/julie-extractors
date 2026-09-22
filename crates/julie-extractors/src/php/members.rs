// PHP Extractor - Property and constant extraction

use super::{
    PhpExtractor, determine_visibility, extract_modifiers, find_child,
    functions::extract_attribute_markers, type_facts,
};
use crate::base::{Symbol, SymbolKind, SymbolOptions, Visibility};
use std::collections::HashMap;
use tree_sitter::Node;

/// Extract PHP property declarations: one symbol per `property_element`
/// (`public $x, $y;` declares two properties), or the promoted parameter.
pub(super) fn extract_property(
    extractor: &mut PhpExtractor,
    node: Node,
    parent_id: Option<&str>,
) -> Vec<Symbol> {
    if node.kind() == "property_promotion_parameter" {
        return promoted_parameter_name_node(extractor, &node)
            .map(|name_node| {
                extract_property_element(extractor, node, name_node, None, true, parent_id)
            })
            .into_iter()
            .collect();
    }
    let mut cursor = node.walk();
    let elements: Vec<Node> = node
        .children(&mut cursor)
        .filter(|child| child.kind() == "property_element")
        .collect();
    let single = elements.len() == 1;
    elements
        .into_iter()
        .filter_map(|element| {
            let name_node = find_child(extractor, &element, "variable_name")?;
            Some(extract_property_element(
                extractor,
                node,
                name_node,
                Some(element),
                single,
                parent_id,
            ))
        })
        .collect()
}

fn extract_property_element(
    extractor: &mut PhpExtractor,
    node: Node,
    name_node: Node,
    property_element: Option<Node>,
    anchor_on_declaration: bool,
    parent_id: Option<&str>,
) -> Symbol {
    let name = extractor.get_base().get_node_text(&name_node);
    let modifiers = extract_modifiers(extractor, &node);
    let annotations = extract_attribute_markers(extractor, &node);
    let type_node = find_type_node(extractor, &node);
    let attribute_list = node
        .child_by_field_name("attributes")
        .or_else(|| find_child(extractor, &node, "attribute_list"));

    let property_value = match property_element.as_ref() {
        Some(property_element) => extract_property_value(extractor, property_element),
        None => node
            .child_by_field_name("default_value")
            .map(|default| extractor.get_base().get_node_text(&default)),
    };

    let mut signature = String::new();
    if let Some(attr_node) = attribute_list {
        signature.push_str(&extractor.get_base().get_node_text(&attr_node));
        signature.push('\n');
    }
    if !modifiers.is_empty() {
        signature.push_str(&format!("{} ", modifiers.join(" ")));
    }
    if let Some(type_node) = type_node {
        signature.push_str(&format!(
            "{} ",
            extractor.get_base().get_node_text(&type_node)
        ));
    }
    signature.push_str(&name);
    if let Some(value) = property_value {
        signature.push_str(&format!(" = {}", value));
    }

    let mut metadata = HashMap::new();
    metadata.insert("type".to_string(), "property".to_string());
    metadata.insert("modifiers".to_string(), modifiers.join(","));
    if let Some(type_node) = type_node {
        metadata.insert(
            "propertyType".to_string(),
            extractor.get_base().get_node_text(&type_node),
        );
    }

    let doc_comment = extractor.get_base().find_doc_comment(&node);
    let resolved_parent_id = resolve_property_parent_id(extractor, parent_id);
    let anchor = match property_element {
        Some(element) if !anchor_on_declaration => element,
        _ => node,
    };
    let symbol = extractor.get_base_mut().create_symbol(
        &anchor,
        name.replace('$', ""),
        SymbolKind::Property,
        SymbolOptions {
            signature: Some(signature),
            visibility: Some(determine_visibility(&modifiers)),
            parent_id: resolved_parent_id,
            metadata: Some(
                metadata
                    .into_iter()
                    .map(|(k, v)| (k, serde_json::Value::String(v)))
                    .collect(),
            ),
            doc_comment,
            annotations,
        },
    );
    if let Some(type_node) = type_node {
        type_facts::record_declared_type(extractor.get_base_mut(), &symbol.id, type_node);
    }
    symbol
}

fn promoted_parameter_name_node<'a>(extractor: &PhpExtractor, node: &Node<'a>) -> Option<Node<'a>> {
    let name_node = node.child_by_field_name("name")?;
    if name_node.kind() == "variable_name" {
        return Some(name_node);
    }
    find_child(extractor, &name_node, "variable_name")
}

fn resolve_property_parent_id(extractor: &PhpExtractor, parent_id: Option<&str>) -> Option<String> {
    let parent_id = parent_id?;
    if let Some(constructor_parent) = extractor.constructor_parent_ids.get(parent_id) {
        return constructor_parent
            .clone()
            .or_else(|| Some(parent_id.to_string()));
    }

    Some(parent_id.to_string())
}

/// Extract PHP constant declarations: one symbol per `const_element`
/// (`const A = 1, B = 2;` declares two constants).
pub(super) fn extract_constant(
    extractor: &mut PhpExtractor,
    node: Node,
    parent_id: Option<&str>,
) -> Vec<Symbol> {
    let mut cursor = node.walk();
    let elements: Vec<Node> = node
        .children(&mut cursor)
        .filter(|child| child.kind() == "const_element")
        .collect();
    let single = elements.len() == 1;
    elements
        .into_iter()
        .filter_map(|element| {
            let anchor = if single { node } else { element };
            extract_const_element(extractor, node, element, anchor, parent_id)
        })
        .collect()
}

fn extract_const_element(
    extractor: &mut PhpExtractor,
    node: Node,
    const_element: Node,
    anchor: Node,
    parent_id: Option<&str>,
) -> Option<Symbol> {
    let name_node = find_child(extractor, &const_element, "name")?;
    let name = extractor.get_base().get_node_text(&name_node);
    let value = extract_property_value(extractor, &const_element);
    let modifiers = extract_modifiers(extractor, &node);
    let visibility = determine_visibility(&modifiers);
    let annotations = extract_attribute_markers(extractor, &node);
    let type_node = node.child_by_field_name("type");

    let mut signature = format!("{} const ", visibility_keyword(&visibility));
    if let Some(type_node) = type_node {
        signature.push_str(&extractor.get_base().get_node_text(&type_node));
        signature.push(' ');
    }
    signature.push_str(&name);
    if let Some(val) = &value {
        signature.push_str(&format!(" = {}", val));
    }

    let mut metadata = HashMap::new();
    metadata.insert("type".to_string(), "constant".to_string());
    if let Some(val) = value {
        metadata.insert("value".to_string(), val);
    }
    if let Some(type_node) = type_node {
        metadata.insert(
            "constantType".to_string(),
            extractor.get_base().get_node_text(&type_node),
        );
    }

    let doc_comment = extractor.get_base().find_doc_comment(&node);
    let symbol = extractor.get_base_mut().create_symbol(
        &anchor,
        name,
        SymbolKind::Constant,
        SymbolOptions {
            signature: Some(signature),
            visibility: Some(visibility),
            parent_id: parent_id.map(|s| s.to_string()),
            metadata: Some(
                metadata
                    .into_iter()
                    .map(|(k, v)| (k, serde_json::Value::String(v)))
                    .collect(),
            ),
            doc_comment,
            annotations,
        },
    );
    if let Some(type_node) = type_node {
        type_facts::record_declared_type(extractor.get_base_mut(), &symbol.id, type_node);
    }
    Some(symbol)
}

/// `define('NAME', value)` declares a global constant at run time.
pub(super) fn extract_define_constant(
    extractor: &mut PhpExtractor,
    node: Node,
    parent_id: Option<&str>,
) -> Option<Symbol> {
    let function = node.child_by_field_name("function")?;
    if !extractor
        .get_base()
        .get_node_text(&function)
        .trim_start_matches('\\')
        .eq_ignore_ascii_case("define")
    {
        return None;
    }
    let arguments = node.child_by_field_name("arguments")?;
    let mut cursor = arguments.walk();
    let values: Vec<Node> = arguments
        .named_children(&mut cursor)
        .filter(|argument| argument.kind() == "argument")
        .filter_map(|argument| argument.named_child(0))
        .collect();
    let name_node = values.first().filter(|value| value.kind() == "string")?;
    let name = extractor
        .get_base()
        .decode_string_literal(name_node)
        .filter(|name| !name.is_empty() && !name.contains(['{', '$']))?;
    let mut metadata = HashMap::from([(
        "type".to_string(),
        serde_json::Value::String("constant".to_string()),
    )]);
    if let Some(value) = values.get(1) {
        metadata.insert(
            "value".to_string(),
            serde_json::Value::String(extractor.get_base().get_node_text(value)),
        );
    }
    let signature = extractor.get_base().get_node_text(&node);
    let doc_comment = extractor.get_base().find_doc_comment(&node);
    Some(extractor.get_base_mut().create_symbol(
        &node,
        name,
        SymbolKind::Constant,
        SymbolOptions {
            signature: Some(signature),
            visibility: Some(Visibility::Public),
            parent_id: parent_id.map(str::to_string),
            metadata: Some(metadata),
            doc_comment,
            annotations: Vec::new(),
        },
    ))
}

fn visibility_keyword(visibility: &Visibility) -> &'static str {
    match visibility {
        Visibility::Public => "public",
        Visibility::Private => "private",
        Visibility::Protected => "protected",
        Visibility::Internal => "internal",
        Visibility::FilePrivate => "fileprivate",
        Visibility::Open => "open",
    }
}

/// The declared type of a property, promoted parameter, or typed constant,
/// including union, intersection, and DNF types.
pub(super) fn find_type_node<'a>(_extractor: &PhpExtractor, node: &Node<'a>) -> Option<Node<'a>> {
    node.child_by_field_name("type")
}

/// Extract property default value
pub(super) fn extract_property_value(
    extractor: &PhpExtractor,
    property_element: &Node,
) -> Option<String> {
    let mut cursor = property_element.walk();
    let mut found_assignment = false;

    for child in property_element.children(&mut cursor) {
        if found_assignment {
            return Some(extractor.get_base().get_node_text(&child));
        }
        if child.kind() == "=" {
            found_assignment = true;
        }
    }
    None
}
