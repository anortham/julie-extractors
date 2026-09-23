//! Relationship extraction for Bash
//!
//! Handles extraction of relationships between symbols (calls, definitions, usages).

use super::commands::{extract_source_target, is_import_command, is_shell_builtin};
use super::invocations::{Invocation, invocations, static_command_name};
use crate::base::{
    ContainingSymbolIndex, LocalTargetResolution, NormalizedSpan, Relationship, RelationshipKind,
    ScopedSymbolIndex, Symbol, SymbolKind, UnresolvedTarget,
};
use tree_sitter::Node;

impl super::BashExtractor {
    /// Extract relationships between functions and commands they call
    pub(super) fn extract_command_relationships<'a>(
        &mut self,
        node: Node,
        function_symbols: &ContainingSymbolIndex<'a>,
        scoped_index: &ScopedSymbolIndex<'a>,
        relationships: &mut Vec<Relationship>,
    ) {
        let Some((command_name_node, command_name)) = static_command_name(&self.base.content, node)
        else {
            return;
        };
        let targets = invocations(&self.base.content, node, &self.command_scope());
        if !targets.is_empty() {
            for target in targets {
                let Some(caller) = function_symbols
                    .find(target.anchor)
                    .filter(|symbol| symbol.kind == SymbolKind::Function)
                else {
                    continue;
                };
                self.relate_call(caller, &target, scoped_index, relationships);
            }
            return;
        }

        let Some(caller_symbol) = function_symbols
            .find(node)
            .filter(|symbol| symbol.kind == SymbolKind::Function)
            .filter(|symbol| symbol.start_byte != node.start_byte() as u32)
        else {
            return;
        };

        if is_import_command(&command_name, self.test_context()) {
            if let Some(target) = extract_source_target(&self.base, &command_name, node) {
                let pending = self.base.create_pending_relationship(
                    caller_symbol.id.clone(),
                    target,
                    RelationshipKind::Imports,
                    &node,
                    Some(caller_symbol.id.clone()),
                    Some(0.85),
                );
                self.add_structured_pending_relationship(pending);
            }
            return;
        }
        if super::test_calls::DSL_KEYWORDS.contains(&command_name.as_str()) {
            return;
        }

        let target = Invocation {
            name: command_name,
            anchor: command_name_node,
            range: command_name_node.byte_range(),
            arguments: None,
        };
        self.relate_call(caller_symbol, &target, scoped_index, relationships);
    }

    fn relate_call<'a>(
        &mut self,
        caller: &Symbol,
        target: &Invocation<'_>,
        scoped_index: &ScopedSymbolIndex<'a>,
        relationships: &mut Vec<Relationship>,
    ) {
        let span = self
            .base
            .span_for_byte_range(target.range.start, target.range.end)
            .unwrap_or_else(|| NormalizedSpan::from_node(&target.anchor));
        let unresolved_target = UnresolvedTarget::simple(target.name.clone());
        match scoped_index.resolve_call_target(&target.name, Some(caller), None) {
            LocalTargetResolution::Resolved(called_symbol) => {
                if caller.id != called_symbol.id {
                    let mut relationship = self.base.create_relationship_at_target(
                        caller.id.clone(),
                        called_symbol.id.clone(),
                        RelationshipKind::Calls,
                        &target.anchor,
                        Some(0.95),
                        None,
                    );
                    relationship.line_number = span.start_line;
                    relationship.span = Some(span);
                    relationships.push(relationship);
                }
            }
            LocalTargetResolution::Import(_)
            | LocalTargetResolution::Ambiguous
            | LocalTargetResolution::Missing
            | LocalTargetResolution::ReceiverQualified => {
                if !is_shell_builtin(&target.name) {
                    let pending = self
                        .base
                        .create_pending_relationship_at_target(
                            caller.id.clone(),
                            unresolved_target,
                            RelationshipKind::Calls,
                            &target.anchor,
                            Some(caller.id.clone()),
                            Some(0.8),
                        )
                        .with_target_span(span);
                    self.add_structured_pending_relationship(pending);
                }
            }
        }
    }
}
