/// Relationship extraction
/// Handles inheritance relationships and function call relationships
use super::super::base::{
    LocalTargetResolution, Relationship, RelationshipKind, ScopedSymbolIndex, Symbol, SymbolKind,
    UnresolvedTarget,
};
use super::{PythonExtractor, helpers};
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};
use tree_sitter::{Node, Tree};

/// Extract relationships from Python code
pub(crate) fn extract_relationships(
    extractor: &mut PythonExtractor,
    tree: &Tree,
    symbols: &[Symbol],
) -> Vec<Relationship> {
    let mut relationships = Vec::new();

    let symbol_index = ScopedSymbolIndex::new(symbols);

    // Recursively visit all nodes to extract relationships
    visit_node_for_relationships(
        extractor,
        tree.root_node(),
        symbols,
        &symbol_index,
        &mut relationships,
        0,
    );

    relationships
}

/// Visit a node and extract relationships from it
fn visit_node_for_relationships(
    extractor: &mut PythonExtractor,
    node: Node,
    symbols: &[Symbol],
    symbol_index: &ScopedSymbolIndex<'_>,
    relationships: &mut Vec<Relationship>,
    depth: u32,
) {
    if !should_visit_tree_depth(depth) {
        return;
    }

    match node.kind() {
        "class_definition" => {
            extract_class_relationships(extractor, node, symbol_index, relationships);
        }
        "call" => {
            extract_call_relationships(extractor, node, symbols, symbol_index, relationships);
        }
        _ => {}
    }

    // Recursively visit all children
    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        visit_node_for_relationships(
            extractor,
            child,
            symbols,
            symbol_index,
            relationships,
            child_depth,
        );
    }
}

/// Extract inheritance relationships from a class definition
fn extract_class_relationships(
    extractor: &mut PythonExtractor,
    node: Node,
    symbol_index: &ScopedSymbolIndex<'_>,
    relationships: &mut Vec<Relationship>,
) {
    let name_node = match node.child_by_field_name("name") {
        Some(node) => node,
        None => return,
    };

    let class_name = extractor.base().get_node_text(&name_node);
    let class_symbol = match symbol_index.first_by_name(&class_name) {
        Some(symbol) => symbol,
        None => return,
    };

    let Some(superclasses_node) = node.child_by_field_name("superclasses") else {
        return;
    };
    let class_symbol_id = class_symbol.id.clone();
    let mut cursor = superclasses_node.walk();
    let base_nodes: Vec<Node> = superclasses_node.named_children(&mut cursor).collect();
    for base_node in base_nodes {
        let base_node = match base_node.kind() {
            "identifier" | "attribute" => base_node,
            "subscript" => match base_node.child_by_field_name("value") {
                Some(value) => value,
                None => continue,
            },
            _ => continue,
        };
        let base_name = extractor.base().get_node_text(&base_node);
        let local_base = symbol_index
            .first_by_name(&base_name)
            .filter(|symbol| symbol.kind != SymbolKind::Import);
        if let Some(base_symbol) = local_base {
            let relationship_kind = if base_symbol.kind == SymbolKind::Interface {
                RelationshipKind::Implements
            } else {
                RelationshipKind::Extends
            };
            relationships.push(Relationship {
                id: format!(
                    "{}_{}_{:?}_{}",
                    class_symbol_id,
                    base_symbol.id,
                    relationship_kind,
                    node.start_position().row
                ),
                from_symbol_id: class_symbol_id.clone(),
                to_symbol_id: base_symbol.id.clone(),
                kind: relationship_kind,
                file_path: extractor.base().file_path.clone(),
                line_number: (node.start_position().row + 1) as u32,
                span: Some(crate::base::NormalizedSpan::from_node(&node)),
                reference_site_is_exact: false,
                confidence: 0.95,
                metadata: None,
            });
            continue;
        }
        let Some(mut target) = UnresolvedTarget::from_qualified_text(&base_name, &["."]) else {
            continue;
        };
        target.import_context = import_binding_context(&target, symbol_index);
        let target_token_node = base_node
            .child_by_field_name("attribute")
            .unwrap_or(base_node);
        let pending = extractor.base().create_pending_relationship_at_target(
            class_symbol_id.clone(),
            target,
            RelationshipKind::Extends,
            &target_token_node,
            Some(class_symbol_id.clone()),
            Some(0.8),
        );
        extractor.add_structured_pending_relationship(pending);
    }
}

/// The local name of the import binding a target's leading segment names:
/// `O` for `O()`, `billing` for `billing.charge()`, `models` for
/// `models.Model`. `None` when that segment is not an import in this file.
fn import_binding_context(
    target: &UnresolvedTarget,
    symbol_index: &ScopedSymbolIndex<'_>,
) -> Option<String> {
    let root = target
        .namespace_path
        .first()
        .or(target.receiver.as_ref())
        .unwrap_or(&target.terminal_name);
    symbol_index
        .first_by_name(root)
        .filter(|symbol| symbol.kind == SymbolKind::Import)
        .map(|_| root.clone())
}

/// Extract call relationships from a function call
fn extract_call_relationships(
    extractor: &mut PythonExtractor,
    node: Node,
    symbols: &[Symbol],
    symbol_index: &ScopedSymbolIndex<'_>,
    relationships: &mut Vec<Relationship>,
) {
    let Some(function_node) = node.child_by_field_name("function") else {
        return;
    };
    if function_node.kind() == "identifier"
        && extractor.base().get_node_text(&function_node) == "super"
    {
        return;
    }
    let (mut target, receiver_type) = extract_target_from_call(extractor.base(), &function_node);
    target.import_context = import_binding_context(&target, symbol_index);
    let called_method_name = target.terminal_name.clone();
    let target_token_node = function_node
        .child_by_field_name("attribute")
        .unwrap_or(function_node);
    if called_method_name.is_empty() {
        return;
    }
    let anchor = helpers::decorated_definition_name(&node).unwrap_or(node);
    let Some(caller_symbol) = extractor.base().find_containing_symbol(&anchor, symbols) else {
        return;
    };
    let super_call = function_node
        .child_by_field_name("object")
        .is_some_and(|object| helpers::is_super_call(extractor.base(), &object));
    let resolution = if super_call {
        match receiver_type
            .as_deref()
            .and_then(|base_name| same_file_method(symbols, base_name, &called_method_name))
        {
            Some(method) => LocalTargetResolution::Resolved(method),
            None => LocalTargetResolution::Missing,
        }
    } else if target.namespace_path.is_empty() {
        symbol_index.resolve_call_target(
            &called_method_name,
            Some(caller_symbol),
            target.receiver.as_deref(),
        )
    } else {
        LocalTargetResolution::Missing
    };
    match resolution {
        LocalTargetResolution::Import(_) => {
            // An import target needs cross-file resolution; an edge to the
            // import symbol itself is useless for call tracing.
            let pending = extractor
                .base()
                .create_pending_relationship_at_target(
                    caller_symbol.id.clone(),
                    target.clone(),
                    RelationshipKind::Calls,
                    &target_token_node,
                    Some(caller_symbol.id.clone()),
                    Some(0.8),
                )
                .with_receiver_type(receiver_type.clone());
            extractor.add_structured_pending_relationship(pending);
        }
        LocalTargetResolution::Resolved(called_symbol) => {
            let relationship = extractor.base().create_relationship_at_target(
                caller_symbol.id.clone(),
                called_symbol.id.clone(),
                RelationshipKind::Calls,
                &target_token_node,
                Some(0.9),
                None,
            );
            relationships.push(relationship);
        }
        LocalTargetResolution::Ambiguous
        | LocalTargetResolution::ReceiverQualified
        | LocalTargetResolution::Missing => {
            let pending = extractor
                .base()
                .create_pending_relationship_at_target(
                    caller_symbol.id.clone(),
                    target,
                    RelationshipKind::Calls,
                    &target_token_node,
                    Some(caller_symbol.id.clone()),
                    Some(0.7),
                )
                .with_receiver_type(receiver_type);
            extractor.add_structured_pending_relationship(pending);
        }
    }
}

/// The method `method_name` declared directly in the same-file class
/// `class_name`, when exactly one class has that name.
fn same_file_method<'a>(
    symbols: &'a [Symbol],
    class_name: &str,
    method_name: &str,
) -> Option<&'a Symbol> {
    let mut classes = symbols
        .iter()
        .filter(|symbol| symbol.name == class_name && symbol.kind != SymbolKind::Import)
        .filter(|symbol| {
            matches!(
                symbol.kind,
                SymbolKind::Class | SymbolKind::Interface | SymbolKind::Enum
            )
        });
    let class_symbol = classes.next()?;
    if classes.next().is_some() {
        return None;
    }
    symbols.iter().find(|symbol| {
        symbol.name == method_name
            && symbol.kind == SymbolKind::Method
            && symbol.parent_id.as_deref() == Some(class_symbol.id.as_str())
    })
}

/// Extract method name from a call node
fn extract_target_from_call(
    base: &crate::base::BaseExtractor,
    function_node: &Node,
) -> (UnresolvedTarget, Option<String>) {
    match function_node.kind() {
        "identifier" => {
            let name = base.get_node_text(function_node);
            if name == "cls"
                && let Some(class_name) = helpers::enclosing_class_name(base, function_node)
            {
                return (
                    UnresolvedTarget::simple(class_name.clone()),
                    Some(class_name),
                );
            }
            (UnresolvedTarget::simple(name), None)
        }
        "attribute" => {
            if let Some(attribute_node) = function_node.child_by_field_name("attribute") {
                let terminal_name = base.get_node_text(&attribute_node);
                let receiver = function_node.child_by_field_name("object").map(|node| {
                    if helpers::is_super_call(base, &node) {
                        "super".to_string()
                    } else {
                        base.get_node_text(&node)
                    }
                });
                if let Some(receiver) = receiver {
                    let receiver_type = helpers::self_or_cls_receiver_type(base, function_node);
                    let target = UnresolvedTarget::from_qualified_text(
                        &format!("{receiver}.{terminal_name}"),
                        &["."],
                    )
                    .unwrap_or_else(|| UnresolvedTarget {
                        display_name: format!("{receiver}.{terminal_name}"),
                        terminal_name,
                        receiver: Some(receiver),
                        namespace_path: Vec::new(),
                        import_context: None,
                    });
                    (target, receiver_type)
                } else {
                    (UnresolvedTarget::simple(terminal_name), None)
                }
            } else {
                (UnresolvedTarget::simple(String::new()), None)
            }
        }
        _ => (UnresolvedTarget::simple(String::new()), None),
    }
}
