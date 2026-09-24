use super::FSharpExtractor;
use super::calls::{self, Scope};
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
    let containing_symbols = Scope::new(symbols);
    let mut seen = HashSet::new();
    walk(
        extractor,
        tree.root_node(),
        &containing_symbols,
        &mut seen,
        0,
    );
    literals::collect_literals(extractor, tree.root_node(), &containing_symbols);
    extractor.base().identifiers.clone()
}

fn walk(
    extractor: &mut FSharpExtractor,
    node: Node,
    containing_symbols: &Scope<'_>,
    seen: &mut HashSet<(IdentifierKind, u32, u32)>,
    depth: u32,
) {
    if !should_visit_tree_depth(depth) {
        return;
    }

    match node.kind() {
        "application_expression" | "infix_expression" => {
            let callee = calls::call_callee(node, &extractor.base.content);
            if let Some(callee) = callee
                && let Some((name_node, name)) = call_head(extractor.base(), callee)
            {
                let identifier = emit(
                    extractor,
                    name_node,
                    name,
                    IdentifierKind::Call,
                    containing_symbols,
                    seen,
                );
                if let Some(identifier) = identifier
                    && let Some(types_node) = calls::callee_type_arguments(callee)
                {
                    let arguments = crate::base::extract_type_arguments(
                        extractor.base(),
                        types_node,
                        decompose_type_argument,
                    );
                    extractor
                        .base()
                        .record_type_arguments(&identifier, arguments);
                }
            }
        }
        "dot_expression" if !calls::is_within_callee(node, &extractor.base.content) => {
            if let Some(field_node) = node.child_by_field_name("field")
                && let Some(name_node) = terminal_identifier(field_node)
            {
                let name = extractor.base().get_node_text(&name_node);
                emit(
                    extractor,
                    name_node,
                    name,
                    IdentifierKind::MemberAccess,
                    containing_symbols,
                    seen,
                );
            }
        }
        "long_identifier_or_op"
            if !calls::is_within_callee(node, &extractor.base.content)
                && !is_type_node(node)
                && is_member_path(node) =>
        {
            if let Some(name_node) = terminal_identifier(node) {
                let name = extractor.base().get_node_text(&name_node);
                emit(
                    extractor,
                    name_node,
                    name,
                    IdentifierKind::MemberAccess,
                    containing_symbols,
                    seen,
                );
            }
        }
        "identifier_pattern" if is_union_case_pattern(node, &extractor.base.content) => {
            if let Some(name_node) = first_named_child(node).and_then(terminal_identifier) {
                let name = extractor.base().get_node_text(&name_node);
                emit(
                    extractor,
                    name_node,
                    name,
                    IdentifierKind::VariableRef,
                    containing_symbols,
                    seen,
                );
            }
        }
        "generic_type" => {
            emit_generic_type(extractor, node, containing_symbols, seen);
        }
        "postfix_type" => {
            emit_postfix_type(extractor, node, containing_symbols, seen);
        }
        "long_identifier" if is_type_node(node) => {
            if let Some(name_node) = terminal_identifier(node) {
                let name = extractor.base().get_node_text(&name_node);
                emit(
                    extractor,
                    name_node,
                    name,
                    IdentifierKind::TypeUsage,
                    containing_symbols,
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
                    containing_symbols,
                    seen,
                );
            }
        }
        "identifier" if is_value_read(node, &extractor.base.content) => {
            let name = extractor.base().get_node_text(&node);
            emit(
                extractor,
                node,
                name,
                IdentifierKind::VariableRef,
                containing_symbols,
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
        walk(extractor, child, containing_symbols, seen, child_depth);
    }
}

fn emit_generic_type(
    extractor: &mut FSharpExtractor,
    node: Node,
    containing_symbols: &Scope<'_>,
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
        containing_symbols,
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

/// `Order list`: the postfix name is the generic type and the type before it
/// is its argument.
fn emit_postfix_type(
    extractor: &mut FSharpExtractor,
    node: Node,
    containing_symbols: &Scope<'_>,
    seen: &mut HashSet<(IdentifierKind, u32, u32)>,
) {
    let Some(name_node) = postfix_name(node).and_then(terminal_identifier) else {
        return;
    };
    let name = extractor.base().get_node_text(&name_node);
    let Some(identifier) = emit(
        extractor,
        name_node,
        name,
        IdentifierKind::TypeUsage,
        containing_symbols,
        seen,
    ) else {
        return;
    };
    let arguments =
        crate::base::extract_type_arguments(extractor.base(), node, decompose_type_argument);
    extractor
        .base()
        .record_type_arguments(&identifier, arguments);
}

fn postfix_name(node: Node) -> Option<Node> {
    let mut cursor = node.walk();
    let children: Vec<_> = node.named_children(&mut cursor).collect();
    children
        .into_iter()
        .rev()
        .find(|child| child.kind() == "long_identifier")
}

fn decompose_type_argument<'a>(
    base: &BaseExtractor,
    node: Node<'a>,
) -> Option<(String, Option<Node<'a>>)> {
    if !node.is_named() {
        return None;
    }
    if node.parent().is_some_and(|parent| {
        parent.kind() == "postfix_type"
            && postfix_name(parent).is_some_and(|name| name.id() == node.id())
    }) {
        return None;
    }
    let type_node = if node.kind() == "type_attribute" {
        first_named_child(node)?
    } else {
        node
    };
    if type_node.kind() == "postfix_type" {
        let name = postfix_name(type_node).map(|name| base.get_node_text(&name))?;
        return Some((name.trim().to_string(), Some(type_node)));
    }
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
    containing_symbols: &Scope<'_>,
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
    let containing_symbol_id = containing_symbols
        .find(node)
        .map(|symbol| symbol.id.clone());
    let receiver_type = (kind == IdentifierKind::Call)
        .then(|| instance_receiver_type(&extractor.base, node))
        .flatten();
    Some(extractor.base().create_identifier_with_receiver_type(
        &node,
        name.trim().to_string(),
        kind,
        containing_symbol_id,
        receiver_type,
    ))
}

/// The enclosing type name when a call's receiver is the member's own
/// instance identifier (`this.Helper()` inside `member this.Run`).
pub(super) fn instance_receiver_type(base: &BaseExtractor, node: Node) -> Option<String> {
    let mut callee = node;
    while let Some(parent) = callee.parent().filter(|parent| {
        matches!(
            parent.kind(),
            "dot_expression" | "long_identifier_or_op" | "long_identifier"
        )
    }) {
        callee = parent;
    }
    let receiver = callee_receiver_text(base, callee)?;
    let instance = enclosing_member_instance(base, node)?;
    if receiver != instance {
        return None;
    }
    enclosing_type_name(base, node)
}

fn callee_receiver_text(base: &BaseExtractor, head: Node) -> Option<String> {
    match head.kind() {
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

pub(super) fn enclosing_type_name(base: &BaseExtractor, node: Node) -> Option<String> {
    let mut current = node.parent();
    let body = loop {
        let candidate = current?;
        if candidate.kind() == "object_expression" {
            return None;
        }
        if matches!(
            candidate.kind(),
            "anon_type_defn"
                | "type_extension"
                | "delegate_type_defn"
                | "enum_type_defn"
                | "interface_type_defn"
                | "record_type_defn"
                | "type_abbrev_defn"
                | "union_type_defn"
        ) {
            break candidate;
        }
        current = candidate.parent();
    };
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

fn call_head<'a>(base: &BaseExtractor, head: Node<'a>) -> Option<(Node<'a>, String)> {
    match head.kind() {
        "dot_expression" => {
            let field = head.child_by_field_name("field")?;
            let name_node = terminal_identifier(field)?;
            let name = base.get_node_text(&field);
            Some((name_node, name.trim().rsplit('.').next()?.to_string()))
        }
        "long_identifier_or_op" | "long_identifier" => {
            let name_node = terminal_identifier(head)?;
            Some((name_node, base.get_node_text(&name_node).trim().to_string()))
        }
        _ => None,
    }
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
        if parent.kind() == "typed_expression"
            && first_named_child(parent).is_some_and(|callee| callee.id() == current.id())
        {
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

fn is_value_read(node: Node, source: &str) -> bool {
    if in_declaration_name(node)
        || in_import(node)
        || is_type_node(node)
        || calls::is_within_callee(node, source)
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
        if parent.kind() == "typed_pattern" && is_type_kind(current.kind()) {
            return false;
        }
        match parent.kind() {
            "record_field" | "union_type_field" => return current.kind() == "identifier",
            "exception_definition" => {
                return parent
                    .child_by_field_name("exception_name")
                    .is_some_and(|name| name.id() == current.id());
            }
            "function_declaration_left"
            | "value_declaration_left"
            | "identifier_pattern"
            | "type_name"
            | "union_type_case"
            | "enum_type_case"
            | "property_or_ident"
            | "argument_patterns"
            | "argument_name_spec"
            | "primary_constr_args"
            | "active_pattern" => return true,
            "member_signature" | "declaration_expression" | "extern_binding" | "member_defn"
                if current.kind() == "identifier" =>
            {
                return true;
            }
            "named_module" | "namespace" | "module_defn" => {
                return first_named_child(parent).is_some_and(|name| contains_node(name, node));
            }
            _ => {}
        }
        current = parent;
    }
    false
}

fn is_type_kind(kind: &str) -> bool {
    matches!(
        kind,
        "simple_type"
            | "generic_type"
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
    )
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
        match parent.kind() {
            "rule" => {
                return parent
                    .child_by_field_name("pattern")
                    .is_some_and(|pattern| pattern.id() == current.id());
            }
            "match_expression" => return false,
            "identifier_pattern"
            | "typed_pattern"
            | "record_pattern"
            | "named_field_pattern"
            | "type_check_pattern" => {
                return parent.kind() != "typed_pattern" || current.kind() != "simple_type";
            }
            _ => {}
        }
        current = parent;
    }
    false
}

/// The head of a match-rule pattern that names a union case: `Cash` in
/// `| Cash amount ->`, or a bare capitalised case such as `| Empty ->`.
fn is_union_case_pattern(node: Node, source: &str) -> bool {
    let Some(head) = first_named_child(node).filter(|head| head.kind() == "long_identifier_or_op")
    else {
        return false;
    };
    if !in_rule_pattern(node) {
        return false;
    }
    let has_arguments = node.named_child_count() > 1;
    let is_capitalised = source
        .get(head.start_byte()..head.end_byte())
        .and_then(|text| text.rsplit('.').next())
        .and_then(|name| name.chars().next())
        .is_some_and(char::is_uppercase);
    has_arguments || is_capitalised
}

fn in_rule_pattern(node: Node) -> bool {
    let mut current = node;
    while let Some(parent) = current.parent() {
        if parent.kind() == "rule" {
            return parent
                .child_by_field_name("pattern")
                .is_some_and(|pattern| pattern.id() == current.id());
        }
        if !parent.kind().ends_with("_pattern") {
            return false;
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
