//! PowerShell variable extraction.
//! Only plain assignment targets (`$x = ...`, `[T]$x = ...`, `$script:x = ...`)
//! declare variables; reads, member, index, and static-property assignments
//! are identifiers, not symbols.

use crate::base::{BaseExtractor, Symbol, SymbolKind, SymbolOptions, Visibility};
use tree_sitter::Node;

use super::helpers::{variable_key, variable_name};
use super::type_facts;

/// The declared name of a plain assignment target, if the node assigns one.
pub(super) fn assignment_target_name(base: &BaseExtractor, node: Node) -> Option<String> {
    let variable = type_facts::assignment_variable_node(node)?;
    let name = variable_name(&base.get_node_text(&variable));
    (!name.is_empty() && !name.eq_ignore_ascii_case("this")).then_some(name)
}

/// The scope-qualified identity of a plain assignment target, if the node
/// assigns one. See [`variable_key`].
pub(super) fn assignment_target_key(base: &BaseExtractor, node: Node) -> Option<String> {
    assignment_target_name(base, node)?;
    let variable = type_facts::assignment_variable_node(node)?;
    Some(variable_key(&base.get_node_text(&variable)))
}

/// Extract a variable symbol from a plain assignment.
pub(super) fn extract_variable(
    base: &mut BaseExtractor,
    node: Node,
    parent_id: Option<&str>,
) -> Option<Symbol> {
    let name = assignment_target_name(base, node)?;
    let raw = base.get_node_text(&type_facts::assignment_variable_node(node)?);
    let is_global = raw
        .trim_start_matches(['$', '{'])
        .to_ascii_lowercase()
        .starts_with("global:");

    let doc_comment = super::documentation::extract_powershell_doc_comment(base, &node);
    let symbol = base.create_symbol(
        &node,
        name,
        SymbolKind::Variable,
        SymbolOptions {
            signature: Some(base.get_node_text(&node).trim().to_string()),
            visibility: Some(if is_global {
                Visibility::Public
            } else {
                Visibility::Private
            }),
            parent_id: parent_id.map(|s| s.to_string()),
            metadata: None,
            doc_comment,
            annotations: Vec::new(),
        },
    );
    type_facts::record_assignment_facts(base, &symbol.id, node);
    Some(symbol)
}
