// QML Relationship Extraction
// Extracts relationships between QML symbols: function calls, signal connections, component instantiation

use crate::base::{
    BaseExtractor, ContainingSymbolIndex, Relationship, RelationshipKind, Symbol, SymbolKind,
    UnresolvedTarget,
};
use crate::qml::QmlExtractor;
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};
use std::collections::HashMap;
use tree_sitter::{Node, Tree};

/// Extract all relationships from QML code
pub(super) fn extract_relationships(
    extractor: &mut QmlExtractor,
    tree: &Tree,
    symbols: &[Symbol],
) -> Vec<Relationship> {
    let mut relationships = Vec::new();
    let symbol_map = crate::base::ScopedSymbolIndex::unique_symbol_map(symbols);
    let class_symbols = ContainingSymbolIndex::from_iter(
        symbols
            .iter()
            .filter(|symbol| symbol.kind == SymbolKind::Class),
    );
    let class_owners = symbols
        .iter()
        .filter(|symbol| symbol.kind == SymbolKind::Class)
        .map(|symbol| (symbol.start_byte, symbol))
        .collect::<HashMap<_, _>>();
    let object_owners = symbols
        .iter()
        .filter(|symbol| matches!(symbol.kind, SymbolKind::Class | SymbolKind::Field))
        .map(|symbol| (symbol.start_byte, symbol))
        .collect::<HashMap<_, _>>();
    extract_call_relationships(
        extractor,
        tree.root_node(),
        symbols,
        &symbol_map,
        &class_symbols,
        &mut relationships,
        0,
    );
    extract_instantiation_relationships(
        extractor,
        tree.root_node(),
        symbols,
        &class_symbols,
        &class_owners,
        &mut relationships,
        0,
    );
    extract_property_binding_relationships(
        extractor,
        tree.root_node(),
        symbols,
        &class_symbols,
        &object_owners,
        &mut relationships,
        0,
    );
    relationships
}

fn object_owner<'a>(node: Node, owners: &HashMap<u32, &'a Symbol>) -> Option<&'a Symbol> {
    let anchor = node
        .parent()
        .filter(|parent| parent.kind() == "ui_inline_component")
        .unwrap_or(node);
    owners.get(&(anchor.start_byte() as u32)).copied()
}

fn enclosing_object_owner<'a>(
    node: Node,
    owners: &HashMap<u32, &'a Symbol>,
    skip: usize,
) -> Option<&'a Symbol> {
    let mut current = node;
    let mut seen = 0;
    while let Some(parent) = current.parent() {
        if matches!(
            parent.kind(),
            "ui_object_definition" | "ui_object_definition_binding"
        ) && let Some(owner) = object_owner(parent, owners)
        {
            if seen == skip {
                return Some(owner);
            }
            seen += 1;
        }
        current = parent;
    }
    None
}

/// Extract function call relationships
fn extract_call_relationships(
    extractor: &QmlExtractor,
    node: Node,
    symbols: &[Symbol],
    symbol_map: &HashMap<String, &Symbol>,
    class_symbols: &ContainingSymbolIndex<'_>,
    relationships: &mut Vec<Relationship>,
    depth: u32,
) {
    if !should_visit_tree_depth(depth) {
        return;
    }

    // Match JavaScript call expressions (QML uses TypeScript/JavaScript grammar)
    if node.kind() == "call_expression"
        && let Some(function_node) = node.child_by_field_name("function")
    {
        let (function_name, receiver) = match function_node.kind() {
            "identifier" => (extractor.base.get_node_text(&function_node), None),
            "member_expression" => {
                // For member_expression like object.method(), extract just the method name
                if let Some(property) = function_node.child_by_field_name("property") {
                    (
                        extractor.base.get_node_text(&property),
                        function_node
                            .child_by_field_name("object")
                            .map(|node| extractor.base.get_node_text(&node)),
                    )
                } else {
                    (extractor.base.get_node_text(&function_node), None)
                }
            }
            _ => (extractor.base.get_node_text(&function_node), None),
        };

        // Find the containing function (caller)
        if let Some(caller_symbol) = find_containing_function(node, symbols, class_symbols)
            && let Some(called_symbol) = symbol_map
                .get(function_name.as_str())
                .filter(|s| s.kind == SymbolKind::Function || s.kind == SymbolKind::Event)
            && receiver_can_resolve_locally(
                receiver.as_deref(),
                called_symbol,
                symbols,
                find_containing_component(node, class_symbols),
                caller_symbol,
            )
        {
            let relationship = Relationship {
                id: format!(
                    "{}_{}_{:?}_{}",
                    caller_symbol.id,
                    called_symbol.id,
                    RelationshipKind::Calls,
                    node.start_position().row
                ),
                from_symbol_id: caller_symbol.id.clone(),
                to_symbol_id: called_symbol.id.clone(),
                kind: RelationshipKind::Calls,
                file_path: extractor.base.file_path.clone(),
                line_number: (node.start_position().row + 1) as u32,
                span: Some(crate::base::NormalizedSpan::from_node(&node)),
                reference_site_is_exact: false,
                confidence: 1.0,
                metadata: None,
            };
            relationships.push(relationship);
        }
    }

    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        extract_call_relationships(
            extractor,
            child,
            symbols,
            symbol_map,
            class_symbols,
            relationships,
            child_depth,
        );
    }
}

/// A receiver resolves when its id and target are visible from the caller's
/// component scope and no JavaScript local shadows the id.
pub(super) fn receiver_can_resolve_locally(
    receiver: Option<&str>,
    called_symbol: &Symbol,
    symbols: &[Symbol],
    caller_component: Option<&Symbol>,
    caller_symbol: &Symbol,
) -> bool {
    let Some(component) = caller_component else {
        return false;
    };
    if !symbol_is_visible_from_component(called_symbol, component, symbols) {
        return false;
    }
    let Some(receiver) = receiver else {
        return true;
    };
    if symbols.iter().any(|symbol| {
        symbol.kind == SymbolKind::Variable
            && symbol.name == receiver
            && symbol.parent_id.as_deref() == Some(caller_symbol.id.as_str())
    }) {
        return false;
    }

    symbols
        .iter()
        .filter(|symbol| declares_id(symbol, receiver))
        .any(|symbol| {
            let scope = id_member_scope(symbol);
            scope.is_some()
                && scope == called_symbol.parent_id.as_deref()
                && symbol_is_visible_from_component(symbol, component, symbols)
        })
}

fn symbol_is_visible_from_component(
    symbol: &Symbol,
    component: &Symbol,
    symbols: &[Symbol],
) -> bool {
    let Some(symbol_component) = containing_component(symbol, symbols) else {
        return false;
    };
    let mut current = Some(component);
    while let Some(symbol) = current {
        if symbol.id == symbol_component.id {
            return true;
        }
        current = symbol
            .parent_id
            .as_deref()
            .and_then(|parent_id| symbols.iter().find(|candidate| candidate.id == parent_id));
    }
    false
}

fn containing_component<'a>(symbol: &Symbol, symbols: &'a [Symbol]) -> Option<&'a Symbol> {
    let mut current = symbol
        .parent_id
        .as_deref()
        .and_then(|parent_id| symbols.iter().find(|candidate| candidate.id == parent_id));
    while let Some(candidate) = current {
        if candidate.kind == SymbolKind::Class {
            return Some(candidate);
        }
        current = candidate
            .parent_id
            .as_deref()
            .and_then(|parent_id| symbols.iter().find(|ancestor| ancestor.id == parent_id));
    }
    None
}

/// An id is either the class row's `id:` property or the object row that the id
/// itself names.
fn declares_id(symbol: &Symbol, receiver: &str) -> bool {
    if symbol.name != receiver {
        return false;
    }
    let Some(signature) = symbol.signature.as_deref() else {
        return false;
    };
    match symbol.kind {
        SymbolKind::Property => signature.starts_with("id:"),
        SymbolKind::Field => signature.starts_with(&format!("{receiver}: ")),
        _ => false,
    }
}

fn id_member_scope(symbol: &Symbol) -> Option<&str> {
    match symbol.kind {
        SymbolKind::Field => Some(symbol.id.as_str()),
        _ => symbol.parent_id.as_deref(),
    }
}

/// Extract one relationship for each QML object use.
fn extract_instantiation_relationships(
    extractor: &mut QmlExtractor,
    node: Node,
    symbols: &[Symbol],
    class_symbols: &ContainingSymbolIndex<'_>,
    class_owners: &HashMap<u32, &Symbol>,
    relationships: &mut Vec<Relationship>,
    depth: u32,
) {
    if super::is_typeinfo_path(&extractor.base.file_path) {
        return;
    }
    if !should_visit_tree_depth(depth) {
        return;
    }

    if node.kind() == "ui_object_definition"
        && !super::semantics::is_grouped_property_block(&extractor.base, node)
        && let Some(type_name_node) = node.child_by_field_name("type_name")
    {
        let component_type = extractor
            .base
            .get_node_text(&type_name_node)
            .trim()
            .to_string();
        // The root object names the base type the file's component extends;
        // every other object instantiates its type.
        let (kind, from_symbol) = if super::semantics::object_has_class_row(node) {
            (
                RelationshipKind::Extends,
                declaring_class_symbol(node, class_owners),
            )
        } else {
            (
                RelationshipKind::Instantiates,
                find_containing_component(node, class_symbols),
            )
        };
        if let Some(parent_symbol) = from_symbol {
            if let Some(instantiated_symbol) =
                find_local_component_target(&component_type, parent_symbol, symbols)
            {
                relationships.push(extractor.base.create_relationship(
                    parent_symbol.id.clone(),
                    instantiated_symbol.id.clone(),
                    kind,
                    &node,
                    Some(1.0),
                    None,
                ));
            } else {
                let target = qml_component_target(&component_type, symbols);
                let pending = extractor.base.create_pending_relationship(
                    parent_symbol.id.clone(),
                    target,
                    kind,
                    &node,
                    Some(parent_symbol.id.clone()),
                    Some(0.9),
                );
                extractor.add_structured_pending_relationship(pending);
            }
        }
    }

    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        extract_instantiation_relationships(
            extractor,
            child,
            symbols,
            class_symbols,
            class_owners,
            relationships,
            child_depth,
        );
    }
}

/// The `Class` row an object declares: the file root's own row, or the row of
/// the `ui_inline_component` header above an inline component's body.
fn declaring_class_symbol<'a>(
    node: Node,
    class_owners: &HashMap<u32, &'a Symbol>,
) -> Option<&'a Symbol> {
    object_owner(node, class_owners)
}

/// A qualified name (`QQC2.Button`) names an imported module's type, never a
/// same-file class, so it goes straight to the pending path with its receiver.
fn find_local_component_target<'a>(
    component_type: &str,
    parent_symbol: &Symbol,
    symbols: &'a [Symbol],
) -> Option<&'a Symbol> {
    if component_type.contains('.') {
        return None;
    }
    let mut candidates = symbols.iter().filter(|symbol| {
        symbol.kind == SymbolKind::Class
            && symbol.id != parent_symbol.id
            && symbol.file_path == parent_symbol.file_path
            && symbol.name == component_type
    });
    let candidate = candidates.next()?;
    candidates.next().is_none().then_some(candidate)
}

fn qml_component_target(component_type: &str, symbols: &[Symbol]) -> UnresolvedTarget {
    let mut segments = component_type.split('.');
    let terminal_name = segments.next_back().unwrap_or(component_type).to_string();
    let receiver = if component_type.contains('.') {
        Some(segments.collect::<Vec<_>>().join("."))
    } else {
        None
    };
    let import_context = qml_import_context(component_type, receiver.as_deref(), symbols);
    UnresolvedTarget {
        display_name: component_type.to_string(),
        terminal_name,
        receiver,
        namespace_path: Vec::new(),
        import_context,
    }
}

fn qml_import_context(
    component_type: &str,
    receiver: Option<&str>,
    symbols: &[Symbol],
) -> Option<String> {
    let prefix = receiver.unwrap_or(component_type);
    symbols
        .iter()
        .filter(|symbol| {
            let import_kind = symbol
                .metadata
                .as_ref()
                .and_then(|metadata| metadata_string(metadata, "import_kind"));
            symbol.kind == SymbolKind::Import && import_kind.as_deref() != Some("javascript")
        })
        .find_map(|import| {
            let metadata = import.metadata.as_ref()?;
            let source = metadata_string(metadata, "source")
                .or_else(|| metadata_string(metadata, "imported_name"))
                .or_else(|| (!import.name.is_empty()).then(|| import.name.clone()))?;
            let alias = metadata_string(metadata, "local_name")
                .or_else(|| metadata_string(metadata, "alias"));
            let imported_name = metadata_string(metadata, "imported_name");
            (alias.as_deref() == Some(prefix) || imported_name.as_deref() == Some(component_type))
                .then_some(source)
        })
}

fn metadata_string(metadata: &HashMap<String, serde_json::Value>, key: &str) -> Option<String> {
    metadata
        .get(key)
        .and_then(|value| value.as_str())
        .map(str::to_string)
}

/// Extract property binding relationships (width: parent.width, etc.)
fn extract_property_binding_relationships(
    extractor: &QmlExtractor,
    node: Node,
    symbols: &[Symbol],
    class_symbols: &ContainingSymbolIndex<'_>,
    object_owners: &HashMap<u32, &Symbol>,
    relationships: &mut Vec<Relationship>,
    depth: u32,
) {
    if !should_visit_tree_depth(depth) {
        return;
    }

    // Look for member expressions anywhere (they represent property access)
    if node.kind() == "member_expression"
        && let Some(property_node) = node.child_by_field_name("property")
        && let Some(container_symbol) = find_containing_component(node, class_symbols)
        && let Some(scope_symbol) =
            property_binding_scope(extractor, node, symbols, object_owners, container_symbol)
    {
        let property_name = extractor.base.get_node_text(&property_node);

        if let Some(target_symbol) = find_property_target(&property_name, scope_symbol, symbols) {
            // Create a Uses relationship for the property access
            let relationship = Relationship {
                id: format!(
                    "{}_{:?}_{}_{}",
                    container_symbol.id,
                    RelationshipKind::Uses,
                    property_name,
                    node.start_position().row
                ),
                from_symbol_id: container_symbol.id.clone(),
                to_symbol_id: target_symbol.id.clone(),
                kind: RelationshipKind::Uses,
                file_path: extractor.base.file_path.clone(),
                line_number: (node.start_position().row + 1) as u32,
                span: Some(crate::base::NormalizedSpan::from_node(&node)),
                reference_site_is_exact: false,
                confidence: 0.8,
                metadata: None,
            };
            relationships.push(relationship);
        }
    }

    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        extract_property_binding_relationships(
            extractor,
            child,
            symbols,
            class_symbols,
            object_owners,
            relationships,
            child_depth,
        );
    }
}

/// The object scope an explicit receiver names. An id declared in this file
/// resolves in the object it names, `parent` in the scope around the enclosing
/// object, and any other identifier receiver — a singleton, a JavaScript local —
/// resolves nothing. Anything that is not a plain identifier keeps the
/// enclosing-scope rule.
fn property_binding_scope<'a>(
    extractor: &QmlExtractor,
    node: Node,
    symbols: &'a [Symbol],
    object_owners: &HashMap<u32, &'a Symbol>,
    container_symbol: &'a Symbol,
) -> Option<&'a Symbol> {
    let Some(object) = node
        .child_by_field_name("object")
        .filter(|object| object.kind() == "identifier")
    else {
        return Some(enclosing_object_owner(node, object_owners, 0).unwrap_or(container_symbol));
    };
    let receiver = extractor.base.get_node_text(&object);
    if let Some(scope) = id_scope_symbol(&receiver, symbols) {
        return Some(scope);
    }
    if receiver == "parent" {
        return enclosing_object_owner(node, object_owners, 1);
    }
    None
}

/// The row that owns the members of the object an id names.
fn id_scope_symbol<'a>(receiver: &str, symbols: &'a [Symbol]) -> Option<&'a Symbol> {
    let scope_id = symbols
        .iter()
        .filter(|symbol| declares_id(symbol, receiver))
        .find_map(id_member_scope)?;
    symbols.iter().find(|symbol| symbol.id == scope_id)
}

fn find_property_target<'a>(
    property_name: &str,
    container_symbol: &Symbol,
    symbols: &'a [Symbol],
) -> Option<&'a Symbol> {
    symbols
        .iter()
        .find(|symbol| {
            symbol.kind == SymbolKind::Property
                && symbol.name == property_name
                && symbol.parent_id.as_deref() == Some(container_symbol.id.as_str())
        })
        .or_else(|| {
            let matches = symbols
                .iter()
                .filter(|symbol| {
                    symbol.kind == SymbolKind::Property
                        && symbol.name == property_name
                        && symbol.file_path == container_symbol.file_path
                })
                .collect::<Vec<_>>();
            if matches.len() == 1 {
                Some(matches[0])
            } else {
                None
            }
        })
}

/// Find the containing function for a node
/// Also checks for QML signal handlers (ui_script_binding, ui_binding with function bodies)
pub(super) fn find_containing_function<'a>(
    node: Node,
    symbols: &'a [Symbol],
    class_symbols: &ContainingSymbolIndex<'a>,
) -> Option<&'a Symbol> {
    let mut current = node;
    while let Some(parent) = current.parent() {
        match parent.kind() {
            "function_declaration" => {
                if let Some(symbol) = symbols.iter().find(|symbol| {
                    symbol.kind == SymbolKind::Function
                        && symbol.start_byte == parent.start_byte() as u32
                }) {
                    return Some(symbol);
                }
            }
            "ui_script_binding" | "ui_binding" => {
                // Signal handlers like onClicked: { ... } are represented as ui_script_binding
                // Find the containing component instead
                return find_containing_component(parent, class_symbols);
            }
            _ => {}
        }
        current = parent;
    }
    None
}

/// Find the containing QML component for a node
pub(super) fn find_containing_component<'a>(
    node: Node,
    class_symbols: &ContainingSymbolIndex<'a>,
) -> Option<&'a Symbol> {
    find_enclosing_symbol(node, class_symbols)
}

/// Walk out to the nearest enclosing QML object with an indexed symbol row.
fn find_enclosing_symbol<'a>(
    node: Node,
    symbols: &ContainingSymbolIndex<'a>,
) -> Option<&'a Symbol> {
    let mut current = node;
    while let Some(parent) = current.parent() {
        if matches!(
            parent.kind(),
            "ui_object_definition" | "ui_object_definition_binding"
        ) && let Some(symbol) = symbols.find(parent)
        {
            return Some(symbol);
        }
        current = parent;
    }
    None
}

pub(super) fn call_receiver_type(base: &BaseExtractor, function_node: Node) -> Option<String> {
    if function_node.kind() != "member_expression" {
        return None;
    }
    let object = function_node.child_by_field_name("object")?;
    let (type_name, object_id) = enclosing_object_type_and_id(base, function_node)?;
    match object.kind() {
        "this" => Some(type_name),
        "identifier" if object_id.as_deref() == Some(base.get_node_text(&object).as_str()) => {
            Some(type_name)
        }
        _ => None,
    }
}

fn enclosing_object_type_and_id(
    base: &BaseExtractor,
    node: Node,
) -> Option<(String, Option<String>)> {
    let mut current = node.parent();
    while let Some(candidate) = current {
        if candidate.kind() == "ui_object_definition" {
            let type_name = object_type_name(base, candidate)?;
            let object_id = object_id_binding(base, candidate);
            return Some((type_name, object_id));
        }
        current = candidate.parent();
    }
    None
}

fn object_type_name(base: &BaseExtractor, object_node: Node) -> Option<String> {
    if is_root_object(object_node) {
        return std::path::Path::new(&base.file_path)
            .file_stem()
            .and_then(|s| s.to_str())
            .map(|s| s.to_string());
    }
    object_node
        .child_by_field_name("type_name")
        .map(|name_node| base.get_node_text(&name_node))
}

fn is_root_object(object_node: Node) -> bool {
    super::semantics::enclosing_object(object_node).is_none()
}

pub(super) fn object_id_binding(base: &BaseExtractor, object_node: Node) -> Option<String> {
    let initializer = object_node.child_by_field_name("initializer")?;
    let mut cursor = initializer.walk();
    for child in initializer.named_children(&mut cursor) {
        if child.kind() != "ui_binding" {
            continue;
        }
        let Some(name_node) = child.child_by_field_name("name") else {
            continue;
        };
        if base.get_node_text(&name_node) != "id" {
            continue;
        }
        return id_binding_value(base, child);
    }
    None
}

fn id_binding_value(base: &BaseExtractor, binding_node: Node) -> Option<String> {
    let value_node = binding_node.child_by_field_name("value")?;
    if value_node.kind() == "expression_statement" {
        return value_node
            .named_child(0)
            .map(|inner| base.get_node_text(&inner));
    }
    Some(base.get_node_text(&value_node))
}
