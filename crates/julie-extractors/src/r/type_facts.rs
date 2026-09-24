use crate::base::BaseExtractor;
use crate::base::types::TypeNameRules;
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};
use std::collections::{HashMap, HashSet};
use tree_sitter::Node;

use super::RExtractor;
use super::idioms::{assignment_name, bind_arguments, call_name, positional_string_argument};
use super::text_args::clean_r_name;

pub(super) const R_TYPE_NAME_RULES: TypeNameRules = TypeNameRules {
    nullable_suffixes: &[],
    reference_prefixes: &[],
    generic_open: &[],
};

pub(super) fn record_inferred_fact(base: &mut BaseExtractor, symbol_id: &str, class_name: &str) {
    base.record_declared_type_fact(symbol_id, class_name, &R_TYPE_NAME_RULES, true);
}

/// The class of a variable initializer: a same-file constructor, or a call to a
/// same-file S4 generic that declares one `valueClass`.
/// Parentheses are unwrapped, and `lhs |> f(...)` reads as the call `f(lhs, ...)`
/// that R's parser rewrites it to.
pub(super) fn initializer_class(extractor: &RExtractor, right: Node) -> Option<String> {
    let right = without_parentheses(right);
    same_file_constructor_class(extractor, right).or_else(|| {
        generic_call_value_class(
            extractor,
            native_pipe_call(extractor, right).unwrap_or(right),
        )
    })
}

fn without_parentheses(mut node: Node) -> Node {
    while node.kind() == "parenthesized_expression"
        && let Some(body) = node.child_by_field_name("body")
    {
        node = body;
    }
    node
}

fn native_pipe_call<'a>(extractor: &RExtractor, node: Node<'a>) -> Option<Node<'a>> {
    if node.kind() != "binary_operator" {
        return None;
    }
    let operator = node.child_by_field_name("operator")?;
    if extractor.base.get_node_text(&operator) != "|>" {
        return None;
    }
    node.child_by_field_name("rhs")
}

fn generic_call_value_class(extractor: &RExtractor, right: Node) -> Option<String> {
    if right.kind() != "call" {
        return None;
    }
    let callee = right.child_by_field_name("function")?;
    let name = clean_r_name(&extractor.base.get_node_text(&callee))?;
    extractor.generic_value_classes.get(&name)?.clone()
}

/// Declared return classes of same-file S4 generics. `setGeneric(valueClass = "X")`
/// with a `def` function makes R stop unless the value `is()` an `X`. A name maps
/// to `None` when a declaration lacks a `def` function or a single class, runs only
/// under a condition, or disagrees with another, or when an assignment, a `for`
/// variable, `assign()`, `delayedAssign()`, `makeActiveBinding()`, a `with()` data
/// list, or a parameter rebinds the name anywhere in the file.
pub(super) fn collect_generic_value_classes(
    extractor: &RExtractor,
    root: Node,
) -> HashMap<String, Option<String>> {
    let mut classes = HashMap::new();
    collect_value_classes(extractor, root, 0, &mut classes);
    classes
}

fn collect_value_classes(
    extractor: &RExtractor,
    node: Node,
    depth: u32,
    classes: &mut HashMap<String, Option<String>>,
) {
    if !should_visit_tree_depth(depth) {
        return;
    }
    match node.kind() {
        "call" => match call_name(extractor, node).as_deref() {
            Some("setGeneric") if is_methods_call(extractor, node) => {
                if let Some((name, value_class)) = generic_value_class(extractor, node) {
                    classes
                        .entry(name)
                        .and_modify(|known| {
                            if *known != value_class {
                                *known = None;
                            }
                        })
                        .or_insert(value_class);
                }
            }
            Some("assign" | "delayedAssign") => {
                rebind_string_argument(extractor, node, &["x"], classes);
            }
            Some("makeActiveBinding") => {
                rebind_string_argument(extractor, node, &["sym"], classes);
            }
            Some("with" | "within") => rebind_with_data_names(extractor, node, classes),
            _ => {}
        },
        "for_statement" => {
            if let Some(name) = node
                .child_by_field_name("variable")
                .and_then(|variable| clean_r_name(&extractor.base.get_node_text(&variable)))
            {
                classes.insert(name, None);
            }
        }
        "binary_operator" => {
            if let Some(name) = rebound_name(extractor, node) {
                classes.insert(name, None);
            }
        }
        "parameter" => {
            if let Some(name) = node
                .child_by_field_name("name")
                .and_then(|name| clean_r_name(&extractor.base.get_node_text(&name)))
            {
                classes.insert(name, None);
            }
        }
        _ => {}
    }
    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_value_classes(extractor, child, child_depth, classes);
    }
}

/// Without a `def` function, R builds the generic from an existing function or
/// generic of that name, which can drop `valueClass`, and a conditional
/// declaration may never run, so both map to `None`.
fn generic_value_class(extractor: &RExtractor, call: Node) -> Option<(String, Option<String>)> {
    let args = call.child_by_field_name("arguments")?;
    let bound = bind_arguments(extractor, args, &["name", "def", "group", "valueClass"]);
    let name = string_literal(extractor, bound.get("name").copied())?;
    let has_def_function = bound
        .get("def")
        .is_some_and(|def| def.kind() == "function_definition");
    let value_class = string_literal(extractor, bound.get("valueClass").copied())
        .filter(|_| has_def_function && !runs_conditionally(extractor, call));
    Some((name, value_class))
}

fn string_literal(extractor: &RExtractor, value: Option<Node>) -> Option<String> {
    value
        .filter(|value| value.kind() == "string")
        .and_then(|value| clean_r_name(&extractor.base.get_node_text(&value)))
}

fn is_methods_call(extractor: &RExtractor, call: Node) -> bool {
    call.child_by_field_name("function")
        .is_some_and(|callee| match callee.kind() {
            "namespace_operator" => callee
                .child_by_field_name("lhs")
                .is_some_and(|package| extractor.base.get_node_text(&package) == "methods"),
            _ => true,
        })
}

fn runs_conditionally(extractor: &RExtractor, node: Node) -> bool {
    let mut current = node;
    while let Some(parent) = current.parent() {
        let conditional = match parent.kind() {
            "if_statement" => parent.child_by_field_name("condition") != Some(current),
            "binary_operator" => {
                parent.child_by_field_name("rhs") == Some(current)
                    && parent
                        .child_by_field_name("operator")
                        .is_some_and(|operator| {
                            matches!(
                                extractor.base.get_node_text(&operator).as_str(),
                                "&&" | "||"
                            )
                        })
            }
            _ => false,
        };
        if conditional {
            return true;
        }
        current = parent;
    }
    false
}

fn rebind_string_argument(
    extractor: &RExtractor,
    call: Node,
    formals: &[&'static str],
    classes: &mut HashMap<String, Option<String>>,
) {
    let Some(args) = call.child_by_field_name("arguments") else {
        return;
    };
    let bound = bind_arguments(extractor, args, formals);
    if let Some(name) = string_literal(extractor, bound.get(formals[0]).copied()) {
        classes.insert(name, None);
    }
}

fn rebind_with_data_names(
    extractor: &RExtractor,
    call: Node,
    classes: &mut HashMap<String, Option<String>>,
) {
    let Some(data) = call
        .child_by_field_name("arguments")
        .and_then(|args| {
            bind_arguments(extractor, args, &["data"])
                .get("data")
                .copied()
        })
        .filter(|data| data.kind() == "call")
        .and_then(|data| data.child_by_field_name("arguments"))
    else {
        return;
    };
    let mut cursor = data.walk();
    for argument in data.children_by_field_name("argument", &mut cursor) {
        if let Some(name) = argument
            .child_by_field_name("name")
            .and_then(|name| clean_r_name(&extractor.base.get_node_text(&name)))
        {
            classes.insert(name, None);
        }
    }
}

fn rebound_name(extractor: &RExtractor, assignment: Node) -> Option<String> {
    let operator = assignment.child_by_field_name("operator")?;
    let target = match extractor.base.get_node_text(&operator).as_str() {
        "<-" | "<<-" | "=" => assignment.child_by_field_name("lhs")?,
        "->" | "->>" => assignment.child_by_field_name("rhs")?,
        _ => return None,
    };
    assignment_name(extractor, target)
}

fn same_file_constructor_class(extractor: &RExtractor, right: Node) -> Option<String> {
    if right.kind() != "call" {
        return None;
    }
    let callee = right.child_by_field_name("function")?;
    match callee.kind() {
        "identifier" => {
            let name = clean_r_name(&extractor.base.get_node_text(&callee))?;
            if name == "new" {
                let args = right.child_by_field_name("arguments")?;
                let class_name = positional_string_argument(extractor, args, 0)?;
                same_file_class(extractor, &class_name)
            } else {
                same_file_class(extractor, &name)
            }
        }
        "extract_operator" => {
            let object = callee.child_by_field_name("lhs")?;
            if object.kind() != "identifier" {
                return None;
            }
            let method = callee.child_by_field_name("rhs")?;
            if extractor.base.get_node_text(&method) != "new" {
                return None;
            }
            let class_name = clean_r_name(&extractor.base.get_node_text(&object))?;
            same_file_class(extractor, &class_name)
        }
        _ => None,
    }
}

pub(super) fn self_receiver_type(extractor: &RExtractor, function_node: Node) -> Option<String> {
    if function_node.kind() != "extract_operator" {
        return None;
    }
    let object = function_node.child_by_field_name("lhs")?;
    match extractor.base.get_node_text(&object).as_str() {
        "self" | "private" => enclosing_r6_class_name(extractor, function_node),
        "super" => super_receiver_type(extractor, function_node),
        _ => None,
    }
}

/// The `inherit =` base of the R6 class enclosing `node`.
pub(super) fn super_receiver_type(extractor: &RExtractor, node: Node) -> Option<String> {
    let call = enclosing_r6_call(extractor, node)?;
    let args = call.child_by_field_name("arguments")?;
    let mut cursor = args.walk();
    let inherit = args
        .children_by_field_name("argument", &mut cursor)
        .find(|argument| argument_name(extractor, *argument).as_deref() == Some("inherit"))?
        .child_by_field_name("value")?;
    let text = extractor.base.get_node_text(&inherit);
    clean_r_name(text.rsplit("::").next().unwrap_or(&text))
}

fn same_file_class(extractor: &RExtractor, name: &str) -> Option<String> {
    extractor
        .same_file_class_names
        .contains(name)
        .then(|| name.to_string())
}

pub(super) fn collect_same_file_class_names(extractor: &RExtractor, root: Node) -> HashSet<String> {
    let mut names = HashSet::new();
    collect_class_names(extractor, root, 0, &mut names);
    names
}

fn collect_class_names(
    extractor: &RExtractor,
    node: Node,
    depth: u32,
    names: &mut HashSet<String>,
) {
    if !should_visit_tree_depth(depth) {
        return;
    }
    if node.kind() == "call"
        && let Some(name) = declared_class_name(extractor, node)
    {
        names.insert(name);
    }
    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_class_names(extractor, child, child_depth, names);
    }
}

fn declared_class_name(extractor: &RExtractor, call: Node) -> Option<String> {
    match call_name(extractor, call)?.as_str() {
        "setClass" | "new_class" => {
            let args = call.child_by_field_name("arguments")?;
            positional_string_argument(extractor, args, 0)
        }
        "R6Class" | "setRefClass" => {
            let assignment = call.parent()?;
            if assignment.kind() != "binary_operator"
                || assignment.child_by_field_name("rhs")? != call
            {
                return None;
            }
            let operator = assignment.child_by_field_name("operator")?;
            if !matches!(
                extractor.base.get_node_text(&operator).as_str(),
                "<-" | "=" | "<<-"
            ) {
                return None;
            }
            assignment_name(extractor, assignment.child_by_field_name("lhs")?)
        }
        _ => None,
    }
}

pub(super) fn enclosing_r6_class_name(extractor: &RExtractor, node: Node) -> Option<String> {
    r6_class_name(extractor, enclosing_r6_call(extractor, node)?)
}

fn enclosing_r6_call<'a>(extractor: &RExtractor, node: Node<'a>) -> Option<Node<'a>> {
    let mut current = node;
    let mut in_public_or_private = false;
    while let Some(parent) = current.parent() {
        if argument_name(extractor, parent)
            .is_some_and(|name| name == "public" || name == "private")
        {
            in_public_or_private = true;
        }
        if in_public_or_private
            && parent.kind() == "call"
            && call_name(extractor, parent).as_deref() == Some("R6Class")
        {
            return Some(parent);
        }
        current = parent;
    }
    None
}

pub(super) fn argument_name(extractor: &RExtractor, node: Node) -> Option<String> {
    match node.kind() {
        "argument" => {
            let name_node = node.child_by_field_name("name")?;
            clean_r_name(&extractor.base.get_node_text(&name_node))
        }
        "binary_operator" => assignment_name(extractor, node.child_by_field_name("lhs")?),
        _ => None,
    }
}

fn r6_class_name(extractor: &RExtractor, call: Node) -> Option<String> {
    if let Some(args) = call.child_by_field_name("arguments")
        && let Some(name) = positional_string_argument(extractor, args, 0)
    {
        return Some(name);
    }
    let parent = call.parent()?;
    if parent.kind() != "binary_operator" {
        return None;
    }
    assignment_name(extractor, parent.child_by_field_name("lhs")?)
}
