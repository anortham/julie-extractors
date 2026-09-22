//! Call sites: same-file `calls` relationships and pending calls for the rest.

use super::identifiers::{call_callee, instantiation_target, self_receiver_type};
use crate::base::{
    LocalTargetResolution, NormalizedSpan, OwnerIndex, RelationshipKind, ScopedSymbolIndex, Symbol,
    SymbolKind, UnresolvedTarget, is_test_call_symbol,
};
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};
use std::collections::HashMap;
use tree_sitter::Node;

struct CallContext<'a> {
    owners: OwnerIndex<'a>,
    scoped: ScopedSymbolIndex<'a>,
    classes: HashMap<&'a str, &'a Symbol>,
    members: &'a [Symbol],
}

impl super::DartExtractor {
    pub(super) fn extract_call_relationships(&mut self, root: Node, symbols: &[Symbol]) {
        let targets: Vec<Symbol> = symbols
            .iter()
            .filter(|symbol| !is_test_call_symbol(symbol))
            .cloned()
            .collect();
        let context = CallContext {
            owners: OwnerIndex::new(&self.base, symbols),
            scoped: ScopedSymbolIndex::new(&targets),
            classes: targets
                .iter()
                .filter(|symbol| symbol.kind == SymbolKind::Class)
                .map(|symbol| (symbol.name.as_str(), symbol))
                .collect(),
            members: &targets,
        };
        self.walk_call_sites(root, &context, 0);
    }

    fn walk_call_sites(&mut self, node: Node, context: &CallContext<'_>, depth: u32) {
        if !should_visit_tree_depth(depth) {
            return;
        }
        let target = match node.kind() {
            "call_expression" => node
                .child_by_field_name("function")
                .and_then(call_callee)
                .map(|callee| {
                    let terminal = self.base.get_node_text(&callee.name);
                    let receiver = callee
                        .receiver
                        .map(|(start, end)| self.base.content[start..end].to_string());
                    qualified_target(receiver, terminal)
                }),
            "const_object_expression" | "new_expression" | "constructor_invocation" => {
                instantiation_target(&self.base, node).map(|(type_name, constructor)| {
                    match constructor {
                        Some(constructor) => qualified_target(Some(type_name), constructor),
                        None => UnresolvedTarget::simple(type_name),
                    }
                })
            }
            _ => None,
        };
        if let Some(target) = target {
            self.record_call(node, target, context);
        }

        let Some(child_depth) = child_tree_depth(depth) else {
            return;
        };
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.walk_call_sites(child, context, child_depth);
        }
    }

    fn record_call(&mut self, node: Node, target: UnresolvedTarget, context: &CallContext<'_>) {
        let Some(caller) = context.owners.find(node) else {
            return;
        };
        if let Some(called) = resolve(&target, caller, context) {
            self.same_file_calls.push((
                caller.id.clone(),
                called.id.clone(),
                node.start_position().row as u32 + 1,
                NormalizedSpan::from_node(&node),
            ));
            return;
        }
        let pending = self
            .base
            .create_pending_relationship(
                caller.id.clone(),
                target,
                RelationshipKind::Calls,
                &node,
                Some(caller.id.clone()),
                Some(0.7),
            )
            .with_receiver_type(self_receiver_type(&self.base, node));
        self.add_structured_pending_relationship(pending);
    }
}

/// A same-file target: a constructor or member of a same-file class named by
/// the call, else the unique callable the shared scoped rules pick.
fn resolve<'a>(
    target: &UnresolvedTarget,
    caller: &Symbol,
    context: &CallContext<'a>,
) -> Option<&'a Symbol> {
    let receiver = target.receiver.as_deref();
    if let Some(class) = receiver.and_then(|receiver| context.classes.get(receiver)) {
        return class_member(class, &target.terminal_name, context);
    }
    if receiver.is_none()
        && let Some(class) = context.classes.get(target.terminal_name.as_str())
    {
        return class_member(class, &target.terminal_name, context).or(Some(*class));
    }
    match context
        .scoped
        .resolve_call_target(&target.terminal_name, Some(caller), receiver)
    {
        LocalTargetResolution::Resolved(symbol) => Some(symbol),
        _ => None,
    }
}

/// `Type(...)`, `Type.named(...)` or `Type.staticMethod(...)` inside `class`.
fn class_member<'a>(class: &Symbol, name: &str, context: &CallContext<'a>) -> Option<&'a Symbol> {
    let qualified = format!("{}.{name}", class.name);
    let mut matches = context.members.iter().filter(|symbol| {
        symbol.parent_id.as_deref() == Some(class.id.as_str())
            && matches!(symbol.kind, SymbolKind::Method | SymbolKind::Constructor)
            && (symbol.name == qualified
                || (symbol.name == name
                    && (symbol.kind == SymbolKind::Method || name == class.name)))
    });
    let first = matches.next()?;
    matches.next().is_none().then_some(first)
}

fn qualified_target(receiver: Option<String>, terminal_name: String) -> UnresolvedTarget {
    let Some(receiver) = receiver else {
        return UnresolvedTarget::simple(terminal_name);
    };
    let display_name = format!("{receiver}.{terminal_name}");
    UnresolvedTarget::from_qualified_text(&display_name, &["."]).unwrap_or(UnresolvedTarget {
        display_name,
        terminal_name,
        receiver: Some(receiver),
        namespace_path: Vec::new(),
        import_context: None,
    })
}
