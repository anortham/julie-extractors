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
    } else if node.kind() == "ui_object_definition"
        && let Some(symbol) = extract_object(extractor, &node, parent_id.clone())
    {
        let child_parent = Some(symbol.id.clone());
        let members = match symbol.kind {
            SymbolKind::Enum => enum_members(extractor, &node, symbol.id.clone()),
            SymbolKind::Class => export_symbols(extractor, &node, symbol.id.clone()),
            _ => Vec::new(),
        };
        extractor.symbols.push(symbol);
        extractor.symbols.extend(members);
        walk_children(extractor, node, child_parent, depth);
        return;
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

    let type_key = match role {
        TypeInfoRole::Property | TypeInfoRole::Parameter => Some("type"),
        TypeInfoRole::Method if metadata.contains_key("returnType") => Some("returnType"),
        TypeInfoRole::Method => Some("type"),
        _ => None,
    };
    let declared_type = type_key
        .and_then(|key| metadata.get(key))
        .and_then(Value::as_str)
        .map(str::to_owned);
    let options = SymbolOptions {
        parent_id,
        visibility: Some(Visibility::Public),
        metadata: Some(metadata),
        ..Default::default()
    };
    let symbol = extractor.base.create_symbol(node, name, kind, options);
    if let Some(declared_type) = declared_type {
        super::type_facts::record_named_type(&mut extractor.base, &symbol.id, &declared_type);
    }
    Some(symbol)
}

/// One `export` row per QML name a component exports:
/// `"org.kde.plasma.core/Svg 2.0"` exports `Svg` from `org.kde.plasma.core`.
fn export_symbols(extractor: &mut QmlExtractor, node: &Node, parent_id: String) -> Vec<Symbol> {
    let Some(exports_node) = direct_binding_value(extractor, node, "exports") else {
        return Vec::new();
    };
    let mut exports: Vec<(String, String, Vec<Value>)> = Vec::new();
    for entry in string_values(&extractor.base.get_node_text(&exports_node)) {
        let (path, version) = entry.split_once(' ').unwrap_or((entry.as_str(), ""));
        let Some((module, name)) = path.rsplit_once('/') else {
            continue;
        };
        match exports
            .iter_mut()
            .find(|(existing_module, existing, _)| existing == name && existing_module == module)
        {
            Some((_, _, versions)) => versions.push(Value::String(version.to_string())),
            None => exports.push((
                module.to_string(),
                name.to_string(),
                vec![Value::String(version.to_string())],
            )),
        }
    }
    exports
        .into_iter()
        .map(|(module, name, versions)| {
            let metadata = HashMap::from([
                (
                    "typeinfo_kind".to_string(),
                    Value::String("export".to_string()),
                ),
                ("module".to_string(), Value::String(module.clone())),
                ("versions".to_string(), Value::Array(versions)),
            ]);
            extractor.base.create_symbol(
                &exports_node,
                name,
                SymbolKind::Export,
                SymbolOptions {
                    parent_id: Some(parent_id.clone()),
                    signature: Some(format!("export {module}")),
                    visibility: Some(Visibility::Public),
                    metadata: Some(metadata),
                    ..Default::default()
                },
            )
        })
        .collect()
}

/// A pending `extends` edge from each typeinfo `Component` to its `prototype`.
pub(super) fn extract_prototype_relationships(
    extractor: &mut QmlExtractor,
    node: Node,
    symbols: &[Symbol],
    depth: u32,
) {
    if !should_visit_tree_depth(depth) {
        return;
    }
    if node.kind() == "ui_object_definition"
        && let Some(prototype) = direct_binding_value(extractor, &node, "prototype")
        && let Some(component) = symbols.iter().find(|symbol| {
            symbol.kind == SymbolKind::Class && symbol.start_byte == node.start_byte() as u32
        })
    {
        let target = normalize_string(&extractor.base.get_node_text(&prototype));
        if !target.is_empty() {
            let pending = extractor.base.create_pending_relationship(
                component.id.clone(),
                crate::base::UnresolvedTarget::simple(target),
                crate::base::RelationshipKind::Extends,
                &prototype,
                Some(component.id.clone()),
                Some(0.9),
            );
            extractor.add_structured_pending_relationship(pending);
        }
    }
    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        extract_prototype_relationships(extractor, child, symbols, child_depth);
    }
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
        let value_text = extractor
            .base
            .get_node_text(&value_node)
            .trim()
            .trim_end_matches(';')
            .trim_end()
            .to_string();
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
    let mut members = Vec::new();
    for (name, value) in enum_values(&values_text) {
        if name.is_empty() {
            continue;
        }
        let mut metadata = HashMap::new();
        metadata.insert(
            "typeinfo_kind".to_string(),
            Value::String("enum_value".to_string()),
        );
        metadata.insert("value".to_string(), value);
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

/// Enum values in either qmltypes form: the Qt 6 array of names (each value is
/// its name) or the Qt 5 object of name to number.
fn enum_values(text: &str) -> Vec<(String, Value)> {
    if let Ok(Value::Object(entries)) = serde_json::from_str::<Value>(text.trim()) {
        return entries.into_iter().collect();
    }
    string_values(text)
        .into_iter()
        .map(|name| (name.clone(), Value::String(name)))
        .collect()
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
