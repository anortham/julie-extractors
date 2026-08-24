// QML Relationship Extraction
// Extracts relationships between QML symbols: function calls, signal connections, component instantiation

use crate::base::{Relationship, RelationshipKind, Symbol, SymbolKind, UnresolvedTarget};
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
    extract_call_relationships(
        extractor,
        tree.root_node(),
        symbols,
        &symbol_map,
        &mut relationships,
        0,
    );
    extract_instantiation_relationships(
        extractor,
        tree.root_node(),
        symbols,
        &mut relationships,
        0,
    );
    extract_property_binding_relationships(
        extractor,
        tree.root_node(),
        symbols,
        &mut relationships,
        0,
    );
    relationships
}

/// Extract function call relationships
fn extract_call_relationships(
    extractor: &QmlExtractor,
    node: Node,
    symbols: &[Symbol],
    symbol_map: &HashMap<String, &Symbol>,
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
        if let Some(caller_symbol) = find_containing_function(node, symbols)
            && let Some(called_symbol) = symbol_map
                .get(function_name.as_str())
                .filter(|s| s.kind == SymbolKind::Function || s.kind == SymbolKind::Event)
            && receiver_can_resolve_locally(
                receiver.as_deref(),
                caller_symbol,
                called_symbol,
                symbols,
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
            relationships,
            child_depth,
        );
    }
}

fn receiver_can_resolve_locally(
    receiver: Option<&str>,
    caller_symbol: &Symbol,
    called_symbol: &Symbol,
    symbols: &[Symbol],
) -> bool {
    let Some(receiver) = receiver else {
        return true;
    };

    let Some(receiver_symbol) = symbols.iter().find(|symbol| {
        symbol.name == receiver
            && symbol.kind == SymbolKind::Property
            && symbol
                .signature
                .as_deref()
                .is_some_and(|signature| signature.starts_with("id:"))
    }) else {
        return false;
    };

    let receiver_parent_id = receiver_symbol.parent_id.as_deref();
    let caller_scope_id = if caller_symbol.kind == SymbolKind::Class {
        Some(caller_symbol.id.as_str())
    } else {
        caller_symbol.parent_id.as_deref()
    };

    receiver_parent_id.is_some()
        && receiver_parent_id == caller_scope_id
        && called_symbol.parent_id.as_deref() == receiver_parent_id
}

/// Extract one relationship for each QML object use.
fn extract_instantiation_relationships(
    extractor: &mut QmlExtractor,
    node: Node,
    symbols: &[Symbol],
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
        && let Some(type_name_node) = node.child_by_field_name("type_name")
    {
        let component_type = extractor
            .base
            .get_node_text(&type_name_node)
            .trim()
            .to_string();
        if let Some(parent_symbol) = find_containing_component(node, symbols) {
            if let Some(instantiated_symbol) =
                find_local_component_target(&component_type, parent_symbol, symbols)
            {
                relationships.push(extractor.base.create_relationship(
                    parent_symbol.id.clone(),
                    instantiated_symbol.id.clone(),
                    RelationshipKind::Instantiates,
                    &node,
                    Some(1.0),
                    None,
                ));
            } else {
                let target = qml_component_target(&component_type, symbols);
                let pending = extractor.base.create_pending_relationship(
                    parent_symbol.id.clone(),
                    target,
                    RelationshipKind::Instantiates,
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
        extract_instantiation_relationships(extractor, child, symbols, relationships, child_depth);
    }
}

fn find_local_component_target<'a>(
    component_type: &str,
    parent_symbol: &Symbol,
    symbols: &'a [Symbol],
) -> Option<&'a Symbol> {
    let target_name = component_type.rsplit('.').next().unwrap_or(component_type);
    let mut candidates = symbols.iter().filter(|symbol| {
        symbol.kind == SymbolKind::Class
            && symbol.id != parent_symbol.id
            && symbol.file_path == parent_symbol.file_path
            && symbol.name == target_name
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
    relationships: &mut Vec<Relationship>,
    depth: u32,
) {
    if !should_visit_tree_depth(depth) {
        return;
    }

    // Look for member expressions anywhere (they represent property access)
    if node.kind() == "member_expression"
        && let Some(property_node) = node.child_by_field_name("property")
    {
        let property_name = extractor.base.get_node_text(&property_node);

        // Find containing component
        if let Some(container_symbol) = find_containing_component(node, symbols) {
            let Some(target_symbol) =
                find_property_target(&property_name, container_symbol, symbols)
            else {
                return;
            };

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
            relationships,
            child_depth,
        );
    }
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
fn find_containing_function<'a>(node: Node, symbols: &'a [Symbol]) -> Option<&'a Symbol> {
    let mut current = node;
    while let Some(parent) = current.parent() {
        match parent.kind() {
            "function_declaration" => {
                // Find the symbol that matches this function
                let func_line = parent.start_position().row + 1;
                if let Some(symbol) = symbols
                    .iter()
                    .find(|s| s.kind == SymbolKind::Function && s.start_line == func_line as u32)
                {
                    return Some(symbol);
                }
            }
            "ui_script_binding" | "ui_binding" => {
                // Signal handlers like onClicked: { ... } are represented as ui_script_binding
                // Find the containing component instead
                return find_containing_component(parent, symbols);
            }
            _ => {}
        }
        current = parent;
    }
    None
}

/// Find the containing QML component for a node
fn find_containing_component<'a>(node: Node, symbols: &'a [Symbol]) -> Option<&'a Symbol> {
    let mut current = node;
    while let Some(parent) = current.parent() {
        if parent.kind() == "ui_object_definition" {
            // Find the symbol that matches this component
            let comp_line = parent.start_position().row + 1;
            if let Some(symbol) = symbols
                .iter()
                .find(|s| s.kind == SymbolKind::Class && s.start_line == comp_line as u32)
            {
                return Some(symbol);
            }
        }
        current = parent;
    }
    None
}
