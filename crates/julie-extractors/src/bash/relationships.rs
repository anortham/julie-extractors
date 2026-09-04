//! Relationship extraction for Bash
//!
//! Handles extraction of relationships between symbols (calls, definitions, usages).

use super::commands::{extract_source_target, is_import_command, is_shell_builtin};
use crate::base::{
    ContainingSymbolIndex, LocalTargetResolution, Relationship, RelationshipKind,
    ScopedSymbolIndex, SymbolKind, UnresolvedTarget,
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
        let Some(command_name_node) = self.find_command_name_node(node) else {
            return;
        };
        let command_name = self.base.get_node_text(&command_name_node);
        let command_text = self.base.get_node_text(&node);
        let Some(caller_symbol) = function_symbols
            .find(node)
            .filter(|symbol| symbol.kind == SymbolKind::Function)
        else {
            return;
        };

        if is_import_command(&command_name) {
            if let Some(target) = extract_source_target(&command_name, &command_text) {
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

        let unresolved_target = UnresolvedTarget::simple(command_name.clone());

        match scoped_index.resolve_call_target(
            &unresolved_target.terminal_name,
            Some(caller_symbol),
            unresolved_target.receiver.as_deref(),
        ) {
            LocalTargetResolution::Resolved(called_symbol) => {
                if caller_symbol.id != called_symbol.id {
                    let relationship = self.base.create_relationship_at_target(
                        caller_symbol.id.clone(),
                        called_symbol.id.clone(),
                        RelationshipKind::Calls,
                        &command_name_node,
                        Some(0.95),
                        None,
                    );
                    relationships.push(relationship);
                }
            }
            LocalTargetResolution::Import(_)
            | LocalTargetResolution::Ambiguous
            | LocalTargetResolution::Missing
            | LocalTargetResolution::ReceiverQualified => {
                if !is_shell_builtin(&command_name) {
                    let pending = self.base.create_pending_relationship_at_target(
                        caller_symbol.id.clone(),
                        unresolved_target,
                        RelationshipKind::Calls,
                        &command_name_node,
                        Some(caller_symbol.id.clone()),
                        Some(0.8),
                    );
                    self.add_structured_pending_relationship(pending);
                }
            }
        }
    }
}
