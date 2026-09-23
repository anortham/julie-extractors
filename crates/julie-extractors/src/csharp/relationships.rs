// C# Relationship Extraction

use super::partial_classes;
use crate::base::{
    LocalTargetResolution, Relationship, RelationshipKind, ScopedSymbolIndex, Symbol, SymbolKind,
    UnresolvedTarget,
};
use crate::csharp::CSharpExtractor;
use crate::csharp::member_type_relationships::{
    extract_field_type_relationships, extract_parameter_type_name,
    extract_property_type_relationships, find_containing_class,
};
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};
use tree_sitter::Tree;

/// Extract relationships from the tree
pub fn extract_relationships(
    extractor: &mut CSharpExtractor,
    tree: &Tree,
    symbols: &[Symbol],
) -> Vec<Relationship> {
    let mut relationships = Vec::new();
    let sites = CallSites {
        owners: super::scope::MemberScope::new(
            tree.root_node(),
            symbols,
            &extractor.get_base().file_path,
        ),
        targets: ScopedSymbolIndex::new(symbols),
    };
    visit_relationships(
        extractor,
        tree.root_node(),
        symbols,
        &sites,
        &mut relationships,
        0,
    );
    partial_classes::add_linkage_relationships(symbols, &mut relationships);
    relationships
}

/// Indexes over the file's symbols, built once per relationship pass.
struct CallSites<'a> {
    /// The symbol that owns a call or instantiation site; the identifier pass
    /// uses the same scope, so both agree on one containing symbol per site.
    owners: super::scope::MemberScope<'a>,
    targets: ScopedSymbolIndex<'a>,
}

fn visit_relationships(
    extractor: &mut CSharpExtractor,
    node: tree_sitter::Node,
    symbols: &[Symbol],
    sites: &CallSites<'_>,
    relationships: &mut Vec<Relationship>,
    depth: u32,
) {
    if !should_visit_tree_depth(depth) {
        return;
    }

    match node.kind() {
        "class_declaration" | "struct_declaration" | "record_declaration" => {
            extract_inheritance_relationships(extractor, node, symbols, relationships);
            extract_constructor_parameter_relationships(extractor, node, symbols, relationships);
        }
        "interface_declaration" => {
            extract_inheritance_relationships(extractor, node, symbols, relationships);
        }
        "constructor_declaration" => {
            extract_constructor_parameter_relationships(extractor, node, symbols, relationships);
        }
        "field_declaration" => {
            extract_field_type_relationships(extractor, node, symbols, relationships);
        }
        "property_declaration" => {
            extract_property_type_relationships(extractor, node, symbols, relationships);
        }
        "invocation_expression" => {
            crate::csharp::di_relationships::extract_di_registration_relationships(
                extractor,
                node,
                symbols,
                relationships,
            );
            extract_call_relationships(extractor, node, symbols, sites, relationships);
        }
        "object_creation_expression" | "implicit_object_creation_expression" => {
            extract_object_creation_relationships(extractor, node, symbols, sites, relationships);
        }
        _ => {}
    }

    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        visit_relationships(extractor, child, symbols, sites, relationships, child_depth);
    }
}

fn extract_inheritance_relationships(
    extractor: &mut CSharpExtractor,
    node: tree_sitter::Node,
    symbols: &[Symbol],
    relationships: &mut Vec<Relationship>,
) {
    let declaration_start = node.start_byte() as u32;
    let Some(current_symbol) = symbols
        .iter()
        .find(|s| s.start_byte == declaration_start && is_type_symbol(s))
    else {
        return;
    };
    let current_symbol_id = current_symbol.id.clone();
    let (base_targets, file_path, line_number) = {
        let base = extractor.get_base();
        let mut cursor = node.walk();
        let Some(base_list) = node.children(&mut cursor).find(|c| c.kind() == "base_list") else {
            return;
        };
        let mut base_cursor = base_list.walk();
        let targets: Vec<UnresolvedTarget> = base_list
            .named_children(&mut base_cursor)
            .filter_map(|entry| base_type_target(base, entry))
            .collect();
        (
            targets,
            base.file_path.clone(),
            (node.start_position().row + 1) as u32,
        )
    };

    for target in base_targets {
        let local_base = target
            .namespace_path
            .is_empty()
            .then(|| {
                symbols
                    .iter()
                    .find(|s| s.name == target.terminal_name && is_type_symbol(s))
            })
            .flatten();
        if let Some(base_symbol) = local_base {
            let relationship_kind = if base_symbol.kind == SymbolKind::Interface {
                RelationshipKind::Implements
            } else {
                RelationshipKind::Extends
            };

            relationships.push(Relationship {
                id: format!(
                    "{}_{}_{:?}_{}",
                    current_symbol_id,
                    base_symbol.id,
                    relationship_kind,
                    node.start_position().row
                ),
                from_symbol_id: current_symbol_id.clone(),
                to_symbol_id: base_symbol.id.clone(),
                kind: relationship_kind,
                file_path: file_path.clone(),
                line_number,
                span: Some(crate::base::NormalizedSpan::from_node(&node)),
                reference_site_is_exact: false,
                confidence: 1.0,
                metadata: None,
            });
        } else {
            // Cross-file: C# naming convention (IFoo = interface) picks the kind.
            let relationship_kind = if is_interface_name(&target.terminal_name) {
                RelationshipKind::Implements
            } else {
                RelationshipKind::Extends
            };

            let pending = extractor.get_base().create_pending_relationship(
                current_symbol_id.clone(),
                target,
                relationship_kind,
                &node,
                Some(current_symbol_id.clone()),
                Some(0.9),
            );
            extractor.add_structured_pending_relationship(pending);
        }
    }
}

fn is_type_symbol(symbol: &Symbol) -> bool {
    matches!(
        symbol.kind,
        SymbolKind::Class
            | SymbolKind::Interface
            | SymbolKind::Struct
            | SymbolKind::Enum
            | SymbolKind::Type
            | SymbolKind::Delegate
    )
}

/// The target of one base-list entry: the bare type name as terminal, with
/// any qualifier as the namespace path. Primary-constructor arguments are not
/// types and yield `None`.
fn base_type_target(
    base: &crate::base::BaseExtractor,
    entry: tree_sitter::Node,
) -> Option<UnresolvedTarget> {
    let type_node = if entry.kind() == "primary_constructor_base_type" {
        entry.child_by_field_name("type")?
    } else {
        entry
    };
    let mut parts = Vec::new();
    collect_type_name_parts(base, type_node, &mut parts)?;
    let terminal_name = parts.pop()?;
    let display_name = parts
        .iter()
        .chain(std::iter::once(&terminal_name))
        .cloned()
        .collect::<Vec<_>>()
        .join(".");
    Some(UnresolvedTarget {
        display_name,
        terminal_name,
        receiver: None,
        namespace_path: parts,
        import_context: None,
    })
}

fn collect_type_name_parts(
    base: &crate::base::BaseExtractor,
    node: tree_sitter::Node,
    parts: &mut Vec<String>,
) -> Option<()> {
    collect_type_name_parts_at(base, node, parts, 0)
}

fn collect_type_name_parts_at(
    base: &crate::base::BaseExtractor,
    node: tree_sitter::Node,
    parts: &mut Vec<String>,
    depth: u32,
) -> Option<()> {
    let child_depth = crate::tree_traversal::child_tree_depth(depth)?;
    match node.kind() {
        "identifier" => parts.push(base.get_node_text(&node)),
        "generic_name" => {
            let mut cursor = node.walk();
            let name = node
                .children(&mut cursor)
                .find(|child| child.kind() == "identifier")?;
            parts.push(base.get_node_text(&name));
        }
        "qualified_name" => {
            collect_type_name_parts_at(
                base,
                node.child_by_field_name("qualifier")?,
                parts,
                child_depth,
            )?;
            collect_type_name_parts_at(
                base,
                node.child_by_field_name("name")?,
                parts,
                child_depth,
            )?;
        }
        "alias_qualified_name" => {
            collect_type_name_parts_at(
                base,
                node.child_by_field_name("name")?,
                parts,
                child_depth,
            )?;
        }
        _ => return None,
    }
    Some(())
}

/// Check if a type name follows C# interface naming convention (IFoo).
/// Requires 'I' prefix followed by an uppercase letter to avoid false positives
/// with regular names like "Item" or "Index".
fn is_interface_name(name: &str) -> bool {
    let mut chars = name.chars();
    matches!((chars.next(), chars.next()), (Some('I'), Some(c)) if c.is_ascii_uppercase())
}

/// Extract constructor parameter type relationships (DI injection pattern)
///
/// In C#/.NET, dependency injection via constructor parameters is THE primary
/// wiring mechanism. This function creates `Uses` relationships from the
/// containing class to each parameter type, enabling centrality scoring.
///
/// Handles:
/// - Simple types: `ILogger` -> identifier
/// - Generic types: `ILogger<MyService>` -> generic_name (extracts base name `ILogger`)
/// - Nullable types: `ILogger?` -> nullable_type (unwraps to inner type)
/// - Skips predefined types: `string`, `int`, `bool`, etc. (not interesting relationships)
fn extract_constructor_parameter_relationships(
    extractor: &mut CSharpExtractor,
    node: tree_sitter::Node,
    symbols: &[Symbol],
    relationships: &mut Vec<Relationship>,
) {
    // Phase 1: Collect all data using immutable borrow of extractor
    // (we need base for get_node_text, but also need &mut extractor for pending_relationships)
    let (class_symbol_id, param_types) = {
        let base = extractor.get_base();

        let Some(class_symbol) = find_containing_class(base, node, symbols) else {
            return;
        };

        // Find the parameter_list child of the constructor
        let mut cursor = node.walk();
        let param_list = node
            .children(&mut cursor)
            .find(|c| c.kind() == "parameter_list");
        let Some(param_list) = param_list else { return };

        // Collect all parameter type names with their line numbers
        let mut types = Vec::new();
        let mut param_cursor = param_list.walk();
        for param in param_list
            .children(&mut param_cursor)
            .filter(|c| c.kind() == "parameter")
        {
            if let Some(type_name) = extract_parameter_type_name(base, param)
                && !type_name.is_empty()
            {
                let line_number = param.start_position().row as u32 + 1;
                let row = param.start_position().row;
                types.push((
                    type_name,
                    line_number,
                    row,
                    crate::base::NormalizedSpan::from_node(&param),
                ));
            }
        }

        (class_symbol.id.clone(), types)
    };
    // Phase 1 done — immutable borrow of extractor is dropped

    // Phase 2: Create relationships, deduplicating across constructor overloads.
    // A class only needs one Uses edge per type, regardless of how many constructors use it.
    let file_path = extractor.get_base().file_path.clone();
    let type_symbols: Vec<Symbol> = symbols
        .iter()
        .filter(|s| is_type_symbol(s))
        .cloned()
        .collect();
    let symbol_map: std::collections::HashMap<String, &Symbol> =
        crate::base::ScopedSymbolIndex::unique_symbol_map(&type_symbols);

    // Collect already-existing Uses targets for this class (from earlier constructors)
    let mut seen: std::collections::HashSet<String> = relationships
        .iter()
        .filter(|r| r.from_symbol_id == class_symbol_id && r.kind == RelationshipKind::Uses)
        .map(|r| r.to_symbol_id.clone())
        .collect();

    for (type_name, line_number, row, span) in param_types {
        if seen.contains(&type_name) {
            continue; // Already emitted (pending dedup by name)
        }
        match symbol_map.get(&type_name) {
            Some(type_symbol) if !seen.contains(&type_symbol.id) => {
                seen.insert(type_symbol.id.clone());
                relationships.push(Relationship {
                    id: format!(
                        "{}_{}_{:?}_{}",
                        class_symbol_id,
                        type_symbol.id,
                        RelationshipKind::Uses,
                        row
                    ),
                    from_symbol_id: class_symbol_id.clone(),
                    to_symbol_id: type_symbol.id.clone(),
                    kind: RelationshipKind::Uses,
                    file_path: file_path.clone(),
                    line_number,
                    span: Some(span),
                    reference_site_is_exact: false,
                    confidence: 0.9,
                    metadata: None,
                });
            }
            Some(_) => {} // Already seen this resolved type
            None => {
                seen.insert(type_name.clone());
                let mut pending = extractor.get_base().create_pending_relationship(
                    class_symbol_id.clone(),
                    UnresolvedTarget::simple(type_name),
                    RelationshipKind::Uses,
                    &node,
                    Some(class_symbol_id.clone()),
                    Some(0.8),
                );
                pending.pending.line_number = line_number;
                pending.span = Some(span);
                extractor.add_structured_pending_relationship(pending);
            }
        }
    }
}

fn extract_object_creation_relationships(
    extractor: &mut CSharpExtractor,
    node: tree_sitter::Node,
    symbols: &[Symbol],
    sites: &CallSites<'_>,
    relationships: &mut Vec<Relationship>,
) {
    let type_name = {
        let base = extractor.get_base();
        if node.kind() == "implicit_object_creation_expression" {
            super::scope::target_type_of_implicit_new(node).and_then(|type_node| {
                let text = base.get_node_text(&type_node);
                let unqualified = text.split('<').next().unwrap_or(&text);
                unqualified
                    .rsplit('.')
                    .next()
                    .map(|name| name.trim().to_string())
            })
        } else {
            find_first_type_identifier(base, node).filter(|name| {
                !node
                    .child_by_field_name("type")
                    .is_some_and(|type_node| type_node.kind() == "identifier")
                    || !super::scope::is_type_parameter_in_scope(&base.content, node, name)
            })
        }
    };
    let Some(type_name) = type_name else {
        return;
    };
    if type_name.is_empty() {
        return;
    }

    let target = UnresolvedTarget::simple(type_name);

    let caller = sites
        .owners
        .find(node)
        .filter(|symbol| {
            matches!(
                symbol.kind,
                SymbolKind::Function
                    | SymbolKind::Method
                    | SymbolKind::Constructor
                    | SymbolKind::Destructor
                    | SymbolKind::Property
                    | SymbolKind::Event
                    | SymbolKind::Operator
                    | SymbolKind::Class
                    | SymbolKind::Struct
                    | SymbolKind::Module
            )
        })
        .cloned();

    let Some(caller) = caller else {
        return;
    };

    let resolution = sites
        .targets
        .resolve_call_target(&target.terminal_name, Some(&caller), None);
    let resolved_type = match resolution {
        LocalTargetResolution::Resolved(symbol) => Some(symbol),
        _ => None,
    }
    .filter(|symbol| is_constructible_type(symbol))
    .or_else(|| unique_constructible_type(symbols, &target.terminal_name));

    if let Some(type_symbol) = resolved_type {
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

fn is_constructible_type(symbol: &Symbol) -> bool {
    matches!(
        symbol.kind,
        SymbolKind::Class | SymbolKind::Struct | SymbolKind::Type
    )
}

/// The file's one class, struct, or record named `name`; a same-named
/// constructor makes the name ambiguous as a call but not as a type.
fn unique_constructible_type<'a>(symbols: &'a [Symbol], name: &str) -> Option<&'a Symbol> {
    let mut matches = symbols
        .iter()
        .filter(|symbol| symbol.name == name && is_constructible_type(symbol));
    let first = matches.next()?;
    matches.next().is_none().then_some(first)
}

fn find_first_type_identifier(
    base: &crate::base::BaseExtractor,
    node: tree_sitter::Node,
) -> Option<String> {
    find_first_type_identifier_at_depth(base, node, 0)
}

fn find_first_type_identifier_at_depth(
    base: &crate::base::BaseExtractor,
    node: tree_sitter::Node,
    depth: u32,
) -> Option<String> {
    if !should_visit_tree_depth(depth) {
        return None;
    }

    let child_depth = child_tree_depth(depth)?;
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        match child.kind() {
            "identifier" => return Some(base.get_node_text(&child)),
            "qualified_name" | "generic_name" | "predefined_type" => {
                if let Some(name) = find_first_type_identifier_at_depth(base, child, child_depth) {
                    return Some(name);
                }
                return Some(base.get_node_text(&child));
            }
            _ => {}
        }
    }
    None
}

/// Extract method call relationships
///
/// Creates resolved Relationship when target is a local method.
/// Creates PendingRelationship when target is:
/// - An Import symbol (needs cross-file resolution)
/// - Not found in local symbol_map (e.g., method on imported type)
fn extract_call_relationships(
    extractor: &mut CSharpExtractor,
    node: tree_sitter::Node,
    symbols: &[Symbol],
    sites: &CallSites<'_>,
    relationships: &mut Vec<Relationship>,
) {
    let Some(function) = node.child_by_field_name("function") else {
        return;
    };
    let method_name = {
        let base = extractor.get_base();
        match function.kind() {
            "identifier" => base.get_node_text(&function),
            _ => {
                let mut parts = Vec::new();
                collect_chain_parts(extractor, node, 0, &mut parts);
                parts.pop().unwrap_or_default()
            }
        }
    };
    if method_name.is_empty() || (function.kind() == "identifier" && method_name == "nameof") {
        return;
    }

    if is_base_receiver(function) {
        handle_base_call(extractor, node, &method_name, symbols, sites, relationships);
        return;
    }
    let target = unresolved_call_target(extractor, node, &method_name);
    handle_call_target(extractor, node, target, symbols, sites, relationships);
}

fn is_base_receiver(function: tree_sitter::Node) -> bool {
    function.kind() == "member_access_expression"
        && function
            .child(0)
            .is_some_and(|receiver| receiver.kind() == "base")
}

/// `base.M()` targets the base type's `M`, never the calling override.
fn handle_base_call(
    extractor: &mut CSharpExtractor,
    call_node: tree_sitter::Node,
    method_name: &str,
    symbols: &[Symbol],
    sites: &CallSites<'_>,
    relationships: &mut Vec<Relationship>,
) {
    let base = extractor.get_base();
    let Some(caller) = sites.owners.find(call_node).cloned() else {
        return;
    };
    let receiver_type = super::identifiers::self_receiver_type(base, call_node);
    let base_member = receiver_type.as_deref().and_then(|type_name| {
        symbols.iter().find(|candidate| {
            candidate.name == method_name
                && candidate.id != caller.id
                && candidate.parent_id.as_deref().is_some_and(|parent_id| {
                    symbols.iter().any(|owner| {
                        owner.id == parent_id && owner.name == type_name && is_type_symbol(owner)
                    })
                })
        })
    });
    if let Some(called_symbol) = base_member {
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
            file_path: base.file_path.clone(),
            line_number: call_node.start_position().row as u32 + 1,
            span: Some(crate::base::NormalizedSpan::from_node(&call_node)),
            reference_site_is_exact: false,
            confidence: 0.9,
            metadata: None,
        });
        return;
    }
    let target = UnresolvedTarget::from_chain(vec!["base".to_string(), method_name.to_string()]);
    let pending = base
        .create_pending_relationship(
            caller.id.clone(),
            target,
            RelationshipKind::Calls,
            &call_node,
            Some(caller.id.clone()),
            Some(0.7),
        )
        .with_receiver_type(receiver_type);
    extractor.add_structured_pending_relationship(pending);
}

/// Handle a call target - create Relationship or PendingRelationship based on target type
fn handle_call_target(
    extractor: &mut CSharpExtractor,
    call_node: tree_sitter::Node,
    target: UnresolvedTarget,
    symbols: &[Symbol],
    sites: &CallSites<'_>,
    relationships: &mut Vec<Relationship>,
) {
    let base = extractor.get_base();
    let symbol_index = &sites.targets;

    let caller = sites.owners.find(call_node).cloned();
    let Some(caller) = caller else {
        return;
    };
    let is_bare_call = call_node
        .child_by_field_name("function")
        .is_some_and(|function| matches!(function.kind(), "identifier" | "generic_name"));
    let enclosing_type = is_bare_call
        .then(|| find_containing_class(base, call_node, symbols))
        .flatten();
    let receiver_type = super::identifiers::self_receiver_type(base, call_node)
        .or_else(|| enclosing_type.map(|ty| ty.name.clone()));

    let line_number = call_node.start_position().row as u32 + 1;
    let file_path = base.file_path.clone();

    let resolution = match symbol_index.resolve_call_target(
        &target.terminal_name,
        Some(&caller),
        target.receiver.as_deref(),
    ) {
        LocalTargetResolution::Resolved(called_symbol) => {
            LocalTargetResolution::Resolved(called_symbol)
        }
        other => match enclosing_type
            .and_then(|ty| unique_member(symbols, &ty.id, &target.terminal_name))
        {
            Some(member) => LocalTargetResolution::Resolved(member),
            None => other,
        },
    };
    match resolution {
        LocalTargetResolution::Resolved(called_symbol) => {
            // Target is a local method - create resolved Relationship
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
        LocalTargetResolution::Import(_) => {
            let pending = extractor
                .get_base()
                .create_pending_relationship(
                    caller.id.clone(),
                    target,
                    RelationshipKind::Calls,
                    &call_node,
                    Some(caller.id.clone()),
                    Some(0.8),
                )
                .with_receiver_type(receiver_type);
            extractor.add_structured_pending_relationship(pending);
        }
        LocalTargetResolution::Ambiguous
        | LocalTargetResolution::ReceiverQualified
        | LocalTargetResolution::Missing => {
            // Target not found in local symbols - likely a method on imported type
            // Create PendingRelationship for cross-file resolution
            let pending = extractor
                .get_base()
                .create_pending_relationship(
                    caller.id.clone(),
                    target,
                    RelationshipKind::Calls,
                    &call_node,
                    Some(caller.id.clone()),
                    Some(0.7),
                )
                .with_receiver_type(receiver_type);
            extractor.add_structured_pending_relationship(pending);
        }
    }
}

/// The one method of the type `type_id` named `name`; `None` for overloads.
fn unique_member<'a>(symbols: &'a [Symbol], type_id: &str, name: &str) -> Option<&'a Symbol> {
    let mut members = symbols.iter().filter(|symbol| {
        symbol.name == name
            && symbol.kind == SymbolKind::Method
            && symbol.parent_id.as_deref() == Some(type_id)
    });
    let member = members.next()?;
    members.next().is_none().then_some(member)
}

fn unresolved_call_target(
    extractor: &CSharpExtractor,
    node: tree_sitter::Node,
    fallback_name: &str,
) -> UnresolvedTarget {
    unresolved_call_target_at_depth(extractor, node, fallback_name, 0)
}

fn unresolved_call_target_at_depth(
    extractor: &CSharpExtractor,
    node: tree_sitter::Node,
    fallback_name: &str,
    depth: u32,
) -> UnresolvedTarget {
    if !should_visit_tree_depth(depth) {
        return UnresolvedTarget::simple(fallback_name.to_string());
    }

    if node.kind() == "invocation_expression" {
        let mut cursor = node.walk();
        if let Some(first_child) = node.children(&mut cursor).next() {
            let Some(child_depth) = child_tree_depth(depth) else {
                return UnresolvedTarget::simple(fallback_name.to_string());
            };
            return unresolved_call_target_at_depth(
                extractor,
                first_child,
                fallback_name,
                child_depth,
            );
        }
    }

    let mut parts = Vec::new();
    collect_chain_parts(extractor, node, depth, &mut parts);
    if parts.len() >= 2 {
        return UnresolvedTarget::from_chain(parts);
    }

    UnresolvedTarget::simple(fallback_name.to_string())
}

fn collect_chain_parts(
    extractor: &CSharpExtractor,
    node: tree_sitter::Node,
    depth: u32,
    parts: &mut Vec<String>,
) {
    if !should_visit_tree_depth(depth) {
        return;
    }
    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };
    let base = extractor.get_base();
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        match child.kind() {
            "member_access_expression"
            | "conditional_access_expression"
            | "member_binding_expression" => {
                collect_chain_parts(extractor, child, child_depth, parts)
            }
            "identifier" => parts.push(base.get_node_text(&child)),
            "generic_name" => {
                let mut generic_cursor = child.walk();
                if let Some(name) = child
                    .children(&mut generic_cursor)
                    .find(|part| part.kind() == "identifier")
                {
                    parts.push(base.get_node_text(&name));
                }
            }
            _ => {}
        }
    }
}
