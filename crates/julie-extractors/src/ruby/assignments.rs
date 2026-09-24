use super::helpers::infer_symbol_kind_from_assignment;
use super::locals::LocalBindings;
use super::return_types::ReturnTypeIndex;
use super::type_facts;
use crate::base::{BaseExtractor, Symbol, SymbolKind, SymbolOptions, Visibility};
use std::collections::{HashMap, HashSet};
use tree_sitter::Node;

/// Per-file state that assignment extraction reads and updates.
pub(super) struct AssignmentContext<'a> {
    pub(super) same_file_class_names: &'a HashSet<String>,
    pub(super) return_types: &'a ReturnTypeIndex,
    pub(super) locals: &'a mut LocalBindings,
    pub(super) recorded_fields: &'a mut HashSet<(Option<String>, String)>,
    pub(super) recorded_locals: &'a mut HashSet<(Option<String>, String)>,
    pub(super) literal_types: &'a mut HashMap<String, String>,
    pub(super) symbol_map: &'a mut HashMap<String, Symbol>,
}

/// Extract a symbol from an assignment node
pub(super) fn extract_assignment(
    base: &mut BaseExtractor,
    node: Node,
    parent_id: Option<String>,
    context: AssignmentContext<'_>,
) -> Option<Symbol> {
    // Handle various assignment patterns including parallel assignment
    let left_side = node
        .child_by_field_name("left")
        .or_else(|| node.children(&mut node.walk()).next())?;

    // `obj.attr = x` and `h[k] = x` call writer methods; they define nothing.
    if matches!(left_side.kind(), "call" | "element_reference") {
        return None;
    }

    // Handle parallel assignments (a, b, c = 1, 2, 3)
    if left_side.kind() == "left_assignment_list" {
        return handle_parallel_assignment(base, node, left_side, parent_id, context.symbol_map);
    }

    // Handle regular assignments
    let right_side = node
        .child_by_field_name("right")
        .or_else(|| node.children(&mut node.walk()).last());
    let name = base.get_node_text(&left_side);
    let signature = if node.kind() == "operator_assignment" {
        base.get_node_text(&node)
    } else if let Some(right) = right_side {
        format!("{} = {}", name, base.get_node_text(&right))
    } else {
        name.clone()
    };

    let kind = infer_symbol_kind_from_assignment(&left_side, |n| base.get_node_text(n));
    if left_side.kind() == "identifier"
        && !context
            .recorded_locals
            .insert((parent_id.clone(), name.clone()))
    {
        return None;
    }
    let parent_id = if kind == SymbolKind::Field {
        let class_parent = class_parent_id(context.symbol_map, parent_id.clone());
        if !context
            .recorded_fields
            .insert((class_parent.clone(), name.clone()))
        {
            return None;
        }
        class_parent
    } else {
        parent_id
    };
    let mixed_level_ivar = context.return_types.ivar_has_mixed_levels(base, node);
    let record_type = matches!(kind, SymbolKind::Variable | SymbolKind::Field) && !mixed_level_ivar;
    let symbol = base.create_symbol(
        &node,
        name,
        kind,
        SymbolOptions {
            signature: Some(signature),
            visibility: Some(Visibility::Public),
            parent_id,
            metadata: None,
            doc_comment: None,
            annotations: Vec::new(),
        },
    );
    if !mixed_level_ivar
        && node.kind() == "assignment"
        && !type_facts::has_trailing_written_type(base, node)
        && let Some(literal_type) = right_side.and_then(literal_type)
    {
        context
            .literal_types
            .insert(symbol.id.clone(), literal_type.to_string());
    }
    if record_type
        && assigns_value(base, node)
        && let Some(right) = right_side
    {
        type_facts::record_assignment_type(
            base,
            &symbol.id,
            node,
            right,
            type_facts::InitializerContext {
                same_file_class_names: context.same_file_class_names,
                return_types: context.return_types,
                locals: context.locals,
            },
        );
    }
    Some(symbol)
}

/// Whether the target takes the right-hand value itself: `=` or `||=`, not
/// an arithmetic operator assignment such as `+=`.
fn assigns_value(base: &BaseExtractor, node: Node) -> bool {
    node.kind() == "assignment"
        || node
            .child_by_field_name("operator")
            .is_some_and(|operator| base.get_node_text(&operator) == "||=")
}

/// The core class of a literal right-hand side, read from its node kind.
fn literal_type(value: Node) -> Option<&'static str> {
    Some(match value.kind() {
        "string" | "chained_string" | "heredoc_beginning" => "String",
        "array" | "string_array" | "symbol_array" => "Array",
        "hash" => "Hash",
        "simple_symbol" | "delimited_symbol" => "Symbol",
        "regex" => "Regexp",
        "true" | "false" => "Boolean",
        "nil" => "NilClass",
        "integer" => "Integer",
        "float" => "Float",
        "range" => "Range",
        _ => return None,
    })
}

fn class_parent_id(
    symbol_map: &HashMap<String, Symbol>,
    mut parent_id: Option<String>,
) -> Option<String> {
    while let Some(id) = parent_id {
        let symbol = symbol_map.get(&id)?;
        if matches!(symbol.kind, SymbolKind::Class | SymbolKind::Module) {
            return Some(id);
        }
        parent_id = symbol.parent_id.clone();
    }
    None
}

/// Handle parallel assignment patterns (a, b, c = 1, 2, 3)
fn handle_parallel_assignment(
    base: &mut BaseExtractor,
    node: Node,
    left_side: Node,
    parent_id: Option<String>,
    symbol_map: &mut HashMap<String, Symbol>,
) -> Option<Symbol> {
    let full_assignment = base.get_node_text(&node);

    // Extract identifiers from left_assignment_list
    let mut cursor = left_side.walk();
    let identifiers: Vec<_> = left_side
        .children(&mut cursor)
        .filter(|child| child.kind() == "identifier")
        .collect();

    // Extract rest assignments (splat expressions like *rest)
    let mut cursor = left_side.walk();
    let rest_assignments: Vec<_> = left_side
        .children(&mut cursor)
        .filter(|child| child.kind() == "rest_assignment")
        .collect();

    // Create symbols for identifiers
    let mut created_symbols = Vec::new();

    for identifier in &identifiers {
        let name = base.get_node_text(identifier);
        let symbol = base.create_symbol(
            &node,
            name,
            SymbolKind::Variable,
            SymbolOptions {
                signature: Some(full_assignment.clone()),
                visibility: Some(Visibility::Public),
                parent_id: parent_id.clone(),
                metadata: None,
                doc_comment: None,
                annotations: Vec::new(),
            },
        );
        created_symbols.push(symbol);
    }

    // Handle rest assignments
    for rest_node in &rest_assignments {
        if let Some(rest_identifier) = rest_node
            .children(&mut rest_node.walk())
            .find(|c| c.kind() == "identifier")
        {
            let rest_name = base.get_node_text(&rest_identifier);
            let rest_symbol = base.create_symbol(
                &node,
                rest_name,
                SymbolKind::Variable,
                SymbolOptions {
                    signature: Some(full_assignment.clone()),
                    visibility: Some(Visibility::Public),
                    parent_id: parent_id.clone(),
                    metadata: None,
                    doc_comment: None,
                    annotations: Vec::new(),
                },
            );
            created_symbols.push(rest_symbol);
        }
    }

    // Store additional symbols in symbol_map
    // Since this method only returns one symbol, we add the rest to the symbol_map
    for symbol in created_symbols.iter().skip(1) {
        symbol_map.insert(symbol.id.clone(), symbol.clone());
    }

    // Return the first symbol (if any were created)
    created_symbols.into_iter().next()
}
