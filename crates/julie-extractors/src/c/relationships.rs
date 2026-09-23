//! Relationship extraction for function calls and imports
//!
//! This module handles extraction of relationships between symbols, such as function calls
//! and header file imports.

use crate::base::{
    BaseExtractor, LocalTargetResolution, Relationship, RelationshipKind, ScopedSymbolIndex,
    Symbol, SymbolKind, UnresolvedTarget,
};
use crate::c::CExtractor;
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};
use std::collections::HashMap;

use super::helpers;

/// Extract relationships from nodes in the tree
pub(super) fn extract_relationships_from_node(
    extractor: &mut CExtractor,
    node: tree_sitter::Node,
    symbols: &[Symbol],
    relationships: &mut Vec<Relationship>,
) {
    let scoped_index = ScopedSymbolIndex::new(symbols);
    walk_relationships(extractor, node, symbols, &scoped_index, relationships, 0);
}

fn walk_relationships(
    extractor: &mut CExtractor,
    node: tree_sitter::Node,
    symbols: &[Symbol],
    scoped_index: &ScopedSymbolIndex<'_>,
    relationships: &mut Vec<Relationship>,
    depth: u32,
) {
    if !should_visit_tree_depth(depth) || helpers::is_attribute_node(node) {
        return;
    }

    match node.kind() {
        "call_expression"
            if !super::test_calls::is_criterion_macro_call(&extractor.base, &node)
                && !helpers::is_static_assertion(&extractor.base, node) =>
        {
            extract_function_call_relationships(
                extractor,
                node,
                symbols,
                scoped_index,
                relationships,
            );
        }
        "preproc_include" => {
            relationships.extend(include_relationship(&extractor.base, node));
        }
        "type_identifier" => {
            extract_type_use_relationship(extractor, node, symbols, scoped_index, relationships);
        }
        _ => {}
    }

    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk_relationships(
            extractor,
            child,
            symbols,
            scoped_index,
            relationships,
            child_depth,
        );
    }
}

/// Extract function call relationships
fn extract_function_call_relationships(
    extractor: &mut CExtractor,
    node: tree_sitter::Node,
    symbols: &[Symbol],
    scoped_index: &ScopedSymbolIndex<'_>,
    relationships: &mut Vec<Relationship>,
) {
    let Some(function_node) = node.child_by_field_name("function") else {
        return;
    };
    let target_token = helpers::callee_token(function_node);

    let Some((unresolved_target, is_indirect)) =
        call_target_from_function_node(extractor, function_node)
    else {
        return;
    };
    let Some(containing_symbol) = find_containing_symbol(extractor, node, symbols) else {
        return;
    };
    let containing_symbol_id = containing_symbol.id.clone();
    let pending_confidence = if is_indirect { 0.45 } else { 0.7 };
    let relationship_confidence = if is_indirect { 0.5 } else { 1.0 };

    let called_symbol = match scoped_index.resolve_call_target(
        &unresolved_target.terminal_name,
        Some(containing_symbol),
        unresolved_target.receiver.as_deref(),
    ) {
        LocalTargetResolution::Resolved(called_symbol) => Some(called_symbol),
        LocalTargetResolution::Missing if unresolved_target.receiver.is_none() => {
            function_pointer_variable(symbols, &unresolved_target.terminal_name, containing_symbol)
        }
        _ => None,
    };
    match called_symbol {
        Some(called_symbol) => {
            relationships.push(extractor.get_base_mut().create_relationship_at_target(
                containing_symbol_id,
                called_symbol.id.clone(),
                RelationshipKind::Calls,
                &target_token,
                Some(relationship_confidence),
                None,
            ));
        }
        None => {
            let pending = extractor
                .get_base_mut()
                .create_pending_relationship_at_target(
                    containing_symbol_id.clone(),
                    unresolved_target,
                    RelationshipKind::Calls,
                    &target_token,
                    Some(containing_symbol_id),
                    Some(pending_confidence),
                );
            extractor.add_structured_pending_relationship(pending);
        }
    }
}

/// A call through a function-pointer variable calls that variable: the caller's
/// own local first, then a file-scope variable.
fn function_pointer_variable<'a>(
    symbols: &'a [Symbol],
    name: &str,
    caller: &Symbol,
) -> Option<&'a Symbol> {
    let candidates: Vec<&Symbol> = symbols
        .iter()
        .filter(|symbol| {
            symbol.kind == SymbolKind::Variable
                && symbol.name == name
                && symbol
                    .metadata
                    .as_ref()
                    .and_then(|m| m.get("isFunctionPointer"))
                    .and_then(|v| v.as_str())
                    == Some("true")
        })
        .collect();
    let in_scope = |parent: Option<&str>| {
        let matching: Vec<&Symbol> = candidates
            .iter()
            .copied()
            .filter(|symbol| symbol.parent_id.as_deref() == parent)
            .collect();
        (matching.len() == 1).then(|| matching[0])
    };
    in_scope(Some(caller.id.as_str())).or_else(|| in_scope(None))
}

fn call_target_from_function_node(
    extractor: &mut CExtractor,
    function_node: tree_sitter::Node,
) -> Option<(UnresolvedTarget, bool)> {
    let callee = helpers::unwrapped_callee(function_node);
    let dereferenced = callee.id() != function_node.id();
    match callee.kind() {
        "identifier" => {
            let function_name = extractor.get_base_mut().get_node_text(&callee);
            Some((UnresolvedTarget::simple(function_name), dereferenced))
        }
        "field_expression" => {
            let field_node = callee.child_by_field_name("field")?;
            let terminal_name = extractor.get_base_mut().get_node_text(&field_node);
            let expression_text = extractor.get_base_mut().get_node_text(&callee);
            let target = UnresolvedTarget::from_qualified_text(&expression_text, &[".", "->"])
                .unwrap_or_else(|| {
                    let receiver = expression_text
                        .rsplit_once("->")
                        .or_else(|| expression_text.rsplit_once('.'))
                        .map(|(left, _)| left.trim().to_string())
                        .filter(|left| !left.is_empty());
                    match receiver {
                        Some(receiver) => UnresolvedTarget {
                            display_name: expression_text,
                            terminal_name,
                            receiver: Some(receiver),
                            namespace_path: Vec::new(),
                            import_context: None,
                        },
                        None => UnresolvedTarget::simple(terminal_name),
                    }
                });
            Some((target, true))
        }
        _ => {
            let identifier = helpers::find_deepest_identifier(function_node)?;
            let function_name = extractor.get_base_mut().get_node_text(&identifier);
            Some((UnresolvedTarget::simple(function_name), true))
        }
    }
}

fn extract_type_use_relationship(
    extractor: &mut CExtractor,
    node: tree_sitter::Node,
    symbols: &[Symbol],
    scoped_index: &ScopedSymbolIndex<'_>,
    relationships: &mut Vec<Relationship>,
) {
    if helpers::is_type_declaration_name(node)
        || helpers::is_generic_default_label(&extractor.base, node)
    {
        return;
    }

    let type_name = extractor.get_base_mut().get_node_text(&node);
    let Some(source_symbol) = source_symbol_for_type_use(extractor, node, symbols) else {
        return;
    };
    let source_symbol_id = source_symbol.id.clone();

    if let Some(target_symbol) = resolve_type_target(scoped_index, &type_name) {
        if target_symbol.id == source_symbol_id {
            return;
        }
        push_unique_relationship(
            relationships,
            extractor.get_base_mut().create_relationship_at_target(
                source_symbol_id,
                target_symbol.id.clone(),
                RelationshipKind::Uses,
                &node,
                Some(0.8),
                None,
            ),
        );
    } else {
        let pending = extractor
            .get_base_mut()
            .create_pending_relationship_at_target(
                source_symbol_id.clone(),
                UnresolvedTarget::simple(type_name),
                RelationshipKind::Uses,
                &node,
                Some(source_symbol_id),
                Some(0.7),
            );
        extractor.add_structured_pending_relationship(pending);
    }
}

/// The file-to-header import an include directive declares; C++ shares it.
pub(crate) fn include_relationship(
    base: &BaseExtractor,
    node: tree_sitter::Node,
) -> Option<Relationship> {
    let include_path = helpers::extract_include_path(&base.get_node_text(&node))?;
    let from_id = format!("file:{}", base.file_path);
    let to_id = format!("header:{}", include_path);
    Some(Relationship {
        id: format!(
            "{}_{}_{:?}_{}",
            from_id,
            to_id,
            RelationshipKind::Imports,
            node.start_position().row
        ),
        from_symbol_id: from_id,
        to_symbol_id: to_id,
        kind: RelationshipKind::Imports,
        file_path: base.file_path.clone(),
        line_number: (node.start_position().row + 1) as u32,
        span: Some(crate::base::NormalizedSpan::from_node(&node)),
        reference_site_is_exact: false,
        confidence: 1.0,
        metadata: Some(HashMap::from([(
            "includePath".to_string(),
            serde_json::Value::String(include_path),
        )])),
    })
}

/// Find the symbol that contains this node
fn find_containing_symbol<'a>(
    extractor: &CExtractor,
    node: tree_sitter::Node,
    symbols: &'a [Symbol],
) -> Option<&'a Symbol> {
    // Reuse the shared BaseExtractor containment logic so we correctly match
    // call expressions with their enclosing function definition (standard approach).
    extractor.base.find_containing_symbol(&node, symbols)
}

fn source_symbol_for_type_use<'a>(
    extractor: &CExtractor,
    node: tree_sitter::Node,
    symbols: &'a [Symbol],
) -> Option<&'a Symbol> {
    let containing = extractor.base.find_containing_symbol(&node, symbols)?;
    if matches!(containing.kind, SymbolKind::Field | SymbolKind::Property)
        && let Some(parent_id) = containing.parent_id.as_deref()
        && let Some(parent) = symbols.iter().find(|symbol| symbol.id == parent_id)
    {
        return Some(parent);
    }
    Some(containing)
}

fn resolve_type_target<'a>(
    scoped_index: &'a ScopedSymbolIndex<'a>,
    type_name: &str,
) -> Option<&'a Symbol> {
    let candidates: Vec<&Symbol> = scoped_index
        .candidates_by_name(type_name)
        .filter(|symbol| is_type_symbol(&symbol.kind))
        .collect();
    if let [candidate] = candidates.as_slice() {
        return Some(*candidate);
    }

    let top_level: Vec<&Symbol> = candidates
        .iter()
        .copied()
        .filter(|symbol| symbol.parent_id.is_none())
        .collect();
    if let [candidate] = top_level.as_slice() {
        return Some(*candidate);
    }

    None
}

fn is_type_symbol(kind: &SymbolKind) -> bool {
    matches!(
        kind,
        SymbolKind::Class
            | SymbolKind::Struct
            | SymbolKind::Union
            | SymbolKind::Enum
            | SymbolKind::Type
            | SymbolKind::Interface
            | SymbolKind::Trait
    )
}

fn push_unique_relationship(relationships: &mut Vec<Relationship>, relationship: Relationship) {
    if relationships.iter().any(|existing| {
        existing.kind == relationship.kind
            && existing.from_symbol_id == relationship.from_symbol_id
            && existing.to_symbol_id == relationship.to_symbol_id
    }) {
        return;
    }
    relationships.push(relationship);
}
