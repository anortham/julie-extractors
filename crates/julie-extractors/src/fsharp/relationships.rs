use super::FSharpExtractor;
use crate::base::{
    LocalTargetResolution, Relationship, RelationshipKind, ScopedSymbolIndex, Symbol, SymbolKind,
    UnresolvedTarget,
};
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};
use tree_sitter::{Node, Tree};

struct CallTarget<'a> {
    node: Node<'a>,
    display_name: String,
    terminal_name: String,
    receiver: Option<String>,
    namespace_path: Vec<String>,
}

pub(super) fn extract_relationships(
    extractor: &mut FSharpExtractor,
    tree: &Tree,
    symbols: &[Symbol],
) -> Vec<Relationship> {
    extractor.base().clear_pending_relationships();
    let symbol_index = ScopedSymbolIndex::new(symbols);
    let mut relationships = Vec::new();
    walk(
        extractor,
        tree.root_node(),
        symbols,
        &symbol_index,
        &mut relationships,
        0,
    );
    relationships
}

fn walk(
    extractor: &mut FSharpExtractor,
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
        "import_decl" => extract_import(extractor, node, symbols),
        "application_expression" if !is_nested_application(node) => {
            extract_call(extractor, node, symbols, symbol_index, relationships);
        }
        "class_inherits_decl" => extract_type_relationship(
            extractor,
            node,
            symbols,
            symbol_index,
            RelationshipKind::Extends,
            relationships,
        ),
        "interface_implementation" => extract_type_relationship(
            extractor,
            node,
            symbols,
            symbol_index,
            RelationshipKind::Implements,
            relationships,
        ),
        "record_field" | "union_type_field" => {
            extract_field_type_relationship(extractor, node, symbols, symbol_index, relationships)
        }
        _ => {}
    }
    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk(
            extractor,
            child,
            symbols,
            symbol_index,
            relationships,
            child_depth,
        );
    }
}

fn extract_import(extractor: &mut FSharpExtractor, node: Node, symbols: &[Symbol]) {
    let Some(target_node) = first_named_child(node) else {
        return;
    };
    let Some(caller) = extractor.base().find_containing_symbol(&node, symbols) else {
        return;
    };
    let display_name = extractor
        .base()
        .get_node_text(&target_node)
        .trim()
        .to_string();
    if display_name.is_empty() {
        return;
    }
    let target = target_from_path(
        &display_name,
        Some(extractor.base().get_node_text(&node).trim().to_string()),
    );
    let pending = extractor.base().create_pending_relationship_at_target(
        caller.id.clone(),
        target,
        RelationshipKind::Imports,
        &target_node,
        Some(caller.id.clone()),
        Some(0.95),
    );
    extractor.base.add_structured_pending_relationship(pending);
}

fn extract_call(
    extractor: &mut FSharpExtractor,
    node: Node,
    symbols: &[Symbol],
    symbol_index: &ScopedSymbolIndex<'_>,
    relationships: &mut Vec<Relationship>,
) {
    let Some(CallTarget {
        node: target_node,
        display_name,
        terminal_name,
        receiver,
        namespace_path,
    }) = call_target(extractor.base(), node)
    else {
        return;
    };
    let Some(caller) = extractor
        .base()
        .find_containing_symbol(&node, symbols)
        .cloned()
    else {
        return;
    };
    let target = UnresolvedTarget {
        display_name,
        terminal_name: terminal_name.clone(),
        receiver,
        namespace_path,
        import_context: None,
    };
    if target.receiver.is_some() || !target.namespace_path.is_empty() {
        add_pending(extractor, &caller, target, target_node);
        return;
    }
    match symbol_index.resolve_call_target(&terminal_name, Some(&caller), None) {
        LocalTargetResolution::Resolved(called_symbol) => {
            relationships.push(extractor.base().create_relationship_at_target(
                caller.id.clone(),
                called_symbol.id.clone(),
                RelationshipKind::Calls,
                &target_node,
                Some(0.95),
                None,
            ));
        }
        LocalTargetResolution::Import(_)
        | LocalTargetResolution::ReceiverQualified
        | LocalTargetResolution::Ambiguous
        | LocalTargetResolution::Missing => add_pending(extractor, &caller, target, target_node),
    }
}

fn add_pending(
    extractor: &mut FSharpExtractor,
    caller: &Symbol,
    target: UnresolvedTarget,
    target_node: Node,
) {
    let mut pending = extractor.base().create_pending_relationship_at_target(
        caller.id.clone(),
        target,
        RelationshipKind::Calls,
        &target_node,
        Some(caller.id.clone()),
        Some(0.8),
    );
    pending.pending.callee_name = pending.target.terminal_name.clone();
    extractor.base.add_structured_pending_relationship(pending);
}

fn extract_type_relationship(
    extractor: &mut FSharpExtractor,
    node: Node,
    symbols: &[Symbol],
    symbol_index: &ScopedSymbolIndex<'_>,
    kind: RelationshipKind,
    relationships: &mut Vec<Relationship>,
) {
    let Some(target_node) = first_type_child(node) else {
        return;
    };
    let Some(caller) = containing_type_symbol(extractor, node, symbols) else {
        return;
    };
    let display_name = extractor
        .base()
        .get_node_text(&target_node)
        .trim()
        .to_string();
    let (terminal_name, namespace_path) = split_path(&display_name);
    if namespace_path.is_empty()
        && let Some(target) = symbol_index
            .candidates_by_name(&terminal_name)
            .find(|candidate| candidate.id != caller.id)
    {
        relationships.push(extractor.base().create_relationship_at_target(
            caller.id.clone(),
            target.id.clone(),
            kind,
            &target_node,
            Some(0.9),
            None,
        ));
        return;
    }
    let target = UnresolvedTarget {
        display_name,
        terminal_name,
        receiver: None,
        namespace_path,
        import_context: None,
    };
    let mut pending = extractor.base().create_pending_relationship_at_target(
        caller.id.clone(),
        target,
        kind,
        &target_node,
        Some(caller.id.clone()),
        Some(0.75),
    );
    pending.pending.callee_name = pending.target.terminal_name.clone();
    extractor.base.add_structured_pending_relationship(pending);
}

fn extract_field_type_relationship(
    extractor: &mut FSharpExtractor,
    node: Node,
    symbols: &[Symbol],
    symbol_index: &ScopedSymbolIndex<'_>,
    relationships: &mut Vec<Relationship>,
) {
    let Some(type_node) = first_type_child(node) else {
        return;
    };
    let Some(type_base) = first_named_child(type_node) else {
        return;
    };
    let Some(target_node) = terminal_identifier(type_base) else {
        return;
    };
    let display_name = extractor
        .base()
        .get_node_text(&target_node)
        .trim()
        .to_string();
    if display_name.is_empty() {
        return;
    }
    let Some(caller) = containing_type_declaration(extractor, node, symbols) else {
        return;
    };
    if let Some(target) = symbol_index
        .candidates_by_name(&display_name)
        .find(|candidate| candidate.id != caller.id)
    {
        relationships.push(extractor.base().create_relationship_at_target(
            caller.id.clone(),
            target.id.clone(),
            RelationshipKind::Uses,
            &target_node,
            Some(0.8),
            None,
        ));
    }
}

fn containing_type_symbol<'a>(
    extractor: &FSharpExtractor,
    node: Node,
    symbols: &'a [Symbol],
) -> Option<&'a Symbol> {
    extractor
        .base
        .find_containing_symbol(&node, symbols)
        .filter(|symbol| {
            matches!(
                symbol.kind,
                SymbolKind::Class
                    | SymbolKind::Struct
                    | SymbolKind::Union
                    | SymbolKind::Interface
                    | SymbolKind::Enum
                    | SymbolKind::Type
            )
        })
}

fn containing_type_declaration<'a>(
    extractor: &FSharpExtractor,
    node: Node,
    symbols: &'a [Symbol],
) -> Option<&'a Symbol> {
    symbols
        .iter()
        .filter(|symbol| {
            matches!(
                symbol.kind,
                SymbolKind::Class
                    | SymbolKind::Struct
                    | SymbolKind::Union
                    | SymbolKind::Interface
                    | SymbolKind::Enum
                    | SymbolKind::Type
            )
        })
        .filter(|symbol| {
            symbol.file_path == extractor.base.file_path
                && symbol.start_byte <= node.start_byte() as u32
                && symbol.end_byte >= node.end_byte() as u32
        })
        .min_by_key(|symbol| symbol.end_byte.saturating_sub(symbol.start_byte))
}

fn first_type_child(node: Node) -> Option<Node> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor)
        .find(|child| is_type_kind(child.kind()))
}

fn is_type_kind(kind: &str) -> bool {
    matches!(
        kind,
        "simple_type"
            | "generic_type"
            | "atomic_type"
            | "compound_type"
            | "constrained_type"
            | "flexible_type"
            | "function_type"
            | "list_type"
            | "paren_type"
            | "postfix_type"
            | "static_type"
            | "struct_type"
            | "tuple_type"
            | "type_name"
            | "types"
    )
}

fn call_target<'a>(base: &crate::base::BaseExtractor, node: Node<'a>) -> Option<CallTarget<'a>> {
    let head = first_named_child(node)?;
    match head.kind() {
        "application_expression" => call_target(base, head),
        "dot_expression" => {
            let field = head.child_by_field_name("field")?;
            let target_node = terminal_identifier(field)?;
            let receiver_node = head.child_by_field_name("base")?;
            let receiver = base.get_node_text(&receiver_node).trim().to_string();
            let display_name = format!("{}.{}", receiver, base.get_node_text(&field).trim());
            let (terminal_name, namespace_path) = split_path(&display_name);
            let (receiver, namespace_path) = if receiver.contains('.') {
                (None, namespace_path)
            } else {
                (Some(receiver), Vec::new())
            };
            Some(CallTarget {
                node: target_node,
                display_name,
                terminal_name,
                receiver,
                namespace_path,
            })
        }
        "long_identifier_or_op" | "long_identifier" => {
            let target_node = terminal_identifier(head)?;
            let display_name = base.get_node_text(&head).trim().to_string();
            let (terminal_name, path) = split_path(&display_name);
            let prefix = display_name
                .split('.')
                .map(str::trim)
                .filter(|segment| !segment.is_empty())
                .collect::<Vec<_>>();
            let prefix = &prefix[..prefix.len().saturating_sub(1)];
            let receiver = if prefix
                .first()
                .is_some_and(|segment| segment.chars().next().is_some_and(char::is_lowercase))
            {
                Some(prefix.join("."))
            } else {
                None
            };
            let namespace_path = if receiver.is_some() { Vec::new() } else { path };
            Some(CallTarget {
                node: target_node,
                display_name,
                terminal_name,
                receiver,
                namespace_path,
            })
        }
        _ => None,
    }
}

fn target_from_path(display_name: &str, import_context: Option<String>) -> UnresolvedTarget {
    let (terminal_name, namespace_path) = split_path(display_name);
    UnresolvedTarget {
        display_name: display_name.to_string(),
        terminal_name,
        receiver: None,
        namespace_path,
        import_context,
    }
}

fn split_path(path: &str) -> (String, Vec<String>) {
    let segments: Vec<String> = path
        .split('.')
        .map(str::trim)
        .filter(|segment| !segment.is_empty())
        .map(ToOwned::to_owned)
        .collect();
    let terminal = segments.last().cloned().unwrap_or_default();
    let namespace = segments[..segments.len().saturating_sub(1)].to_vec();
    (terminal, namespace)
}

fn is_nested_application(node: Node) -> bool {
    node.parent()
        .is_some_and(|parent| parent.kind() == "application_expression")
}

fn first_named_child(node: Node) -> Option<Node> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor).next()
}

fn terminal_identifier(node: Node) -> Option<Node> {
    if node.kind() == "identifier" {
        return Some(node);
    }
    let mut cursor = node.walk();
    let children: Vec<_> = node.named_children(&mut cursor).collect();
    children.into_iter().rev().find_map(terminal_identifier)
}
