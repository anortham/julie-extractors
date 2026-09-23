//! Programmatic navigation: `navigate("/x")` bound from React Router's
//! `useNavigate()`, and `router.push("/x")` bound from Next.js `useRouter()`.

use std::collections::HashMap;

use tree_sitter::{Node, Tree};

use super::fact_builders::{base_metadata, fact_for_node, insert_string};
use super::js_imports::JsImportIndex;
use super::{NEXTJS_ROUTE_REFERENCE_PATTERN_ID, REACT_ROUTE_REFERENCE_PATTERN_ID};
use crate::base::types::StructuralFact;
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};

const NEXT_ROUTER_NAVIGATION_METHODS: &[&str] = &["push", "replace", "prefetch"];

#[derive(Clone, Copy, PartialEq)]
enum Router {
    React,
    Next,
}

struct HookBinding<'a> {
    router: Router,
    import_source: &'a str,
}

pub(super) fn collect_programmatic_navigation_references(
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
    imports: &JsImportIndex,
) -> Vec<StructuralFact> {
    if imports.react_router_navigate_hooks.is_empty() && imports.next_router_hooks.is_empty() {
        return Vec::new();
    }
    let mut calls = Vec::new();
    collect_calls(tree.root_node(), &mut calls, 0);

    // ponytail: bindings are file-wide by name, so a shadowing local of the
    // same name also counts; scope-aware binding if that shows up in practice.
    let mut bindings: HashMap<&str, HookBinding> = HashMap::new();
    for call in &calls {
        let Some(hook) = call
            .child_by_field_name("function")
            .filter(|function| function.kind() == "identifier")
            .and_then(|function| content.get(function.byte_range()))
        else {
            continue;
        };
        let binding = if let Some(source) = imports.react_router_navigate_hooks.get(hook) {
            HookBinding {
                router: Router::React,
                import_source: source,
            }
        } else if let Some(source) = imports.next_router_hooks.get(hook) {
            HookBinding {
                router: Router::Next,
                import_source: source,
            }
        } else {
            continue;
        };
        if let Some(name) = call
            .parent()
            .filter(|parent| parent.kind() == "variable_declarator")
            .and_then(|declarator| declarator.child_by_field_name("name"))
            .filter(|name| name.kind() == "identifier")
            .and_then(|name| content.get(name.byte_range()))
        {
            bindings.insert(name, binding);
        }
    }

    let mut facts = Vec::new();
    for call in &calls {
        let Some(function) = call.child_by_field_name("function") else {
            continue;
        };
        let (binding, router) = match function.kind() {
            "identifier" => {
                let Some(binding) = content
                    .get(function.byte_range())
                    .and_then(|name| bindings.get(name))
                    .filter(|binding| binding.router == Router::React)
                else {
                    continue;
                };
                (binding, Router::React)
            }
            "member_expression" => {
                let method = function
                    .child_by_field_name("property")
                    .and_then(|property| content.get(property.byte_range()));
                let Some(binding) = function
                    .child_by_field_name("object")
                    .filter(|object| object.kind() == "identifier")
                    .and_then(|object| content.get(object.byte_range()))
                    .and_then(|name| bindings.get(name))
                    .filter(|binding| binding.router == Router::Next)
                    .filter(|_| {
                        method
                            .is_some_and(|method| NEXT_ROUTER_NAVIGATION_METHODS.contains(&method))
                    })
                else {
                    continue;
                };
                (binding, Router::Next)
            }
            _ => continue,
        };
        let Some(target_path) = first_argument_path(*call, content) else {
            continue;
        };
        let Some(navigation_call) = content.get(function.byte_range()) else {
            continue;
        };

        let mut metadata = base_metadata("frontend_navigation");
        let (pattern_id, source_kind) = match router {
            Router::React => {
                insert_string(&mut metadata, "framework", "react");
                insert_string(&mut metadata, "library", "react_router");
                (REACT_ROUTE_REFERENCE_PATTERN_ID, "react_router_navigate")
            }
            Router::Next => {
                insert_string(&mut metadata, "framework", "nextjs");
                (NEXTJS_ROUTE_REFERENCE_PATTERN_ID, "next_router_navigation")
            }
        };
        insert_string(&mut metadata, "target_path", &target_path);
        insert_string(&mut metadata, "navigation_call", navigation_call);
        insert_string(&mut metadata, "import_source", binding.import_source);
        insert_string(&mut metadata, "route_source", "string_literal");
        insert_string(&mut metadata, "source_kind", source_kind);
        insert_string(&mut metadata, "verb", "GET");
        facts.push(fact_for_node(
            file_path,
            language,
            pattern_id,
            "route_reference",
            *call,
            metadata,
        ));
    }
    facts
}

pub(super) fn collect_calls<'t>(node: Node<'t>, calls: &mut Vec<Node<'t>>, depth: u32) {
    if !should_visit_tree_depth(depth) {
        return;
    }
    if node.kind() == "call_expression" {
        calls.push(node);
    }
    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_calls(child, calls, child_depth);
    }
}

/// The static route path a navigation call's first argument names: a plain
/// string or a template string without substitutions.
fn first_argument_path(call: Node, content: &str) -> Option<String> {
    let arguments = call.child_by_field_name("arguments")?;
    let mut cursor = arguments.walk();
    let argument = arguments.named_children(&mut cursor).next()?;
    let path = static_string_value(argument, content)?;
    (!path.is_empty() && !path.starts_with("//") && !path.contains("://")).then_some(path)
}

pub(super) fn static_string_value(node: Node, content: &str) -> Option<String> {
    match node.kind() {
        "string" => {}
        "template_string" => {
            let mut cursor = node.walk();
            if node
                .named_children(&mut cursor)
                .any(|child| child.kind() == "template_substitution")
            {
                return None;
            }
        }
        _ => return None,
    }
    let text = content.get(node.byte_range())?;
    text.get(1..text.len().checked_sub(1)?).map(str::to_string)
}
