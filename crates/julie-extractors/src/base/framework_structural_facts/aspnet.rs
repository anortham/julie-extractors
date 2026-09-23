use std::collections::{BTreeSet, HashMap};

use serde_json::Value;
use tree_sitter::{Node, Tree};

use super::helpers::{
    base_metadata, fact_for_node, fact_for_span, find_matching_paren,
    find_matching_paren_backwards, find_top_level_comma_or_end, insert_string,
    is_comment_or_string_node, is_csharp_identifier, is_identifier_boundary, node_text,
    parse_csharp_string_literal, parse_first_route_argument, parse_handler_argument,
    skip_ascii_whitespace, skip_ascii_whitespace_until, smallest_node_covering_range,
};
use super::{
    ASPNET_ATTRIBUTE_ROUTE_PATTERN_ID, ASPNET_MINIMAL_API_ROUTE_GROUP_PATTERN_ID,
    ASPNET_MINIMAL_API_ROUTE_PATTERN_ID,
};
use crate::base::http_boundary::{ParamFlavor, join_route_templates, normalize_route_template};
use crate::base::span::NormalizedSpan;
use crate::base::types::StructuralFact;
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};

#[path = "aspnet_fsharp.rs"]
mod fsharp;

const ASPNET_ROUTE_METHODS: &[(&str, &str)] = &[
    ("MapGet", "GET"),
    ("MapPost", "POST"),
    ("MapPut", "PUT"),
    ("MapPatch", "PATCH"),
    ("MapDelete", "DELETE"),
    ("MapHead", "HEAD"),
    ("MapOptions", "OPTIONS"),
    ("MapMethods", ""),
];

/// Endpoint maps that register a route without an HTTP verb of their own.
const ASPNET_ENDPOINT_METHODS: &[(&str, &str)] = &[
    ("MapHub", "signalr_hub"),
    ("Map", "any_verb"),
    ("MapHealthChecks", "health_checks"),
];

#[derive(Clone, Copy)]
enum RouteMethodKind {
    Verb(&'static str),
    Endpoint(&'static str),
}

pub(super) fn collect_aspnet_minimal_api_routes(
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
) -> Vec<StructuralFact> {
    let mut facts = Vec::new();
    let route_groups = collect_aspnet_minimal_api_route_groups(language, tree, file_path, content);
    let group_prefixes = route_groups
        .iter()
        .filter_map(|fact| {
            let metadata = fact.metadata.as_ref()?;
            let group_variable = metadata.get("group_variable")?.as_str()?;
            let route_prefix = metadata
                .get("effective_route_template")
                .or_else(|| metadata.get("route_prefix"))?
                .as_str()?;
            Some((group_variable.to_string(), route_prefix.to_string()))
        })
        .collect::<HashMap<_, _>>();
    facts.extend(route_groups);

    let route_methods = ASPNET_ROUTE_METHODS
        .iter()
        .map(|(name, verb)| (*name, RouteMethodKind::Verb(verb)))
        .chain(
            ASPNET_ENDPOINT_METHODS
                .iter()
                .map(|(name, kind)| (*name, RouteMethodKind::Endpoint(kind))),
        );
    for (method_name, route_kind) in route_methods {
        let mut search_start = 0;
        while let Some(relative_start) = content[search_start..].find(method_name) {
            let method_start = search_start + relative_start;
            search_start = method_start + method_name.len();

            if !is_identifier_boundary(content, method_start, method_name.len()) {
                continue;
            }

            let (type_argument, open_paren) = generic_type_argument(content, search_start);
            if content.as_bytes().get(open_paren) != Some(&b'(') {
                continue;
            }

            let Some(close_paren) = find_matching_paren(content, open_paren) else {
                continue;
            };
            let Some((route_template, route_arg_end, route_source)) =
                parse_first_route_argument(content, open_paren + 1, close_paren)
            else {
                continue;
            };
            let Some(node) =
                smallest_node_covering_range(tree.root_node(), method_start, close_paren + 1)
            else {
                continue;
            };
            if is_comment_or_string_node(node.kind()) {
                continue;
            }
            let Some(span) =
                NormalizedSpan::from_content_range(content, method_start, close_paren + 1)
            else {
                continue;
            };

            let mut metadata = base_metadata("framework", "aspnet");
            insert_string(&mut metadata, "api_style", "minimal_api");
            insert_string(&mut metadata, "route_template", &route_template);
            insert_string(&mut metadata, "route_source", route_source);
            let mut normalized_source = route_template.clone();
            let route_group_prefix = parse_csharp_member_receiver(content, method_start)
                .and_then(|receiver| group_prefixes.get(receiver).cloned())
                .or_else(|| parse_chained_map_group_prefix(content, method_start));
            if let Some(route_group_prefix) = route_group_prefix {
                let effective = join_route_templates(&route_group_prefix, &route_template);
                insert_string(&mut metadata, "route_group_prefix", &route_group_prefix);
                insert_string(&mut metadata, "effective_route_template", &effective);
                insert_string(&mut metadata, "route_group_source", "map_group");
                normalized_source = effective;
            }
            insert_normalized_route_template(&mut metadata, &normalized_source);

            let verb = match route_kind {
                RouteMethodKind::Verb(verb) => verb,
                RouteMethodKind::Endpoint(endpoint_kind) => {
                    insert_string(&mut metadata, "endpoint_kind", endpoint_kind);
                    if let Some(hub_type) = type_argument {
                        insert_string(&mut metadata, "hub_type", hub_type);
                    }
                    if let Some(handler) =
                        parse_handler_argument(content, route_arg_end, close_paren)
                    {
                        insert_string(&mut metadata, "handler_kind", handler.kind);
                        if let Some(name) = handler.name {
                            insert_string(&mut metadata, "handler_name", &name);
                        }
                    }
                    facts.push(fact_for_span(
                        file_path,
                        language,
                        ASPNET_MINIMAL_API_ROUTE_PATTERN_ID,
                        "route_call",
                        node.kind(),
                        span,
                        metadata,
                    ));
                    continue;
                }
            };
            let (verbs, handler_arg_end) = if method_name == "MapMethods" {
                let comma = skip_ascii_whitespace_until(content, route_arg_end, close_paren);
                let methods_start = skip_ascii_whitespace_until(content, comma + 1, close_paren);
                let methods_end = find_top_level_comma_or_end(content, methods_start, close_paren);
                (
                    minimal_api_method_values(&content[methods_start..methods_end]),
                    methods_end,
                )
            } else {
                (vec![Some(verb.to_string())], route_arg_end)
            };
            if let Some(handler) = parse_handler_argument(content, handler_arg_end, close_paren) {
                insert_string(&mut metadata, "handler_kind", handler.kind);
                if let Some(name) = handler.name {
                    insert_string(&mut metadata, "handler_name", &name);
                }
            }

            for verb in verbs {
                let mut metadata = metadata.clone();
                if let Some(verb) = verb {
                    insert_string(&mut metadata, "verb", &verb);
                } else {
                    insert_string(&mut metadata, "verb_source", "unknown");
                }
                facts.push(fact_for_span(
                    file_path,
                    language,
                    ASPNET_MINIMAL_API_ROUTE_PATTERN_ID,
                    "route_call",
                    node.kind(),
                    span,
                    metadata,
                ));
            }
        }
    }

    facts
}

/// A generic argument between a method name and its argument list: C#
/// `MapHub<ChatHub>(` or VB `MapHub(Of ChatHub)(`. Returns the argument and
/// the offset of the argument list's `(`.
fn generic_type_argument(content: &str, after_name: usize) -> (Option<&str>, usize) {
    let start = skip_ascii_whitespace(content, after_name);
    let rest = &content[start..];
    let (argument, end) = if rest.starts_with('<') {
        let Some(close) = rest.find('>') else {
            return (None, start);
        };
        (&rest[1..close], start + close + 1)
    } else if rest.len() > 4 && rest[..4].eq_ignore_ascii_case("(Of ") {
        let Some(close) = find_matching_paren(content, start) else {
            return (None, start);
        };
        (&content[start + 4..close], close + 1)
    } else {
        return (None, start);
    };
    (Some(argument.trim()), skip_ascii_whitespace(content, end))
}

fn minimal_api_method_values(expression: &str) -> Vec<Option<String>> {
    let expression = expression.trim();
    let values = if expression.starts_with('[') && expression.ends_with(']') {
        &expression[1..expression.len() - 1]
    } else if expression.starts_with("new") && expression.ends_with('}') {
        let Some(open) = expression.find('{') else {
            return vec![None];
        };
        &expression[open + 1..expression.len() - 1]
    } else {
        return vec![None];
    };
    let mut verbs = BTreeSet::new();
    let mut start = 0;
    while start < values.len() {
        let end = find_top_level_comma_or_end(values, start, values.len());
        let value = values[start..end].trim();
        if !value.is_empty() {
            let verb = parse_csharp_string_literal(value, 0)
                .filter(|(_, end, _)| *end == value.len())
                .map(|(value, _, _)| value)
                .or_else(|| {
                    value
                        .strip_prefix("HttpMethods.")
                        .filter(|value| {
                            matches!(
                                *value,
                                "Get"
                                    | "Post"
                                    | "Put"
                                    | "Patch"
                                    | "Delete"
                                    | "Head"
                                    | "Options"
                                    | "Connect"
                                    | "Trace"
                            )
                        })
                        .map(str::to_string)
                })
                .filter(|value| {
                    !value.is_empty() && value.bytes().all(|b| b.is_ascii_alphabetic() || b == b'-')
                })
                .map(|value| value.to_ascii_uppercase());
            verbs.insert(verb);
        }
        start = end + 1;
    }
    if verbs.is_empty() {
        verbs.insert(None);
    }
    verbs.into_iter().collect()
}

fn collect_aspnet_minimal_api_route_groups(
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
) -> Vec<StructuralFact> {
    let mut facts = Vec::new();
    let mut effective_prefixes: HashMap<String, String> = HashMap::new();
    let method_name = "MapGroup";
    let mut search_start = 0;

    while let Some(relative_start) = content[search_start..].find(method_name) {
        let method_start = search_start + relative_start;
        search_start = method_start + method_name.len();

        if !is_identifier_boundary(content, method_start, method_name.len()) {
            continue;
        }

        let open_paren = skip_ascii_whitespace(content, search_start);
        if content.as_bytes().get(open_paren) != Some(&b'(') {
            continue;
        }

        let Some(close_paren) = find_matching_paren(content, open_paren) else {
            continue;
        };
        let Some((route_prefix, _, route_source)) =
            parse_first_route_argument(content, open_paren + 1, close_paren)
        else {
            continue;
        };
        let Some(node) =
            smallest_node_covering_range(tree.root_node(), method_start, close_paren + 1)
        else {
            continue;
        };
        if is_comment_or_string_node(node.kind()) {
            continue;
        }
        let Some(span) = NormalizedSpan::from_content_range(content, method_start, close_paren + 1)
        else {
            continue;
        };

        let mut metadata = base_metadata("framework", "aspnet");
        insert_string(&mut metadata, "api_style", "minimal_api");
        insert_string(&mut metadata, "route_prefix", &route_prefix);
        insert_string(&mut metadata, "route_source", route_source);
        insert_string(&mut metadata, "source_kind", "map_group");
        let parent_prefix = parse_csharp_member_receiver(content, method_start)
            .and_then(|receiver| effective_prefixes.get(receiver).cloned())
            .or_else(|| parse_chained_map_group_prefix(content, method_start));
        let effective = match parent_prefix {
            Some(parent_prefix) => {
                let effective = join_route_templates(&parent_prefix, &route_prefix);
                insert_string(&mut metadata, "parent_route_prefix", &parent_prefix);
                insert_string(&mut metadata, "effective_route_template", &effective);
                effective
            }
            None => route_prefix.clone(),
        };
        insert_normalized_route_template(&mut metadata, &effective);
        if let Some(group_variable) = parse_map_group_assignment_variable(content, method_start) {
            insert_string(&mut metadata, "group_variable", &group_variable);
            effective_prefixes.insert(group_variable, effective);
        }

        facts.push(fact_for_span(
            file_path,
            language,
            ASPNET_MINIMAL_API_ROUTE_GROUP_PATTERN_ID,
            "route_group",
            node.kind(),
            span,
            metadata,
        ));
    }

    facts
}

fn parse_map_group_assignment_variable(content: &str, method_start: usize) -> Option<String> {
    let statement_start = content[..method_start]
        .rfind(['\n', ';', '{'])
        .map(|index| index + 1)
        .unwrap_or(0);
    let before_method = content.get(statement_start..method_start)?;
    let equals = before_method.rfind('=')?;
    let candidate = before_method[..equals].split_whitespace().last()?;
    is_csharp_identifier(candidate).then(|| candidate.to_string())
}

fn parse_csharp_member_receiver(content: &str, method_start: usize) -> Option<&str> {
    let bytes = content.as_bytes();
    let mut dot = method_start;
    while dot > 0 && bytes.get(dot - 1).is_some_and(u8::is_ascii_whitespace) {
        dot -= 1;
    }
    if dot == 0 || bytes.get(dot - 1) != Some(&b'.') {
        return None;
    }

    let mut end = dot - 1;
    while end > 0 && bytes.get(end - 1).is_some_and(u8::is_ascii_whitespace) {
        end -= 1;
    }
    let mut start = end;
    while start > 0 && bytes.get(start - 1).is_some_and(is_csharp_identifier_byte) {
        start -= 1;
    }
    let receiver = content.get(start..end)?;
    is_csharp_identifier(receiver).then_some(receiver)
}

fn parse_chained_map_group_prefix(content: &str, method_start: usize) -> Option<String> {
    let bytes = content.as_bytes();
    let mut dot = method_start;
    while dot > 0 && bytes.get(dot - 1).is_some_and(u8::is_ascii_whitespace) {
        dot -= 1;
    }
    if dot == 0 || bytes.get(dot - 1) != Some(&b'.') {
        return None;
    }

    let mut cursor = dot - 1;
    while cursor > 0 && bytes.get(cursor - 1).is_some_and(u8::is_ascii_whitespace) {
        cursor -= 1;
    }
    if cursor == 0 || bytes.get(cursor - 1) != Some(&b')') {
        return None;
    }
    let close_paren = cursor - 1;
    let open_paren = find_matching_paren_backwards(content, close_paren)?;

    let mut method_end = open_paren;
    while method_end > 0
        && bytes
            .get(method_end - 1)
            .is_some_and(u8::is_ascii_whitespace)
    {
        method_end -= 1;
    }
    let mut method_name_start = method_end;
    while method_name_start > 0
        && bytes
            .get(method_name_start - 1)
            .is_some_and(is_csharp_identifier_byte)
    {
        method_name_start -= 1;
    }
    if content.get(method_name_start..method_end) != Some("MapGroup") {
        return None;
    }

    parse_first_route_argument(content, open_paren + 1, close_paren)
        .map(|(route_prefix, _, _)| route_prefix)
}

fn is_csharp_identifier_byte(byte: &u8) -> bool {
    matches!(byte, b'_' | b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9')
}

/// Collect `aspnet.attribute_route.v1` facts for attribute-routed controllers.
///
/// Uses tree-sitter attribution (attribute node -> owning class/method
/// declaration) rather than raw text association. Each .NET language maps its
/// syntax onto one controller model, so C#, VB.NET, and F# controllers share
/// the routing rules. Conventional (non-attribute) routing is intentionally out
/// of scope. Attributes whose route argument is not a plain string literal
/// (interpolation, concatenation, `nameof`, constants) stay silent.
pub(super) fn collect_aspnet_attribute_routes(
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
) -> Vec<StructuralFact> {
    let mut controllers = Vec::new();
    collect_route_controllers(tree.root_node(), language, content, &mut controllers, 0);
    let mut facts = Vec::new();
    for controller in &controllers {
        emit_controller_routes(controller, language, file_path, &mut facts);
    }
    facts
}

/// One route-relevant attribute: its normalized name (last segment, no
/// `Attribute` suffix) and its first positional argument.
struct RouteAttribute<'t> {
    node: Node<'t>,
    text: String,
    name: String,
    argument: AttributeRouteArgument,
}

struct RouteAction<'t> {
    name: Option<String>,
    attributes: Vec<RouteAttribute<'t>>,
}

struct RouteController<'t> {
    name: Option<String>,
    attributes: Vec<RouteAttribute<'t>>,
    actions: Vec<RouteAction<'t>>,
}

fn collect_route_controllers<'t>(
    node: Node<'t>,
    language: &str,
    content: &str,
    controllers: &mut Vec<RouteController<'t>>,
    depth: u32,
) {
    if !should_visit_tree_depth(depth) {
        return;
    }
    let controller = match (language, node.kind()) {
        ("csharp", "class_declaration") => Some(csharp_controller(node, content)),
        ("vbnet", "class_block") => Some(vbnet_controller(node, content)),
        ("fsharp", "anon_type_defn") => Some(fsharp::fsharp_controller(node, content)),
        _ => None,
    };
    controllers.extend(controller);

    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_route_controllers(child, language, content, controllers, child_depth);
    }
}

fn csharp_controller<'t>(class_node: Node<'t>, content: &str) -> RouteController<'t> {
    let mut actions = Vec::new();
    if let Some(body) = class_node.child_by_field_name("body") {
        let mut cursor = body.walk();
        for member in body.children(&mut cursor) {
            if member.kind() == "method_declaration" {
                actions.push(RouteAction {
                    name: declaration_name(member, content),
                    attributes: csharp_route_attributes(member, content),
                });
            }
        }
    }
    RouteController {
        name: declaration_name(class_node, content),
        attributes: csharp_route_attributes(class_node, content),
        actions,
    }
}

fn declaration_name(node: Node<'_>, content: &str) -> Option<String> {
    node.child_by_field_name("name")
        .and_then(|name| node_text(content, name))
        .map(str::to_string)
}

/// Attribute lists that are direct children of a C# declaration; a nested
/// class's own attributes belong to that inner declaration.
fn csharp_route_attributes<'t>(declaration: Node<'t>, content: &str) -> Vec<RouteAttribute<'t>> {
    let mut attributes = Vec::new();
    let mut cursor = declaration.walk();
    for child in declaration.children(&mut cursor) {
        if child.kind() != "attribute_list" {
            continue;
        }
        let mut list_cursor = child.walk();
        for attribute in child.children(&mut list_cursor) {
            if attribute.kind() != "attribute" {
                continue;
            }
            if let Some(name) = attribute_route_name(content, attribute) {
                attributes.push(RouteAttribute {
                    node: attribute,
                    text: node_text(content, attribute)
                        .unwrap_or_default()
                        .to_string(),
                    name,
                    argument: attribute_route_argument(content, attribute),
                });
            }
        }
    }
    attributes
}

fn vbnet_controller<'t>(class_node: Node<'t>, content: &str) -> RouteController<'t> {
    let wrapper = class_node
        .parent()
        .filter(|parent| parent.kind() == "type_declaration")
        .unwrap_or(class_node);
    let mut blocks = Vec::new();
    let mut previous = wrapper.prev_sibling();
    while let Some(sibling) = previous.filter(|sibling| sibling.kind() == "attribute_block") {
        blocks.push(sibling);
        previous = sibling.prev_sibling();
    }
    blocks.reverse();
    blocks.extend(vbnet_attribute_blocks(wrapper));

    let mut actions = Vec::new();
    let mut cursor = class_node.walk();
    for member in class_node.children(&mut cursor) {
        if member.kind() == "method_declaration" {
            actions.push(RouteAction {
                name: declaration_name(member, content),
                attributes: vbnet_route_attributes(&vbnet_attribute_blocks(member), content),
            });
        }
    }
    RouteController {
        name: declaration_name(class_node, content),
        attributes: vbnet_route_attributes(&blocks, content),
        actions,
    }
}

fn vbnet_attribute_blocks(declaration: Node<'_>) -> Vec<Node<'_>> {
    let mut cursor = declaration.walk();
    declaration
        .children(&mut cursor)
        .filter(|child| child.kind() == "attribute_block")
        .collect()
}

fn vbnet_route_attributes<'t>(blocks: &[Node<'t>], content: &str) -> Vec<RouteAttribute<'t>> {
    let mut attributes = Vec::new();
    for block in blocks {
        let mut cursor = block.walk();
        for attribute in block.children(&mut cursor) {
            if attribute.kind() != "attribute" || attribute.child_by_field_name("target").is_some()
            {
                continue;
            }
            let Some(name) = attribute_route_name(content, attribute) else {
                continue;
            };
            let mut attribute_cursor = attribute.walk();
            let arguments = attribute
                .children(&mut attribute_cursor)
                .find(|child| child.kind() == "argument_list");
            attributes.push(RouteAttribute {
                node: attribute,
                text: node_text(content, attribute)
                    .unwrap_or_default()
                    .to_string(),
                name,
                argument: vbnet_route_argument(content, arguments),
            });
        }
    }
    attributes
}

/// The first positional argument of a VB attribute (`name:=` arguments are
/// named); `""` inside a VB string escapes one quote.
fn vbnet_route_argument(content: &str, arguments: Option<Node<'_>>) -> AttributeRouteArgument {
    let Some(arguments) = arguments else {
        return AttributeRouteArgument::Absent;
    };
    let mut cursor = arguments.walk();
    for argument in arguments.named_children(&mut cursor) {
        if argument.child_by_field_name("name").is_some() {
            continue;
        }
        let value = if argument.kind() == "argument" {
            argument.named_child(argument.named_child_count().saturating_sub(1) as u32)
        } else {
            Some(argument)
        };
        return value
            .filter(|value| value.kind() == "string_literal")
            .and_then(|value| node_text(content, value))
            .and_then(|text| text.strip_prefix('"')?.strip_suffix('"'))
            .map(|text| AttributeRouteArgument::Literal(text.replace("\"\"", "\"")))
            .unwrap_or(AttributeRouteArgument::NonLiteral);
    }
    AttributeRouteArgument::Absent
}

fn emit_controller_routes(
    controller: &RouteController<'_>,
    language: &str,
    file_path: &str,
    facts: &mut Vec<StructuralFact>,
) {
    let controller_token = controller
        .name
        .as_deref()
        .map(controller_token_from_class_name);

    // Class-level [Route("...")] and Web API 2 [RoutePrefix("...")] attributes ->
    // controller_route facts. The first literal template becomes the controller
    // template shared with methods.
    let mut controller_template: Option<String> = None;
    for attribute in &controller.attributes {
        if !is_route_attribute(&attribute.name) && attribute.name != "RoutePrefix" {
            continue;
        }
        let AttributeRouteArgument::Literal(template) = &attribute.argument else {
            continue;
        };
        if controller_template.is_none() {
            controller_template = Some(template.clone());
        }
        let (effective, tokens) =
            substitute_route_tokens(template, controller_token.as_deref(), None);
        let mut metadata = base_metadata("framework", "aspnet");
        insert_string(&mut metadata, "api_style", "attribute_routing");
        insert_string(&mut metadata, "attribute_kind", "controller_route");
        insert_string(&mut metadata, "route_template", template);
        insert_string(&mut metadata, "effective_route_template", &effective);
        insert_normalized_route_template(&mut metadata, &effective);
        insert_route_tokens(&mut metadata, tokens);
        facts.push(fact_for_node(
            file_path,
            language,
            ASPNET_ATTRIBUTE_ROUTE_PATTERN_ID,
            "attribute_route",
            attribute.node,
            metadata,
        ));
    }

    for action in &controller.actions {
        emit_action_routes(
            action,
            language,
            file_path,
            controller_token.as_deref(),
            controller_template.as_deref(),
            facts,
        );
    }
}

fn emit_action_routes(
    action: &RouteAction<'_>,
    language: &str,
    file_path: &str,
    controller_token: Option<&str>,
    controller_template: Option<&str>,
    facts: &mut Vec<StructuralFact>,
) {
    let verbs_of = |attribute: &RouteAttribute<'_>| -> Vec<String> {
        match attribute_route_verb(&attribute.name) {
            Some(verb) => vec![verb.to_string()],
            None if attribute.name == "AcceptVerbs" => accept_verbs(&attribute.text),
            None => Vec::new(),
        }
    };
    let has_http_verb = action
        .attributes
        .iter()
        .any(|attribute| !verbs_of(attribute).is_empty());
    let method_route_template = action.attributes.iter().find_map(|attribute| {
        match (&attribute.argument, is_route_attribute(&attribute.name)) {
            (AttributeRouteArgument::Literal(template), true) => Some(template.clone()),
            _ => None,
        }
    });

    for attribute in &action.attributes {
        let verbs = verbs_of(attribute);
        let is_route = is_route_attribute(&attribute.name);
        if verbs.is_empty() && !is_route {
            continue;
        }
        // A method-level [Route] only emits its own `route` fact when the method
        // has no verb attribute; otherwise the verb attributes carry its
        // template.
        if verbs.is_empty() && has_http_verb {
            continue;
        }

        let method_template = match (&attribute.argument, attribute.name.as_str()) {
            (_, "AcceptVerbs") => method_route_template.clone(),
            (AttributeRouteArgument::NonLiteral, _) => continue,
            (AttributeRouteArgument::Absent, _) => method_route_template.clone(),
            (AttributeRouteArgument::Literal(template), _) => Some(template.clone()),
        };
        let verbs: Vec<Option<String>> = if verbs.is_empty() {
            vec![None]
        } else {
            verbs.into_iter().map(Some).collect()
        };
        for verb in verbs {
            emit_action_route(
                action,
                attribute,
                verb.as_deref(),
                method_template.as_deref(),
                controller_token,
                controller_template,
                language,
                file_path,
                facts,
            );
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn emit_action_route(
    action: &RouteAction<'_>,
    attribute: &RouteAttribute<'_>,
    verb: Option<&str>,
    method_template: Option<&str>,
    controller_token: Option<&str>,
    controller_template: Option<&str>,
    language: &str,
    file_path: &str,
    facts: &mut Vec<StructuralFact>,
) {
    let mut metadata = base_metadata("framework", "aspnet");
    insert_string(&mut metadata, "api_style", "attribute_routing");
    if let Some(verb) = verb {
        insert_string(&mut metadata, "verb", verb);
        insert_string(&mut metadata, "attribute_kind", "http_method");
    } else {
        insert_string(&mut metadata, "attribute_kind", "route");
    }
    if let Some(method_template) = method_template {
        insert_string(&mut metadata, "route_template", method_template);
    }
    if let Some(controller_template) = controller_template {
        insert_string(
            &mut metadata,
            "controller_route_template",
            controller_template,
        );
    }

    if let Some(raw) = join_effective_route(controller_template, method_template) {
        let (effective, tokens) =
            substitute_route_tokens(&raw, controller_token, action.name.as_deref());
        insert_string(&mut metadata, "effective_route_template", &effective);
        insert_normalized_route_template(&mut metadata, &effective);
        insert_route_tokens(&mut metadata, tokens);
    }

    facts.push(fact_for_node(
        file_path,
        language,
        ASPNET_ATTRIBUTE_ROUTE_PATTERN_ID,
        "attribute_route",
        attribute.node,
        metadata,
    ));
}

/// The verbs an `[AcceptVerbs(...)]` attribute lists, as string literals
/// (`"GET"`) or `HttpVerbs` members (`HttpVerbs.Post`).
fn accept_verbs(attribute_text: &str) -> Vec<String> {
    let arguments = attribute_text
        .split_once('(')
        .map_or("", |(_, arguments)| arguments);
    arguments
        .split(|c: char| !(c.is_ascii_alphanumeric() || c == '.'))
        .filter_map(|token| {
            let verb = token.strip_prefix("HttpVerbs.").unwrap_or(token);
            let quoted = arguments.contains(&format!("\"{token}\""));
            (quoted || token.starts_with("HttpVerbs."))
                .then(|| verb.to_ascii_uppercase())
                .filter(|verb| !verb.is_empty())
        })
        .collect()
}

/// Normalize an attribute's name: take the last `.`-separated segment and strip
/// a trailing `Attribute` suffix (`Microsoft.AspNetCore.Mvc.HttpGetAttribute`
/// -> `HttpGet`).
fn attribute_route_name(content: &str, attribute: Node<'_>) -> Option<String> {
    let name_node = attribute.child_by_field_name("name")?;
    let raw = node_text(content, name_node)?;
    let last = raw.rsplit('.').next().unwrap_or(raw).trim();
    let normalized = last.strip_suffix("Attribute").unwrap_or(last);
    if normalized.is_empty() {
        return None;
    }
    Some(normalized.to_string())
}

fn is_route_attribute(name: &str) -> bool {
    name == "Route"
}

fn attribute_route_verb(name: &str) -> Option<&'static str> {
    match name {
        "HttpGet" => Some("GET"),
        "HttpPost" => Some("POST"),
        "HttpPut" => Some("PUT"),
        "HttpPatch" => Some("PATCH"),
        "HttpDelete" => Some("DELETE"),
        "HttpHead" => Some("HEAD"),
        "HttpOptions" => Some("OPTIONS"),
        _ => None,
    }
}

enum AttributeRouteArgument {
    /// No argument list, or an empty one (`[HttpGet]`, `[HttpGet()]`).
    Absent,
    /// A first argument that is not a plain string literal -> stay silent.
    NonLiteral,
    /// A first-positional string-literal template.
    Literal(String),
}

fn attribute_route_argument(content: &str, attribute: Node<'_>) -> AttributeRouteArgument {
    let mut cursor = attribute.walk();
    let argument_list = attribute
        .children(&mut cursor)
        .find(|child| child.kind() == "attribute_argument_list");
    let Some(argument_list) = argument_list else {
        return AttributeRouteArgument::Absent;
    };

    let mut list_cursor = argument_list.walk();
    for argument in argument_list.children(&mut list_cursor) {
        if argument.kind() != "attribute_argument" {
            continue;
        }
        if is_named_attribute_argument(content, argument) {
            continue;
        }
        return match parse_csharp_string_literal(content, argument.start_byte()) {
            Some((value, literal_end, _))
                if skip_ascii_whitespace_until(content, literal_end, argument.end_byte())
                    == argument.end_byte() =>
            {
                AttributeRouteArgument::Literal(value)
            }
            None => AttributeRouteArgument::NonLiteral,
            Some(_) => AttributeRouteArgument::NonLiteral,
        };
    }

    AttributeRouteArgument::Absent
}

fn is_named_attribute_argument(content: &str, argument: Node<'_>) -> bool {
    let Some(raw) = node_text(content, argument) else {
        return false;
    };
    let trimmed = raw.trim_start();
    let bytes = trimmed.as_bytes();
    if bytes
        .first()
        .is_none_or(|byte| !matches!(byte, b'_' | b'a'..=b'z' | b'A'..=b'Z'))
    {
        return false;
    }
    let mut cursor = 1;
    while cursor < bytes.len() && is_csharp_identifier_byte(&bytes[cursor]) {
        cursor += 1;
    }
    cursor = skip_ascii_whitespace_until(trimmed, cursor, trimmed.len());
    bytes.get(cursor) == Some(&b'=')
}

/// The controller substitution value: class name minus a trailing `Controller`.
fn controller_token_from_class_name(class_name: &str) -> String {
    match class_name.strip_suffix("Controller") {
        Some(stripped) if !stripped.is_empty() => stripped.to_string(),
        _ => class_name.to_string(),
    }
}

/// Join a controller template with a method template into the raw effective
/// template (before token substitution). Returns `None` when neither template
/// contributes a path segment.
fn join_effective_route(
    controller_template: Option<&str>,
    method_template: Option<&str>,
) -> Option<String> {
    match (controller_template, method_template) {
        (Some(_), Some(method)) if is_absolute_route_template(method) => Some(method.to_string()),
        (Some(controller), Some(method)) => Some(join_route_templates(controller, method)),
        (Some(controller), None) => Some(controller.to_string()),
        (None, Some(method)) => Some(method.to_string()),
        (None, None) => None,
    }
}

fn is_absolute_route_template(template: &str) -> bool {
    template.starts_with('/') || template.starts_with("~/")
}

fn insert_normalized_route_template(metadata: &mut HashMap<String, Value>, template: &str) {
    let normalized = normalize_route_template(template, ParamFlavor::Braces);
    insert_string(metadata, "normalized_route_template", &normalized.template);
}

fn slash_normalized_route_template(template: &str) -> String {
    let normalized = template
        .strip_prefix("~/")
        .unwrap_or_else(|| template.trim_start_matches('/'));
    format!("/{normalized}")
}

/// Substitute `[controller]`/`[action]` tokens using the lowercased identifiers
/// and normalize a single leading `/`. Returns the substituted template and the
/// list of tokens actually replaced.
fn substitute_route_tokens(
    raw: &str,
    controller_token: Option<&str>,
    action_token: Option<&str>,
) -> (String, Vec<&'static str>) {
    let mut output = raw.to_string();
    let mut tokens = Vec::new();
    if let Some(controller) = controller_token
        && output.contains("[controller]")
    {
        output = output.replace("[controller]", &controller.to_ascii_lowercase());
        tokens.push("controller");
    }
    if let Some(action) = action_token
        && output.contains("[action]")
    {
        output = output.replace("[action]", &action.to_ascii_lowercase());
        tokens.push("action");
    }
    (slash_normalized_route_template(&output), tokens)
}

fn insert_route_tokens(metadata: &mut HashMap<String, Value>, tokens: Vec<&'static str>) {
    metadata.insert(
        "route_tokens".to_string(),
        Value::Array(
            tokens
                .into_iter()
                .map(|token| Value::String(token.to_string()))
                .collect(),
        ),
    );
}
