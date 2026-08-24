use super::QmlExtractor;
use crate::base::{Symbol, SymbolKind, SymbolOptions, Visibility};
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};
use serde_json::Value;
use std::collections::HashMap;
use std::path::Path;
use tree_sitter::Node;

pub(crate) fn is_typeinfo_path(file_path: &str) -> bool {
    Path::new(file_path)
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("qmltypes"))
}

pub(super) fn extract(extractor: &mut QmlExtractor, root: Node) {
    walk(extractor, root, None, 0);
}

fn walk(extractor: &mut QmlExtractor, node: Node, parent_id: Option<String>, depth: u32) {
    if !should_visit_tree_depth(depth) {
        return;
    }

    if node.kind() == "ui_import" {
        if let Some(symbol) = super::imports::extract(extractor, &node, parent_id.clone()) {
            extractor.symbols.push(symbol);
        }
    } else if node.kind() == "ui_object_definition" {
        if let Some(symbol) = extract_object(extractor, &node, parent_id.clone()) {
            let child_parent = Some(symbol.id.clone());
            let enum_members = if symbol.kind == SymbolKind::Enum {
                enum_members(extractor, &node, symbol.id.clone())
            } else {
                Vec::new()
            };
            extractor.symbols.push(symbol);
            extractor.symbols.extend(enum_members);
            walk_children(extractor, node, child_parent, depth);
            return;
        }
    }

    walk_children(extractor, node, parent_id, depth);
}

fn walk_children(extractor: &mut QmlExtractor, node: Node, parent_id: Option<String>, depth: u32) {
    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk(extractor, child, parent_id.clone(), child_depth);
    }
}

fn extract_object(
    extractor: &mut QmlExtractor,
    node: &Node,
    parent_id: Option<String>,
) -> Option<Symbol> {
    let type_name_node = node.child_by_field_name("type_name")?;
    let type_name = extractor.base.get_node_text(&type_name_node);
    let bindings = direct_bindings(extractor, node);
    let role = symbol_role(&type_name, parent_id.is_some());
    let (name, kind) = match role {
        TypeInfoRole::Module => (type_name.clone(), SymbolKind::Module),
        TypeInfoRole::Type | TypeInfoRole::AttachedType | TypeInfoRole::Extension => (
            binding_string(&bindings, "name").unwrap_or_else(|| type_name.clone()),
            SymbolKind::Class,
        ),
        TypeInfoRole::Property => (binding_string(&bindings, "name")?, SymbolKind::Property),
        TypeInfoRole::Signal => (binding_string(&bindings, "name")?, SymbolKind::Event),
        TypeInfoRole::Method => (binding_string(&bindings, "name")?, SymbolKind::Method),
        TypeInfoRole::Parameter => (binding_string(&bindings, "name")?, SymbolKind::Variable),
        TypeInfoRole::Enum => (binding_string(&bindings, "name")?, SymbolKind::Enum),
        TypeInfoRole::EnumValue => (binding_string(&bindings, "name")?, SymbolKind::EnumMember),
        TypeInfoRole::Unknown => return None,
    };

    let mut metadata = HashMap::new();
    metadata.insert(
        "typeinfo_kind".to_string(),
        Value::String(role.metadata_name().to_string()),
    );
    for (key, value) in bindings {
        if key != "name" {
            metadata.insert(key, value);
        }
    }

    let options = SymbolOptions {
        parent_id,
        visibility: Some(Visibility::Public),
        metadata: Some(metadata),
        ..Default::default()
    };
    Some(extractor.base.create_symbol(node, name, kind, options))
}

fn direct_bindings(extractor: &QmlExtractor, node: &Node) -> HashMap<String, Value> {
    let mut values = HashMap::new();
    let Some(initializer) = node.child_by_field_name("initializer") else {
        return values;
    };
    let mut cursor = initializer.walk();
    for child in initializer.children(&mut cursor) {
        if child.kind() != "ui_binding" {
            continue;
        }
        let Some(name_node) = child.child_by_field_name("name") else {
            continue;
        };
        let name = extractor.base.get_node_text(&name_node);
        let Some(value_node) = child.child_by_field_name("value") else {
            continue;
        };
        let value_text = extractor.base.get_node_text(&value_node).trim().to_string();
        if value_text.is_empty() {
            continue;
        }
        let value = if let Ok(value) = serde_json::from_str::<Value>(&value_text) {
            value
        } else {
            Value::String(normalize_string(&value_text))
        };
        values.insert(name, value);
    }
    values
}

fn enum_members(extractor: &mut QmlExtractor, node: &Node, parent_id: String) -> Vec<Symbol> {
    let Some(values_node) = direct_binding_value(extractor, node, "values") else {
        return Vec::new();
    };
    let values_text = extractor.base.get_node_text(&values_node);
    let names = string_values(&values_text);
    let mut members = Vec::new();
    for name in names {
        if name.is_empty() {
            continue;
        }
        let mut metadata = HashMap::new();
        metadata.insert(
            "typeinfo_kind".to_string(),
            Value::String("enum_value".to_string()),
        );
        metadata.insert("value".to_string(), Value::String(name.clone()));
        members.push(extractor.base.create_symbol(
            &values_node,
            name,
            SymbolKind::EnumMember,
            SymbolOptions {
                parent_id: Some(parent_id.clone()),
                visibility: Some(Visibility::Public),
                metadata: Some(metadata),
                ..Default::default()
            },
        ));
    }
    members
}

fn string_values(text: &str) -> Vec<String> {
    let text = text.trim();
    if let Ok(Value::Array(values)) = serde_json::from_str::<Value>(text) {
        return values
            .into_iter()
            .filter_map(|value| value.as_str().map(str::to_owned))
            .collect();
    }
    let Some(inner) = text
        .strip_prefix('[')
        .and_then(|value| value.strip_suffix(']'))
    else {
        return Vec::new();
    };
    inner
        .split(',')
        .map(normalize_string)
        .filter(|value| !value.is_empty())
        .collect()
}

fn direct_binding_value<'tree>(
    extractor: &QmlExtractor,
    node: &Node<'tree>,
    key: &str,
) -> Option<Node<'tree>> {
    let initializer = node.child_by_field_name("initializer")?;
    let mut cursor = initializer.walk();
    for child in initializer.children(&mut cursor) {
        if child.kind() != "ui_binding" {
            continue;
        }
        let Some(name_node) = child.child_by_field_name("name") else {
            continue;
        };
        if extractor.base.get_node_text(&name_node) == key {
            return child.child_by_field_name("value");
        }
    }
    None
}

fn normalize_string(value: &str) -> String {
    let value = value.trim();
    if value.len() >= 2
        && ((value.starts_with('"') && value.ends_with('"'))
            || (value.starts_with('\'') && value.ends_with('\'')))
    {
        value[1..value.len() - 1].to_string()
    } else {
        value.to_string()
    }
}

fn binding_string(bindings: &HashMap<String, Value>, key: &str) -> Option<String> {
    bindings
        .get(key)
        .and_then(|value| value.as_str())
        .map(str::to_owned)
}

#[derive(Clone, Copy)]
enum TypeInfoRole {
    Module,
    Type,
    AttachedType,
    Extension,
    Property,
    Signal,
    Method,
    Parameter,
    Enum,
    EnumValue,
    Unknown,
}

impl TypeInfoRole {
    fn metadata_name(self) -> &'static str {
        match self {
            Self::Module => "module",
            Self::Type => "type",
            Self::AttachedType => "attached_type",
            Self::Extension => "extension",
            Self::Property => "property",
            Self::Signal => "signal",
            Self::Method => "method",
            Self::Parameter => "parameter",
            Self::Enum => "enum",
            Self::EnumValue => "enum_value",
            Self::Unknown => "unknown",
        }
    }
}

fn symbol_role(type_name: &str, nested: bool) -> TypeInfoRole {
    match type_name {
        "Module" => TypeInfoRole::Module,
        "Component" => TypeInfoRole::Type,
        "AttachedType" => TypeInfoRole::AttachedType,
        "Extension" => TypeInfoRole::Extension,
        "Property" => TypeInfoRole::Property,
        "Signal" => TypeInfoRole::Signal,
        "Method" => TypeInfoRole::Method,
        "Parameter" => TypeInfoRole::Parameter,
        "Enum" => TypeInfoRole::Enum,
        "EnumValue" => TypeInfoRole::EnumValue,
        _ if nested => TypeInfoRole::Unknown,
        _ => TypeInfoRole::Unknown,
    }
}
