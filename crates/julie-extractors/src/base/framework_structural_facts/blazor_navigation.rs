use tree_sitter::{Node, Tree};

use super::RAZOR_ROUTE_REFERENCE_PATTERN_ID;
use super::helpers::{base_metadata, fact_for_node, fact_for_span, insert_string, node_text};
use super::static_arg::{StaticArgLang, static_route_arg};
use crate::base::span::NormalizedSpan;
use crate::base::types::StructuralFact;
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};

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
    collect_receiver_declarations(tree.root_node(), content, 0, &mut declarations);

    let mut facts = Vec::new();
    collect_navigation_calls(
        tree.root_node(),
        language,
        file_path,
        content,
        &declarations,
        0,
        &mut facts,
    );
    if language == "razor" {
        collect_razor_hrefs(tree.root_node(), file_path, content, 0, &mut facts);
    }
    facts
}

fn collect_receiver_declarations(
    node: Node<'_>,
    content: &str,
    depth: u32,
    declarations: &mut Vec<ReceiverDeclaration>,
) {
    if !should_visit_tree_depth(depth) {
        return;
    }

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

    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };

    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        collect_receiver_declarations(child, content, child_depth, declarations);
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
    depth: u32,
    facts: &mut Vec<StructuralFact>,
) {
    if !should_visit_tree_depth(depth) {
        return;
    }

    if node.kind() == "invocation_expression"
        && let Some(fact) = navigation_call_fact(node, language, file_path, content, declarations)
    {
        facts.push(fact);
    }

    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };

    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        collect_navigation_calls(
            child,
            language,
            file_path,
            content,
            declarations,
            child_depth,
            facts,
        );
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
    let (written, route_source) = match static_route_arg(expression, content, StaticArgLang::CSharp)
    {
        Some(literal) => (literal.to_string(), "string_literal"),
        None => (
            interpolated_route(expression, content)?,
            "interpolated_string",
        ),
    };
    let target = internal_route(&written, true)?;

    Some(route_reference_fact(
        invocation,
        language,
        file_path,
        &RouteReference {
            target,
            written: &written,
            source_kind,
            route_source,
        },
    ))
}

/// The text of a C# interpolated string (`$"/orders/{id}"`), holes kept as
/// written. A string that starts with a hole has no static route.
fn interpolated_route(expression: Node<'_>, content: &str) -> Option<String> {
    if expression.kind() != "interpolated_string_expression" {
        return None;
    }
    let mut cursor = expression.walk();
    let parts: Vec<Node> = expression
        .named_children(&mut cursor)
        .filter(|child| matches!(child.kind(), "string_content" | "interpolation"))
        .collect();
    if parts.first()?.kind() != "string_content" {
        return None;
    }
    let start = parts.first()?.start_byte();
    let end = parts.last()?.end_byte();
    content.get(start..end).map(str::to_string)
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
    depth: u32,
    facts: &mut Vec<StructuralFact>,
) {
    if !should_visit_tree_depth(depth) {
        return;
    }

    if node.kind() == "element"
        && let Some(fact) = href_route_reference_fact(node, content, file_path)
    {
        facts.push(fact);
    }

    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };

    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        collect_razor_hrefs(child, file_path, content, child_depth, facts);
    }
}

/// A `name="value"` attribute of an element's opening tag, with the byte
/// range of its value and the Razor expressions inside the value.
pub(super) struct TagAttribute<'a> {
    pub(super) name: &'a str,
    pub(super) value: &'a str,
    pub(super) value_start: usize,
    expressions: Vec<(usize, usize)>,
}

impl TagAttribute<'_> {
    /// The value with each Razor expression (`@order.Id`, `@(order.Id)`)
    /// written as `{order.Id}`, and whether it held one.
    pub(super) fn templated_value(&self, content: &str) -> (String, bool) {
        let mut out = String::new();
        let mut cursor = self.value_start;
        for &(start, end) in &self.expressions {
            out.push_str(&content[cursor..start]);
            let expression = content[start..end].trim_start_matches('@');
            let expression = expression
                .strip_prefix('(')
                .and_then(|inner| inner.strip_suffix(')'))
                .unwrap_or(expression);
            out.push('{');
            out.push_str(expression);
            out.push('}');
            cursor = end;
        }
        out.push_str(&content[cursor..self.value_start + self.value.len()]);
        (out, !self.expressions.is_empty())
    }
}

/// The tag name and valued attributes of an element's opening tag. Razor
/// markup attributes are not grammar nodes, so the tag text is scanned,
/// stepping over Razor expressions whose quotes and `>` belong to C#.
pub(super) fn opening_tag_attributes<'a>(
    node: Node<'_>,
    content: &'a str,
) -> Option<(&'a str, Vec<TagAttribute<'a>>)> {
    let bytes = content.as_bytes();
    let mut expressions = Vec::new();
    collect_tag_expressions(node, 0, &mut expressions);
    expressions.sort_unstable();
    let skip_expression = |cursor: usize| {
        expressions
            .iter()
            .find(|(start, _)| *start == cursor)
            .map(|&(_, end)| end)
    };

    let tag_start = node.start_byte() + usize::from(bytes.get(node.start_byte()) == Some(&b'<'));
    let mut cursor = tag_start;
    while cursor < node.end_byte() && is_attribute_name_byte(bytes[cursor]) {
        cursor += 1;
    }
    let tag = &content[tag_start..cursor];

    let mut attributes = Vec::new();
    loop {
        cursor = skip_whitespace(bytes, cursor, node.end_byte());
        if cursor >= node.end_byte() || matches!(bytes[cursor], b'>' | b'/') {
            return Some((tag, attributes));
        }
        if let Some(end) = skip_expression(cursor) {
            cursor = end;
            continue;
        }
        let name_start = cursor;
        while cursor < node.end_byte()
            && (is_attribute_name_byte(bytes[cursor]) || bytes[cursor] == b'@')
        {
            cursor += 1;
        }
        if cursor == name_start {
            return Some((tag, attributes));
        }
        let name = &content[name_start..cursor];
        cursor = skip_whitespace(bytes, cursor, node.end_byte());
        if bytes.get(cursor) != Some(&b'=') {
            continue;
        }
        cursor = skip_whitespace(bytes, cursor + 1, node.end_byte());
        let quote = *bytes.get(cursor)?;
        let quoted = matches!(quote, b'\'' | b'"');
        let value_start = cursor + usize::from(quoted);
        cursor = value_start;
        let mut value_expressions = Vec::new();
        while cursor < node.end_byte() {
            if let Some(end) = skip_expression(cursor) {
                value_expressions.push((cursor, end));
                cursor = end;
                continue;
            }
            let byte = bytes[cursor];
            if (quoted && byte == quote)
                || (!quoted && (byte.is_ascii_whitespace() || byte == b'>'))
            {
                break;
            }
            cursor += 1;
        }
        if quoted && bytes.get(cursor) != Some(&quote) {
            return Some((tag, attributes));
        }
        attributes.push(TagAttribute {
            name,
            value: &content[value_start..cursor],
            value_start,
            expressions: value_expressions,
        });
        cursor += usize::from(quoted);
    }
}

/// The Razor expressions of an element's own tag and attributes, not of its
/// child elements.
fn collect_tag_expressions(node: Node<'_>, depth: u32, out: &mut Vec<(usize, usize)>) {
    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        if crate::razor::is_razor_expression_node_kind(child.kind()) {
            out.push((child.start_byte(), child.end_byte()));
        } else if child.kind() != "element" && should_visit_tree_depth(child_depth) {
            collect_tag_expressions(child, child_depth, out);
        }
    }
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

/// A navigation target inside the app: a path (`/orders`) as written, or a
/// base-relative target (`orders/list`, `""`) resolved against the app base
/// path, which Blazor apps set to `/`. Absolute URLs, protocol-relative
/// URLs, fragments, queries, and other schemes leave the app.
struct InternalRoute {
    path: String,
    base_relative: bool,
}

fn internal_route(value: &str, allow_base_relative: bool) -> Option<InternalRoute> {
    if value.starts_with("//") || value.starts_with('{') {
        return None;
    }
    if value.starts_with('/') {
        return Some(InternalRoute {
            path: value.to_string(),
            base_relative: false,
        });
    }
    let has_scheme = value
        .split_once(':')
        .is_some_and(|(scheme, _)| !scheme.contains(['/', '{']));
    if !allow_base_relative || has_scheme || value.starts_with(['#', '?', '.']) {
        return None;
    }
    Some(InternalRoute {
        path: format!("/{value}"),
        base_relative: true,
    })
}

struct RouteReference<'a> {
    target: InternalRoute,
    written: &'a str,
    source_kind: &'a str,
    route_source: &'a str,
}

fn route_reference_fact(
    node: Node<'_>,
    language: &str,
    file_path: &str,
    reference: &RouteReference<'_>,
) -> StructuralFact {
    fact_for_node(
        file_path,
        language,
        RAZOR_ROUTE_REFERENCE_PATTERN_ID,
        "route_reference",
        node,
        route_reference_metadata(reference),
    )
}

/// An `href` route reference. A base-relative target counts only on a
/// navigation element (`<a>`, `<NavLink>`), since `<link href="css/app.css">`
/// names an asset.
fn href_route_reference_fact(
    node: Node<'_>,
    content: &str,
    file_path: &str,
) -> Option<StructuralFact> {
    let (tag, attributes) = opening_tag_attributes(node, content)?;
    let href = attributes
        .iter()
        .find(|attribute| attribute.name.eq_ignore_ascii_case("href"))?;
    let (written, templated) = href.templated_value(content);
    if written.contains('@') {
        return None;
    }
    let is_navigation_element = tag.eq_ignore_ascii_case("a") || tag == "NavLink";
    let target = internal_route(&written, is_navigation_element)?;
    let span = NormalizedSpan::from_content_range(
        content,
        href.value_start,
        href.value_start + href.value.len(),
    )?;
    let reference = RouteReference {
        target,
        written: &written,
        source_kind: "href",
        route_source: if templated {
            "template_expression"
        } else {
            "string_literal"
        },
    };
    Some(fact_for_span(
        file_path,
        "razor",
        RAZOR_ROUTE_REFERENCE_PATTERN_ID,
        "route_reference",
        "attribute_value",
        span,
        route_reference_metadata(&reference),
    ))
}

fn route_reference_metadata(
    reference: &RouteReference<'_>,
) -> std::collections::HashMap<String, serde_json::Value> {
    let mut metadata = base_metadata("frontend_navigation", "blazor");
    insert_string(&mut metadata, "target_path", &reference.target.path);
    insert_string(&mut metadata, "source_kind", reference.source_kind);
    insert_string(&mut metadata, "route_source", reference.route_source);
    if reference.target.base_relative {
        metadata.insert("base_relative".to_string(), serde_json::Value::Bool(true));
        insert_string(&mut metadata, "raw_target", reference.written);
    }
    metadata
}
