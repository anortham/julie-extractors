//! PowerShell relationship extraction
//! Handles inheritance, method calls, and other symbol relationships

use crate::base::{
    BaseExtractor, Relationship, RelationshipKind, Symbol, SymbolKind, UnresolvedTarget,
};
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};
use tree_sitter::Node;

use super::helpers::{class_base_name_nodes, find_class_name_node, invoked_command};

/// Extract relationships from the AST
pub(super) fn walk_tree_for_relationships(
    extractor: &mut super::PowerShellExtractor,
    node: Node,
    symbols: &[Symbol],
    relationships: &mut Vec<Relationship>,
    depth: u32,
) {
    if !should_visit_tree_depth(depth) {
        return;
    }

    match node.kind() {
        "command" => {
            extract_command_relationships(extractor, node, symbols, relationships);
        }
        "invocation_expression" | "invokation_expression" => {
            extract_invocation_relationships(extractor, node, symbols);
        }
        "class_definition" | "class_statement" => {
            extract_inheritance_relationships(extractor, node, symbols, relationships);
        }
        _ => {}
    }

    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk_tree_for_relationships(extractor, child, symbols, relationships, child_depth);
    }
}

/// Extract relationships from command calls. The caller is the innermost
/// function, method, constructor, or Pester block that contains the command.
fn extract_command_relationships(
    extractor: &mut super::PowerShellExtractor,
    node: Node,
    symbols: &[Symbol],
    relationships: &mut Vec<Relationship>,
) {
    let Some((command_name_node, command_name)) = invoked_command(&extractor.base, node) else {
        return;
    };
    let Some(caller) = extractor
        .base
        .find_containing_symbol(&node, symbols)
        .filter(|symbol| is_call_scope(symbol) && symbol.start_byte != node.start_byte() as u32)
    else {
        return;
    };

    match local_command_target(symbols, &command_name) {
        Some(command_symbol) => {
            if caller.id != command_symbol.id {
                relationships.push(extractor.base.create_relationship_at_target(
                    caller.id.clone(),
                    command_symbol.id.clone(),
                    RelationshipKind::Calls,
                    &command_name_node,
                    None,
                    None,
                ));
            }
        }
        None if !super::commands::is_builtin_cmdlet(&command_name) => {
            let pending = extractor.base.create_pending_relationship_at_target(
                caller.id.clone(),
                UnresolvedTarget::simple(command_name),
                RelationshipKind::Calls,
                &command_name_node,
                Some(caller.id.clone()),
                Some(0.7),
            );
            extractor.add_structured_pending_relationship(pending);
        }
        None => {}
    }
}

/// The same-file function a command name runs, matched without regard to
/// case as PowerShell does, directly or through a same-file alias
/// (`Set-Alias gt Get-Thing`). An ambiguous name resolves to nothing.
fn local_command_target<'a>(symbols: &'a [Symbol], command_name: &str) -> Option<&'a Symbol> {
    let unique_function = |name: &str| {
        let mut matches = symbols.iter().filter(|symbol| {
            symbol.kind == SymbolKind::Function
                && is_command_function(symbol)
                && symbol.name.eq_ignore_ascii_case(name)
        });
        let first = matches.next()?;
        matches.next().is_none().then_some(first)
    };
    unique_function(command_name).or_else(|| {
        symbols
            .iter()
            .filter(|symbol| {
                symbol.kind == SymbolKind::Import && symbol.name.eq_ignore_ascii_case(command_name)
            })
            .find_map(|alias| {
                let target = alias.metadata.as_ref()?.get("aliasTarget")?.as_str()?;
                unique_function(target)
            })
    })
}

/// Functions a command can call: declared functions, not Pester blocks or
/// build tasks, which share the function kind.
fn is_command_function(symbol: &Symbol) -> bool {
    symbol.metadata.as_ref().is_none_or(|metadata| {
        ["role", "is_test", "test_container"]
            .iter()
            .all(|key| !metadata.contains_key(*key))
    })
}

fn is_call_scope(symbol: &Symbol) -> bool {
    matches!(
        symbol.kind,
        SymbolKind::Function | SymbolKind::Method | SymbolKind::Constructor
    )
}

/// Extract base-type relationships of a class. .NET naming decides the kind
/// of a cross-file base: `IName` is an interface, anything else a base class.
fn extract_inheritance_relationships(
    extractor: &mut super::PowerShellExtractor,
    node: Node,
    symbols: &[Symbol],
    relationships: &mut Vec<Relationship>,
) {
    let Some(class_symbol) = find_class_name_node(node).and_then(|name_node| {
        let name_span = name_node.start_byte() as u32;
        symbols.iter().find(|symbol| {
            symbol.kind == SymbolKind::Class
                && symbol.start_byte <= name_span
                && name_span < symbol.end_byte
        })
    }) else {
        return;
    };

    for base_node in class_base_name_nodes(node) {
        let base_name = extractor.base.get_node_text(&base_node);
        match symbols.iter().find(|symbol| {
            symbol.kind == SymbolKind::Class && symbol.name.eq_ignore_ascii_case(&base_name)
        }) {
            Some(base_class) => relationships.push(extractor.base.create_relationship_at_target(
                class_symbol.id.clone(),
                base_class.id.clone(),
                RelationshipKind::Extends,
                &base_node,
                None,
                None,
            )),
            None => {
                let kind = if is_interface_name(&base_name) {
                    RelationshipKind::Implements
                } else {
                    RelationshipKind::Extends
                };
                let pending = extractor.base.create_pending_relationship_at_target(
                    class_symbol.id.clone(),
                    UnresolvedTarget::simple(base_name),
                    kind,
                    &base_node,
                    Some(class_symbol.id.clone()),
                    Some(0.9),
                );
                extractor.add_structured_pending_relationship(pending);
            }
        }
    }
}

fn is_interface_name(name: &str) -> bool {
    let mut chars = name.chars();
    matches!((chars.next(), chars.next()), (Some('I'), Some(c)) if c.is_ascii_uppercase())
}

fn extract_invocation_relationships(
    extractor: &mut super::PowerShellExtractor,
    node: Node,
    symbols: &[Symbol],
) {
    let Some((name_node, method_name)) =
        super::type_facts::invocation_member_name(&extractor.base, node)
    else {
        return;
    };
    let Some(caller) = extractor
        .base
        .find_containing_symbol(&node, symbols)
        .filter(|symbol| is_call_scope(symbol))
    else {
        return;
    };
    let receiver_type = super::type_facts::this_receiver_type(&extractor.base, node);
    let pending = extractor
        .base
        .create_pending_relationship_at_target(
            caller.id.clone(),
            qualified_call_target(&extractor.base, node, method_name),
            RelationshipKind::Calls,
            &name_node,
            Some(caller.id.clone()),
            Some(0.7),
        )
        .with_receiver_type(receiver_type);
    extractor.add_structured_pending_relationship(pending);
}

fn qualified_call_target(
    base: &BaseExtractor,
    node: Node,
    method_name: String,
) -> UnresolvedTarget {
    let mut parts = vec![method_name.clone()];
    let mut current = node.named_child(0);
    while let Some(receiver) = current {
        match receiver.kind() {
            "member_access" => {
                let Some(member) = super::type_facts::invocation_member_name(base, receiver) else {
                    break;
                };
                parts.push(member.1);
                current = receiver.named_child(0);
            }
            "type_literal" => {
                let type_name = base.get_node_text(&receiver);
                let type_name = type_name.trim_matches(|c| c == '[' || c == ']');
                parts.extend(type_name.rsplit('.').map(str::to_string));
                break;
            }
            "variable" => {
                let variable = super::helpers::variable_name(&base.get_node_text(&receiver));
                if !variable.eq_ignore_ascii_case("this") {
                    parts.push(variable);
                }
                break;
            }
            _ => break,
        }
    }
    parts.reverse();
    UnresolvedTarget::from_qualified_text(&parts.join("."), &["."])
        .unwrap_or_else(|| UnresolvedTarget::simple(method_name))
}
