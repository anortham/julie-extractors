use super::helpers::{UseLeaf, extract_impl_target_names, push_path_segments, use_leaves};
/// Rust relationship extraction
/// - Trait implementations
/// - Type references in fields
/// - Function calls
use crate::base::{
    LocalTargetResolution, Relationship, RelationshipKind, ScopedSymbolIndex, Symbol, SymbolKind,
    UnresolvedTarget,
};
use crate::rust::RustExtractor;
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};
use std::collections::HashMap;
use tree_sitter::{Node, Tree};

/// Extract all relationships between Rust symbols
pub(super) fn extract_relationships(
    extractor: &mut RustExtractor,
    tree: &Tree,
    symbols: &[Symbol],
) -> Vec<Relationship> {
    let mut relationships = Vec::new();
    let symbol_map: HashMap<String, &Symbol> =
        crate::base::ScopedSymbolIndex::unique_symbol_map(symbols);
    let symbol_index = ScopedSymbolIndex::new(symbols);

    walk_tree_for_relationships(
        extractor,
        tree.root_node(),
        &symbol_map,
        symbols,
        &symbol_index,
        &mut relationships,
        0,
    );
    relationships
}

fn walk_tree_for_relationships(
    extractor: &mut RustExtractor,
    node: Node,
    symbol_map: &HashMap<String, &Symbol>,
    symbols: &[Symbol],
    symbol_index: &ScopedSymbolIndex<'_>,
    relationships: &mut Vec<Relationship>,
    depth: u32,
) {
    if !should_visit_tree_depth(depth) {
        return;
    }

    match node.kind() {
        "impl_item" => {
            extract_impl_relationships(extractor, node, symbol_map, relationships);
        }
        "trait_item" => {
            extract_supertrait_relationships(extractor, node, symbol_map, relationships);
        }
        "struct_item" | "enum_item" => {
            extract_type_relationships(extractor, node, symbol_map, relationships);
        }
        "call_expression" => {
            extract_call_relationships(extractor, node, symbols, symbol_index, relationships);
        }
        "identifier" => {
            extract_macro_token_call(extractor, node, symbols, symbol_index, relationships);
        }
        "use_declaration" | "extern_crate_declaration" => {
            extract_use_import_relationship(extractor, node, symbols);
        }
        _ => {}
    }

    // Recursively process children
    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk_tree_for_relationships(
            extractor,
            child,
            symbol_map,
            symbols,
            symbol_index,
            relationships,
            child_depth,
        );
    }
}

fn extract_use_import_relationship(extractor: &mut RustExtractor, node: Node, symbols: &[Symbol]) {
    let use_text = extractor.get_base_mut().get_node_text(&node);
    let leaves = use_leaves(extractor.get_base_mut(), node);
    for UseLeaf { name, path, .. } in leaves {
        let Some(import_symbol) = symbols.iter().find(|symbol| {
            symbol.kind == SymbolKind::Import
                && symbol.name == name
                && symbol.start_byte == node.start_byte() as u32
        }) else {
            continue;
        };
        let Some((terminal_name, namespace_path)) = path.split_last() else {
            continue;
        };
        let target = UnresolvedTarget {
            display_name: path.join("::"),
            terminal_name: terminal_name.clone(),
            receiver: None,
            namespace_path: namespace_path.to_vec(),
            import_context: Some(use_text.trim().to_string()),
        };
        let pending = extractor.get_base_mut().create_pending_relationship(
            import_symbol.id.clone(),
            target,
            RelationshipKind::Imports,
            &node,
            Some(import_symbol.id.clone()),
            Some(1.0),
        );
        extractor.add_structured_pending_relationship(pending);
    }
}

/// Extract trait implementation relationships
fn extract_impl_relationships(
    extractor: &mut RustExtractor,
    node: Node,
    symbol_map: &HashMap<String, &Symbol>,
    relationships: &mut Vec<Relationship>,
) {
    let base = extractor.get_base_mut();
    let Some(type_name) = extract_impl_target_names(base, node).type_name else {
        return;
    };
    let Some(type_symbol) = symbol_map.get(&type_name) else {
        return;
    };
    if let Some(trait_node) = node.child_by_field_name("trait") {
        emit_trait_edge(
            extractor,
            node,
            type_symbol,
            trait_node,
            RelationshipKind::Implements,
            symbol_map,
            relationships,
        );
    }
}

/// Extract `extends` edges from a trait to each of its supertraits.
fn extract_supertrait_relationships(
    extractor: &mut RustExtractor,
    node: Node,
    symbol_map: &HashMap<String, &Symbol>,
    relationships: &mut Vec<Relationship>,
) {
    let (Some(name), Some(bounds)) = (
        node.child_by_field_name("name"),
        node.child_by_field_name("bounds"),
    ) else {
        return;
    };
    let trait_name = extractor.get_base_mut().get_node_text(&name);
    let Some(trait_symbol) = symbol_map.get(&trait_name) else {
        return;
    };
    for bound in bounds.named_children(&mut bounds.walk()) {
        emit_trait_edge(
            extractor,
            node,
            trait_symbol,
            bound,
            RelationshipKind::Extends,
            symbol_map,
            relationships,
        );
    }
}

/// Emit a resolved edge to a same-file trait, or a pending edge that keeps the
/// trait path for cross-file resolution.
fn emit_trait_edge(
    extractor: &mut RustExtractor,
    node: Node,
    from: &Symbol,
    trait_node: Node,
    kind: RelationshipKind,
    symbol_map: &HashMap<String, &Symbol>,
    relationships: &mut Vec<Relationship>,
) {
    let trait_path = match trait_node.kind() {
        "generic_type" => trait_node.child_by_field_name("type"),
        "type_identifier" | "scoped_type_identifier" => Some(trait_node),
        _ => None,
    };
    let Some(trait_path) = trait_path else {
        return;
    };
    let base = extractor.get_base_mut();
    let mut segments = Vec::new();
    push_path_segments(base, trait_path, &mut segments);
    let Some(terminal_name) = segments.pop() else {
        return;
    };

    let local_trait = symbol_map
        .get(&terminal_name)
        .filter(|symbol| segments.is_empty() && symbol.kind == SymbolKind::Interface);
    if let Some(trait_symbol) = local_trait {
        relationships.push(Relationship {
            id: format!(
                "{}_{}_{:?}_{}",
                from.id,
                trait_symbol.id,
                kind,
                node.start_position().row
            ),
            from_symbol_id: from.id.clone(),
            to_symbol_id: trait_symbol.id.clone(),
            kind,
            file_path: base.file_path.clone(),
            line_number: node.start_position().row as u32 + 1,
            span: Some(crate::base::NormalizedSpan::from_node(&node)),
            reference_site_is_exact: false,
            confidence: 0.95,
            metadata: None,
        });
        return;
    }

    let mut display = segments.clone();
    display.push(terminal_name.clone());
    let target = UnresolvedTarget {
        display_name: display.join("::"),
        terminal_name,
        receiver: None,
        namespace_path: segments,
        import_context: None,
    };
    let pending = base.create_pending_relationship(
        from.id.clone(),
        target,
        kind,
        &trait_node,
        Some(from.id.clone()),
        Some(0.9),
    );
    extractor.add_structured_pending_relationship(pending);
}

/// Extract type references in struct/enum fields
fn extract_type_relationships(
    extractor: &mut RustExtractor,
    node: Node,
    symbol_map: &HashMap<String, &Symbol>,
    relationships: &mut Vec<Relationship>,
) {
    let base = extractor.get_base_mut();
    let name_node = node.child_by_field_name("name");
    if let Some(name_node) = name_node {
        let type_name = base.get_node_text(&name_node);
        if let Some(type_symbol) = symbol_map.get(&type_name) {
            // Look for field types that reference other symbols
            let declaration_list = node
                .children(&mut node.walk())
                .find(|c| c.kind() == "field_declaration_list" || c.kind() == "enum_variant_list");

            if let Some(decl_list) = declaration_list {
                for field in decl_list.children(&mut decl_list.walk()) {
                    if field.kind() == "field_declaration" || field.kind() == "enum_variant" {
                        extract_field_type_references(
                            extractor,
                            field,
                            type_symbol,
                            symbol_map,
                            relationships,
                        );
                    }
                }
            }
        }
    }
}

/// Extract type references within a field
fn extract_field_type_references(
    extractor: &mut RustExtractor,
    field_node: Node,
    container_symbol: &Symbol,
    symbol_map: &HashMap<String, &Symbol>,
    relationships: &mut Vec<Relationship>,
) {
    let base = extractor.get_base_mut();
    // Find type references in field declarations
    for child in field_node.children(&mut field_node.walk()) {
        if child.kind() == "type_identifier" {
            let referenced_type_name = base.get_node_text(&child);
            if let Some(referenced_symbol) = symbol_map.get(&referenced_type_name)
                && referenced_symbol.id != container_symbol.id
            {
                relationships.push(Relationship {
                    id: format!(
                        "{}_{}_{:?}_{}",
                        container_symbol.id,
                        referenced_symbol.id,
                        RelationshipKind::Uses,
                        field_node.start_position().row
                    ),
                    from_symbol_id: container_symbol.id.clone(),
                    to_symbol_id: referenced_symbol.id.clone(),
                    kind: RelationshipKind::Uses,
                    file_path: base.file_path.clone(),
                    line_number: field_node.start_position().row as u32 + 1,
                    span: Some(crate::base::NormalizedSpan::from_node(&field_node)),
                    reference_site_is_exact: false,
                    confidence: 0.8,
                    metadata: None,
                });
            }
        }
    }
}

/// Extract function call relationships
///
/// Creates resolved Relationship when target is a local function/method.
/// Creates PendingRelationship when target is:
/// - An Import symbol (needs cross-file resolution)
/// - Not found in local symbol_map (e.g., method on imported type)
fn extract_call_relationships(
    extractor: &mut RustExtractor,
    node: Node,
    symbols: &[Symbol],
    symbol_index: &ScopedSymbolIndex<'_>,
    relationships: &mut Vec<Relationship>,
) {
    let function_node = node.child_by_field_name("function").map(|function| {
        if function.kind() == "generic_function" {
            function.child_by_field_name("function").unwrap_or(function)
        } else {
            function
        }
    });
    if let Some(func_node) = function_node {
        // Handle method calls (receiver.method())
        if func_node.kind() == "field_expression" {
            let method_node = func_node.child_by_field_name("field");
            if let Some(method_node) = method_node {
                let method_name = extractor.get_base_mut().get_node_text(&method_node);
                let target = if let Some(receiver_node) = func_node.child_by_field_name("value") {
                    let receiver = extractor.get_base_mut().get_node_text(&receiver_node);
                    UnresolvedTarget {
                        display_name: format!("{receiver}.{method_name}"),
                        terminal_name: method_name.clone(),
                        receiver: Some(receiver),
                        namespace_path: Vec::new(),
                        import_context: None,
                    }
                } else {
                    UnresolvedTarget::simple(method_name.clone())
                };
                handle_call_target(
                    extractor,
                    node,
                    &method_name,
                    target,
                    symbols,
                    symbol_index,
                    relationships,
                );
            }
        }
        // Handle direct function calls
        else if func_node.kind() == "identifier" {
            let function_name = extractor.get_base_mut().get_node_text(&func_node);
            handle_call_target(
                extractor,
                node,
                &function_name,
                UnresolvedTarget::simple(function_name.clone()),
                symbols,
                symbol_index,
                relationships,
            );
        }
        // Handle qualified/scoped calls: crate::module::function()
        else if func_node.kind() == "scoped_identifier"
            && let Some(target) = scoped_identifier_to_unresolved_target(extractor, func_node)
        {
            let function_name = target.terminal_name.clone();
            handle_call_target(
                extractor,
                node,
                &function_name,
                target,
                symbols,
                symbol_index,
                relationships,
            );
        }
    }
}

fn extract_macro_token_call(
    extractor: &mut RustExtractor,
    node: Node,
    symbols: &[Symbol],
    symbol_index: &ScopedSymbolIndex<'_>,
    relationships: &mut Vec<Relationship>,
) {
    let Some(call) = super::helpers::macro_token_call(extractor.get_base_mut(), node) else {
        return;
    };
    let mut display = call.namespace_path.clone();
    display.push(call.name.clone());
    let display_name = match &call.receiver {
        Some(receiver) => format!("{receiver}.{}", call.name),
        None => display.join("::"),
    };
    let target = UnresolvedTarget {
        display_name,
        terminal_name: call.name.clone(),
        receiver: call.receiver,
        namespace_path: call.namespace_path,
        import_context: None,
    };
    handle_call_target(
        extractor,
        node,
        &call.name,
        target,
        symbols,
        symbol_index,
        relationships,
    );
}

fn scoped_identifier_to_unresolved_target(
    extractor: &mut RustExtractor,
    scoped_identifier: Node,
) -> Option<UnresolvedTarget> {
    let base = extractor.get_base_mut();
    let mut segments = Vec::new();
    push_path_segments(base, scoped_identifier, &mut segments);
    let terminal_name = segments.last()?.clone();
    let namespace_path = segments[..segments.len() - 1].to_vec();

    Some(UnresolvedTarget {
        display_name: segments.join("::"),
        terminal_name,
        receiver: None,
        namespace_path,
        import_context: None,
    })
}

fn add_structured_pending_call(
    extractor: &mut RustExtractor,
    caller: &Symbol,
    call_node: Node,
    unresolved_target: UnresolvedTarget,
    confidence: f32,
    receiver_type: Option<String>,
) {
    let mut pending = extractor
        .get_base_mut()
        .create_pending_relationship(
            caller.id.clone(),
            unresolved_target,
            RelationshipKind::Calls,
            &call_node,
            Some(caller.id.clone()),
            Some(confidence),
        )
        .with_receiver_type(receiver_type);
    pending.pending.callee_name = pending.target.terminal_name.clone();
    extractor.add_structured_pending_relationship(pending);
}

/// Handle a call target and decide whether it can be resolved inside this file.
fn handle_call_target(
    extractor: &mut RustExtractor,
    call_node: Node,
    callee_name: &str,
    unresolved_target: UnresolvedTarget,
    symbols: &[Symbol],
    symbol_index: &ScopedSymbolIndex<'_>,
    relationships: &mut Vec<Relationship>,
) {
    let caller = extractor
        .get_base_mut()
        .find_containing_symbol(&call_node, symbols)
        .cloned();
    let Some(caller) = caller else {
        return;
    };

    let receiver_type = super::identifiers::self_receiver_type(extractor, call_node);
    let line_number = call_node.start_position().row as u32 + 1;
    let file_path = extractor.get_base_mut().file_path.clone();

    if !unresolved_target.namespace_path.is_empty() {
        add_structured_pending_call(
            extractor,
            &caller,
            call_node,
            unresolved_target,
            0.7,
            receiver_type,
        );
        return;
    }

    match symbol_index.resolve_call_target(
        callee_name,
        Some(&caller),
        unresolved_target.receiver.as_deref(),
    ) {
        LocalTargetResolution::Import(_) => {
            add_structured_pending_call(
                extractor,
                &caller,
                call_node,
                unresolved_target,
                0.8,
                receiver_type,
            );
        }
        LocalTargetResolution::ReceiverQualified => {
            add_structured_pending_call(
                extractor,
                &caller,
                call_node,
                unresolved_target,
                0.7,
                receiver_type,
            );
        }
        LocalTargetResolution::Resolved(called_symbol) => {
            // Target is a local function/method - create resolved Relationship
            relationships.push(Relationship {
                id: format!(
                    "{}_{}_{:?}_{}",
                    caller.id,
                    called_symbol.id,
                    RelationshipKind::Calls,
                    call_node.start_position().row
                ),
                from_symbol_id: caller.id.clone(),
                to_symbol_id: called_symbol.id.clone(),
                kind: RelationshipKind::Calls,
                file_path,
                line_number,
                span: Some(crate::base::NormalizedSpan::from_node(&call_node)),
                reference_site_is_exact: false,
                confidence: 0.9,
                metadata: None,
            });
        }
        LocalTargetResolution::Ambiguous | LocalTargetResolution::Missing => {
            add_structured_pending_call(
                extractor,
                &caller,
                call_node,
                unresolved_target,
                0.7,
                receiver_type,
            );
        }
    }
}
