use super::helpers;
use crate::base::{
    LocalTargetResolution, Relationship, RelationshipKind, ScopedSymbolIndex, Symbol, SymbolKind,
    UnresolvedTarget,
};
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};
use crate::vbnet::VbNetExtractor;
use tree_sitter::Tree;

pub fn extract_relationships(
    extractor: &mut VbNetExtractor,
    tree: &Tree,
    symbols: &[Symbol],
) -> Vec<Relationship> {
    let mut relationships = Vec::new();
    visit_relationships(extractor, tree.root_node(), symbols, &mut relationships, 0);
    relationships
}

fn visit_relationships(
    extractor: &mut VbNetExtractor,
    node: tree_sitter::Node,
    symbols: &[Symbol],
    relationships: &mut Vec<Relationship>,
    depth: u32,
) {
    if !should_visit_tree_depth(depth) {
        return;
    }

    match node.kind() {
        "class_block" | "structure_block" => {
            extract_type_relationships(extractor, node, symbols, relationships);
        }
        "interface_block" => {
            extract_interface_relationships(extractor, node, symbols, relationships);
        }
        "method_declaration"
        | "abstract_method_declaration"
        | "property_declaration"
        | "event_declaration" => {
            extract_member_implements(extractor, node, symbols, relationships);
            if node.kind() == "property_declaration" {
                extract_property_type_relationships(extractor, node, symbols, relationships);
            }
        }
        "constructor_declaration" => {
            extract_constructor_uses_relationships(extractor, node, symbols, relationships);
        }
        "field_declaration" => {
            extract_field_type_relationships(extractor, node, symbols, relationships);
        }
        "invocation_expression" | "invocation" | "element_access" | "call_statement" => {
            extract_call_relationships(extractor, node, symbols, relationships);
        }
        "member_access" if helpers::misparsed_new_root(node).is_some() => {
            extract_misparsed_new_member_access(extractor, node, symbols, relationships);
        }
        "new_expression" | "object_creation_expression" => {
            extract_new_expression_relationships(extractor, node, symbols, relationships);
        }
        _ => {}
    }

    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        visit_relationships(extractor, child, symbols, relationships, child_depth);
    }
}

fn extract_type_relationships(
    extractor: &mut VbNetExtractor,
    node: tree_sitter::Node,
    symbols: &[Symbol],
    relationships: &mut Vec<Relationship>,
) {
    let Some(current) = find_containing_type(extractor, node, symbols) else {
        return;
    };
    let (inherits, implements) = {
        let base = extractor.get_base();
        (
            helpers::extract_inherits(base, &node),
            helpers::extract_implements(base, &node),
        )
    };
    let current_id = current.id.clone();
    for (names, kind) in [
        (inherits, RelationshipKind::Extends),
        (implements, RelationshipKind::Implements),
    ] {
        for type_name in names {
            emit_type_edge(
                extractor,
                node,
                &current_id,
                &type_name,
                kind.clone(),
                symbols,
                relationships,
            );
        }
    }
}

fn extract_interface_relationships(
    extractor: &mut VbNetExtractor,
    node: tree_sitter::Node,
    symbols: &[Symbol],
    relationships: &mut Vec<Relationship>,
) {
    let Some(current) = find_containing_type(extractor, node, symbols) else {
        return;
    };
    let current_id = current.id.clone();
    let inherits = helpers::extract_inherits(extractor.get_base(), &node);
    for type_name in inherits {
        emit_type_edge(
            extractor,
            node,
            &current_id,
            &type_name,
            RelationshipKind::Extends,
            symbols,
            relationships,
        );
    }
}

/// Resolves a base-list entry against the file's type symbols by its
/// generic-stripped terminal name, else leaves a pending edge.
fn emit_type_edge(
    extractor: &mut VbNetExtractor,
    node: tree_sitter::Node,
    current_id: &str,
    type_name: &str,
    kind: RelationshipKind,
    symbols: &[Symbol],
    relationships: &mut Vec<Relationship>,
) {
    let target = helpers::unresolved_type_target(type_name)
        .unwrap_or_else(|| UnresolvedTarget::simple(type_name.to_string()));
    match find_vb_type_symbol(symbols, &target.terminal_name).filter(|s| s.id != current_id) {
        Some(type_symbol) => relationships.push(Relationship {
            id: format!(
                "{}_{}_{:?}_{}",
                current_id,
                type_symbol.id,
                kind,
                node.start_position().row
            ),
            from_symbol_id: current_id.to_string(),
            to_symbol_id: type_symbol.id.clone(),
            kind,
            file_path: extractor.get_base().file_path.clone(),
            line_number: (node.start_position().row + 1) as u32,
            span: Some(crate::base::NormalizedSpan::from_node(&node)),
            reference_site_is_exact: false,
            confidence: 1.0,
            metadata: None,
        }),
        None => {
            let pending = extractor.get_base().create_pending_relationship(
                current_id.to_string(),
                target,
                kind,
                &node,
                Some(current_id.to_string()),
                Some(0.9),
            );
            extractor.add_structured_pending_relationship(pending);
        }
    }
}

/// `Sub DoWork() Implements IJob.Run` links the member to the interface
/// member it implements: resolved when that member is in the file, else a
/// pending edge whose receiver is the interface.
fn extract_member_implements(
    extractor: &mut VbNetExtractor,
    node: tree_sitter::Node,
    symbols: &[Symbol],
    relationships: &mut Vec<Relationship>,
) {
    let Some(clause) = node.child_by_field_name("implements") else {
        return;
    };
    let start = node.start_byte() as u32;
    let Some(member) = symbols.iter().find(|symbol| {
        symbol.start_byte == start
            && matches!(
                symbol.kind,
                SymbolKind::Method | SymbolKind::Property | SymbolKind::Event
            )
    }) else {
        return;
    };
    let member_id = member.id.clone();
    let mut cursor = clause.walk();
    let targets: Vec<Vec<String>> = clause
        .named_children(&mut cursor)
        .filter(|child| child.kind() == "namespace_name")
        .map(|name| {
            let mut name_cursor = name.walk();
            name.named_children(&mut name_cursor)
                .filter(|part| part.kind() == "identifier")
                .map(|part| extractor.get_base().get_node_text(&part))
                .collect()
        })
        .collect();
    for parts in targets {
        let [.., interface_name, member_name] = parts.as_slice() else {
            continue;
        };
        let implemented = find_vb_type_symbol(symbols, interface_name).and_then(|interface| {
            symbols.iter().find(|candidate| {
                candidate.parent_id.as_deref() == Some(interface.id.as_str())
                    && candidate.name.eq_ignore_ascii_case(member_name)
            })
        });
        match implemented {
            Some(target) => relationships.push(Relationship {
                id: format!(
                    "{}_{}_{:?}_{}",
                    member_id,
                    target.id,
                    RelationshipKind::Implements,
                    clause.start_position().row
                ),
                from_symbol_id: member_id.clone(),
                to_symbol_id: target.id.clone(),
                kind: RelationshipKind::Implements,
                file_path: extractor.get_base().file_path.clone(),
                line_number: clause.start_position().row as u32 + 1,
                span: Some(crate::base::NormalizedSpan::from_node(&clause)),
                reference_site_is_exact: false,
                confidence: 1.0,
                metadata: None,
            }),
            None => {
                let pending = extractor.get_base().create_pending_relationship(
                    member_id.clone(),
                    UnresolvedTarget::from_chain(parts.clone()),
                    RelationshipKind::Implements,
                    &clause,
                    Some(member_id.clone()),
                    Some(0.9),
                );
                extractor.add_structured_pending_relationship(pending);
            }
        }
    }
}

fn extract_constructor_uses_relationships(
    extractor: &mut VbNetExtractor,
    node: tree_sitter::Node,
    symbols: &[Symbol],
    relationships: &mut Vec<Relationship>,
) {
    let parameter_list = node.child_by_field_name("parameters");
    let Some(parameter_list) = parameter_list else {
        return;
    };

    let Some(container) = find_containing_type(extractor, node, symbols) else {
        return;
    };

    let mut cursor = parameter_list.walk();
    for parameter in parameter_list.children(&mut cursor) {
        if parameter.kind() != "parameter" {
            continue;
        }
        if let Some(type_name) = helpers::extract_as_clause_type(extractor.get_base(), &parameter) {
            emit_uses_relationship(
                extractor,
                node,
                &container.id,
                &type_name,
                symbols,
                relationships,
            );
        }
    }
}

fn extract_field_type_relationships(
    extractor: &mut VbNetExtractor,
    node: tree_sitter::Node,
    symbols: &[Symbol],
    relationships: &mut Vec<Relationship>,
) {
    let Some(container) = find_containing_type(extractor, node, symbols) else {
        return;
    };

    let mut cursor = node.walk();
    let declarator = node
        .children(&mut cursor)
        .find(|child| child.kind() == "variable_declarator");
    let Some(declarator) = declarator else {
        return;
    };

    if let Some(type_name) = helpers::extract_as_clause_type(extractor.get_base(), &declarator) {
        emit_uses_relationship(
            extractor,
            node,
            &container.id,
            &type_name,
            symbols,
            relationships,
        );
    }
}

fn extract_property_type_relationships(
    extractor: &mut VbNetExtractor,
    node: tree_sitter::Node,
    symbols: &[Symbol],
    relationships: &mut Vec<Relationship>,
) {
    let Some(container) = find_containing_type(extractor, node, symbols) else {
        return;
    };

    if let Some(type_name) = helpers::extract_as_clause_type(extractor.get_base(), &node) {
        emit_uses_relationship(
            extractor,
            node,
            &container.id,
            &type_name,
            symbols,
            relationships,
        );
    }
}

fn find_containing_type<'a>(
    extractor: &VbNetExtractor,
    node: tree_sitter::Node,
    symbols: &'a [Symbol],
) -> Option<&'a Symbol> {
    let base = extractor.get_base();
    let mut current = Some(node);
    while let Some(candidate) = current {
        let expected_kind = match candidate.kind() {
            "class_block" | "module_block" => SymbolKind::Class,
            "structure_block" => SymbolKind::Struct,
            "interface_block" => SymbolKind::Interface,
            _ => {
                current = candidate.parent();
                continue;
            }
        };

        if let Some(name_node) = candidate.child_by_field_name("name") {
            let type_name = base.get_node_text(&name_node);
            let start_line = candidate.start_position().row as u32 + 1;

            if let Some(symbol) = symbols.iter().find(|symbol| {
                symbol.name == type_name
                    && symbol.kind == expected_kind
                    && symbol.file_path == base.file_path
                    && symbol.start_line == start_line
            }) {
                return Some(symbol);
            }

            return symbols.iter().find(|symbol| {
                symbol.name == type_name
                    && symbol.kind == expected_kind
                    && symbol.file_path == base.file_path
            });
        }

        current = candidate.parent();
    }

    None
}

fn emit_uses_relationship(
    extractor: &mut VbNetExtractor,
    node: tree_sitter::Node,
    container_symbol_id: &str,
    type_name: &str,
    symbols: &[Symbol],
    relationships: &mut Vec<Relationship>,
) {
    let Some(target) = helpers::unresolved_type_target(type_name) else {
        return;
    };

    let already_resolved = relationships.iter().any(|relationship| {
        relationship.from_symbol_id == container_symbol_id
            && relationship.kind == RelationshipKind::Uses
            && (relationship.to_symbol_id == target.terminal_name
                || symbols
                    .iter()
                    .find(|symbol| symbol.id == relationship.to_symbol_id)
                    .is_some_and(|symbol| symbol.name.eq_ignore_ascii_case(&target.terminal_name)))
    });
    if already_resolved {
        return;
    }

    let already_pending = extractor.get_pending_relationships().iter().any(|pending| {
        pending.from_symbol_id == container_symbol_id
            && pending.kind == RelationshipKind::Uses
            && pending.callee_name == target.display_name
    });
    if already_pending {
        return;
    }

    match find_vb_type_symbol(symbols, &target.terminal_name) {
        Some(type_symbol) => {
            relationships.push(Relationship {
                id: format!(
                    "{}_{}_{:?}_{}",
                    container_symbol_id,
                    type_symbol.id,
                    RelationshipKind::Uses,
                    node.start_position().row
                ),
                from_symbol_id: container_symbol_id.to_string(),
                to_symbol_id: type_symbol.id.clone(),
                kind: RelationshipKind::Uses,
                file_path: extractor.get_base().file_path.clone(),
                line_number: node.start_position().row as u32 + 1,
                span: Some(crate::base::NormalizedSpan::from_node(&node)),
                reference_site_is_exact: false,
                confidence: 0.9,
                metadata: None,
            });
        }
        None => {
            let pending = extractor.get_base().create_pending_relationship(
                container_symbol_id.to_string(),
                target,
                RelationshipKind::Uses,
                &node,
                Some(container_symbol_id.to_string()),
                Some(0.9),
            );
            extractor.add_structured_pending_relationship(pending);
        }
    }
}

fn extract_call_relationships(
    extractor: &mut VbNetExtractor,
    node: tree_sitter::Node,
    symbols: &[Symbol],
    relationships: &mut Vec<Relationship>,
) {
    let Some(callee) = helpers::call_callee(node) else {
        return;
    };
    if let Some(type_name) = helpers::misparsed_new_type_name(extractor.get_base(), callee) {
        emit_instantiation(extractor, node, &type_name, symbols, relationships);
        return;
    }
    let method_name = {
        let base = extractor.get_base();
        match callee.kind() {
            "identifier" => base.get_node_text(&callee),
            _ => callee
                .child_by_field_name("member")
                .map(|member| base.get_node_text(&member))
                .unwrap_or_default(),
        }
    };

    if method_name.is_empty() {
        return;
    }
    if is_mybase_member(extractor.get_base(), callee) {
        extract_mybase_call(extractor, node, &method_name, symbols, relationships);
        return;
    }
    if helpers::is_indexed_value(extractor.get_base(), callee, symbols) {
        return;
    }

    let base = extractor.get_base();
    let symbol_index = ScopedSymbolIndex::new(symbols);
    let target = unresolved_call_target(extractor, callee, &method_name);
    let receiver_type = super::identifiers::self_receiver_type(base, node);
    let Some(caller) = find_caller(base, node, symbols) else {
        return;
    };

    let line_number = node.start_position().row as u32 + 1;
    let file_path = base.file_path.clone();

    match symbol_index.resolve_call_target(
        &target.terminal_name,
        Some(caller),
        target.receiver.as_deref(),
    ) {
        LocalTargetResolution::Resolved(called_symbol) => {
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
                file_path,
                line_number,
                span: Some(crate::base::NormalizedSpan::from_node(&node)),
                reference_site_is_exact: false,
                confidence: 0.9,
                metadata: None,
            });
        }
        LocalTargetResolution::Import(_) => {
            let pending = extractor
                .get_base()
                .create_pending_relationship(
                    caller.id.clone(),
                    target,
                    RelationshipKind::Calls,
                    &node,
                    Some(caller.id.clone()),
                    Some(0.8),
                )
                .with_receiver_type(receiver_type.clone());
            extractor.add_structured_pending_relationship(pending);
        }
        LocalTargetResolution::Ambiguous
        | LocalTargetResolution::ReceiverQualified
        | LocalTargetResolution::Missing => {
            if let Some(called_symbol) =
                find_vb_case_insensitive_call_target(symbols, &target, caller)
            {
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
                    file_path,
                    line_number,
                    span: Some(crate::base::NormalizedSpan::from_node(&node)),
                    reference_site_is_exact: false,
                    confidence: 0.9,
                    metadata: None,
                });
                return;
            }

            let pending = extractor
                .get_base()
                .create_pending_relationship(
                    caller.id.clone(),
                    target,
                    RelationshipKind::Calls,
                    &node,
                    Some(caller.id.clone()),
                    Some(0.7),
                )
                .with_receiver_type(receiver_type);
            extractor.add_structured_pending_relationship(pending);
        }
    }
}

fn is_mybase_member(base: &crate::base::BaseExtractor, callee: tree_sitter::Node) -> bool {
    callee.kind() == "member_access"
        && callee.child_by_field_name("object").is_some_and(|object| {
            object.kind() == "me_expression"
                && base.get_node_text(&object).eq_ignore_ascii_case("MyBase")
        })
}

/// `MyBase.M()` targets the inherited `M`, never the calling override: it
/// resolves when the base type and its member are in the file, else it stays
/// pending with the `MyBase` receiver and the declared base type.
fn extract_mybase_call(
    extractor: &mut VbNetExtractor,
    node: tree_sitter::Node,
    method_name: &str,
    symbols: &[Symbol],
    relationships: &mut Vec<Relationship>,
) {
    let base = extractor.get_base();
    let Some(caller) = find_caller(base, node, symbols) else {
        return;
    };
    let receiver_type = super::identifiers::self_receiver_type(base, node);
    let base_member = receiver_type.as_deref().and_then(|type_name| {
        let owner = find_vb_type_symbol(symbols, type_name)?;
        symbols.iter().find(|candidate| {
            candidate.name.eq_ignore_ascii_case(method_name)
                && candidate.id != caller.id
                && candidate.parent_id.as_deref() == Some(owner.id.as_str())
        })
    });
    if let Some(called_symbol) = base_member {
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
            file_path: base.file_path.clone(),
            line_number: node.start_position().row as u32 + 1,
            span: Some(crate::base::NormalizedSpan::from_node(&node)),
            reference_site_is_exact: false,
            confidence: 0.9,
            metadata: None,
        });
        return;
    }
    let caller_id = caller.id.clone();
    let target = UnresolvedTarget::from_chain(vec!["MyBase".to_string(), method_name.to_string()]);
    let pending = base
        .create_pending_relationship(
            caller_id.clone(),
            target,
            RelationshipKind::Calls,
            &node,
            Some(caller_id),
            Some(0.7),
        )
        .with_receiver_type(receiver_type);
    extractor.add_structured_pending_relationship(pending);
}

fn extract_new_expression_relationships(
    extractor: &mut VbNetExtractor,
    node: tree_sitter::Node,
    symbols: &[Symbol],
    relationships: &mut Vec<Relationship>,
) {
    if is_misparsed_new_root(node) {
        return;
    }
    let type_name = {
        let base = extractor.get_base();
        node.child_by_field_name("type")
            .and_then(|type_node| helpers::type_name_text(base, type_node))
    };
    let Some(type_name) = type_name else {
        return;
    };
    emit_instantiation(extractor, node, &type_name, symbols, relationships);
}

/// True for the bare `New A` at the root of a misparsed `New A.B.C` chain;
/// the outermost member access or invocation of the chain owns the edge.
fn is_misparsed_new_root(node: tree_sitter::Node) -> bool {
    helpers::is_bare_new(node)
        && node.parent().is_some_and(|parent| {
            parent.kind() == "member_access"
                && parent
                    .child_by_field_name("object")
                    .is_some_and(|object| object.id() == node.id())
        })
}

/// A bare member access that ends a misparsed `New A.B.C` chain without an
/// argument list (`Dim x = New A.B.C`).
fn extract_misparsed_new_member_access(
    extractor: &mut VbNetExtractor,
    node: tree_sitter::Node,
    symbols: &[Symbol],
    relationships: &mut Vec<Relationship>,
) {
    let is_outermost = node.parent().is_none_or(|parent| {
        parent.kind() != "member_access" && helpers::call_callee(parent).is_none()
    });
    if !is_outermost {
        return;
    }
    if let Some(type_name) = helpers::misparsed_new_type_name(extractor.get_base(), node) {
        emit_instantiation(extractor, node, &type_name, symbols, relationships);
    }
}

fn emit_instantiation(
    extractor: &mut VbNetExtractor,
    node: tree_sitter::Node,
    type_name: &str,
    symbols: &[Symbol],
    relationships: &mut Vec<Relationship>,
) {
    let Some(target) = helpers::unresolved_type_target(type_name) else {
        return;
    };

    let Some(caller) = find_caller(extractor.get_base(), node, symbols).cloned() else {
        return;
    };

    if let Some(type_symbol) = find_vb_type_symbol(symbols, &target.terminal_name) {
        relationships.push(Relationship {
            id: format!(
                "{}_{}_{:?}_{}",
                caller.id,
                type_symbol.id,
                RelationshipKind::Instantiates,
                node.start_position().row
            ),
            from_symbol_id: caller.id.clone(),
            to_symbol_id: type_symbol.id.clone(),
            kind: RelationshipKind::Instantiates,
            file_path: extractor.get_base().file_path.clone(),
            line_number: node.start_position().row as u32 + 1,
            span: Some(crate::base::NormalizedSpan::from_node(&node)),
            reference_site_is_exact: false,
            confidence: 0.9,
            metadata: None,
        });
        return;
    }

    let pending = extractor.get_base().create_pending_relationship(
        caller.id.clone(),
        target,
        RelationshipKind::Instantiates,
        &node,
        Some(caller.id.clone()),
        Some(0.9),
    );
    extractor.add_structured_pending_relationship(pending);
}

fn find_vb_type_symbol<'a>(symbols: &'a [Symbol], type_name: &str) -> Option<&'a Symbol> {
    symbols.iter().find(|symbol| {
        symbol.name.eq_ignore_ascii_case(type_name)
            && matches!(
                symbol.kind,
                SymbolKind::Class
                    | SymbolKind::Interface
                    | SymbolKind::Struct
                    | SymbolKind::Enum
                    | SymbolKind::Trait
                    | SymbolKind::Type
            )
    })
}

fn find_vb_case_insensitive_call_target<'a>(
    symbols: &'a [Symbol],
    target: &UnresolvedTarget,
    caller: &Symbol,
) -> Option<&'a Symbol> {
    if target.receiver.is_some() {
        return None;
    }

    let mut matches = symbols.iter().filter(|symbol| {
        symbol.file_path == caller.file_path
            && symbol.name.eq_ignore_ascii_case(&target.terminal_name)
            && matches!(
                symbol.kind,
                SymbolKind::Function | SymbolKind::Method | SymbolKind::Constructor
            )
    });

    let first = matches.next()?;
    if matches.next().is_some() {
        None
    } else {
        Some(first)
    }
}

fn unresolved_call_target(
    extractor: &VbNetExtractor,
    node: tree_sitter::Node,
    fallback_name: &str,
) -> UnresolvedTarget {
    let callee_expression = match node.kind() {
        "invocation_expression" | "invocation" => {
            let mut cursor = node.walk();
            node.children(&mut cursor).next()
        }
        _ => Some(node),
    };

    let Some(callee_expression) = callee_expression else {
        return UnresolvedTarget::simple(fallback_name.to_string());
    };

    let mut identifiers = Vec::new();
    if callee_expression.kind() == "implicit_member_access"
        && let Some(with_target) = helpers::with_target(callee_expression)
    {
        collect_spine(extractor, with_target, &mut identifiers);
    }
    collect_spine(extractor, callee_expression, &mut identifiers);

    if identifiers.len() >= 2 {
        let terminal_name = identifiers
            .pop()
            .unwrap_or_else(|| fallback_name.to_string());
        let receiver = identifiers.pop();
        let namespace_path = identifiers;
        let mut display_parts = namespace_path.clone();
        if let Some(receiver_name) = receiver.as_ref() {
            display_parts.push(receiver_name.clone());
        }
        display_parts.push(terminal_name.clone());
        return UnresolvedTarget {
            display_name: display_parts.join("."),
            terminal_name,
            receiver,
            namespace_path,
            import_context: None,
        };
    }

    UnresolvedTarget::simple(fallback_name.to_string())
}

/// Collects the dotted name spine of a callee (`A.B.C`). A call result,
/// index, or any other expression in the object position breaks the spine,
/// so argument names never become target segments.
fn collect_spine(extractor: &VbNetExtractor, node: tree_sitter::Node, segments: &mut Vec<String>) {
    collect_spine_at_depth(extractor, node, segments, 0);
}

fn collect_spine_at_depth(
    extractor: &VbNetExtractor,
    node: tree_sitter::Node,
    segments: &mut Vec<String>,
    depth: u32,
) {
    if !should_visit_tree_depth(depth) {
        segments.clear();
        return;
    }
    match node.kind() {
        "identifier" => segments.push(extractor.get_base().get_node_text(&node)),
        "me_expression" => {}
        "member_access" | "null_conditional_member_access" | "implicit_member_access" => {
            if let Some(object) = node.child_by_field_name("object")
                && let Some(child_depth) = child_tree_depth(depth)
            {
                collect_spine_at_depth(extractor, object, segments, child_depth);
            }
            match node.child_by_field_name("member") {
                Some(member) => segments.push(extractor.get_base().get_node_text(&member)),
                None => segments.clear(),
            }
        }
        _ => segments.clear(),
    }
}

/// The symbol a call or instantiation belongs to: the enclosing method,
/// constructor, operator, property, or event; otherwise (a field
/// initializer) the enclosing type.
fn find_caller<'a>(
    base: &crate::base::BaseExtractor,
    node: tree_sitter::Node,
    symbols: &'a [Symbol],
) -> Option<&'a Symbol> {
    helpers::enclosing_member_symbol(node, symbols).or_else(|| {
        base.find_containing_symbol(&node, symbols)
            .filter(|symbol| {
                matches!(
                    symbol.kind,
                    SymbolKind::Function
                        | SymbolKind::Method
                        | SymbolKind::Constructor
                        | SymbolKind::Class
                        | SymbolKind::Struct
                )
            })
    })
}
