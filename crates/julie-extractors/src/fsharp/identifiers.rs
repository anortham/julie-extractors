use super::FSharpExtractor;
use super::literals;
use crate::base::{BaseExtractor, Identifier, IdentifierKind, Symbol};
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};
use std::collections::HashSet;
use tree_sitter::{Node, Tree};

pub(super) fn extract_identifiers(
    extractor: &mut FSharpExtractor,
    tree: &Tree,
    symbols: &[Symbol],
) -> Vec<Identifier> {
    extractor.base().identifiers.clear();
    extractor.base().literals.clear();
    let mut seen = HashSet::new();
    walk(extractor, tree.root_node(), symbols, &mut seen, 0);
    literals::collect_literals(extractor, tree.root_node(), symbols);
    extractor.base().identifiers.clone()
}

fn walk(
    extractor: &mut FSharpExtractor,
    node: Node,
    symbols: &[Symbol],
    seen: &mut HashSet<(IdentifierKind, u32, u32)>,
    depth: u32,
) {
    if !should_visit_tree_depth(depth) {
        return;
    }

    match node.kind() {
        "application_expression" if !is_nested_application(node) => {
            if let Some((name_node, name)) = call_head(extractor.base(), node) {
                emit(
                    extractor,
                    name_node,
                    name,
                    IdentifierKind::Call,
                    symbols,
                    seen,
                );
            }
        }
        "dot_expression" if !is_within_call_head(node) => {
            if let Some(field_node) = node.child_by_field_name("field")
                && let Some(name_node) = terminal_identifier(field_node)
            {
                let name = extractor.base().get_node_text(&name_node);
                emit(
                    extractor,
                    name_node,
                    name,
                    IdentifierKind::MemberAccess,
                    symbols,
                    seen,
                );
            }
        }
        "long_identifier_or_op"
            if !is_within_call_head(node) && !is_type_node(node) && is_member_path(node) =>
        {
            if let Some(name_node) = terminal_identifier(node) {
                let name = extractor.base().get_node_text(&name_node);
                emit(
                    extractor,
                    name_node,
                    name,
                    IdentifierKind::MemberAccess,
                    symbols,
                    seen,
                );
            }
        }
        "generic_type" => {
            emit_generic_type(extractor, node, symbols, seen);
        }
        "long_identifier" if is_type_node(node) => {
            if let Some(name_node) = terminal_identifier(node) {
                let name = extractor.base().get_node_text(&name_node);
                emit(
                    extractor,
                    name_node,
                    name,
                    IdentifierKind::TypeUsage,
                    symbols,
                    seen,
                );
            }
        }
        "simple_type" if is_type_node(node) => {
            if let Some(name_node) = terminal_identifier(node) {
                let name = extractor.base().get_node_text(&name_node);
                emit(
                    extractor,
                    name_node,
                    name,
                    IdentifierKind::TypeUsage,
                    symbols,
                    seen,
                );
            }
        }
        "identifier" if is_value_read(node) => {
            let name = extractor.base().get_node_text(&node);
            emit(
                extractor,
                node,
                name,
                IdentifierKind::VariableRef,
                symbols,
                seen,
            );
        }
        _ => {}
    }

    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk(extractor, child, symbols, seen, child_depth);
    }
}

fn emit_generic_type(
    extractor: &mut FSharpExtractor,
    node: Node,
    symbols: &[Symbol],
    seen: &mut HashSet<(IdentifierKind, u32, u32)>,
) {
    let Some(type_node) = first_named_child(node) else {
        return;
    };
    let Some(name_node) = terminal_identifier(type_node) else {
        return;
    };
    let name = extractor.base().get_node_text(&name_node);
    let identifier = emit(
        extractor,
        name_node,
        name,
        IdentifierKind::TypeUsage,
        symbols,
        seen,
    );
    let Some(identifier) = identifier else {
        return;
    };
    let Some(arguments_node) = node
        .children(&mut node.walk())
        .find(|child| child.kind() == "type_attributes")
    else {
        return;
    };
    let arguments = crate::base::extract_type_arguments(
        extractor.base(),
        arguments_node,
        decompose_type_argument,
    );
    extractor
        .base()
        .record_type_arguments(&identifier, arguments);
}

fn decompose_type_argument<'a>(
    base: &BaseExtractor,
    node: Node<'a>,
) -> Option<(String, Option<Node<'a>>)> {
    if !node.is_named() {
        return None;
    }
    let type_node = if node.kind() == "type_attribute" {
        first_named_child(node)?
    } else {
        node
    };
    let type_name = if type_node.kind() == "generic_type" {
        first_named_child(type_node)
            .map(|child| base.get_node_text(&child))
            .unwrap_or_else(|| base.get_node_text(&type_node))
    } else {
        base.get_node_text(&type_node)
    };
    if type_name.trim().is_empty() {
        return None;
    }
    let nested = if type_node.kind() == "generic_type" {
        type_node
            .children(&mut type_node.walk())
            .find(|child| child.kind() == "type_attributes")
    } else {
        None
    };
    Some((type_name.trim().to_string(), nested))
}

fn emit(
    extractor: &mut FSharpExtractor,
    node: Node,
    name: String,
    kind: IdentifierKind,
    symbols: &[Symbol],
    seen: &mut HashSet<(IdentifierKind, u32, u32)>,
) -> Option<Identifier> {
    if name.trim().is_empty() {
        return None;
    }
    let key = (
        kind.clone(),
        node.start_byte() as u32,
        node.end_byte() as u32,
    );
    if !seen.insert(key) {
        return extractor
            .base()
            .identifiers
            .iter()
            .find(|identifier| {
                identifier.kind == kind
                    && identifier.start_byte == node.start_byte() as u32
                    && identifier.end_byte == node.end_byte() as u32
            })
            .cloned();
    }
    let containing_symbol_id = extractor
        .base()
        .find_containing_symbol(&node, symbols)
        .map(|symbol| symbol.id.clone());
    let receiver_type = (kind == IdentifierKind::Call)
        .then(|| instance_receiver_type(&extractor.base, node))
        .flatten();
    Some(
        extractor
            .base()
            .create_identifier_with_receiver_type(
                &node,
                name.trim().to_string(),
                kind,
                containing_symbol_id,
                receiver_type,
            ),
    )
}

pub(super) fn instance_receiver_type(base: &BaseExtractor, node: Node) -> Option<String> {
    let application = ancestor_kind(node, "application_expression")?;
    let receiver = call_receiver_text(base, application)?;
    let instance = enclosing_member_instance(base, node)?;
    if receiver != instance {
        return None;
    }
    enclosing_type_name(base, node)
}




fn call_receiver_text(base: &BaseExtractor, node: Node) -> Option<String> {
    let head = first_named_child(node)?;
    match head.kind() {
        "application_expression" => call_receiver_text(base, head),
        "dot_expression" => {
            let receiver_node = head.child_by_field_name("base")?;
            let text = base.get_node_text(&receiver_node);
            let text = text.trim();
            if text.is_empty() {
                None
            } else {
                Some(text.to_string())
            }
        }
        "long_identifier_or_op" | "long_identifier" => {
            let display = base.get_node_text(&head);
            let segments: Vec<_> = display
                .split('.')
                .map(str::trim)
                .filter(|segment| !segment.is_empty())
                .collect();
            if segments.len() < 2 {
                return None;
            }
            let prefix = &segments[..segments.len() - 1];
            if prefix
                .first()
                .is_some_and(|segment| segment.chars().next().is_some_and(char::is_lowercase))
            {
                Some(prefix.join("."))
            } else {
                None
            }
        }
        _ => None,

    }
}

fn enclosing_member_instance(base: &BaseExtractor, node: Node) -> Option<String> {
    let member = ancestor_kind(node, "member_defn")?;
    let mut cursor = member.walk();
    let definition = member
        .children(&mut cursor)
        .find(|child| child.kind() == "method_or_prop_defn")?;
    let name = definition.child_by_field_name("name")?;
    let instance = name.child_by_field_name("instance")?;
    let text = base.get_node_text(&instance);
    let text = text.trim();
    if text.is_empty() {
        None
    } else {
        Some(text.to_string())
    }
}

fn enclosing_type_name(base: &BaseExtractor, node: Node) -> Option<String> {
    let type_definition = ancestor_kind(node, "type_definition")?;
    let mut cursor = type_definition.walk();
    let body = type_definition.children(&mut cursor).find(|child| {
        matches!(
            child.kind(),
            "anon_type_defn"
                | "delegate_type_defn"
                | "enum_type_defn"
                | "interface_type_defn"
                | "record_type_defn"
                | "type_abbrev_defn"
                | "union_type_defn"
        )
    })?;
    let mut body_cursor = body.walk();
    let type_name = body
        .children(&mut body_cursor)
        .find(|child| child.kind() == "type_name")?;
    let name_node = type_name.child_by_field_name("type_name")?;
    let text = base.get_node_text(&name_node);
    let text = text.trim();
    if text.is_empty() {
        None
    } else {
        Some(text.to_string())
    }
}

fn ancestor_kind<'a>(node: Node<'a>, kind: &str) -> Option<Node<'a>> {
    let mut current = Some(node);
    while let Some(candidate) = current {
        if candidate.kind() == kind {
            return Some(candidate);
        }
        current = candidate.parent();
    }
    None
}

fn call_head<'a>(base: &BaseExtractor, node: Node<'a>) -> Option<(Node<'a>, String)> {
    let head = first_named_child(node)?;
    match head.kind() {
        "application_expression" => call_head(base, head),
        "dot_expression" => {
            let field = head.child_by_field_name("field")?;
            let name_node = terminal_identifier(field)?;
            let receiver = head.child_by_field_name("base")?;
            let name = format!(
                "{}.{}",
                base.get_node_text(&receiver).trim(),
                base.get_node_text(&field).trim()
            );
            Some((name_node, name.rsplit('.').next()?.to_string()))
        }
        "long_identifier_or_op" | "long_identifier" => {
            let name_node = terminal_identifier(head)?;
            Some((name_node, base.get_node_text(&name_node).trim().to_string()))
        }
        _ => None,
    }
}

fn is_nested_application(node: Node) -> bool {
    node.parent()
        .is_some_and(|parent| parent.kind() == "application_expression")
}

fn is_within_call_head(node: Node) -> bool {
    let mut current = node;
    while let Some(parent) = current.parent() {
        if parent.kind() == "application_expression" {
            return first_named_child(parent).is_some_and(|head| contains_node(head, node));
        }
        current = parent;
    }
    false
}

fn is_type_node(node: Node) -> bool {
    if in_declaration_name(node) || in_import(node) {
        return false;
    }
    let mut current = node;
    while let Some(parent) = current.parent() {
        if in_declaration_name(parent) || in_import(parent) {
            return false;
        }
        if matches!(
            parent.kind(),
            "simple_type"
                | "generic_type"
                | "type_attribute"
                | "atomic_type"
                | "compound_type"
                | "constrained_type"
                | "flexible_type"
                | "function_type"
                | "list_type"
                | "paren_type"
                | "postfix_type"
                | "static_type"
                | "struct_type"
                | "tuple_type"
                | "typecast_expression"
                | "typed_expression"
                | "typed_pattern"
                | "type_check_pattern"
                | "types"
        ) {
            return true;
        }
        current = parent;
    }
    false
}

fn is_value_read(node: Node) -> bool {
    if in_declaration_name(node)
        || in_import(node)
        || is_type_node(node)
        || is_within_call_head(node)
        || in_member_path(node)
        || is_dot_field(node)
        || in_pattern(node)
        || in_attribute(node)
    {
        return false;
    }
    true
}

fn in_declaration_name(node: Node) -> bool {
    let mut current = node;
    while let Some(parent) = current.parent() {
        if matches!(
            parent.kind(),
            "function_declaration_left"
                | "value_declaration_left"
                | "identifier_pattern"
                | "type_name"
                | "record_field"
                | "union_type_case"
                | "union_type_field"
                | "property_or_ident"
        ) {
            return true;
        }
        if matches!(parent.kind(), "named_module" | "namespace" | "module_defn") {
            return first_named_child(parent).is_some_and(|name| contains_node(name, node));
        }
        current = parent;
    }
    false
}

fn in_import(node: Node) -> bool {
    let mut current = node;
    while let Some(parent) = current.parent() {
        if parent.kind() == "import_decl" {
            return true;
        }
        current = parent;
    }
    false
}

fn in_pattern(node: Node) -> bool {
    let mut current = node;
    while let Some(parent) = current.parent() {
        if matches!(
            parent.kind(),
            "identifier_pattern"
                | "typed_pattern"
                | "record_pattern"
                | "named_field_pattern"
                | "type_check_pattern"
                | "match_expression"
        ) {
            return parent.kind() != "typed_pattern" || current.kind() != "simple_type";
        }
        current = parent;
    }
    false
}

fn in_attribute(node: Node) -> bool {
    let mut current = node;
    while let Some(parent) = current.parent() {
        if parent.kind() == "attribute" || parent.kind() == "attributes" {
            return true;
        }
        current = parent;
    }
    false
}

fn is_member_path(node: Node) -> bool {
    let Some(long_identifier) = first_named_child(node) else {
        return false;
    };
    if long_identifier.kind() != "long_identifier" {
        return false;
    }
    long_identifier
        .named_children(&mut long_identifier.walk())
        .filter(|child| child.kind() == "identifier")
        .count()
        > 1
}

fn in_member_path(node: Node) -> bool {
    let mut current = node;
    while let Some(parent) = current.parent() {
        if parent.kind() == "long_identifier_or_op" && is_member_path(parent) {
            return true;
        }
        current = parent;
    }
    false
}

fn is_dot_field(node: Node) -> bool {
    let mut current = node;
    while let Some(parent) = current.parent() {
        if parent.kind() == "dot_expression"
            && parent
                .child_by_field_name("field")
                .is_some_and(|field| contains_node(field, node))
        {
            return true;
        }
        current = parent;
    }
    false
}

fn terminal_identifier(node: Node) -> Option<Node> {
    if node.kind() == "identifier" {
        return Some(node);
    }
    let mut cursor = node.walk();
    let children: Vec<_> = node.named_children(&mut cursor).collect();
    children.into_iter().rev().find_map(terminal_identifier)
}

fn first_named_child(node: Node) -> Option<Node> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor).next()
}

fn contains_node(outer: Node, inner: Node) -> bool {
    outer.start_byte() <= inner.start_byte() && outer.end_byte() >= inner.end_byte()
}
