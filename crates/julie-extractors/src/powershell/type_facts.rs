use crate::base::BaseExtractor;
use crate::base::types::TypeNameRules;
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};
use std::collections::{HashMap, HashSet};
use std::ops::Range;
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

    if is_plain_assignment(base, node)
        && let Some(value) = node.child_by_field_name("value")
    {
        record_inferred_rhs(base, symbol_id, value, index);
    }
}

/// A plain `=`. A compound operator (`+=`, `??=`, ...) combines the value with
/// the variable's current one, so the value's type is not the variable's.
fn is_plain_assignment(base: &BaseExtractor, node: Node) -> bool {
    direct_child(node, "ERROR").is_none()
        && direct_child(node, "assignement_operator")
            .is_some_and(|operator| base.get_node_text(&operator).trim() == "=")
}

/// Type name endings of collections that PowerShell unrolls into the
/// pipeline when a function outputs them.
const UNROLLED_NAME_SUFFIXES: &[&str] = &[
    "collection",
    "list",
    "array",
    "enumerable",
    "enumerator",
    "queue",
    "stack",
    "datatable",
];

const ALIAS_COMMANDS: &[&str] = &["set-alias", "new-alias", "sal", "nal"];

/// `(name, takes_value)` for the parameters of `Set-Alias` and `New-Alias`.
const ALIAS_PARAMETERS: &[(&str, bool)] = &[
    ("name", true),
    ("value", true),
    ("description", true),
    ("option", true),
    ("scope", true),
    ("force", false),
    ("passthru", false),
    ("whatif", false),
    ("confirm", false),
];

/// The file's class names and the declared return types of its functions
/// (`[OutputType([T])]`) and class methods, keyed case-insensitively.
#[derive(Debug, Default)]
pub(super) struct ReturnTypeIndex {
    classes: HashSet<String>,
    class_bases: HashMap<String, Vec<String>>,
    callables: HashMap<(Option<String>, String), Vec<ReturnEntry>>,
    /// Names that `Set-Alias` or `New-Alias` define. An alias wins over a
    /// function of the same name.
    aliases: HashSet<String>,
    /// A `Set-Alias` or `New-Alias` whose name is not a literal.
    has_unknown_alias: bool,
}

#[derive(Debug)]
struct ReturnEntry {
    is_static: bool,
    /// `None` when the callable declares no single return type.
    returns: Option<ReducedType>,
    /// The byte range of the function or method that holds a nested function.
    /// PowerShell defines the function only in that scope.
    scope: Option<Range<usize>>,
}

impl ReturnTypeIndex {
    pub(super) fn build(base: &BaseExtractor, root: Node) -> Self {
        let mut index = Self::default();
        let mut stack = vec![root];
        while let Some(node) = stack.pop() {
            match node.kind() {
                "class_statement" => {
                    if let Some(name) = find_class_name_node(node) {
                        let name_text = base.get_node_text(&name).to_ascii_lowercase();
                        index
                            .class_bases
                            .insert(name_text.clone(), class_base_names(base, node, name));
                        index.classes.insert(name_text);
                    }
                }
                "function_statement" => {
                    if let Some(name) = find_function_name_node(node) {
                        let raw = base.get_node_text(&name);
                        let key = (None, split_function_scope(&raw).1.to_ascii_lowercase());
                        let returns = output_type(base, node);
                        let scope = enclosing_scope(node).map(|scope| scope.byte_range());
                        index.add(key, false, returns, scope);
                        for attribute in function_attributes(base, node, "Alias") {
                            index.add_alias_attribute(base, attribute);
                        }
                    }
                }
                "command" => index.add_alias(base, node),
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
                        index.add(key, has_modifier(base, node, "static"), returns, None);
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
        scope: Option<Range<usize>>,
    ) {
        self.callables.entry(key).or_default().push(ReturnEntry {
            is_static,
            returns,
            scope,
        });
    }

    /// Record the name a `Set-Alias` or `New-Alias` command defines: the
    /// `-Name` value, or else the first positional argument.
    fn add_alias(&mut self, base: &BaseExtractor, command: Node) {
        let Some(name_node) = command.child_by_field_name("command_name") else {
            return;
        };
        let command_name = base.get_node_text(&name_node).to_ascii_lowercase();
        let command_name = command_name.rsplit('\\').next().unwrap_or_default();
        if !ALIAS_COMMANDS.contains(&command_name) {
            return;
        }
        match alias_name(base, command) {
            Some(name) => {
                self.aliases.insert(name);
            }
            None => self.has_unknown_alias = true,
        }
    }

    /// Record the names a function's `[Alias(...)]` attribute defines.
    fn add_alias_attribute(&mut self, base: &BaseExtractor, attribute: Node) {
        let mut stack = vec![attribute];
        while let Some(node) = stack.pop() {
            match node.kind() {
                "attribute_name" => {}
                "string_literal" => match literal_alias_name(&base.get_node_text(&node)) {
                    Some(name) => {
                        self.aliases.insert(name);
                    }
                    None => self.has_unknown_alias = true,
                },
                "variable" | "sub_expression" | "expandable_string_literal" => {
                    self.has_unknown_alias = true
                }
                _ => stack.extend(node.named_children(&mut node.walk())),
            }
        }
    }

    fn has_class(&self, name: &str) -> bool {
        self.classes.contains(&name.to_ascii_lowercase())
    }

    /// The return type every same-named method of `owner` with this
    /// staticness agrees on.
    fn lookup_method(&self, owner: &str, name: &str, is_static: bool) -> Option<&ReducedType> {
        let key = (Some(owner.to_ascii_lowercase()), name.to_ascii_lowercase());
        agreed_return(
            self.callables
                .get(&key)?
                .iter()
                .filter(|entry| entry.is_static == is_static),
        )
    }

    /// The output type every same-named function agrees on, when `call` can
    /// see one of them and no alias hides the name. A collection output type
    /// records nothing: PowerShell unrolls it, so the variable holds one item,
    /// an `object[]`, or `$null`.
    fn lookup_function(&self, name: &str, call: Node) -> Option<&ReducedType> {
        let name = name.to_ascii_lowercase();
        if self.has_unknown_alias || self.aliases.contains(&name) {
            return None;
        }
        let entries = self.callables.get(&(None, name))?;
        let visible = entries.iter().any(|entry| {
            entry
                .scope
                .as_ref()
                .is_none_or(|scope| scope.contains(&call.start_byte()))
        });
        if !visible {
            return None;
        }
        agreed_return(entries.iter()).filter(|returns| !self.is_unrolled(returns))
    }

    fn is_unrolled(&self, returns: &ReducedType) -> bool {
        returns.is_array || returns.is_generic || self.is_collection_name(&returns.base_name, 0)
    }

    /// A known collection name, or a same-file class that derives from one.
    fn is_collection_name(&self, name: &str, depth: u32) -> bool {
        if !should_visit_tree_depth(depth) {
            return true;
        }
        let name = name.to_ascii_lowercase();
        let last = name.rsplit('.').next().unwrap_or_default();
        UNROLLED_NAME_SUFFIXES
            .iter()
            .any(|suffix| last.ends_with(suffix))
            || self.class_bases.get(&name).is_some_and(|bases| {
                bases
                    .iter()
                    .any(|base_name| self.is_collection_name(base_name, depth + 1))
            })
    }
}

fn agreed_return<'a>(
    mut entries: impl Iterator<Item = &'a ReturnEntry>,
) -> Option<&'a ReducedType> {
    let first = entries.next()?.returns.as_ref()?;
    entries
        .all(|other| other.returns.as_ref() == Some(first))
        .then_some(first)
}

/// The function or method that scopes a nested function. A function in a
/// script block counts as top level, because Pester blocks and dot-sourced
/// blocks define it in the caller's scope. An enclosing node with a parse
/// error is not trusted: error recovery often nests later top-level functions
/// in a function that is missing its closing brace.
fn enclosing_scope(node: Node) -> Option<Node> {
    let mut current = node.parent();
    while let Some(candidate) = current {
        if matches!(
            candidate.kind(),
            "function_statement" | "class_method_definition"
        ) {
            return (!candidate.has_error()).then_some(candidate);
        }
        current = candidate.parent();
    }
    None
}

/// The base type names after `class Name :`, with generic arguments removed.
/// The grammar parses a generic base as an ERROR node, so this reads the text.
fn class_base_names(base: &BaseExtractor, class: Node, name: Node) -> Vec<String> {
    let text = base.get_node_text(&class);
    let header = text
        .get(name.end_byte() - class.start_byte()..)
        .unwrap_or_default();
    let header = header.split('{').next().unwrap_or_default().trim_start();
    let Some(bases) = header.strip_prefix(':') else {
        return Vec::new();
    };
    let mut names = Vec::new();
    let mut depth = 0usize;
    let mut current = String::new();
    for ch in bases.chars() {
        match ch {
            '[' => depth += 1,
            ']' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => names.push(std::mem::take(&mut current)),
            _ if depth == 0 => current.push(ch),
            _ => {}
        }
    }
    names.push(current);
    names
        .into_iter()
        .map(|name| name.trim().to_ascii_lowercase())
        .filter(|name| !name.is_empty())
        .collect()
}

/// The lowercased literal name a `Set-Alias` or `New-Alias` command defines.
/// `None` when the name is not a literal or the command has no name.
fn alias_name(base: &BaseExtractor, command: Node) -> Option<String> {
    let elements = direct_child(command, "command_elements")?;
    let mut cursor = elements.walk();
    let mut expects_value = false;
    let mut expects_name = false;
    let mut positional = 0;
    for element in elements.named_children(&mut cursor) {
        if element.kind() == "command_argument_sep" {
            continue;
        }
        let text = base.get_node_text(&element);
        if element.kind() == "command_parameter" {
            let parameter = text.trim_start_matches('-').to_ascii_lowercase();
            let has_colon = parameter.ends_with(':');
            let parameter = parameter.trim_end_matches(':');
            let mut matches = ALIAS_PARAMETERS
                .iter()
                .filter(|(name, _)| name.starts_with(parameter));
            let known = match (matches.next(), matches.next()) {
                (Some(only), None) => Some(*only),
                _ => ALIAS_PARAMETERS
                    .iter()
                    .find(|(name, _)| *name == parameter)
                    .copied(),
            };
            match known {
                Some(("name", _)) => expects_name = true,
                Some((_, takes_value)) => expects_value = takes_value || has_colon,
                None => return None,
            }
            continue;
        }
        if text.trim_start().starts_with('@') {
            return None;
        }
        if expects_name || (!expects_value && positional == 0) {
            return literal_alias_name(&text);
        }
        if !expects_value {
            positional += 1;
        }
        expects_value = false;
    }
    None
}

fn literal_alias_name(text: &str) -> Option<String> {
    let name = text.trim().trim_matches(['"', '\'']);
    let is_literal = !name.is_empty()
        && !name.contains([
            '$', '@', '(', ')', '{', '}', '[', ']', ',', ';', '`', '"', '\'',
        ]);
    is_literal.then(|| name.to_ascii_lowercase())
}

/// The one type a function's `[OutputType(...)]` attributes declare. A string
/// or literal output type, or two different types, declares none.
fn output_type(base: &BaseExtractor, function: Node) -> Option<ReducedType> {
    let mut types = Vec::new();
    for attribute in function_attributes(base, function, "OutputType") {
        collect_output_types(base, attribute, &mut types, 0)?;
    }
    let first = types.first()?;
    types
        .iter()
        .all(|other| other == first)
        .then(|| first.clone())
}

/// The function's own attributes (on its `param` block) with this name.
fn function_attributes<'a>(base: &BaseExtractor, function: Node<'a>, name: &str) -> Vec<Node<'a>> {
    let mut found = Vec::new();
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
            found.extend(list.children(&mut attributes).filter(|attribute| {
                attribute.kind() == "attribute"
                    && direct_child(*attribute, "attribute_name").is_some_and(|attribute_name| {
                        base.get_node_text(&attribute_name)
                            .eq_ignore_ascii_case(name)
                    })
            }));
        }
    }
    found
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
    is_generic: bool,
}

fn reduce_type_literal(base: &BaseExtractor, type_literal: Node) -> Option<ReducedType> {
    let spec = direct_child(type_literal, "type_spec")?;
    let declared = base.get_node_text(&type_literal).trim().to_string();
    if direct_child(spec, "array_type_name").is_some() {
        return Some(ReducedType {
            base_name: base.get_node_text(&spec).trim().to_string(),
            declared,
            is_array: true,
            is_generic: false,
        });
    }
    let generic = direct_child(spec, "generic_type_name");
    let name = generic
        .and_then(|generic| direct_child(generic, "type_name"))
        .or_else(|| direct_child(spec, "type_name"))?;
    Some(ReducedType {
        base_name: base.get_node_text(&name).trim().to_string(),
        declared,
        is_array: false,
        is_generic: generic.is_some(),
    })
}

/// Record the type an untyped assignment's value produces (`is_inferred=true`):
/// a cast, `[T]::new()` or `New-Object T` for a same-file class `T`, or a call
/// with a declared return type to a same-file function, a `$this` method of
/// the enclosing class, or a static method of a same-file class. Parentheses
/// are looked through; an operator, a pipeline, a redirection, or a longer
/// chain records nothing.
fn record_inferred_rhs(
    base: &mut BaseExtractor,
    symbol_id: &str,
    value: Node,
    index: &ReturnTypeIndex,
) {
    let Some(core) = value_core(value, 0) else {
        return;
    };
    if has_redirection(core) {
        return;
    }
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
    let core = unwrap_value(value);
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
            let (target, name) = invoked_command(base, core)?;
            if base.get_node_text(&target).contains(['/', '\\']) {
                return None;
            }
            index.lookup_function(&name, core)
        }
        "invokation_expression" | "invocation_expression" => {
            let (_, method) = invocation_member_name(base, core)?;
            let is_static = direct_child(core, "::").is_some();
            if is_static {
                let owner = reduce_type_literal(base, direct_child(core, "type_literal")?)?;
                if owner.is_array || owner.is_generic {
                    return None;
                }
                index.lookup_method(&owner.base_name, &method, true)
            } else {
                if in_script_block_within_class(core) {
                    return None;
                }
                let owner = this_receiver_type(base, core)?;
                index.lookup_method(&owner, &method, false)
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
            if reduced.is_array || reduced.is_generic || reduced.base_name.contains('.') {
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

/// `$this` in a script block need not be the class instance: an
/// `Add-Member -MemberType ScriptMethod` or `Register-ObjectEvent -Action`
/// block binds it to another object when it runs.
fn in_script_block_within_class(node: Node) -> bool {
    let mut current = node.parent();
    while let Some(candidate) = current {
        match candidate.kind() {
            "script_block_expression" => return true,
            "class_statement" => return false,
            _ => current = candidate.parent(),
        }
    }
    false
}

fn has_redirection(core: Node) -> bool {
    matches!(core.kind(), "command" | "command_expression")
        && direct_child(core, "command_elements")
            .is_some_and(|elements| direct_child(elements, "redirection").is_some())
}

/// Look through wrappers that hold exactly one child. A wrapper with an
/// operator token (`-not x`, `,x`, `-join x`) has two children and changes
/// the value's type, so it stops the unwrap.
fn unwrap_value(node: Node) -> Node {
    let mut current = node;
    while EXPR_WRAPPERS.contains(&current.kind()) && current.child_count() == 1 {
        let Some(child) = current.child(0) else {
            return current;
        };
        current = child;
    }
    current
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
