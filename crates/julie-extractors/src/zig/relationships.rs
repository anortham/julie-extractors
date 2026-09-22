use crate::base::{
    BaseExtractor, ContainingSymbolIndex, LocalTargetResolution, Relationship, RelationshipKind,
    ScopedSymbolIndex, Symbol, SymbolKind, UnresolvedTarget,
};
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};
use crate::zig::ZigExtractor;
use crate::zig::helpers::unwrap_logical_not;
use tree_sitter::{Node, Tree};

/// Extract relationships between symbols (calls, composition, inheritance)
pub(super) fn extract_relationships(
    extractor: &mut ZigExtractor,
    tree: &Tree,
    symbols: &[Symbol],
) -> Vec<Relationship> {
    let mut relationships = Vec::new();
    let containing_symbols = extractor.base.containing_symbol_index(symbols);
    let call_targets: Vec<Symbol> = symbols
        .iter()
        .filter(|symbol| !is_test_declaration(symbol))
        .cloned()
        .collect();
    let scoped_index = ScopedSymbolIndex::new(&call_targets);
    traverse_for_relationships(
        extractor,
        tree.root_node(),
        symbols,
        &containing_symbols,
        &scoped_index,
        &mut relationships,
        0,
    );
    relationships
}

fn traverse_for_relationships(
    extractor: &mut ZigExtractor,
    node: Node,
    symbols: &[Symbol],
    containing_symbols: &ContainingSymbolIndex<'_>,
    scoped_index: &ScopedSymbolIndex<'_>,
    relationships: &mut Vec<Relationship>,
    depth: u32,
) {
    if !should_visit_tree_depth(depth) {
        return;
    }

    let base = extractor.get_base_mut();
    match node.kind() {
        "struct_declaration" => {
            extract_struct_relationships(base, node, symbols, relationships);
        }
        "const_declaration"
            if base
                .find_child_by_type(&node, "struct_declaration")
                .is_some() =>
        {
            extract_struct_relationships(base, node, symbols, relationships);
        }
        "call_expression" => {
            extract_function_call_relationships(
                extractor,
                node,
                containing_symbols,
                scoped_index,
                relationships,
            );
        }
        _ => {}
    }

    // Recursively traverse children
    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        traverse_for_relationships(
            extractor,
            child,
            symbols,
            containing_symbols,
            scoped_index,
            relationships,
            child_depth,
        );
    }
}

fn extract_struct_relationships(
    base: &mut BaseExtractor,
    node: Node,
    symbols: &[Symbol],
    relationships: &mut Vec<Relationship>,
) {
    if node.kind() != "struct_declaration" {
        return;
    }

    // Find a symbol that matches this struct_declaration by position
    let struct_symbol = symbols
        .iter()
        .find(|s| {
            s.kind == SymbolKind::Struct
                && s.start_line == (node.start_position().row + 1) as u32
                && s.start_column == node.start_position().column as u32
        })
        .or_else(|| {
            // Try finding by nearby position (within a few lines)
            symbols.iter().find(|s| {
                s.kind == SymbolKind::Struct
                    && (s.start_line as i32 - (node.start_position().row + 1) as i32).abs() <= 2
            })
        });

    if let Some(target_symbol) = struct_symbol {
        traverse_struct_fields(base, node, symbols, relationships, target_symbol);
    }
}

fn traverse_struct_fields(
    base: &mut BaseExtractor,
    node: Node,
    symbols: &[Symbol],
    relationships: &mut Vec<Relationship>,
    target_symbol: &Symbol,
) {
    let mut cursor = node.walk();
    for field_node in node.children(&mut cursor) {
        if field_node.kind() == "container_field"
            && let Some(field_name_node) = base.find_child_by_type(&field_node, "identifier")
        {
            let _field_name = base.get_node_text(&field_name_node);

            // Look for type information
            let type_node = base
                .find_child_by_type(&field_node, "type_expression")
                .or_else(|| base.find_child_by_type(&field_node, "builtin_type"))
                .or_else(|| base.find_child_by_type(&field_node, "slice_type"))
                .or_else(|| base.find_child_by_type(&field_node, "pointer_type"))
                .or_else(|| {
                    // Look for identifier after colon
                    let mut field_cursor = field_node.walk();
                    let field_children: Vec<Node> =
                        field_node.children(&mut field_cursor).collect();
                    let colon_index = field_children.iter().position(|c| c.kind() == ":")?;
                    field_children.get(colon_index + 1).copied()
                });

            if let Some(type_node) = type_node {
                let type_name = base.get_node_text(&type_node).trim().to_string();

                // Look for referenced symbols that are struct types
                let referenced_symbol = symbols.iter().find(|s| {
                    s.name == type_name
                        && matches!(
                            s.kind,
                            SymbolKind::Struct
                                | SymbolKind::Union
                                | SymbolKind::Type
                                | SymbolKind::Enum
                        )
                });

                if let Some(referenced_symbol) = referenced_symbol
                    && referenced_symbol.id != target_symbol.id
                {
                    // Create composition relationship
                    relationships.push(Relationship {
                        id: format!(
                            "{}_{}_{:?}_{}",
                            target_symbol.id,
                            referenced_symbol.id,
                            RelationshipKind::Composition,
                            field_node.start_position().row
                        ),
                        from_symbol_id: target_symbol.id.clone(),
                        to_symbol_id: referenced_symbol.id.clone(),
                        kind: RelationshipKind::Composition,
                        file_path: base.file_path.clone(),
                        line_number: (field_node.start_position().row + 1) as u32,
                        span: Some(crate::base::NormalizedSpan::from_node(&field_node)),
                        reference_site_is_exact: false,
                        confidence: 0.8,
                        metadata: None,
                    });
                }
            }
        }
    }
}

fn collect_field_chain(base: &BaseExtractor, node: Node, parts: &mut Vec<String>) -> bool {
    let node = unwrap_logical_not(node);
    match node.kind() {
        "identifier" => {
            parts.push(base.get_node_text(&node));
            true
        }
        "field_expression" => {
            let (Some(object), Some(member)) = (
                node.child_by_field_name("object"),
                node.child_by_field_name("member"),
            ) else {
                return false;
            };
            collect_field_chain(base, object, parts) && collect_field_chain(base, member, parts)
        }
        _ => false,
    }
}

/// The callee of a call: a bare name, a `a.b.f` chain, a member called on an
/// expression result (receiver = the expression text), or a decl literal
/// `.init(..)` whose receiver is the declared type of the variable it
/// initializes. Arguments are never callees.
fn call_target(base: &BaseExtractor, call: Node) -> Option<UnresolvedTarget> {
    let function = unwrap_logical_not(call.child_by_field_name("function")?);
    match function.kind() {
        "identifier" => Some(UnresolvedTarget::simple(base.get_node_text(&function))),
        "field_expression" => {
            let member = base.get_node_text(&function.child_by_field_name("member")?);
            let receiver = match function.child_by_field_name("object") {
                Some(object) => {
                    let mut parts = Vec::new();
                    if collect_field_chain(base, object, &mut parts) {
                        parts.push(member);
                        return Some(UnresolvedTarget::from_chain(parts));
                    }
                    base.get_node_text(&unwrap_logical_not(object))
                }
                None => {
                    let declaration = call
                        .parent()
                        .filter(|parent| parent.kind() == "variable_declaration")?;
                    let declared = declaration.child_by_field_name("type")?;
                    let name = super::type_facts::base_type_name_node(declared)?;
                    base.get_node_text(&name)
                }
            };
            Some(UnresolvedTarget {
                display_name: format!("{receiver}.{member}"),
                terminal_name: member,
                receiver: Some(receiver),
                namespace_path: Vec::new(),
                import_context: None,
            })
        }
        _ => None,
    }
}

fn extract_function_call_relationships(
    extractor: &mut ZigExtractor,
    node: Node,
    containing_symbols: &ContainingSymbolIndex<'_>,
    scoped_index: &ScopedSymbolIndex<'_>,
    relationships: &mut Vec<Relationship>,
) {
    let base = extractor.get_base_mut();
    let unresolved_target = call_target(base, node);

    if let Some(unresolved_target) = unresolved_target {
        let caller_symbol = containing_symbols.find(node).filter(|symbol| {
            matches!(
                symbol.kind,
                SymbolKind::Function | SymbolKind::Method | SymbolKind::Constructor
            )
        });

        if let Some(caller_symbol) = caller_symbol {
            // Now check if the called function exists locally
            let line_number = (node.start_position().row + 1) as u32;
            let file_path = base.file_path.clone();

            match scoped_index.resolve_call_target(
                &unresolved_target.terminal_name,
                Some(caller_symbol),
                unresolved_target.receiver.as_deref(),
            ) {
                LocalTargetResolution::Resolved(called_symbol) => {
                    if caller_symbol.id != called_symbol.id {
                        relationships.push(Relationship {
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
                            file_path,
                            line_number,
                            span: Some(crate::base::NormalizedSpan::from_node(&node)),
                            reference_site_is_exact: false,
                            confidence: 0.9,
                            metadata: None,
                        });
                    }
                }
                LocalTargetResolution::Import(_)
                | LocalTargetResolution::Ambiguous
                | LocalTargetResolution::Missing
                | LocalTargetResolution::ReceiverQualified => {
                    let receiver_type =
                        super::type_facts::self_receiver_type(&extractor.base, node);
                    let pending = extractor
                        .get_base_mut()
                        .create_pending_relationship(
                            caller_symbol.id.clone(),
                            unresolved_target,
                            RelationshipKind::Calls,
                            &node,
                            Some(caller_symbol.id.clone()),
                            Some(0.7),
                        )
                        .with_receiver_type(receiver_type);
                    extractor.add_structured_pending_relationship(pending);
                }
            }
        }
    }
}

/// A `test` block is named after what it tests (`test square {}`), so it must
/// never compete with the tested declaration as a call target.
fn is_test_declaration(symbol: &Symbol) -> bool {
    symbol
        .signature
        .as_deref()
        .is_some_and(|signature| signature == "test" || signature.starts_with("test "))
}
