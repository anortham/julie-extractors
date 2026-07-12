use tree_sitter::{Node, Tree};

use super::RAZOR_ROUTE_REFERENCE_PATTERN_ID;
use super::helpers::{base_metadata, fact_for_node, fact_for_span, insert_string, node_text};
use super::static_arg::{StaticArgLang, static_route_arg};
use crate::base::span::NormalizedSpan;
use crate::base::types::StructuralFact;

struct ReceiverDeclaration {
    name: String,
    navigation_manager: bool,
    start_byte: usize,
    scope_start: usize,
    scope_end: usize,
    order_independent: bool,
}

pub(super) fn collect_blazor_navigation_facts(
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
) -> Vec<StructuralFact> {
    let mut declarations = Vec::new();
    collect_receiver_declarations(tree.root_node(), content, &mut declarations);

    let mut facts = Vec::new();
    collect_navigation_calls(
        tree.root_node(),
        language,
        file_path,
        content,
        &declarations,
        &mut facts,
    );
    if language == "razor" {
        collect_razor_hrefs(tree.root_node(), file_path, content, &mut facts);
    }
    facts
}

fn collect_receiver_declarations(
    node: Node<'_>,
    content: &str,
    declarations: &mut Vec<ReceiverDeclaration>,
) {
    match node.kind() {
        "variable_declaration" => {
            if let Some(type_name) = node
                .child_by_field_name("type")
                .and_then(|child| node_text(content, child))
            {
                let mut cursor = node.walk();
                for child in node.named_children(&mut cursor) {
                    if child.kind() == "variable_declarator" {
                        insert_declaration(child, node, type_name, content, declarations);
                    }
                }
            }
        }
        "parameter" | "property_declaration" => {
            if let Some(type_name) = node
                .child_by_field_name("type")
                .and_then(|child| node_text(content, child))
            {
                insert_declaration(node, node, type_name, content, declarations);
            }
        }
        _ => {}
    }

    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        collect_receiver_declarations(child, content, declarations);
    }
}

fn insert_declaration(
    name_owner: Node<'_>,
    declaration: Node<'_>,
    type_name: &str,
    content: &str,
    declarations: &mut Vec<ReceiverDeclaration>,
) {
    let Some(name) = name_owner
        .child_by_field_name("name")
        .and_then(|child| node_text(content, child))
    else {
        return;
    };
    let Some(scope) = declaration_scope(declaration) else {
        return;
    };
    declarations.push(ReceiverDeclaration {
        name: name.to_string(),
        navigation_manager: is_navigation_manager_type(type_name),
        start_byte: declaration.start_byte(),
        scope_start: scope.start_byte(),
        scope_end: scope.end_byte(),
        order_independent: matches!(
            scope.kind(),
            "class_declaration" | "struct_declaration" | "record_declaration" | "compilation_unit"
        ),
    });
}

fn declaration_scope(mut node: Node<'_>) -> Option<Node<'_>> {
    while let Some(parent) = node.parent() {
        if matches!(
            parent.kind(),
            "block"
                | "razor_block"
                | "method_declaration"
                | "constructor_declaration"
                | "local_function_statement"
                | "lambda_expression"
                | "anonymous_method_expression"
                | "class_declaration"
                | "struct_declaration"
                | "record_declaration"
                | "compilation_unit"
        ) {
            return Some(parent);
        }
        node = parent;
    }
    None
}

fn is_navigation_manager_type(value: &str) -> bool {
    value == "NavigationManager" || value.ends_with(".NavigationManager")
}

fn collect_navigation_calls(
    node: Node<'_>,
    language: &str,
    file_path: &str,
    content: &str,
    declarations: &[ReceiverDeclaration],
    facts: &mut Vec<StructuralFact>,
) {
    if node.kind() == "invocation_expression"
        && let Some(fact) = navigation_call_fact(node, language, file_path, content, declarations)
    {
        facts.push(fact);
    }

    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        collect_navigation_calls(child, language, file_path, content, declarations, facts);
    }
}

fn navigation_call_fact(
    invocation: Node<'_>,
    language: &str,
    file_path: &str,
    content: &str,
    declarations: &[ReceiverDeclaration],
) -> Option<StructuralFact> {
    let function = invocation.child_by_field_name("function")?;
    if function.kind() != "member_access_expression" {
        return None;
    }
    let method = function
        .child_by_field_name("name")
        .and_then(|node| node_text(content, node))?;
    let source_kind = match method {
        "NavigateTo" => "navigate_to",
        "NavigateToLogin" => "navigate_to_login",
        _ => return None,
    };
    let receiver = function.child_by_field_name("expression")?;
    let receiver_name = proven_receiver_name(receiver, content)?;
    if !receiver_is_navigation_manager(receiver_name, invocation, declarations) {
        return None;
    }

    let arguments = invocation.child_by_field_name("arguments")?;
    let first_argument = arguments.named_child(0)?;
    if first_argument.kind() != "argument" {
        return None;
    }
    let expression = first_argument.named_child(0)?;
    let target_path = static_route_arg(expression, content, StaticArgLang::CSharp)?;
    if !is_internal_route(target_path) {
        return None;
    }

    Some(route_reference_fact(
        invocation,
        language,
        file_path,
        target_path,
        source_kind,
    ))
}

fn receiver_is_navigation_manager(
    receiver_name: &str,
    invocation: Node<'_>,
    declarations: &[ReceiverDeclaration],
) -> bool {
    declarations
        .iter()
        .filter(|declaration| {
            declaration.name == receiver_name
                && declaration.scope_start <= invocation.start_byte()
                && declaration.scope_end >= invocation.end_byte()
                && (declaration.order_independent
                    || declaration.start_byte <= invocation.start_byte())
        })
        .min_by(|left, right| {
            let left_scope = left.scope_end - left.scope_start;
            let right_scope = right.scope_end - right.scope_start;
            left_scope
                .cmp(&right_scope)
                .then_with(|| right.start_byte.cmp(&left.start_byte))
        })
        .is_some_and(|declaration| declaration.navigation_manager)
}

fn proven_receiver_name<'a>(receiver: Node<'_>, content: &'a str) -> Option<&'a str> {
    match receiver.kind() {
        "identifier" => node_text(content, receiver),
        "member_access_expression" => {
            let expression = receiver.child_by_field_name("expression")?;
            if node_text(content, expression)? != "this" {
                return None;
            }
            receiver
                .child_by_field_name("name")
                .and_then(|name| node_text(content, name))
        }
        _ => None,
    }
}

fn collect_razor_hrefs(
    node: Node<'_>,
    file_path: &str,
    content: &str,
    facts: &mut Vec<StructuralFact>,
) {
    if node.kind() == "element"
        && let Some((target_path, value_start, value_end)) = href_literal(node, content)
        && is_internal_route(target_path)
        && !has_razor_expression_in_range(node, value_start, value_end)
        && let Some(fact) =
            href_route_reference_fact(content, file_path, target_path, value_start, value_end)
    {
        facts.push(fact);
    }

    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        collect_razor_hrefs(child, file_path, content, facts);
    }
}

fn href_literal<'a>(node: Node<'_>, content: &'a str) -> Option<(&'a str, usize, usize)> {
    let bytes = content.as_bytes();
    let mut cursor = node.start_byte() + 1;
    let end = opening_tag_end(bytes, cursor, node.end_byte())?;
    while cursor < end && is_attribute_name_byte(bytes[cursor]) {
        cursor += 1;
    }

    loop {
        cursor = skip_whitespace(bytes, cursor, end);
        if cursor >= end || matches!(bytes[cursor], b'>' | b'/') {
            return None;
        }
        let name_start = cursor;
        while cursor < end && is_attribute_name_byte(bytes[cursor]) {
            cursor += 1;
        }
        if cursor == name_start {
            return None;
        }
        let name = &content[name_start..cursor];
        cursor = skip_whitespace(bytes, cursor, end);
        if bytes.get(cursor) != Some(&b'=') {
            continue;
        }
        cursor = skip_whitespace(bytes, cursor + 1, end);
        let quote = *bytes.get(cursor)?;
        let quoted = matches!(quote, b'\'' | b'"');
        let value_start = cursor + usize::from(quoted);
        cursor = value_start;
        while cursor < end
            && if quoted {
                bytes[cursor] != quote
            } else {
                !bytes[cursor].is_ascii_whitespace() && bytes[cursor] != b'>'
            }
        {
            cursor += 1;
        }
        if quoted && (cursor >= end || bytes[cursor] != quote) {
            return None;
        }
        let value_end = cursor;
        cursor += usize::from(quoted);
        if name.eq_ignore_ascii_case("href") {
            return Some((&content[value_start..value_end], value_start, value_end));
        }
    }
}

fn opening_tag_end(bytes: &[u8], mut cursor: usize, element_end: usize) -> Option<usize> {
    let mut quote = None;
    while cursor < element_end {
        let byte = bytes[cursor];
        match quote {
            Some(active_quote) if byte == active_quote => quote = None,
            None if matches!(byte, b'\'' | b'"') => quote = Some(byte),
            None if byte == b'>' => return Some(cursor + 1),
            _ => {}
        }
        cursor += 1;
    }
    None
}

fn skip_whitespace(bytes: &[u8], mut cursor: usize, end: usize) -> usize {
    while cursor < end && bytes[cursor].is_ascii_whitespace() {
        cursor += 1;
    }
    cursor
}

fn is_attribute_name_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b':' | b'.')
}

fn has_razor_expression_in_range(node: Node<'_>, start: usize, end: usize) -> bool {
    if node.start_byte() >= start && node.end_byte() <= end && node.kind().starts_with("razor_") {
        return true;
    }
    let mut cursor = node.walk();
    node.named_children(&mut cursor)
        .any(|child| has_razor_expression_in_range(child, start, end))
}

fn is_internal_route(value: &str) -> bool {
    value.starts_with('/') && !value.starts_with("//")
}

fn route_reference_fact(
    node: Node<'_>,
    language: &str,
    file_path: &str,
    target_path: &str,
    source_kind: &str,
) -> StructuralFact {
    let metadata = route_reference_metadata(target_path, source_kind);
    fact_for_node(
        file_path,
        language,
        RAZOR_ROUTE_REFERENCE_PATTERN_ID,
        "route_reference",
        node,
        metadata,
    )
}

fn href_route_reference_fact(
    content: &str,
    file_path: &str,
    target_path: &str,
    value_start: usize,
    value_end: usize,
) -> Option<StructuralFact> {
    let span = NormalizedSpan::from_content_range(content, value_start, value_end)?;
    Some(fact_for_span(
        file_path,
        "razor",
        RAZOR_ROUTE_REFERENCE_PATTERN_ID,
        "route_reference",
        "attribute_value",
        span,
        route_reference_metadata(target_path, "href"),
    ))
}

fn route_reference_metadata(
    target_path: &str,
    source_kind: &str,
) -> std::collections::HashMap<String, serde_json::Value> {
    let mut metadata = base_metadata("frontend_navigation", "blazor");
    insert_string(&mut metadata, "target_path", target_path);
    insert_string(&mut metadata, "source_kind", source_kind);
    insert_string(&mut metadata, "route_source", "string_literal");
    metadata
}
