//! Sinatra route DSL: `get "/users/:id" do ... end` and `before "/admin/*"`.
//!
//! A route call counts in a class whose superclass is `Sinatra::Base` or
//! `Sinatra::Application`, or at the top level of a classic app that
//! requires `sinatra`. Only a static string path emits a fact.

use tree_sitter::{Node, Tree};

use super::helpers::{base_metadata, fact_for_span, insert_string, insert_string_array};
use super::{SINATRA_FILTER_PATTERN_ID, SINATRA_ROUTE_PATTERN_ID};
use crate::base::http_boundary::{ParamFlavor, normalize_route_template};
use crate::base::span::NormalizedSpan;
use crate::base::types::StructuralFact;
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};

const ROUTE_VERBS: &[&str] = &[
    "get", "post", "put", "patch", "delete", "head", "options", "link", "unlink",
];

pub(super) fn collect_sinatra_routes(
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
) -> Vec<StructuralFact> {
    if !content.contains("Sinatra") && !content.contains("sinatra") {
        return Vec::new();
    }
    let classic = requires_sinatra(tree.root_node(), content);
    let mut facts = Vec::new();
    visit(
        tree.root_node(),
        &Scan {
            language,
            file_path,
            content,
        },
        classic,
        &mut facts,
        0,
    );
    facts
}

struct Scan<'a> {
    language: &'a str,
    file_path: &'a str,
    content: &'a str,
}

fn visit(node: Node, scan: &Scan<'_>, in_app: bool, facts: &mut Vec<StructuralFact>, depth: u32) {
    if !should_visit_tree_depth(depth) {
        return;
    }
    let in_app = match node.kind() {
        "class" => is_sinatra_app_class(node, scan.content),
        "module" | "method" | "singleton_method" => false,
        _ => in_app,
    };
    if in_app && node.kind() == "call" {
        push_route_fact(node, scan, facts);
    }
    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        visit(child, scan, in_app, facts, child_depth);
    }
}

fn push_route_fact(call: Node, scan: &Scan<'_>, facts: &mut Vec<StructuralFact>) {
    if call.child_by_field_name("receiver").is_some() || call.child_by_field_name("block").is_none()
    {
        return;
    }
    let Some(method) = call.child_by_field_name("method") else {
        return;
    };
    let method = &scan.content[method.byte_range()];
    let is_route = ROUTE_VERBS.contains(&method);
    if !is_route && !matches!(method, "before" | "after") {
        return;
    }
    let Some(route_template) = call
        .child_by_field_name("arguments")
        .and_then(|arguments| arguments.named_child(0))
        .and_then(|first| static_string(first, scan.content))
    else {
        return;
    };
    let Some(span) =
        NormalizedSpan::from_content_range(scan.content, call.start_byte(), call.end_byte())
    else {
        return;
    };
    let mut metadata = base_metadata("framework", "sinatra");
    insert_string(&mut metadata, "api_style", "dsl_routing");
    insert_string(&mut metadata, "route_template", &route_template);
    let normalized = normalize_route_template(&route_template, ParamFlavor::Colon);
    insert_string(
        &mut metadata,
        "normalized_route_template",
        &normalized.template,
    );
    if !normalized.dynamic_segments.is_empty() {
        insert_string_array(
            &mut metadata,
            "dynamic_segments",
            normalized.dynamic_segments,
        );
    }
    let (pattern_id, capture_name) = if is_route {
        insert_string(&mut metadata, "verb", &method.to_ascii_uppercase());
        insert_string(&mut metadata, "verb_source", "attested");
        (SINATRA_ROUTE_PATTERN_ID, "route")
    } else {
        insert_string(&mut metadata, "filter_kind", method);
        (SINATRA_FILTER_PATTERN_ID, "filter")
    };
    facts.push(fact_for_span(
        scan.file_path,
        scan.language,
        pattern_id,
        capture_name,
        call.kind(),
        span,
        metadata,
    ));
}

/// The text of a string literal with no interpolation.
fn static_string(node: Node, content: &str) -> Option<String> {
    if node.kind() != "string" {
        return None;
    }
    let mut cursor = node.walk();
    let mut text = String::new();
    for child in node.named_children(&mut cursor) {
        if child.kind() != "string_content" {
            return None;
        }
        text.push_str(&content[child.byte_range()]);
    }
    Some(text)
}

fn is_sinatra_app_class(class: Node, content: &str) -> bool {
    class
        .child_by_field_name("superclass")
        .and_then(|superclass| superclass.named_child(0))
        .is_some_and(|base| {
            matches!(
                &content[base.byte_range()],
                "Sinatra::Base" | "Sinatra::Application"
            )
        })
}

/// A top-level `require "sinatra"` makes the file a classic Sinatra app.
fn requires_sinatra(root: Node, content: &str) -> bool {
    let mut cursor = root.walk();
    root.named_children(&mut cursor).any(|statement| {
        statement.kind() == "call"
            && statement
                .child_by_field_name("method")
                .is_some_and(|method| &content[method.byte_range()] == "require")
            && statement
                .child_by_field_name("arguments")
                .and_then(|arguments| arguments.named_child(0))
                .and_then(|path| static_string(path, content))
                .is_some_and(|path| path == "sinatra")
    })
}
