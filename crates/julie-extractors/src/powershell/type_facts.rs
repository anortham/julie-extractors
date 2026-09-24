use crate::base::BaseExtractor;
use crate::base::types::TypeNameRules;
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};
use std::collections::{HashMap, HashSet};
use tree_sitter::Node;

use super::helpers::{
    find_class_name_node, find_command_name_node, find_function_name_node, find_method_name_node,
    has_modifier, invoked_command, split_function_scope,
};

pub(super) const TYPE_NAME_RULES: TypeNameRules = TypeNameRules {
    nullable_suffixes: &[],
    reference_prefixes: &[],
    generic_open: &['['],
};

/// Array base names such as `string[]` are reduced structurally, so the
/// `[` generic opener must not cut the array suffix off again.
const ARRAY_TYPE_NAME_RULES: TypeNameRules = TypeNameRules {
    nullable_suffixes: &[],
    reference_prefixes: &[],
    generic_open: &[],
};

const EXPR_WRAPPERS: &[&str] = &[
    "pipeline",
    "pipeline_chain",
    "logical_expression",
    "bitwise_expression",
    "comparison_expression",
    "additive_expression",
    "multiplicative_expression",
    "format_expression",
    "range_expression",
    "array_literal_expression",
    "unary_expression",
    "expression_with_unary_operator",
];

pub(super) fn record_declared_type_literal(base: &mut BaseExtractor, symbol_id: &str, node: Node) {
    let Some(type_node) = find_first_kind(node, "type_literal", 0) else {
        return;
    };
    record_type_literal(base, symbol_id, type_node, false);
}

pub(super) fn record_assignment_facts(
    base: &mut BaseExtractor,
    symbol_id: &str,
    node: Node,
    index: &ReturnTypeIndex,
) {
    if let Some(left) = direct_child(node, "left_assignment_expression")
        && let Some(type_node) = find_first_kind(left, "type_literal", 0)
    {
        record_type_literal(base, symbol_id, type_node, false);
    }

    if let Some(value) = node.child_by_field_name("value") {
        record_inferred_rhs(base, symbol_id, value, index);
    }
}

/// The file's class names and the declared return types of its functions
/// (`[OutputType([T])]`) and class methods, keyed case-insensitively.
#[derive(Debug, Default)]
pub(super) struct ReturnTypeIndex {
    classes: HashSet<String>,
    callables: HashMap<(Option<String>, String), Vec<ReturnEntry>>,
}

#[derive(Debug)]
struct ReturnEntry {
    is_static: bool,
    /// `None` when the callable declares no single return type.
    returns: Option<ReducedType>,
}

impl ReturnTypeIndex {
    pub(super) fn build(base: &BaseExtractor, root: Node) -> Self {
        let mut index = Self::default();
        let mut stack = vec![root];
        while let Some(node) = stack.pop() {
            match node.kind() {
                "class_statement" => {
                    if let Some(name) = find_class_name_node(node) {
                        index
                            .classes
                            .insert(base.get_node_text(&name).to_ascii_lowercase());
                    }
                }
                "function_statement" => {
                    if let Some(name) = find_function_name_node(node) {
                        let raw = base.get_node_text(&name);
                        let key = (None, split_function_scope(&raw).1.to_ascii_lowercase());
                        let returns = output_type(base, node);
                        index.add(key, false, returns);
                    }
                }
                "class_method_definition" => {
                    if let (Some(owner), Some(name)) = (
                        enclosing_class_name(base, node),
                        find_method_name_node(node),
                    ) {
                        let key = (
                            Some(owner.to_ascii_lowercase()),
                            base.get_node_text(&name).to_ascii_lowercase(),
                        );
                        let returns = direct_child(node, "type_literal")
                            .and_then(|type_node| reduce_type_literal(base, type_node));
                        index.add(key, has_modifier(base, node, "static"), returns);
                    }
                }
                _ => {}
            }
            stack.extend(node.named_children(&mut node.walk()));
        }
        index
    }

    fn add(
        &mut self,
        key: (Option<String>, String),
        is_static: bool,
        returns: Option<ReducedType>,
    ) {
        self.callables
            .entry(key)
            .or_default()
            .push(ReturnEntry { is_static, returns });
    }

    fn has_class(&self, name: &str) -> bool {
        self.classes.contains(&name.to_ascii_lowercase())
    }

    /// The return type every same-named callable of `owner` with this
    /// staticness agrees on.
    fn lookup(&self, owner: Option<&str>, name: &str, is_static: bool) -> Option<&ReducedType> {
        let key = (
            owner.map(str::to_ascii_lowercase),
            name.to_ascii_lowercase(),
        );
        let mut returns = self
            .callables
            .get(&key)?
            .iter()
            .filter(|entry| entry.is_static == is_static)
            .map(|entry| entry.returns.as_ref());
        let first = returns.next()??;
        returns.all(|other| other == Some(first)).then_some(first)
    }
}

/// The one type a function's `[OutputType(...)]` attributes declare. A string
/// or literal output type, or two different types, declares none.
fn output_type(base: &BaseExtractor, function: Node) -> Option<ReducedType> {
    let mut types = Vec::new();
    let mut cursor = function.walk();
    for block in function
        .children(&mut cursor)
        .filter(|child| child.kind() == "script_block")
    {
        let Some(param_block) = direct_child(block, "param_block") else {
            continue;
        };
        let mut lists = param_block.walk();
        for list in param_block
            .children(&mut lists)
            .filter(|child| child.kind() == "attribute_list")
        {
            let mut attributes = list.walk();
            for attribute in list
                .children(&mut attributes)
                .filter(|child| child.kind() == "attribute")
            {
                let is_output_type =
                    direct_child(attribute, "attribute_name").is_some_and(|name| {
                        base.get_node_text(&name).eq_ignore_ascii_case("OutputType")
                    });
                if is_output_type {
                    collect_output_types(base, attribute, &mut types, 0)?;
                }
            }
        }
    }
    let first = types.first()?;
    types
        .iter()
        .all(|other| other == first)
        .then(|| first.clone())
}

/// Push the positional type literals of an `OutputType` attribute. `None`
/// when a positional argument is a string or other literal.
fn collect_output_types(
    base: &BaseExtractor,
    node: Node,
    types: &mut Vec<ReducedType>,
    depth: u32,
) -> Option<()> {
    if !should_visit_tree_depth(depth) {
        return None;
    }
    match node.kind() {
        "attribute_name" => return Some(()),
        "attribute_argument" if direct_child(node, "simple_name").is_some() => return Some(()),
        "type_literal" => {
            types.push(reduce_type_literal(base, node)?);
            return Some(());
        }
        kind if kind.ends_with("_literal") => return None,
        _ => {}
    }
    let child_depth = child_tree_depth(depth)?;
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        collect_output_types(base, child, types, child_depth)?;
    }
    Some(())
}

pub(super) fn enclosing_class_name(base: &BaseExtractor, node: Node) -> Option<String> {
    let mut current = node.parent();
    while let Some(candidate) = current {
        if candidate.kind() == "class_statement" {
            return find_class_name_node(candidate).map(|n| base.get_node_text(&n));
        }
        current = candidate.parent();
    }
    None
}

pub(super) fn this_receiver_type(base: &BaseExtractor, node: Node) -> Option<String> {
    let variable = direct_child(node, "variable")?;
    let name = super::helpers::variable_name(&base.get_node_text(&variable));
    if !name.eq_ignore_ascii_case("this") {
        return None;
    }
    enclosing_class_name(base, node)
}

pub(super) fn invocation_member_name<'a>(
    base: &BaseExtractor,
    node: Node<'a>,
) -> Option<(Node<'a>, String)> {
    let member_name = direct_child(node, "member_name")?;
    let simple = find_first_kind(member_name, "simple_name", 0)?;
    Some((simple, base.get_node_text(&simple)))
}

/// The `variable` a plain assignment targets. Member, index, and static
/// property targets (`$o.P =`, `$h[k] =`, `[T]::P =`) have none.
pub(super) fn assignment_variable_node(node: Node) -> Option<Node> {
    if node.kind() != "assignment_expression" {
        return None;
    }
    let mut current = direct_child(node, "left_assignment_expression")?;
    loop {
        current = match current.kind() {
            "variable" => return Some(current),
            "cast_expression" => {
                let mut cursor = current.walk();
                current.named_children(&mut cursor).last()?
            }
            kind if kind == "left_assignment_expression" || EXPR_WRAPPERS.contains(&kind) => {
                first_named_child(current)?
            }
            _ => return None,
        };
    }
}

pub(super) fn record_type_literal(
    base: &mut BaseExtractor,
    symbol_id: &str,
    type_node: Node,
    is_inferred: bool,
) {
    if let Some(reduced) = reduce_type_literal(base, type_node) {
        record_reduced_type(base, symbol_id, &reduced, is_inferred);
    }
}

fn record_reduced_type(
    base: &mut BaseExtractor,
    symbol_id: &str,
    reduced: &ReducedType,
    is_inferred: bool,
) {
    if reduced.base_name.eq_ignore_ascii_case("void") {
        return;
    }
    let rules = if reduced.is_array {
        &ARRAY_TYPE_NAME_RULES
    } else {
        &TYPE_NAME_RULES
    };
    base.record_declared_type_fact_with_declared(
        symbol_id,
        &reduced.base_name,
        &reduced.declared,
        rules,
        is_inferred,
    );
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ReducedType {
    base_name: String,
    declared: String,
    is_array: bool,
}

fn reduce_type_literal(base: &BaseExtractor, type_literal: Node) -> Option<ReducedType> {
    let spec = direct_child(type_literal, "type_spec")?;
    let declared = base.get_node_text(&type_literal).trim().to_string();
    if direct_child(spec, "array_type_name").is_some() {
        return Some(ReducedType {
            base_name: base.get_node_text(&spec).trim().to_string(),
            declared,
            is_array: true,
        });
    }
    let name = direct_child(spec, "generic_type_name")
        .and_then(|generic| direct_child(generic, "type_name"))
        .or_else(|| direct_child(spec, "type_name"))?;
    Some(ReducedType {
        base_name: base.get_node_text(&name).trim().to_string(),
        declared,
        is_array: false,
    })
}

/// Record the type an untyped assignment's value produces (`is_inferred=true`):
/// a cast, `[T]::new()` or `New-Object T` for a same-file class `T`, or a call
/// with a declared return type to a same-file function, a `$this` method of
/// the enclosing class, or a static method of a same-file class. Parentheses
/// are looked through; a pipeline or a longer chain records nothing.
fn record_inferred_rhs(
    base: &mut BaseExtractor,
    symbol_id: &str,
    value: Node,
    index: &ReturnTypeIndex,
) {
    let Some(core) = value_core(value, 0) else {
        return;
    };
    if core.kind() == "cast_expression" {
        if let Some(type_node) = direct_child(core, "type_literal") {
            record_type_literal(base, symbol_id, type_node, true);
        }
        return;
    }
    if let Some(type_name) = inferred_constructor_name(base, core, index) {
        base.record_declared_type_fact_with_declared(
            symbol_id,
            &type_name,
            &type_name,
            &TYPE_NAME_RULES,
            true,
        );
        return;
    }
    if let Some(returns) = call_return_type(base, core, index) {
        let returns = returns.clone();
        record_reduced_type(base, symbol_id, &returns, true);
    }
}

fn value_core(value: Node, depth: u32) -> Option<Node> {
    if !should_visit_tree_depth(depth) {
        return None;
    }
    let core = unwrap_expr(value);
    if core.kind() == "parenthesized_expression" {
        return value_core(first_named_child(core)?, child_tree_depth(depth)?);
    }
    Some(core)
}

fn call_return_type<'a>(
    base: &BaseExtractor,
    core: Node,
    index: &'a ReturnTypeIndex,
) -> Option<&'a ReducedType> {
    match core.kind() {
        "command" => {
            let (_, name) = invoked_command(base, core)?;
            index.lookup(None, &name, false)
        }
        "invokation_expression" | "invocation_expression" => {
            let (_, method) = invocation_member_name(base, core)?;
            let is_static = direct_child(core, "::").is_some();
            if is_static {
                let owner = reduce_type_literal(base, direct_child(core, "type_literal")?)?;
                if owner.is_array {
                    return None;
                }
                index.lookup(Some(&owner.base_name), &method, true)
            } else {
                let owner = this_receiver_type(base, core)?;
                index.lookup(Some(&owner), &method, false)
            }
        }
        _ => None,
    }
}

fn inferred_constructor_name(
    base: &BaseExtractor,
    core: Node,
    index: &ReturnTypeIndex,
) -> Option<String> {
    match core.kind() {
        "invokation_expression" | "invocation_expression" => {
            let (_, member) = invocation_member_name(base, core)?;
            if !member.eq_ignore_ascii_case("new") {
                return None;
            }
            let type_node = direct_child(core, "type_literal")?;
            let reduced = reduce_type_literal(base, type_node)?;
            if reduced.is_array || reduced.base_name.contains('.') {
                return None;
            }
            index
                .has_class(&reduced.base_name)
                .then_some(reduced.base_name)
        }
        "command" | "command_expression" => new_object_type_name(base, core, index),
        _ => None,
    }
}

fn new_object_type_name(
    base: &BaseExtractor,
    command: Node,
    index: &ReturnTypeIndex,
) -> Option<String> {
    let name_node = find_command_name_node(command)?;
    if !base
        .get_node_text(&name_node)
        .eq_ignore_ascii_case("New-Object")
    {
        return None;
    }
    let elements = direct_child(command, "command_elements")?;
    let mut cursor = elements.walk();
    for child in elements.children(&mut cursor) {
        if matches!(
            child.kind(),
            "command_argument_sep" | "command_parameter" | "redirection"
        ) {
            continue;
        }
        let text = base.get_node_text(&child).trim().to_string();
        if text.is_empty() || text.starts_with('-') {
            continue;
        }
        if text.contains('.') {
            return None;
        }
        return index.has_class(&text).then_some(text);
    }
    None
}

fn unwrap_expr(node: Node) -> Node {
    let mut current = node;
    loop {
        if !EXPR_WRAPPERS.contains(&current.kind()) || current.named_child_count() != 1 {
            return current;
        }
        let Some(child) = first_named_child(current) else {
            return current;
        };
        current = child;
    }
}

fn direct_child<'a>(node: Node<'a>, kind: &str) -> Option<Node<'a>> {
    let mut cursor = node.walk();
    node.children(&mut cursor)
        .find(|child| child.kind() == kind)
}

fn first_named_child(node: Node) -> Option<Node> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor).next()
}

fn find_first_kind<'a>(node: Node<'a>, kind: &str, depth: u32) -> Option<Node<'a>> {
    if !should_visit_tree_depth(depth) {
        return None;
    }
    if node.kind() == kind {
        return Some(node);
    }
    let child_depth = child_tree_depth(depth)?;
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if let Some(found) = find_first_kind(child, kind, child_depth) {
            return Some(found);
        }
    }
    None
}
