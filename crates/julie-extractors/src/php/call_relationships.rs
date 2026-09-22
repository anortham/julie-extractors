// PHP Extractor - call and object creation relationships

use super::{
    PhpExtractor, identifiers,
    relationships::{strip_php_namespace, unresolved_php_type_target},
};
use crate::base::{
    LocalTargetResolution, Relationship, RelationshipKind, ScopedSymbolIndex, Symbol, SymbolKind,
    UnresolvedTarget,
};
use std::collections::HashMap;
use tree_sitter::Node;

pub(super) fn extract_call_relationships(
    extractor: &mut PhpExtractor,
    node: Node,
    symbols: &[Symbol],
    relationships: &mut Vec<Relationship>,
) {
    let base = extractor.get_base();

    let called_function_name = match node.kind() {
        "function_call_expression" => match node.child_by_field_name("function") {
            Some(name_node) if is_static_name(name_node) => base.get_node_text(&name_node),
            _ => return,
        },
        "member_call_expression" | "nullsafe_member_call_expression" | "scoped_call_expression" => {
            match node.child_by_field_name("name") {
                Some(name_node) if name_node.kind() == "name" => base.get_node_text(&name_node),
                _ => return,
            }
        }
        "object_creation_expression" => match node.named_child(0) {
            Some(class_name_node) if is_static_name(class_name_node) => {
                let raw = base.get_node_text(&class_name_node);
                match raw.trim() {
                    "self" | "static" => match enclosing_class_name(extractor, node) {
                        Some(class_name) => class_name,
                        None => return,
                    },
                    raw => strip_php_namespace(raw).to_string(),
                }
            }
            _ => return,
        },
        _ => return,
    };

    if called_function_name.is_empty() {
        return;
    }

    let rel_kind = if node.kind() == "object_creation_expression" {
        RelationshipKind::Instantiates
    } else {
        RelationshipKind::Calls
    };

    // A Pest `it(...)->with(...)` symbol starts where its own DSL call chain
    // starts; those calls declare the symbol and call nothing.
    if let Some(caller_symbol) = base
        .find_containing_symbol(&node, symbols)
        .filter(|symbol| {
            matches!(
                symbol.kind,
                SymbolKind::Function | SymbolKind::Method | SymbolKind::Constructor
            ) && symbol.start_byte != node.start_byte() as u32
        })
    {
        let line_number = (call_site(node).start_position().row + 1) as u32;
        let file_path = base.file_path.clone();
        let target = unresolved_call_target(extractor, node, &called_function_name);

        if rel_kind == RelationshipKind::Calls {
            resolve_call_relationship(
                extractor,
                node,
                symbols,
                relationships,
                caller_symbol,
                target,
                file_path,
                line_number,
            );
            return;
        }

        resolve_instantiates_relationship(
            extractor,
            node,
            symbols,
            relationships,
            caller_symbol,
            &called_function_name,
            target,
            file_path,
            line_number,
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn resolve_call_relationship(
    extractor: &mut PhpExtractor,
    node: Node,
    symbols: &[Symbol],
    relationships: &mut Vec<Relationship>,
    caller_symbol: &Symbol,
    target: UnresolvedTarget,
    file_path: String,
    line_number: u32,
) {
    let symbol_index = ScopedSymbolIndex::new(symbols);
    let enclosing_class = enclosing_class_name(extractor, node);
    let resolver_receiver = match target.receiver.as_deref() {
        Some("static") => Some("self"),
        Some(receiver)
            if node.kind() == "scoped_call_expression"
                && enclosing_class.as_deref() == Some(receiver) =>
        {
            Some("self")
        }
        receiver => receiver,
    };
    match symbol_index.resolve_call_target(
        &target.terminal_name,
        Some(caller_symbol),
        resolver_receiver,
    ) {
        LocalTargetResolution::Resolved(called_symbol) => {
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
        LocalTargetResolution::Import(_) => {
            add_pending_relationship(
                extractor,
                node,
                caller_symbol,
                target,
                RelationshipKind::Calls,
                0.8,
            );
        }
        LocalTargetResolution::Ambiguous
        | LocalTargetResolution::ReceiverQualified
        | LocalTargetResolution::Missing => {
            add_pending_relationship(
                extractor,
                node,
                caller_symbol,
                target,
                RelationshipKind::Calls,
                0.7,
            );
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn resolve_instantiates_relationship(
    extractor: &mut PhpExtractor,
    node: Node,
    symbols: &[Symbol],
    relationships: &mut Vec<Relationship>,
    caller_symbol: &Symbol,
    called_function_name: &str,
    target: UnresolvedTarget,
    file_path: String,
    line_number: u32,
) {
    let symbol_map: HashMap<String, &Symbol> = ScopedSymbolIndex::unique_symbol_map(symbols);
    match symbol_map.get(called_function_name) {
        Some(called_symbol)
            if !matches!(
                called_symbol.kind,
                SymbolKind::Class | SymbolKind::Interface | SymbolKind::Struct | SymbolKind::Enum
            ) =>
        {
            add_pending_relationship(
                extractor,
                node,
                caller_symbol,
                target,
                RelationshipKind::Instantiates,
                0.7,
            );
        }
        Some(called_symbol) if called_symbol.kind == SymbolKind::Import => {
            add_pending_relationship(
                extractor,
                node,
                caller_symbol,
                target,
                RelationshipKind::Instantiates,
                0.8,
            );
        }
        Some(called_symbol) => {
            relationships.push(Relationship {
                id: format!(
                    "{}_{}_{:?}_{}",
                    caller_symbol.id,
                    called_symbol.id,
                    RelationshipKind::Instantiates,
                    node.start_position().row
                ),
                from_symbol_id: caller_symbol.id.clone(),
                to_symbol_id: called_symbol.id.clone(),
                kind: RelationshipKind::Instantiates,
                file_path,
                line_number,
                span: Some(crate::base::NormalizedSpan::from_node(&node)),
                reference_site_is_exact: false,
                confidence: 0.9,
                metadata: None,
            });
        }
        None => {
            add_pending_relationship(
                extractor,
                node,
                caller_symbol,
                target,
                RelationshipKind::Instantiates,
                0.7,
            );
        }
    }
}

fn add_pending_relationship(
    extractor: &mut PhpExtractor,
    node: Node,
    caller_symbol: &Symbol,
    target: UnresolvedTarget,
    kind: RelationshipKind,
    confidence: f32,
) {
    let receiver_type = identifiers::php_call_receiver_type(extractor.get_base(), node);
    let pending = extractor
        .get_base()
        .create_pending_relationship_at_target(
            caller_symbol.id.clone(),
            target,
            kind,
            &call_site(node),
            Some(caller_symbol.id.clone()),
            Some(confidence),
        )
        .with_receiver_type(receiver_type);
    extractor.add_structured_pending_relationship(pending);
}

fn unresolved_call_target(
    extractor: &PhpExtractor,
    node: Node,
    fallback_name: &str,
) -> UnresolvedTarget {
    match node.kind() {
        "member_call_expression" | "nullsafe_member_call_expression" => {
            member_call_target(extractor, node, fallback_name)
        }
        "scoped_call_expression" => scoped_call_target(extractor, node, fallback_name),
        "object_creation_expression" => node
            .named_child(0)
            .map(|class_node| extractor.get_base().get_node_text(&class_node))
            .filter(|class_name| !matches!(class_name.trim(), "self" | "static"))
            .map(|class_name| unresolved_php_type_target(&class_name))
            .unwrap_or_else(|| unresolved_php_type_target(fallback_name)),
        _ => unresolved_php_type_target(fallback_name),
    }
}

/// The token that names the callee: the method name of a member or scoped
/// call, so each link of a multi-line chain sits on its own line.
fn call_site(node: Node) -> Node {
    match node.kind() {
        "member_call_expression" | "nullsafe_member_call_expression" | "scoped_call_expression" => {
            node.child_by_field_name("name").unwrap_or(node)
        }
        _ => node,
    }
}

/// A callee or class written as a name. `$cb()`, `$obj->$name()`, and
/// `new $class` name nothing statically.
fn is_static_name(node: Node) -> bool {
    matches!(node.kind(), "name" | "qualified_name" | "relative_name")
}

/// The class, enum, or trait whose body holds `node`; `self` and `static`
/// name it.
fn enclosing_class_name(extractor: &PhpExtractor, node: Node) -> Option<String> {
    let mut current = node.parent();
    while let Some(ancestor) = current {
        match ancestor.kind() {
            "class_declaration" | "enum_declaration" | "trait_declaration" => {
                return ancestor
                    .child_by_field_name("name")
                    .map(|name| extractor.get_base().get_node_text(&name));
            }
            "anonymous_class" => return None,
            _ => current = ancestor.parent(),
        }
    }
    None
}

/// The receiver of `$query->where()` is `query`, of `$this->repo->find()` is
/// `repo` under namespace `this`, and of `Order::where()` is `Order`. A call,
/// `new`, or other expression receiver has no name, as in Java: the target is
/// the bare method name.
fn member_call_target(
    extractor: &PhpExtractor,
    node: Node,
    fallback_name: &str,
) -> UnresolvedTarget {
    let terminal_name = node
        .child_by_field_name("name")
        .map(|name| extractor.get_base().get_node_text(&name))
        .unwrap_or_else(|| fallback_name.to_string());
    let path = node
        .child_by_field_name("object")
        .and_then(|object| receiver_path(extractor, object));
    let Some(mut path) = path else {
        return UnresolvedTarget::simple(terminal_name);
    };
    let receiver = path
        .pop()
        .map(|receiver| receiver.trim_start_matches('$').to_string());
    let display_name = path
        .iter()
        .chain(receiver.iter())
        .chain(std::iter::once(&terminal_name))
        .cloned()
        .collect::<Vec<_>>()
        .join(".");
    UnresolvedTarget {
        display_name,
        terminal_name,
        receiver,
        namespace_path: path,
        import_context: None,
    }
}

/// `$a->b->c` as `[$a, b, c]`. Only variables and property names count.
fn receiver_path(extractor: &PhpExtractor, node: Node) -> Option<Vec<String>> {
    match node.kind() {
        "variable_name" => Some(vec![extractor.get_base().get_node_text(&node)]),
        "member_access_expression" | "nullsafe_member_access_expression" => {
            let name = node.child_by_field_name("name")?;
            if name.kind() != "name" {
                return None;
            }
            let mut path = receiver_path(extractor, node.child_by_field_name("object")?)?;
            path.push(extractor.get_base().get_node_text(&name));
            Some(path)
        }
        _ => None,
    }
}

fn scoped_call_target(
    extractor: &PhpExtractor,
    node: Node,
    fallback_name: &str,
) -> UnresolvedTarget {
    let terminal_name = node
        .child_by_field_name("name")
        .map(|name| extractor.get_base().get_node_text(&name))
        .unwrap_or_else(|| fallback_name.to_string());
    let Some(scope) = node.child_by_field_name("scope") else {
        return UnresolvedTarget::simple(terminal_name);
    };
    let receiver_target = match scope.kind() {
        "name" | "qualified_name" | "relative_name" | "relative_scope" => {
            unresolved_php_type_target(&extractor.get_base().get_node_text(&scope))
        }
        "class_constant_access_expression" => {
            let mut cursor = scope.walk();
            let parts: Vec<String> = scope
                .named_children(&mut cursor)
                .map(|part| {
                    (part.kind() == "name").then(|| extractor.get_base().get_node_text(&part))
                })
                .collect::<Option<_>>()
                .unwrap_or_default();
            let Some((terminal, namespace)) = parts.split_last() else {
                return UnresolvedTarget::simple(terminal_name);
            };
            UnresolvedTarget {
                display_name: parts.join("\\"),
                terminal_name: terminal.clone(),
                receiver: None,
                namespace_path: namespace.to_vec(),
                import_context: None,
            }
        }
        "variable_name" => UnresolvedTarget::simple(
            extractor
                .get_base()
                .get_node_text(&scope)
                .trim_start_matches('$')
                .to_string(),
        ),
        _ => return UnresolvedTarget::simple(terminal_name),
    };
    let receiver = receiver_target.terminal_name;
    let display_name = receiver_target
        .namespace_path
        .iter()
        .chain([&receiver, &terminal_name])
        .cloned()
        .collect::<Vec<_>>()
        .join(".");
    UnresolvedTarget {
        display_name,
        terminal_name,
        receiver: Some(receiver),
        namespace_path: receiver_target.namespace_path,
        import_context: None,
    }
}
