// PHP Extractor - Relationship extraction (inheritance, implementation, function calls)

use super::{PhpExtractor, find_child};
use crate::base::{Relationship, RelationshipKind, Symbol, SymbolKind, UnresolvedTarget};
use std::collections::HashMap;
use tree_sitter::Node;

/// Strip PHP namespace prefix from a qualified name, returning the last component.
/// e.g. `\App\Http\Controller` -> `Controller`, `Controller` -> `Controller`
pub(super) fn strip_php_namespace(name: &str) -> &str {
    name.trim_start_matches('\\')
        .rsplit('\\')
        .next()
        .unwrap_or(name)
}

pub(super) fn unresolved_php_type_target(raw_name: &str) -> UnresolvedTarget {
    let parts: Vec<String> = raw_name
        .trim()
        .trim_start_matches('\\')
        .split('\\')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .map(ToString::to_string)
        .collect();

    let Some(terminal_name) = parts.last().cloned() else {
        return UnresolvedTarget::simple(raw_name.trim().to_string());
    };

    UnresolvedTarget {
        display_name: parts.join("\\"),
        terminal_name,
        receiver: None,
        namespace_path: parts[..parts.len().saturating_sub(1)].to_vec(),
        import_context: None,
    }
}

/// A trait `use` in a class, trait, or enum body gives a `uses` relationship
/// to each named trait: resolved to a same-file trait, pending otherwise.
pub(crate) fn extract_trait_use_relationships(
    extractor: &mut PhpExtractor,
    node: Node,
    symbols: &[Symbol],
    relationships: &mut Vec<Relationship>,
) {
    let Some(owner_node) = node
        .parent()
        .filter(|parent| parent.kind() == "declaration_list")
        .and_then(|list| list.parent())
    else {
        return;
    };
    let owner_start = owner_node.start_byte() as u32;
    let Some(owner) = symbols.iter().find(|symbol| {
        symbol.start_byte == owner_start
            && matches!(
                symbol.kind,
                SymbolKind::Class | SymbolKind::Trait | SymbolKind::Enum
            )
    }) else {
        return;
    };
    let owner_id = owner.id.clone();
    let mut cursor = node.walk();
    let trait_nodes: Vec<Node> = node
        .named_children(&mut cursor)
        .filter(|child| matches!(child.kind(), "name" | "qualified_name" | "relative_name"))
        .collect();
    for trait_node in trait_nodes {
        let target = unresolved_php_type_target(&extractor.get_base().get_node_text(&trait_node));
        let local_trait = symbols
            .iter()
            .find(|s| s.name == target.terminal_name && s.kind == SymbolKind::Trait);
        if let Some(trait_symbol) = local_trait {
            relationships.push(Relationship {
                id: format!(
                    "{}_{}_{:?}_{}",
                    owner_id,
                    trait_symbol.id,
                    RelationshipKind::Uses,
                    trait_node.start_position().row
                ),
                from_symbol_id: owner_id.clone(),
                to_symbol_id: trait_symbol.id.clone(),
                kind: RelationshipKind::Uses,
                file_path: extractor.get_base().file_path.clone(),
                line_number: trait_node.start_position().row as u32 + 1,
                span: Some(crate::base::NormalizedSpan::from_node(&trait_node)),
                reference_site_is_exact: true,
                confidence: 1.0,
                metadata: None,
            });
        } else {
            let pending = extractor.get_base().create_pending_relationship_at_target(
                owner_id.clone(),
                target,
                RelationshipKind::Uses,
                &trait_node,
                Some(owner_id.clone()),
                Some(0.9),
            );
            extractor.add_structured_pending_relationship(pending);
        }
    }
}

/// Extract inheritance and implementation relationships of a class, an
/// enum, or an anonymous class.
pub(super) fn extract_class_relationships(
    extractor: &mut PhpExtractor,
    node: Node,
    symbols: &[Symbol],
    relationships: &mut Vec<Relationship>,
) {
    let node_start = node.start_byte() as u32;
    let Some(class_symbol) = symbols.iter().find(|symbol| {
        symbol.start_byte == node_start
            && matches!(symbol.kind, SymbolKind::Class | SymbolKind::Enum)
    }) else {
        return;
    };

    // Inheritance relationships
    if let Some(extends_node) = find_child(extractor, &node, "base_clause") {
        let raw_name = extractor
            .get_base()
            .get_node_text(&extends_node)
            .replace("extends", "")
            .trim()
            .to_string();
        let base_target = unresolved_php_type_target(&raw_name);
        let base_class_name = base_target.terminal_name.clone();

        // Try same-file resolution first
        if let Some(base_class_symbol) = symbols
            .iter()
            .find(|s| s.name == base_class_name && s.kind == SymbolKind::Class)
        {
            relationships.push(Relationship {
                id: format!(
                    "{}_{}_{:?}_{}",
                    class_symbol.id,
                    base_class_symbol.id,
                    RelationshipKind::Extends,
                    node.start_position().row
                ),
                from_symbol_id: class_symbol.id.clone(),
                to_symbol_id: base_class_symbol.id.clone(),
                kind: RelationshipKind::Extends,
                file_path: extractor.get_base().file_path.clone(),
                line_number: node.start_position().row as u32 + 1,
                span: Some(crate::base::NormalizedSpan::from_node(&node)),
                reference_site_is_exact: false,
                confidence: 1.0,
                metadata: Some({
                    let mut metadata = HashMap::new();
                    metadata.insert(
                        "baseClass".to_string(),
                        serde_json::Value::String(base_class_name),
                    );
                    metadata
                }),
            });
        } else {
            // Bug 2: base class not found in same file, emit PendingRelationship for cross-file resolution
            let pending = extractor.get_base().create_pending_relationship(
                class_symbol.id.clone(),
                base_target,
                RelationshipKind::Extends,
                &node,
                Some(class_symbol.id.clone()),
                Some(0.9),
            );
            extractor.add_structured_pending_relationship(pending);
        }
    }

    // Implementation relationships
    if let Some(implements_node) = find_child(extractor, &node, "class_interface_clause") {
        let interface_names: Vec<String> = extractor
            .get_base()
            .get_node_text(&implements_node)
            .replace("implements", "")
            .split(',')
            .map(|name| name.trim().to_string())
            .filter(|name| !name.is_empty())
            .collect();

        for raw_interface_name in interface_names {
            let interface_target = unresolved_php_type_target(&raw_interface_name);
            let interface_name = interface_target.terminal_name.clone();
            // Bug 3: removed same-file filter, search all in-scope symbols
            let interface_symbol = symbols
                .iter()
                .find(|s| s.name == interface_name && s.kind == SymbolKind::Interface);

            if let Some(iface) = interface_symbol {
                // Same-file interface found: create a resolved Relationship
                relationships.push(Relationship {
                    id: format!(
                        "{}_{}_{:?}_{}",
                        class_symbol.id,
                        iface.id,
                        RelationshipKind::Implements,
                        node.start_position().row
                    ),
                    from_symbol_id: class_symbol.id.clone(),
                    to_symbol_id: iface.id.clone(),
                    kind: RelationshipKind::Implements,
                    file_path: extractor.get_base().file_path.clone(),
                    line_number: node.start_position().row as u32 + 1,
                    span: Some(crate::base::NormalizedSpan::from_node(&node)),
                    reference_site_is_exact: false,
                    confidence: 1.0,
                    metadata: Some({
                        let mut metadata = HashMap::new();
                        metadata.insert(
                            "interface".to_string(),
                            serde_json::Value::String(interface_name),
                        );
                        metadata
                    }),
                });
            } else {
                // Bug 3: interface not found in symbols, emit PendingRelationship instead of fabricating an ID
                let pending = extractor.get_base().create_pending_relationship(
                    class_symbol.id.clone(),
                    interface_target,
                    RelationshipKind::Implements,
                    &node,
                    Some(class_symbol.id.clone()),
                    Some(0.9),
                );
                extractor.add_structured_pending_relationship(pending);
            }
        }
    }
}

/// Extract interface inheritance relationships
pub(super) fn extract_interface_relationships(
    extractor: &mut PhpExtractor,
    node: Node,
    symbols: &[Symbol],
    relationships: &mut Vec<Relationship>,
) {
    let interface_symbol = find_interface_symbol(extractor, node, symbols);
    if interface_symbol.is_none() {
        return;
    }
    let interface_symbol = interface_symbol.unwrap();

    // Interface inheritance
    if let Some(extends_node) = find_child(extractor, &node, "base_clause") {
        let base_interface_names: Vec<String> = extractor
            .get_base()
            .get_node_text(&extends_node)
            .replace("extends", "")
            .split(',')
            .map(|name| name.trim().to_string())
            .filter(|name| !name.is_empty())
            .collect();

        for raw_base_interface_name in base_interface_names {
            let base_interface_target = unresolved_php_type_target(&raw_base_interface_name);
            let base_interface_name = base_interface_target.terminal_name.clone();
            // Try to find the base interface in current symbols
            let base_symbol = symbols
                .iter()
                .find(|s| s.name == base_interface_name && s.kind == SymbolKind::Interface);

            if let Some(base) = base_symbol {
                // Same-file interface found: create resolved Relationship
                relationships.push(Relationship {
                    id: format!(
                        "{}_{}_{:?}_{}",
                        interface_symbol.id,
                        base.id,
                        RelationshipKind::Extends,
                        node.start_position().row
                    ),
                    from_symbol_id: interface_symbol.id.clone(),
                    to_symbol_id: base.id.clone(),
                    kind: RelationshipKind::Extends,
                    file_path: extractor.get_base().file_path.clone(),
                    line_number: node.start_position().row as u32 + 1,
                    span: Some(crate::base::NormalizedSpan::from_node(&node)),
                    reference_site_is_exact: false,
                    confidence: 1.0,
                    metadata: Some({
                        let mut metadata = HashMap::new();
                        metadata.insert(
                            "baseInterface".to_string(),
                            serde_json::Value::String(base_interface_name),
                        );
                        metadata
                    }),
                });
            } else {
                // Cross-file: emit PendingRelationship instead of fabricating an ID
                let pending = extractor.get_base().create_pending_relationship(
                    interface_symbol.id.clone(),
                    base_interface_target,
                    RelationshipKind::Extends,
                    &node,
                    Some(interface_symbol.id.clone()),
                    Some(0.9),
                );
                extractor.add_structured_pending_relationship(pending);
            }
        }
    }
}

/// Find interface symbol by node
pub(super) fn find_interface_symbol<'a>(
    extractor: &PhpExtractor,
    node: Node,
    symbols: &'a [Symbol],
) -> Option<&'a Symbol> {
    let name_node = node.child_by_field_name("name")?;
    let name = extractor.get_base().get_node_text(&name_node);

    symbols.iter().find(|s| {
        s.name == name
            && s.kind == SymbolKind::Interface
            && s.file_path == extractor.get_base().file_path
    })
}
