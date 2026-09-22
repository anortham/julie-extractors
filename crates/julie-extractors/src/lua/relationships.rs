use super::{scope, type_facts};
use crate::base::{ContainingSymbolIndex, RelationshipKind, Symbol, SymbolKind, UnresolvedTarget};
use crate::lua::{LuaExtractor, helpers};
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

    if node.kind() == "function_call" {
        // `require(...)` is handled during symbol extraction as an import symbol.
        if let Some(identifier) = helpers::find_child_by_type(&node, "identifier") {
            let callee_name = extractor.base().get_node_text(&identifier);
            process_function_call(extractor, node, &callee_name, None, context);
        }
        // Handle method calls: obj:method() or obj.method()
        else if let Some(method_expr) =
            helpers::find_child_by_type(&node, "method_index_expression")
                .or_else(|| helpers::find_child_by_type(&node, "dot_index_expression"))
        {
            let full_expr = extractor.base().get_node_text(&method_expr);
            // Extract the method name (everything after : or .)
            let method_name = if let Some(colon_pos) = full_expr.rfind(':') {
                &full_expr[colon_pos + 1..]
            } else if let Some(dot_pos) = full_expr.rfind('.') {
                &full_expr[dot_pos + 1..]
            } else {
                &full_expr
            };
            process_function_call(extractor, node, method_name, Some(&full_expr), context);
        }
    }

    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        traverse_tree_for_relationships(extractor, child, context, child_depth);
    }
}

fn process_function_call(
    extractor: &mut LuaExtractor,
    node: Node,
    callee_name: &str,
    full_expr: Option<&str>,
    context: &CallContext,
) {
    if callee_name == "require" {
        return;
    }

    let caller = context
        .callers
        .find(node)
        .filter(|caller| caller.start_byte != node.start_byte() as u32);
    if let Some(caller_symbol) = caller {
        let mut target = if let Some(full_expr) = full_expr {
            let normalized = full_expr.replace(':', ".");
            let receiver = normalized
                .rsplit_once('.')
                .map(|(receiver, _)| receiver.to_string());
            UnresolvedTarget::from_qualified_text(&normalized, &["."]).unwrap_or(UnresolvedTarget {
                display_name: normalized,
                terminal_name: callee_name.to_string(),
                receiver,
                namespace_path: Vec::new(),
                import_context: None,
            })
        } else {
            UnresolvedTarget::simple(callee_name.to_string())
        };
        if let Some(receiver) = target.receiver.as_deref()
            && scope::resolve_binding(receiver, node.start_byte() as u32, context.symbols)
                .is_some_and(|binding| binding.kind == SymbolKind::Import)
        {
            target.import_context = Some(receiver.to_string());
        }
        let can_resolve_locally = target
            .receiver
            .as_deref()
            .is_none_or(|receiver| matches!(receiver, "self"));

        match context
            .callees
            .get(callee_name)
            .filter(|_| can_resolve_locally)
        {
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
