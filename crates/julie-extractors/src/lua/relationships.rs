use super::identifiers::identifier_chain;
use super::{core, scope, type_facts};
use crate::base::{ContainingSymbolIndex, RelationshipKind, Symbol, SymbolKind, UnresolvedTarget};
use crate::lua::LuaExtractor;
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};
use std::collections::HashMap;
use tree_sitter::{Node, Tree};

struct CallContext<'a> {
    symbols: &'a [Symbol],
    callers: ContainingSymbolIndex<'a>,
    callees: HashMap<String, &'a Symbol>,
}

fn is_callable(symbol: &Symbol) -> bool {
    matches!(symbol.kind, SymbolKind::Function | SymbolKind::Method)
}

/// Extract relationships such as function call edges from the Lua AST.
pub(super) fn extract_relationships(extractor: &mut LuaExtractor, tree: &Tree, symbols: &[Symbol]) {
    let file_path = extractor.base().file_path.clone();
    let callables = || {
        symbols
            .iter()
            .filter(|symbol| symbol.file_path == file_path && is_callable(symbol))
    };
    let mut by_name: HashMap<String, Vec<&Symbol>> = HashMap::new();
    for symbol in callables() {
        by_name.entry(symbol.name.clone()).or_default().push(symbol);
    }
    let context = CallContext {
        symbols,
        callers: ContainingSymbolIndex::from_iter(callables()),
        callees: by_name
            .into_iter()
            .filter_map(|(name, candidates)| match candidates.as_slice() {
                [symbol] => Some((name, *symbol)),
                _ => None,
            })
            .collect(),
    };

    traverse_tree_for_relationships(extractor, tree.root_node(), &context, 0);
    extract_extends(extractor, tree, symbols);
}

/// `extends` edges from each class to the base its `baseClass` metadata names:
/// resolved when the base binds to a same-file class or table, pending otherwise
/// (with the import context when the base is a `require` binding).
fn extract_extends(extractor: &mut LuaExtractor, tree: &Tree, symbols: &[Symbol]) {
    let file_path = extractor.base().file_path.clone();
    for class in symbols
        .iter()
        .filter(|symbol| symbol.file_path == file_path && symbol.kind == SymbolKind::Class)
    {
        let Some(base_name) = class
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.get("baseClass"))
            .and_then(|base| base.as_str())
        else {
            continue;
        };
        let Some(node) = tree
            .root_node()
            .descendant_for_byte_range(class.start_byte as usize, class.end_byte as usize)
        else {
            continue;
        };
        let binding = scope::resolve_binding(base_name, class.start_byte, symbols)
            .filter(|binding| binding.id != class.id);
        match binding {
            Some(base) if matches!(base.kind, SymbolKind::Class | SymbolKind::Variable) => {
                let relationship = extractor.base().create_relationship(
                    class.id.clone(),
                    base.id.clone(),
                    RelationshipKind::Extends,
                    &node,
                    Some(0.9),
                    None,
                );
                extractor.relationships.push(relationship);
            }
            binding => {
                let mut target = UnresolvedTarget::simple(base_name.to_string());
                if binding.is_some_and(|binding| binding.kind == SymbolKind::Import) {
                    target.import_context = Some(base_name.to_string());
                }
                let pending = extractor.base().create_pending_relationship(
                    class.id.clone(),
                    target,
                    RelationshipKind::Extends,
                    &node,
                    Some(class.id.clone()),
                    Some(0.7),
                );
                extractor.add_structured_pending_relationship(pending);
            }
        }
    }
}

fn traverse_tree_for_relationships(
    extractor: &mut LuaExtractor,
    node: Node,
    context: &CallContext,
    depth: u32,
) {
    if !should_visit_tree_depth(depth) {
        return;
    }

    if node.kind() == "function_call"
        && let Some(callee) = node.child_by_field_name("name")
    {
        process_function_call(extractor, node, callee, context);
    }

    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        traverse_tree_for_relationships(extractor, child, context, child_depth);
    }
}

/// The unresolved target of a call's `name` node. Only identifier chains
/// contribute a receiver and namespace; any other receiver expression keeps the
/// terminal name alone. `require("m").f()` records `m` as the import context.
fn call_target(base: &crate::base::BaseExtractor, callee: Node) -> Option<UnresolvedTarget> {
    let (table, member) = match callee.kind() {
        "identifier" => return Some(UnresolvedTarget::simple(base.get_node_text(&callee))),
        "dot_index_expression" => (
            callee.child_by_field_name("table")?,
            callee.child_by_field_name("field")?,
        ),
        "method_index_expression" => (
            callee.child_by_field_name("table")?,
            callee.child_by_field_name("method")?,
        ),
        _ => return None,
    };
    let member = base.get_node_text(&member);
    if let Some(mut chain) = identifier_chain(base, table) {
        chain.push(member);
        return Some(UnresolvedTarget::from_chain(chain));
    }
    let mut target = UnresolvedTarget::simple(member);
    if let Some(require) = core::require_import(base, table) {
        target.import_context = Some(require.module_path().to_string());
    }
    Some(target)
}

/// The callable a qualified same-file call names: `M.f()` where `M` resolves to
/// a table symbol owning exactly one callable child `f`.
fn qualified_local_callee<'a>(
    base: &crate::base::BaseExtractor,
    callee: Node,
    name: &str,
    context: &CallContext<'a>,
) -> Option<&'a Symbol> {
    let table = match callee.kind() {
        "dot_index_expression" | "method_index_expression" => {
            callee.child_by_field_name("table")?
        }
        _ => return None,
    };
    let owner_id = scope::resolve_table_symbol_id(base, table, context.symbols)?;
    let mut candidates = context.symbols.iter().filter(|symbol| {
        is_callable(symbol) && symbol.name == name && symbol.parent_id.as_deref() == Some(&owner_id)
    });
    let callee = candidates.next()?;
    candidates.next().is_none().then_some(callee)
}

fn process_function_call(
    extractor: &mut LuaExtractor,
    node: Node,
    callee: Node,
    context: &CallContext,
) {
    let Some(mut target) = call_target(extractor.base(), callee) else {
        return;
    };
    if callee.kind() == "identifier" && target.terminal_name == "require" {
        return;
    }
    let callee_name = target.terminal_name.clone();

    let caller = context
        .callers
        .find(node)
        .filter(|caller| caller.start_byte != node.start_byte() as u32);
    if let Some(caller_symbol) = caller {
        if let Some(receiver) = target.receiver.as_deref()
            && scope::resolve_binding(receiver, node.start_byte() as u32, context.symbols)
                .is_some_and(|binding| binding.kind == SymbolKind::Import)
        {
            target.import_context = Some(receiver.to_string());
        }
        let can_resolve_locally = target.namespace_path.is_empty()
            && target
                .receiver
                .as_deref()
                .is_none_or(|receiver| matches!(receiver, "self"))
            && target.import_context.is_none();
        let local_callee = qualified_local_callee(extractor.base(), callee, &callee_name, context)
            .or_else(|| {
                context
                    .callees
                    .get(callee_name.as_str())
                    .copied()
                    .filter(|_| can_resolve_locally)
            });

        match local_callee {
            Some(callee_symbol) => {
                // Target is a local function - create resolved Relationship
                if caller_symbol.id != callee_symbol.id {
                    let relationship = extractor.base().create_relationship(
                        caller_symbol.id.clone(),
                        callee_symbol.id.clone(),
                        RelationshipKind::Calls,
                        &node,
                        Some(0.9),
                        None,
                    );
                    extractor.relationships.push(relationship);
                }
            }
            None => {
                // Target not found in local symbols - likely a cross-file call
                // Create PendingRelationship for cross-file resolution
                let pending = extractor
                    .base()
                    .create_pending_relationship(
                        caller_symbol.id.clone(),
                        target,
                        RelationshipKind::Calls,
                        &node,
                        Some(caller_symbol.id.clone()),
                        Some(0.7),
                    )
                    .with_receiver_type(type_facts::call_receiver_type(extractor.base(), node));
                extractor.add_structured_pending_relationship(pending);
            }
        }
    }
}
