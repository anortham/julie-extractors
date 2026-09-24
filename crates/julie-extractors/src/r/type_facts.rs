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
    extractor.name_bindings.generic_value_class(&name)
}

/// How a file binds names: `setGeneric` return classes, names bound to a
/// same-file class generator, and names rebound any other way.
#[derive(Default)]
pub(super) struct NameBindings {
    generics: HashMap<String, Option<String>>,
    generators: HashSet<String>,
    rebound: HashSet<String>,
}

impl NameBindings {
    /// `setGeneric(valueClass = "X")` with a `def` function makes R stop unless
    /// the value `is()` an `X`. No class when a declaration lacks a `def`
    /// function or a single class, may not run, binds elsewhere, or disagrees
    /// with another, or when anything else in the file binds the name.
    fn generic_value_class(&self, name: &str) -> Option<String> {
        if self.generators.contains(name) || self.rebound.contains(name) {
            return None;
        }
        self.generics.get(name)?.clone()
    }

    fn calls_constructor(&self, name: &str) -> bool {
        !self.rebound.contains(name)
    }
}

pub(super) fn collect_name_bindings(extractor: &RExtractor, root: Node) -> NameBindings {
    let mut bindings = NameBindings::default();
    collect_bindings(extractor, root, 0, &mut bindings);
    bindings
}

fn collect_bindings(extractor: &RExtractor, node: Node, depth: u32, bindings: &mut NameBindings) {
    if !should_visit_tree_depth(depth) {
        return;
    }
    match node.kind() {
        "call" => match call_name(extractor, node).as_deref() {
            Some("setGeneric") if is_methods_call(extractor, node) => {
                if let Some((name, value_class)) = generic_value_class(extractor, node) {
                    bindings
                        .generics
                        .entry(name)
                        .and_modify(|known| {
                            if *known != value_class {
                                *known = None;
                            }
                        })
                        .or_insert(value_class);
                }
            }
            Some("assign" | "delayedAssign" | "list2env") => {
                rebind_argument_entries(extractor, node, &["x"], &["x"], &mut bindings.rebound);
            }
            Some("makeActiveBinding") => {
                rebind_argument_entries(extractor, node, &["sym"], &["sym"], &mut bindings.rebound);
            }
            Some("with" | "within") => {
                rebind_argument_entries(
                    extractor,
                    node,
                    &["data"],
                    &["data"],
                    &mut bindings.rebound,
                );
            }
            Some("setRefClass") => rebind_argument_entries(
                extractor,
                node,
                &["Class", "fields", "contains", "methods"],
                &["fields", "methods"],
                &mut bindings.rebound,
            ),
            _ if is_ref_class_member_call(extractor, node) => {
                if let Some(args) = node.child_by_field_name("arguments") {
                    rebind_entries(extractor, args, true, &mut bindings.rebound);
                }
            }
            _ => {}
        },
        "for_statement" => {
            bindings.rebound.extend(
                node.child_by_field_name("variable")
                    .and_then(|variable| clean_r_name(&extractor.base.get_node_text(&variable))),
            );
        }
        "binary_operator" => collect_assignment(extractor, node, bindings),
        "parameter" => {
            bindings.rebound.extend(
                node.child_by_field_name("name")
                    .and_then(|name| clean_r_name(&extractor.base.get_node_text(&name))),
            );
        }
        _ => {}
    }
    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_bindings(extractor, child, child_depth, bindings);
    }
}

const SET_GENERIC_FORMALS: [&str; 10] = [
    "name",
    "def",
    "group",
    "valueClass",
    "where",
    "package",
    "signature",
    "useAsDefault",
    "genericFunction",
    "simpleInheritanceOnly",
];

/// Without a `def` function, R builds the generic from an existing function or
/// generic of that name, which can drop `valueClass`. A declaration that may not
/// run, or that `where =` binds into another environment, types nothing. An
/// argument name that R would partially match (`wh =`) also types nothing.
fn generic_value_class(extractor: &RExtractor, call: Node) -> Option<(String, Option<String>)> {
    let args = call.child_by_field_name("arguments")?;
    let bound = bind_arguments(extractor, args, &SET_GENERIC_FORMALS[..5]);
    let name = string_literal(extractor, bound.get("name").copied())?;
    let has_def_function = bound
        .get("def")
        .is_some_and(|def| def.kind() == "function_definition");
    let value_class = string_literal(extractor, bound.get("valueClass").copied()).filter(|_| {
        has_def_function
            && !bound.contains_key("where")
            && only_exact_formal_names(extractor, args, &SET_GENERIC_FORMALS)
            && !runs_conditionally(extractor, call)
    });
    Some((name, value_class))
}

fn only_exact_formal_names(extractor: &RExtractor, args: Node, formals: &[&str]) -> bool {
    let mut cursor = args.walk();
    args.children_by_field_name("argument", &mut cursor)
        .filter_map(|argument| argument.child_by_field_name("name"))
        .all(|name| {
            clean_r_name(&extractor.base.get_node_text(&name))
                .is_some_and(|name| formals.contains(&name.as_str()))
        })
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

/// `Gen$methods(...)` and `Gen$fields(...)` add Reference Class members, which
/// the class's methods see as bare names.
fn is_ref_class_member_call(extractor: &RExtractor, call: Node) -> bool {
    call.child_by_field_name("function")
        .filter(|callee| callee.kind() == "extract_operator")
        .and_then(|callee| callee.child_by_field_name("rhs"))
        .is_some_and(|member| {
            matches!(
                extractor.base.get_node_text(&member).as_str(),
                "methods" | "fields"
            )
        })
}

fn runs_conditionally(extractor: &RExtractor, node: Node) -> bool {
    let mut current = node;
    while let Some(parent) = current.parent() {
        let conditional = match parent.kind() {
            "if_statement" | "while_statement" => {
                parent.child_by_field_name("condition") != Some(current)
            }
            "for_statement" => parent.child_by_field_name("body") == Some(current),
            "repeat_statement" | "function_definition" => true,
            "call" => call_name(extractor, parent).as_deref() == Some("switch"),
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

/// Rebinds the entries of the `rebinding` arguments of `call`, matched against
/// R's leading `formals`.
fn rebind_argument_entries(
    extractor: &RExtractor,
    call: Node,
    formals: &[&'static str],
    rebinding: &[&'static str],
    rebound: &mut HashSet<String>,
) {
    let Some(args) = call.child_by_field_name("arguments") else {
        return;
    };
    let bound = bind_arguments(extractor, args, formals);
    for formal in rebinding {
        if let Some(value) = bound.get(formal) {
            rebind_entries(extractor, *value, true, rebound);
        }
    }
}

/// Names a value binds: a string, or the named and string entries of a call
/// such as `list(f = ...)` or `c("f")`, one unnamed list level deep.
fn rebind_entries(
    extractor: &RExtractor,
    value: Node,
    nested: bool,
    rebound: &mut HashSet<String>,
) {
    let args = match value.kind() {
        "string" => {
            rebound.extend(string_literal(extractor, Some(value)));
            return;
        }
        "arguments" => value,
        "call" => match value.child_by_field_name("arguments") {
            Some(args) => args,
            None => return,
        },
        _ => return,
    };
    let mut cursor = args.walk();
    for argument in args.children_by_field_name("argument", &mut cursor) {
        match argument
            .child_by_field_name("name")
            .and_then(|name| clean_r_name(&extractor.base.get_node_text(&name)))
        {
            Some(name) => {
                rebound.insert(name);
            }
            None => {
                if let Some(entry) = argument.child_by_field_name("value")
                    && (nested || entry.kind() == "string")
                {
                    rebind_entries(extractor, entry, false, rebound);
                }
            }
        }
    }
}

fn collect_assignment(extractor: &RExtractor, assignment: Node, bindings: &mut NameBindings) {
    let Some(operator) = assignment.child_by_field_name("operator") else {
        return;
    };
    let (target, value) = match extractor.base.get_node_text(&operator).as_str() {
        "<-" | "<<-" | "=" => ("lhs", "rhs"),
        "->" | "->>" => ("rhs", "lhs"),
        _ => return,
    };
    let (Some(target), Some(value)) = (
        assignment.child_by_field_name(target),
        assignment.child_by_field_name(value),
    ) else {
        return;
    };
    if let Some(name) = assignment_name(extractor, target)
        && value.kind() == "call"
        && declared_class_name(extractor, value).as_deref() == Some(name.as_str())
    {
        bindings.generators.insert(name);
        return;
    }
    rebind_target(extractor, target, &mut bindings.rebound);
}

/// Every name an assignment target rebinds. `f(x) <- v` and `x$m <- v` rebind
/// `x`. When `x` is an environment, `x$m <- v` and `x[["m"]] <- v` also bind
/// `m` in it.
fn rebind_target(extractor: &RExtractor, mut target: Node, rebound: &mut HashSet<String>) {
    loop {
        let next = match target.kind() {
            "extract_operator" => {
                rebound.extend(
                    target
                        .child_by_field_name("rhs")
                        .and_then(|member| assignment_name(extractor, member)),
                );
                target.child_by_field_name("lhs")
            }
            "subset" | "subset2" => {
                rebound.extend(
                    target
                        .child_by_field_name("arguments")
                        .and_then(|args| positional_string_argument(extractor, args, 0)),
                );
                target.child_by_field_name("function")
            }
            "call" => target.child_by_field_name("arguments").and_then(|args| {
                let mut cursor = args.walk();
                args.children_by_field_name("argument", &mut cursor)
                    .next()
                    .and_then(|argument| argument.child_by_field_name("value"))
            }),
            _ => {
                rebound.extend(assignment_name(extractor, target));
                None
            }
        };
        match next {
            Some(next) => target = next,
            None => return,
        }
    }
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
            } else if extractor.name_bindings.calls_constructor(&name) {
                same_file_class(extractor, &name)
            } else {
                None
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
            if !extractor.name_bindings.calls_constructor(&class_name) {
                return None;
            }
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
