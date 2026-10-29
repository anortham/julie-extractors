//! R superassignment: `name <<- value` and `value ->> name` inside a function.
//!
//! R assigns the name in the nearest enclosing function environment that
//! already binds it, and else in the global environment. The search starts
//! above the function that runs the assignment, so that function's own
//! locals never count.

use super::RExtractor;
use super::idioms;
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};
use std::collections::HashSet;
use tree_sitter::Node;

/// The assigned identifier of a superassignment inside a function, with its
/// operator. `None` for any other binary operator, for a superassignment at
/// the top level (an ordinary global assignment), and for a target that is
/// not a plain name (`obj$v <<- 1`).
pub(super) fn target<'tree>(
    extractor: &RExtractor,
    node: Node<'tree>,
) -> Option<(Node<'tree>, String)> {
    if node.kind() != "binary_operator" {
        return None;
    }
    let operator = extractor
        .base
        .get_node_text(&node.child_by_field_name("operator")?);
    let target = match operator.as_str() {
        "<<-" => node.child_by_field_name("lhs")?,
        "->>" => node.child_by_field_name("rhs")?,
        _ => return None,
    };
    (target.kind() == "identifier" && idioms::inside_function(node)).then_some((target, operator))
}

/// The name of `assign("name", value, envir = .GlobalEnv)`, which binds a
/// global like a superassignment. `globalenv()` names the same environment.
pub(super) fn global_assign_name(extractor: &RExtractor, call: Node) -> Option<String> {
    if idioms::call_name(extractor, call).as_deref() != Some("assign") {
        return None;
    }
    let arguments = call.child_by_field_name("arguments")?;
    let mut cursor = arguments.walk();
    let global = arguments
        .children_by_field_name("argument", &mut cursor)
        .any(|argument| {
            argument
                .child_by_field_name("name")
                .is_some_and(|name| extractor.base.get_node_text(&name) == "envir")
                && argument.child_by_field_name("value").is_some_and(|value| {
                    matches!(
                        extractor.base.get_node_text(&value).as_str(),
                        ".GlobalEnv" | "globalenv()" | "base::globalenv()"
                    )
                })
        });
    if !global {
        return None;
    }
    idioms::positional_string_argument(extractor, arguments, 0)
}

/// Names a top-level `<-`, `=`, `<<-`, `->`, or `->>` assigns anywhere in the
/// file outside a function.
pub(super) fn top_level_names(extractor: &RExtractor, root: Node) -> HashSet<String> {
    let mut names = HashSet::new();
    collect_assigned_names(extractor, root, &mut names, true, 0);
    names
}

/// Whether a function that encloses the function running the
/// superassignment binds `name` as a parameter, a local assignment, or a
/// `for` variable.
pub(super) fn enclosing_function_binds(extractor: &RExtractor, node: Node, name: &str) -> bool {
    let mut scope = enclosing_function(node);
    while let Some(function) = scope.and_then(enclosing_function) {
        if function_binds(extractor, function, name) {
            return true;
        }
        scope = Some(function);
    }
    false
}

fn enclosing_function(node: Node) -> Option<Node> {
    let mut current = node.parent();
    while let Some(parent) = current {
        if parent.kind() == "function_definition" {
            return Some(parent);
        }
        current = parent.parent();
    }
    None
}

fn function_binds(extractor: &RExtractor, function: Node, name: &str) -> bool {
    if let Some(parameters) = function.child_by_field_name("parameters") {
        let mut cursor = parameters.walk();
        let is_parameter = parameters
            .children_by_field_name("parameter", &mut cursor)
            .filter_map(|parameter| parameter.child_by_field_name("name"))
            .any(|parameter| extractor.base.get_node_text(&parameter) == name);
        if is_parameter {
            return true;
        }
    }
    let Some(body) = function.child_by_field_name("body") else {
        return false;
    };
    let mut names = HashSet::new();
    collect_assigned_names(extractor, body, &mut names, false, 0);
    names.contains(name)
}

/// Collect assigned names below `node`, never inside a nested function. At the
/// top level a superassignment binds the global name too; inside a function
/// it binds a name of an enclosing scope, not of this one.
fn collect_assigned_names(
    extractor: &RExtractor,
    node: Node,
    names: &mut HashSet<String>,
    top_level: bool,
    depth: u32,
) {
    if !should_visit_tree_depth(depth) || node.kind() == "function_definition" {
        return;
    }
    let assigned = match node.kind() {
        "binary_operator" => node.child_by_field_name("operator").and_then(|operator| {
            match extractor.base.get_node_text(&operator).as_str() {
                "<-" | "=" => node.child_by_field_name("lhs"),
                "<<-" if top_level => node.child_by_field_name("lhs"),
                "->" => node.child_by_field_name("rhs"),
                "->>" if top_level => node.child_by_field_name("rhs"),
                _ => None,
            }
        }),
        "for_statement" => node.child_by_field_name("variable"),
        _ => None,
    };
    if let Some(name) = assigned.and_then(|target| idioms::assignment_name(extractor, target)) {
        names.insert(name);
    }
    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_assigned_names(extractor, child, names, top_level, child_depth);
    }
}
