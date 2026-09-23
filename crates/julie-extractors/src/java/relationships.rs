/// Inheritance, implementation, and call relationship extraction
use crate::base::{
    BaseExtractor, LocalTargetResolution, Relationship, RelationshipKind, ScopedSymbolIndex,
    Symbol, SymbolKind, UnresolvedTarget,
};
use crate::java::JavaExtractor;
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};
use std::collections::HashMap;
use tree_sitter::Node;

use super::helpers;

/// Extract inheritance relationships from a class, interface, enum or record
/// declaration. Targets drop type arguments and split qualified names, so
/// `Base<String>` targets `Base` and `java.io.Serializable` targets
/// `Serializable` in namespace `java.io`.
pub(super) fn extract_inheritance_relationships(
    extractor: &mut JavaExtractor,
    node: Node,
    symbols: &[Symbol],
    relationships: &mut Vec<Relationship>,
) {
    let Some(type_symbol) = find_type_symbol(extractor, node, symbols) else {
        return;
    };

    let mut supertypes: Vec<(RelationshipKind, Node)> = Vec::new();
    if let Some(superclass) = helpers::superclass_type_node(node) {
        supertypes.push((RelationshipKind::Extends, superclass));
    }
    for interface in helpers::type_list_nodes(node, "super_interfaces") {
        supertypes.push((RelationshipKind::Implements, interface));
    }
    for interface in helpers::type_list_nodes(node, "extends_interfaces") {
        supertypes.push((RelationshipKind::Extends, interface));
    }

    extract_suite_selection_relationships(extractor, node, type_symbol, symbols, relationships);

    let file_path = extractor.base().file_path.clone();
    let line_number = (node.start_position().row + 1) as u32;
    for (kind, type_node) in supertypes {
        let target = helpers::type_reference_target(extractor.base(), type_node);
        let local_base = target
            .receiver
            .is_none()
            .then(|| {
                symbols.iter().find(|s| {
                    s.name == target.terminal_name
                        && s.file_path == file_path
                        && match kind {
                            RelationshipKind::Implements => s.kind == SymbolKind::Interface,
                            _ => matches!(s.kind, SymbolKind::Class | SymbolKind::Interface),
                        }
                })
            })
            .flatten();

        if let Some(base_type_symbol) = local_base {
            let metadata_key = match kind {
                RelationshipKind::Implements => "interface",
                _ => "baseType",
            };
            relationships.push(Relationship {
                id: format!(
                    "{}_{}_{:?}_{}",
                    type_symbol.id,
                    base_type_symbol.id,
                    kind,
                    node.start_position().row
                ),
                from_symbol_id: type_symbol.id.clone(),
                to_symbol_id: base_type_symbol.id.clone(),
                kind,
                file_path: file_path.clone(),
                line_number,
                span: Some(crate::base::NormalizedSpan::from_node(&node)),
                reference_site_is_exact: false,
                confidence: 1.0,
                metadata: Some(HashMap::from([(
                    metadata_key.to_string(),
                    serde_json::Value::String(target.terminal_name),
                )])),
            });
        } else {
            let pending = extractor.base().create_pending_relationship(
                type_symbol.id.clone(),
                target,
                kind,
                &node,
                Some(type_symbol.id.clone()),
                Some(0.9),
            );
            extractor.add_structured_pending_relationship(pending);
        }
    }
}

/// A JUnit Platform `@SelectClasses({A.class, B.class})` suite references each
/// selected class: resolved when the class is declared in this file, pending
/// otherwise.
fn extract_suite_selection_relationships(
    extractor: &mut JavaExtractor,
    node: Node,
    suite: &Symbol,
    symbols: &[Symbol],
    relationships: &mut Vec<Relationship>,
) {
    let Some(modifiers) = node
        .children(&mut node.walk())
        .find(|child| child.kind() == "modifiers")
    else {
        return;
    };
    let mut selected = Vec::new();
    for annotation in modifiers.children(&mut modifiers.walk()) {
        let is_select_classes = annotation.kind() == "annotation"
            && annotation.child_by_field_name("name").is_some_and(|name| {
                let text = extractor.base().get_node_text(&name);
                text == "SelectClasses" || text.ends_with(".SelectClasses")
            });
        if !is_select_classes {
            continue;
        }
        let Some(arguments) = annotation.child_by_field_name("arguments") else {
            continue;
        };
        collect_class_literal_types(arguments, &mut selected, 0);
    }

    let file_path = extractor.base().file_path.clone();
    for type_node in selected {
        let target = helpers::type_reference_target(extractor.base(), type_node);
        let local = target
            .receiver
            .is_none()
            .then(|| {
                symbols.iter().find(|s| {
                    s.name == target.terminal_name
                        && s.file_path == file_path
                        && s.kind == SymbolKind::Class
                })
            })
            .flatten();
        match local {
            Some(selected_class) => relationships.push(Relationship {
                id: format!(
                    "{}_{}_{:?}_{}",
                    suite.id,
                    selected_class.id,
                    RelationshipKind::References,
                    type_node.start_byte()
                ),
                from_symbol_id: suite.id.clone(),
                to_symbol_id: selected_class.id.clone(),
                kind: RelationshipKind::References,
                file_path: file_path.clone(),
                line_number: (type_node.start_position().row + 1) as u32,
                span: Some(crate::base::NormalizedSpan::from_node(&type_node)),
                reference_site_is_exact: true,
                confidence: 1.0,
                metadata: None,
            }),
            None => {
                let pending = extractor.base().create_pending_relationship(
                    suite.id.clone(),
                    target,
                    RelationshipKind::References,
                    &type_node,
                    Some(suite.id.clone()),
                    Some(0.9),
                );
                extractor.add_structured_pending_relationship(pending);
            }
        }
    }
}

fn collect_class_literal_types<'tree>(node: Node<'tree>, out: &mut Vec<Node<'tree>>, depth: u32) {
    if !should_visit_tree_depth(depth) {
        return;
    }
    if node.kind() == "class_literal" {
        if let Some(type_node) = node.named_child(0) {
            out.push(type_node);
        }
        return;
    }
    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };
    for child in node.named_children(&mut node.walk()) {
        collect_class_literal_types(child, out, child_depth);
    }
}

/// The type symbol declared by this node, matched by span so that nested
/// types sharing a name each own their own edges.
fn find_type_symbol<'a>(
    extractor: &JavaExtractor,
    node: Node,
    symbols: &'a [Symbol],
) -> Option<&'a Symbol> {
    symbols.iter().find(|s| {
        s.start_byte == node.start_byte() as u32
            && s.end_byte == node.end_byte() as u32
            && matches!(
                s.kind,
                SymbolKind::Class | SymbolKind::Interface | SymbolKind::Enum
            )
            && s.file_path == extractor.base().file_path
    })
}

/// Extract method call relationships
///
/// Creates resolved Relationship when target is a local method.
/// Creates PendingRelationship when target is:
/// - An Import symbol (needs cross-file resolution)
/// - Not found in local symbol_map (e.g., method on imported type)
pub(super) fn extract_call_relationships(
    extractor: &mut JavaExtractor,
    node: Node,
    symbols: &[Symbol],
    relationships: &mut Vec<Relationship>,
    depth: u32,
) {
    let symbol_index = ScopedSymbolIndex::new(symbols);
    let symbol_map = ScopedSymbolIndex::unique_symbol_map(symbols);

    // Find method invocation nodes in this subtree
    walk_tree_for_calls(
        extractor,
        node,
        &symbol_index,
        &symbol_map,
        symbols,
        relationships,
        depth,
    );
}

fn walk_tree_for_calls(
    extractor: &mut JavaExtractor,
    node: Node,
    symbol_index: &ScopedSymbolIndex<'_>,
    symbol_map: &HashMap<String, &Symbol>,
    all_symbols: &[Symbol],
    relationships: &mut Vec<Relationship>,
    depth: u32,
) {
    if !should_visit_tree_depth(depth) {
        return;
    }

    let call = match node.kind() {
        "method_invocation" => method_invocation_target(extractor, node)
            .map(|target| (CallTarget::Method(target), None)),
        "method_reference" => method_reference_target(extractor, node),
        "object_creation_expression" => node
            .child_by_field_name("type")
            .map(|type_node| {
                CallTarget::Constructor(helpers::type_reference_target(extractor.base(), type_node))
            })
            .map(|target| (target, None)),
        "explicit_constructor_invocation" => explicit_constructor_target(extractor, node)
            .map(|target| (CallTarget::Constructor(target), None)),
        _ => None,
    };
    if let Some((target, receiver_type)) = call
        && let Some(caller) = find_caller(node, all_symbols, &extractor.base().file_path)
    {
        match target {
            CallTarget::Method(target) => emit_method_call(
                extractor,
                node,
                caller,
                target,
                receiver_type
                    .or_else(|| super::identifiers::self_receiver_type(extractor.base(), node)),
                symbol_index,
                relationships,
            ),
            CallTarget::Constructor(target) => {
                emit_constructor_call(extractor, node, caller, target, symbol_map, relationships)
            }
        }
    }

    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk_tree_for_calls(
            extractor,
            child,
            symbol_index,
            symbol_map,
            all_symbols,
            relationships,
            child_depth,
        );
    }
}

enum CallTarget {
    Method(UnresolvedTarget),
    Constructor(UnresolvedTarget),
}

/// The symbol that owns a call: the innermost method or constructor, else the
/// field, constant or enum constant whose initializer holds it, else the type
/// whose initializer block holds it.
fn find_caller<'a>(node: Node, symbols: &'a [Symbol], file_path: &str) -> Option<&'a Symbol> {
    let tiers: [fn(&SymbolKind) -> bool; 3] = [
        |kind| {
            matches!(
                kind,
                SymbolKind::Function | SymbolKind::Method | SymbolKind::Constructor
            )
        },
        |kind| {
            matches!(
                kind,
                SymbolKind::Property | SymbolKind::Constant | SymbolKind::EnumMember
            )
        },
        |kind| {
            matches!(
                kind,
                SymbolKind::Class | SymbolKind::Interface | SymbolKind::Enum
            )
        },
    ];
    tiers.iter().find_map(|in_tier| {
        BaseExtractor::find_containing_symbol_from_iter(
            &node,
            symbols
                .iter()
                .filter(|symbol| symbol.file_path == file_path && in_tier(&symbol.kind)),
        )
    })
}

fn method_invocation_target(extractor: &JavaExtractor, node: Node) -> Option<UnresolvedTarget> {
    let base = extractor.base();
    let method_name = node
        .children(&mut node.walk())
        .filter(|child| child.kind() == "identifier")
        .last()
        .map(|child| base.get_node_text(&child))?;
    Some(unresolved_call_target(extractor, node, &method_name))
}

/// `this::handle`, `User::getName`, `System.out::println` call the member;
/// `Type::new` calls the constructor of `Type`.
fn method_reference_target(
    extractor: &JavaExtractor,
    node: Node,
) -> Option<(CallTarget, Option<String>)> {
    let base = extractor.base();
    let object = node.named_child(0)?;
    let member = node.child((node.child_count() as u32).checked_sub(1)?)?;
    if member.kind() == "new" {
        return Some((
            CallTarget::Constructor(helpers::type_reference_target(base, object)),
            None,
        ));
    }
    if member.kind() != "identifier" || member.id() == object.id() {
        return None;
    }
    let name = base.get_node_text(&member);
    let target = match object.kind() {
        "identifier"
        | "type_identifier"
        | "field_access"
        | "scoped_type_identifier"
        | "generic_type" => {
            let object_text = base.get_node_text(&helpers::generic_base_node(object));
            UnresolvedTarget::from_qualified_text(&format!("{object_text}.{name}"), &["."])
                .unwrap_or_else(|| UnresolvedTarget::simple(name))
        }
        _ => UnresolvedTarget::simple(name),
    };
    Some((
        CallTarget::Method(target),
        super::identifiers::method_reference_receiver_type(base, node),
    ))
}

/// `this(...)` calls a constructor of the enclosing class and `super(...)` a
/// constructor of its superclass.
fn explicit_constructor_target(extractor: &JavaExtractor, node: Node) -> Option<UnresolvedTarget> {
    let base = extractor.base();
    let constructor = node.child_by_field_name("constructor")?;
    let declaration = std::iter::successors(node.parent(), |n| n.parent()).find(|n| {
        matches!(
            n.kind(),
            "class_declaration" | "enum_declaration" | "record_declaration"
        )
    })?;
    match constructor.kind() {
        "this" => declaration
            .child_by_field_name("name")
            .map(|name| UnresolvedTarget::simple(base.get_node_text(&name))),
        "super" => helpers::superclass_type_node(declaration)
            .map(|superclass| helpers::type_reference_target(base, superclass)),
        _ => None,
    }
}

fn emit_method_call(
    extractor: &mut JavaExtractor,
    node: Node,
    caller: &Symbol,
    target: UnresolvedTarget,
    receiver_type: Option<String>,
    symbol_index: &ScopedSymbolIndex<'_>,
    relationships: &mut Vec<Relationship>,
) {
    match symbol_index.resolve_call_target(
        &target.terminal_name,
        Some(caller),
        target.receiver.as_deref(),
    ) {
        LocalTargetResolution::Resolved(called_symbol) => {
            push_call(extractor, node, caller, called_symbol, relationships);
        }
        resolution => {
            let confidence = match resolution {
                LocalTargetResolution::Import(_) => 0.8,
                _ => 0.7,
            };
            let pending = extractor
                .base()
                .create_pending_relationship(
                    caller.id.clone(),
                    target,
                    RelationshipKind::Calls,
                    &node,
                    Some(caller.id.clone()),
                    Some(confidence),
                )
                .with_receiver_type(receiver_type);
            extractor.add_structured_pending_relationship(pending);
        }
    }
}

fn emit_constructor_call(
    extractor: &mut JavaExtractor,
    node: Node,
    caller: &Symbol,
    target: UnresolvedTarget,
    symbol_map: &HashMap<String, &Symbol>,
    relationships: &mut Vec<Relationship>,
) {
    let local = target
        .receiver
        .is_none()
        .then(|| symbol_map.get(target.terminal_name.as_str()).copied())
        .flatten();
    match local {
        Some(called_symbol) if called_symbol.kind != SymbolKind::Import => {
            push_call(extractor, node, caller, called_symbol, relationships);
        }
        local => {
            let confidence = if local.is_some() { 0.8 } else { 0.7 };
            let pending = extractor.base().create_pending_relationship(
                caller.id.clone(),
                target,
                RelationshipKind::Calls,
                &node,
                Some(caller.id.clone()),
                Some(confidence),
            );
            extractor.add_structured_pending_relationship(pending);
        }
    }
}

fn push_call(
    extractor: &JavaExtractor,
    node: Node,
    caller: &Symbol,
    called_symbol: &Symbol,
    relationships: &mut Vec<Relationship>,
) {
    relationships.push(Relationship {
        id: format!(
            "{}_{}_{:?}_{}",
            caller.id,
            called_symbol.id,
            RelationshipKind::Calls,
            node.start_position().row
        ),
        from_symbol_id: caller.id.clone(),
        to_symbol_id: called_symbol.id.clone(),
        kind: RelationshipKind::Calls,
        file_path: extractor.base().file_path.clone(),
        line_number: node.start_position().row as u32 + 1,
        span: Some(crate::base::NormalizedSpan::from_node(&node)),
        reference_site_is_exact: false,
        confidence: 0.9,
        metadata: None,
    });
}

fn unresolved_call_target(
    extractor: &JavaExtractor,
    node: Node,
    fallback_name: &str,
) -> UnresolvedTarget {
    let base = extractor.base();
    let mut parts = Vec::new();
    if let Some(name) = node
        .child_by_field_name("name")
        .filter(|name| name.kind() == "identifier")
    {
        parts.push(base.get_node_text(&name));
    }
    let mut object = node.child_by_field_name("object");
    while let Some(current) = object {
        match current.kind() {
            "identifier" => {
                parts.push(base.get_node_text(&current));
                break;
            }
            "field_access" => {
                let Some(field) = current
                    .child_by_field_name("field")
                    .filter(|field| field.kind() == "identifier")
                else {
                    break;
                };
                parts.push(base.get_node_text(&field));
                object = current.child_by_field_name("object");
            }
            _ => break,
        }
    }
    parts.reverse();

    if parts.len() >= 2 {
        return UnresolvedTarget::from_chain(parts);
    }

    UnresolvedTarget::simple(fallback_name.to_string())
}
