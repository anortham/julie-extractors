//! Spring outbound HTTP clients in Java: `RestTemplate`, `WebClient`,
//! `RestClient`, and OpenFeign `@FeignClient` interfaces.
//!
//! A call counts only when its receiver is proven in the same file: a field,
//! parameter or local declared with the client type, a `var` local built from
//! it, or a direct `new RestTemplate()` / `WebClient.create(…)` /
//! `RestClient.builder()…build()` chain. Java declares field types, so a field
//! receiver is proven by its declaration. Every collector is import-gated and
//! reads only static string-literal URLs.

use tree_sitter::{Node, Tree};

use super::super::super::helpers::node_text;
use super::super::super::spring::{
    MappingArguments, annotation_elements, declaration_span, java_annotations,
    java_type_declarations, join_prefix, mapping_annotation_kind, static_templates,
};
use super::super::super::static_arg::{StaticArgLang, static_route_arg};
use super::super::client_fact;
use super::super::{verb_for_lower_method, verb_for_token};
use crate::base::types::StructuralFact;
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};

struct Gates {
    rest_template: bool,
    rest_client: bool,
    web_client: bool,
}

pub(super) fn collect_spring_client_requests(
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
) -> Vec<StructuralFact> {
    let gates = Gates {
        rest_template: content.contains("org.springframework.web.client.RestTemplate")
            || content.contains("org.springframework.web.client.*"),
        rest_client: content.contains("org.springframework.web.client.RestClient")
            || content.contains("org.springframework.web.client.*"),
        web_client: content.contains("org.springframework.web.reactive.function.client."),
    };
    let mut facts = Vec::new();
    if gates.rest_template || gates.rest_client || gates.web_client {
        walk(
            tree.root_node(),
            &gates,
            language,
            tree,
            file_path,
            content,
            0,
            &mut facts,
        );
    }
    if content.contains("org.springframework.cloud.openfeign") {
        collect_feign_requests(language, tree, file_path, content, &mut facts);
    }
    facts
}

#[allow(clippy::too_many_arguments)]
fn walk(
    node: Node,
    gates: &Gates,
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
    depth: u32,
    facts: &mut Vec<StructuralFact>,
) {
    if !should_visit_tree_depth(depth) {
        return;
    }
    if node.kind() == "method_invocation"
        && let Some((client, target_path, verb)) = spring_request(node, gates, content)
    {
        facts.extend(client_fact(
            language,
            tree,
            file_path,
            content,
            node.start_byte(),
            node.end_byte(),
            client,
            target_path,
            verb,
            "attested",
            None,
        ));
    }
    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };
    for child in node.children(&mut node.walk()) {
        walk(
            child,
            gates,
            language,
            tree,
            file_path,
            content,
            child_depth,
            facts,
        );
    }
}

/// `(client, url, verb)` of a proven Spring client call.
fn spring_request<'a>(
    call: Node,
    gates: &Gates,
    content: &'a str,
) -> Option<(&'static str, &'a str, &'static str)> {
    let method = node_text(content, call.child_by_field_name("name")?)?;
    let receiver = call.child_by_field_name("object")?;
    let arguments = call.child_by_field_name("arguments")?;
    let first_argument = || arguments.named_child(0);

    if gates.rest_template && receiver_proven(receiver, content, "RestTemplate") {
        let verb = match method {
            "getForObject" | "getForEntity" => "GET",
            "postForObject" | "postForEntity" | "postForLocation" => "POST",
            "put" => "PUT",
            "delete" => "DELETE",
            "patchForObject" => "PATCH",
            "exchange" => http_method_constant(arguments.named_child(1)?, content)?,
            _ => return None,
        };
        let url = static_route_arg(first_argument()?, content, StaticArgLang::Java)?;
        return Some(("spring_resttemplate", url, verb));
    }

    if method != "uri" || receiver.kind() != "method_invocation" {
        return None;
    }
    let verb_call_name = node_text(content, receiver.child_by_field_name("name")?)?;
    let verb = match verb_call_name {
        "method" => http_method_constant(
            receiver.child_by_field_name("arguments")?.named_child(0)?,
            content,
        )?,
        name => verb_for_lower_method(name)?,
    };
    let client_receiver = receiver.child_by_field_name("object")?;
    let client = if gates.web_client && receiver_proven(client_receiver, content, "WebClient") {
        "spring_webclient"
    } else if gates.rest_client && receiver_proven(client_receiver, content, "RestClient") {
        "spring_restclient"
    } else {
        return None;
    };
    let url = static_route_arg(first_argument()?, content, StaticArgLang::Java)?;
    Some((client, url, verb))
}

/// `HttpMethod.GET` (or a bare `GET` static import) as an uppercase verb.
fn http_method_constant(node: Node, content: &str) -> Option<&'static str> {
    let terminal = match node.kind() {
        "field_access" => node.child_by_field_name("field")?,
        "identifier" => node,
        _ => return None,
    };
    let text = node_text(content, terminal)?;
    text.chars()
        .all(|c| c.is_ascii_uppercase())
        .then(|| verb_for_token(text))
        .flatten()
}

/// The receiver is a value of `type_name`: a declared field, parameter or local,
/// `this.field`, or a constructor or factory chain of the exact type.
fn receiver_proven(receiver: Node, content: &str, type_name: &str) -> bool {
    match receiver.kind() {
        "identifier" => node_text(content, receiver).is_some_and(|name| {
            declared_type(receiver, name, content)
                .is_some_and(|declaration| declaration_is_client(declaration, content, type_name))
        }),
        "field_access" => {
            let object_is_this = receiver
                .child_by_field_name("object")
                .is_some_and(|object| object.kind() == "this");
            let field = receiver
                .child_by_field_name("field")
                .and_then(|field| node_text(content, field));
            object_is_this
                && field.is_some_and(|name| {
                    field_declaration(receiver, name, content).is_some_and(|declaration| {
                        declaration_is_client(declaration, content, type_name)
                    })
                })
        }
        "object_creation_expression" | "method_invocation" => {
            constructs_client(receiver, content, type_name)
        }
        _ => false,
    }
}

/// A declaration's stated type and initializer.
#[derive(Clone, Copy)]
struct Declaration<'tree> {
    type_node: Node<'tree>,
    value: Option<Node<'tree>>,
}

fn declaration_is_client(declaration: Declaration, content: &str, type_name: &str) -> bool {
    let type_node = match declaration.type_node.kind() {
        "generic_type" => declaration.type_node.named_child(0),
        _ => Some(declaration.type_node),
    };
    let Some(type_text) = type_node.and_then(|node| node_text(content, node)) else {
        return false;
    };
    if type_text == "var" {
        return declaration
            .value
            .is_some_and(|value| constructs_client(value, content, type_name));
    }
    type_text == type_name || type_text.ends_with(&format!(".{type_name}"))
}

/// `new RestTemplate(…)`, `WebClient.create(…)`, `WebClient.builder()….build()`.
fn constructs_client(node: Node, content: &str, type_name: &str) -> bool {
    match node.kind() {
        "object_creation_expression" => node
            .child_by_field_name("type")
            .and_then(|type_node| node_text(content, type_node))
            .is_some_and(|text| text == type_name || text.ends_with(&format!(".{type_name}"))),
        "method_invocation" => {
            let mut call = node;
            let mut depth = 0;
            while let Some(object) = call.child_by_field_name("object")
                && object.kind() == "method_invocation"
                && should_visit_tree_depth(depth)
            {
                call = object;
                depth += 1;
            }
            let factory = call
                .child_by_field_name("name")
                .and_then(|name| node_text(content, name));
            let root = call
                .child_by_field_name("object")
                .and_then(|object| node_text(content, object));
            matches!(factory, Some("create" | "builder"))
                && root.is_some_and(|root| {
                    root == type_name || root.ends_with(&format!(".{type_name}"))
                })
        }
        _ => false,
    }
}

/// The innermost parameter, local or field declaring `name` around `from`.
fn declared_type<'tree>(
    from: Node<'tree>,
    name: &str,
    content: &str,
) -> Option<Declaration<'tree>> {
    let mut current = from.parent();
    while let Some(node) = current {
        match node.kind() {
            "method_declaration" | "constructor_declaration" => {
                if let Some(declaration) = parameter_declaration(node, name, content)
                    .or_else(|| local_declaration(node, name, content, 0))
                {
                    return Some(declaration);
                }
            }
            "class_body" | "interface_body" | "enum_body" => {
                if let Some(declaration) = member_field(node, name, content) {
                    return Some(declaration);
                }
            }
            _ => {}
        }
        current = node.parent();
    }
    None
}

fn field_declaration<'tree>(
    from: Node<'tree>,
    name: &str,
    content: &str,
) -> Option<Declaration<'tree>> {
    let mut current = from.parent();
    while let Some(node) = current {
        if matches!(node.kind(), "class_body" | "enum_body") {
            return member_field(node, name, content);
        }
        current = node.parent();
    }
    None
}

fn parameter_declaration<'tree>(
    callable: Node<'tree>,
    name: &str,
    content: &str,
) -> Option<Declaration<'tree>> {
    let parameters = callable.child_by_field_name("parameters")?;
    parameters
        .named_children(&mut parameters.walk())
        .filter(|parameter| parameter.kind() == "formal_parameter")
        .find(|parameter| {
            parameter
                .child_by_field_name("name")
                .and_then(|node| node_text(content, node))
                == Some(name)
        })
        .and_then(|parameter| {
            Some(Declaration {
                type_node: parameter.child_by_field_name("type")?,
                value: None,
            })
        })
}

fn member_field<'tree>(body: Node<'tree>, name: &str, content: &str) -> Option<Declaration<'tree>> {
    body.named_children(&mut body.walk())
        .filter(|member| member.kind() == "field_declaration")
        .find_map(|field| declarator_named(field, name, content))
}

fn local_declaration<'tree>(
    node: Node<'tree>,
    name: &str,
    content: &str,
    depth: u32,
) -> Option<Declaration<'tree>> {
    if !should_visit_tree_depth(depth) {
        return None;
    }
    if node.kind() == "local_variable_declaration" {
        return declarator_named(node, name, content);
    }
    let child_depth = child_tree_depth(depth)?;
    node.named_children(&mut node.walk())
        .filter(|child| !matches!(child.kind(), "class_body" | "lambda_expression"))
        .find_map(|child| local_declaration(child, name, content, child_depth))
}

fn declarator_named<'tree>(
    declaration: Node<'tree>,
    name: &str,
    content: &str,
) -> Option<Declaration<'tree>> {
    let type_node = declaration.child_by_field_name("type")?;
    let mut cursor = declaration.walk();
    declaration
        .children_by_field_name("declarator", &mut cursor)
        .find(|declarator| {
            declarator
                .child_by_field_name("name")
                .and_then(|node| node_text(content, node))
                == Some(name)
        })
        .map(|declarator| Declaration {
            type_node,
            value: declarator.child_by_field_name("value"),
        })
}

/// An OpenFeign `@FeignClient` interface declares one outbound request per
/// Spring mapping method. The target joins the client `path` element, a
/// type-level `@RequestMapping` prefix and the method template.
fn collect_feign_requests(
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
    facts: &mut Vec<StructuralFact>,
) {
    for declaration in java_type_declarations(tree.root_node()) {
        let annotations = java_annotations(declaration, content);
        let Some(feign) = annotations
            .iter()
            .find(|annotation| annotation.name == "FeignClient")
        else {
            continue;
        };
        let feign_path = annotation_elements(feign.node, content)
            .into_iter()
            .find(|(name, _)| name.as_deref() == Some("path"))
            .map(|(_, value)| static_templates(value, content));
        let feign_path = match feign_path {
            Some(Some(paths)) if paths.len() == 1 => paths[0].clone(),
            Some(_) => continue,
            None => String::new(),
        };
        let type_prefix = match annotations
            .iter()
            .find(|annotation| annotation.name == "RequestMapping")
            .map(|annotation| MappingArguments::parse(annotation.node, content))
        {
            Some(mapping) if mapping.templates.len() == 1 => mapping.templates[0].clone(),
            Some(mapping) if mapping.had_route_argument => continue,
            _ => String::new(),
        };
        let prefix = join_non_empty(&feign_path, &type_prefix);
        let Some(body) = declaration.child_by_field_name("body") else {
            continue;
        };
        for method in body.named_children(&mut body.walk()) {
            if method.kind() != "method_declaration" {
                continue;
            }
            for annotation in java_annotations(method, content) {
                let Some((default_verb, _)) = mapping_annotation_kind(annotation.name) else {
                    continue;
                };
                let arguments = MappingArguments::parse(annotation.node, content);
                let templates = match (arguments.templates.is_empty(), arguments.had_route_argument)
                {
                    (true, true) => continue,
                    (true, false) => vec![String::new()],
                    (false, _) => arguments.templates,
                };
                let verbs: Vec<&str> = match default_verb {
                    Some(verb) => vec![verb],
                    None if arguments.verbs.is_empty() => vec!["GET"],
                    None => arguments
                        .verbs
                        .iter()
                        .filter_map(|verb| verb_for_token(verb))
                        .collect(),
                };
                let verb_source = if default_verb.is_none() && arguments.verbs.is_empty() {
                    "default"
                } else {
                    "attested"
                };
                let (start, end) = declaration_span(method);
                for template in &templates {
                    let target = match join_non_empty(&prefix, template) {
                        target if target.is_empty() => "/".to_string(),
                        target => target,
                    };
                    for verb in &verbs {
                        facts.extend(client_fact(
                            language,
                            tree,
                            file_path,
                            content,
                            start,
                            end,
                            "openfeign",
                            &target,
                            verb,
                            verb_source,
                            None,
                        ));
                    }
                }
            }
        }
    }
}

fn join_non_empty(prefix: &str, template: &str) -> String {
    match (prefix.is_empty(), template.is_empty()) {
        (true, _) => template.to_string(),
        (false, true) => prefix.to_string(),
        (false, false) => join_prefix(prefix, template),
    }
}
