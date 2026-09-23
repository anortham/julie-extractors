//! Helper functions for node finding, attributes, and modifiers
//! Provides utilities for navigating the PowerShell AST and extracting node information

use crate::base::BaseExtractor;
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};
use regex::Regex;
use std::sync::LazyLock;
use tree_sitter::Node;

/// Matches `[Parameter(...)]` attributes
static PARAMETER_ATTR_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\[Parameter[^\]]*\]").unwrap());

/// Matches a PowerShell `param(...)` block opener.
static PARAM_BLOCK_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\bparam\s*\(").unwrap());

/// Matches type annotation brackets: `[TypeName]`
static BRACKET_TYPE_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\[(\w+)\]").unwrap());

/// The bare name of a variable reference: no `$`/`@` sigil, no braces, and no
/// scope or drive qualifier (`$script:Hits`, `$env:PATH`, `${global:x}`).
pub(super) fn variable_name(raw: &str) -> String {
    split_qualifier(raw).1.to_string()
}

/// The identity of a variable reference inside one scope: its effective scope
/// or drive plus its case-insensitive name. `$x`, `$local:x`, `$private:x`,
/// and `$variable:x` name one variable; `$script:x` and `$env:x` do not.
pub(super) fn variable_key(raw: &str) -> String {
    let (qualifier, name) = split_qualifier(raw);
    let scope = qualifier
        .map(str::to_ascii_lowercase)
        .filter(|scope| !matches!(scope.as_str(), "local" | "private" | "variable"))
        .unwrap_or_default();
    format!("{scope}:{}", name.to_ascii_lowercase())
}

fn split_qualifier(raw: &str) -> (Option<&str>, &str) {
    let name = raw.trim_start_matches(['$', '@']);
    let name = name
        .strip_prefix('{')
        .and_then(|inner| inner.strip_suffix('}'))
        .unwrap_or(name);
    match name.split_once(':') {
        Some((qualifier, rest)) if is_variable_qualifier(qualifier) => (Some(qualifier), rest),
        _ => (None, name),
    }
}

/// Whether a variable reference carries the `env:` drive qualifier.
pub(super) fn is_environment_reference(raw: &str) -> bool {
    raw.trim_start_matches(['$', '{'])
        .split_once(':')
        .is_some_and(|(qualifier, _)| qualifier.eq_ignore_ascii_case("env"))
}

fn is_variable_qualifier(qualifier: &str) -> bool {
    [
        "global", "script", "local", "private", "using", "env", "variable", "workflow",
    ]
    .iter()
    .any(|known| qualifier.eq_ignore_ascii_case(known))
}

/// Find the function name node from a function_statement
pub(super) fn find_function_name_node<'a>(node: Node<'a>) -> Option<Node<'a>> {
    let mut cursor = node.walk();
    let children: Vec<_> = node.children(&mut cursor).collect();
    children
        .into_iter()
        .find(|child| matches!(child.kind(), "function_name" | "identifier" | "cmdlet_name"))
}

/// Find the variable name node from an assignment or variable node
pub(super) fn find_variable_name_node<'a>(node: Node<'a>) -> Option<Node<'a>> {
    let mut cursor = node.walk();
    let children: Vec<_> = node.children(&mut cursor).collect();
    children.into_iter().find(|child| {
        matches!(
            child.kind(),
            "left_assignment_expression" | "variable" | "identifier"
        )
    })
}

/// Find the parameter name node from a parameter_definition
pub(super) fn find_parameter_name_node<'a>(node: Node<'a>) -> Option<Node<'a>> {
    let mut cursor = node.walk();
    let children: Vec<_> = node.children(&mut cursor).collect();
    children
        .into_iter()
        .find(|child| matches!(child.kind(), "variable" | "parameter_name"))
}

/// Find the class name node from a class_statement
pub(super) fn find_class_name_node<'a>(node: Node<'a>) -> Option<Node<'a>> {
    let mut cursor = node.walk();
    let children: Vec<_> = node.children(&mut cursor).collect();
    children
        .into_iter()
        .find(|child| matches!(child.kind(), "simple_name" | "identifier" | "type_name"))
}

/// Find the method name node from a class_method_definition
pub(super) fn find_method_name_node<'a>(node: Node<'a>) -> Option<Node<'a>> {
    let mut cursor = node.walk();
    let children: Vec<_> = node.children(&mut cursor).collect();
    children
        .into_iter()
        .find(|child| matches!(child.kind(), "simple_name" | "identifier" | "method_name"))
}

/// Find the property name node from a class_property_definition
pub(super) fn find_property_name_node<'a>(node: Node<'a>) -> Option<Node<'a>> {
    let mut cursor = node.walk();
    let children: Vec<_> = node.children(&mut cursor).collect();
    children
        .into_iter()
        .find(|child| matches!(child.kind(), "variable" | "property_name" | "identifier"))
}

/// Find the enum name node from an enum_statement
pub(super) fn find_enum_name_node<'a>(node: Node<'a>) -> Option<Node<'a>> {
    let mut cursor = node.walk();
    let children: Vec<_> = node.children(&mut cursor).collect();
    children
        .into_iter()
        .find(|child| matches!(child.kind(), "simple_name" | "identifier" | "type_name"))
}

/// Find the enum member name node from an enum_member
pub(super) fn find_enum_member_name_node<'a>(node: Node<'a>) -> Option<Node<'a>> {
    let mut cursor = node.walk();
    let children: Vec<_> = node.children(&mut cursor).collect();
    children
        .into_iter()
        .find(|child| matches!(child.kind(), "simple_name" | "identifier"))
}

/// Find the command name node from a command or command_expression
pub(super) fn find_command_name_node<'a>(node: Node<'a>) -> Option<Node<'a>> {
    let mut cursor = node.walk();
    let children: Vec<_> = node.children(&mut cursor).collect();
    children
        .into_iter()
        .find(|child| matches!(child.kind(), "command_name" | "identifier" | "cmdlet_name"))
}

/// The command a `command` node runs, as its name node and the name a caller
/// resolves: `Get-Thing` and `& Get-Thing` run `Get-Thing`; `& ./x.ps1` and
/// `& "$PSScriptRoot\x.ps1"` run the script file `x.ps1`. Dot-sourcing (an
/// import) and dynamic targets (`& $exe`, `& { }`) run no named command.
pub(super) fn invoked_command<'a>(
    base: &BaseExtractor,
    node: Node<'a>,
) -> Option<(Node<'a>, String)> {
    let target = node.child_by_field_name("command_name")?;
    if is_property_statement(base, node) {
        return None;
    }
    if target.kind() == "command_name" {
        let name = base.get_node_text(&target);
        return (!name.eq_ignore_ascii_case("using")).then_some((target, name));
    }
    if target.kind() != "command_name_expr" || !is_call_operator_invocation(base, node) {
        return None;
    }
    let inner = target.named_child(0)?;
    let text = match inner.kind() {
        "path_command_name" | "command_name" => base.get_node_text(&inner),
        "string_literal" => base
            .get_node_text(&inner)
            .trim_matches(['"', '\''])
            .to_string(),
        _ => return None,
    };
    let name = script_file_name(&text)?;
    Some((inner, name))
}

/// `Key = value` inside a DSC resource or similar keyword block: the grammar
/// reads it as a command named `Key`, but it assigns a property.
fn is_property_statement(base: &BaseExtractor, node: Node) -> bool {
    node.child_by_field_name("command_elements")
        .and_then(|elements| {
            let mut cursor = elements.walk();
            elements
                .named_children(&mut cursor)
                .find(|child| child.kind() != "command_argument_sep")
        })
        .is_some_and(|first| first.kind() == "generic_token" && base.get_node_text(&first) == "=")
}

fn is_call_operator_invocation(base: &BaseExtractor, node: Node) -> bool {
    invokation_operator(base, node).as_deref() == Some("&")
}

/// Whether a command dot-sources a script (`. ./common.ps1`).
pub(super) fn is_dot_sourcing(base: &BaseExtractor, node: Node) -> bool {
    invokation_operator(base, node).as_deref() == Some(".")
}

fn invokation_operator(base: &BaseExtractor, node: Node) -> Option<String> {
    let mut cursor = node.walk();
    node.children(&mut cursor)
        .find(|child| child.kind() == "command_invokation_operator")
        .map(|operator| base.get_node_text(&operator))
}

/// The last path segment of a command path, or the whole name for a bare
/// command. A segment that still holds an interpolation hole names nothing.
pub(super) fn script_file_name(path: &str) -> Option<String> {
    let name = path.rsplit(['/', '\\']).next()?.trim();
    (!name.is_empty() && !name.contains('$') && !name.contains('(')).then(|| name.to_string())
}

/// The function name without a scope qualifier (`global:Get-Tool` is
/// `Get-Tool`), and the qualifier in lowercase when one is present.
pub(super) fn split_function_scope(raw: &str) -> (Option<String>, &str) {
    match raw.split_once(':') {
        Some((scope, name))
            if ["global", "script", "local", "private"]
                .iter()
                .any(|known| scope.eq_ignore_ascii_case(known)) =>
        {
            (Some(scope.to_ascii_lowercase()), name)
        }
        _ => (None, raw),
    }
}

/// A command's argument values, each with the lowercase name of the
/// parameter it binds to (`-Name X` binds `X` to `name`). Positional values
/// bind to `None`. A value list (`'a', 'b'`) is one argument.
pub(super) fn command_arguments<'a>(
    base: &BaseExtractor,
    command: Node<'a>,
) -> Vec<(Option<String>, Node<'a>)> {
    let Some(elements) = command.child_by_field_name("command_elements") else {
        return Vec::new();
    };
    let mut arguments: Vec<(Option<String>, Node<'a>)> = Vec::new();
    let mut pending_parameter: Option<String> = None;
    let mut cursor = elements.walk();
    for element in elements.named_children(&mut cursor) {
        match element.kind() {
            "command_argument_sep" | "redirection" | "comment" => {}
            "command_parameter" => {
                let text = base.get_node_text(&element);
                let name = text.trim_start_matches('-').trim_end_matches(':');
                pending_parameter = Some(name.to_ascii_lowercase());
            }
            _ => {
                let continues_list = base.get_node_text(&element).starts_with(',');
                let parameter = match (continues_list, arguments.last()) {
                    (true, Some((previous, _))) => previous.clone(),
                    _ => pending_parameter.take(),
                };
                arguments.push((parameter, element));
            }
        }
    }
    arguments
}

/// The literal words of an argument value: bare tokens and the unquoted text
/// of string literals, in source order. An interpolated string keeps its
/// `$var` holes.
pub(super) fn argument_words(base: &BaseExtractor, value: Node) -> Vec<String> {
    let mut words = Vec::new();
    collect_argument_words(base, value, &mut words, 0);
    words
}

fn collect_argument_words(base: &BaseExtractor, node: Node, words: &mut Vec<String>, depth: u32) {
    if !should_visit_tree_depth(depth) {
        return;
    }
    match node.kind() {
        "generic_token" | "command_name" | "path_command_name" => {
            words.extend(
                base.get_node_text(&node)
                    .split(',')
                    .map(str::trim)
                    .filter(|word| !word.is_empty())
                    .map(str::to_string),
            );
            return;
        }
        "string_literal" => {
            let text = base.get_node_text(&node);
            let text = text.trim();
            let text = text
                .strip_prefix('@')
                .and_then(|inner| inner.strip_suffix('@'))
                .unwrap_or(text);
            words.push(text.trim_matches(['"', '\'']).to_string());
            return;
        }
        "variable" | "sub_expression" | "script_block_expression" | "hash_literal_expression" => {
            return;
        }
        _ => {}
    }
    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        collect_argument_words(base, child, words, child_depth);
    }
}

/// The module or script a path names: its last segment without a
/// `.psm1`/`.psd1`/`.ps1`/`.dll` extension (`$PSScriptRoot\Helpers.psm1` is
/// `Helpers`). A bare module name (`Az.Accounts`) is kept whole.
pub(super) fn module_stem(path: &str) -> Option<String> {
    let segment = path.rsplit(['/', '\\']).next()?.trim();
    let lower = segment.to_ascii_lowercase();
    let stem = [".psm1", ".psd1", ".ps1", ".dll"]
        .iter()
        .find_map(|extension| {
            lower
                .ends_with(extension)
                .then(|| &segment[..segment.len() - extension.len()])
        })
        .unwrap_or(segment);
    (!stem.is_empty() && !stem.contains(['$', '(', '*'])).then(|| stem.to_string())
}

/// Find the configuration name node from a configuration statement
pub(super) fn find_configuration_name_node<'a>(node: Node<'a>) -> Option<Node<'a>> {
    let mut cursor = node.walk();
    let children: Vec<_> = node.children(&mut cursor).collect();
    children
        .into_iter()
        .find(|child| child.kind() == "identifier")
}

/// Extract the value from an enum member (if assigned)
pub(super) fn extract_enum_member_value(base: &BaseExtractor, node: Node) -> Option<String> {
    let mut cursor = node.walk();
    let children: Vec<_> = node.children(&mut cursor).collect();

    // Look for assignment pattern: name = value
    for (i, child) in children.iter().enumerate() {
        if child.kind() == "=" && i + 1 < children.len() {
            return Some(base.get_node_text(&children[i + 1]));
        }
    }
    None
}

/// Check if a node has an attribute (e.g., [CmdletBinding], [Parameter])
pub(super) fn has_attribute(base: &BaseExtractor, node: Node, attribute_name: &str) -> bool {
    let node_text = base.get_node_text(&node);
    node_text.contains(&format!("[{}", attribute_name))
}

/// Whether a class member carries a modifier keyword (`static`, `hidden`) as
/// one of its own `class_attribute` children. PowerShell keywords ignore case.
pub(super) fn has_modifier(base: &BaseExtractor, node: Node, modifier: &str) -> bool {
    let mut cursor = node.walk();
    node.children(&mut cursor).any(|child| {
        child.kind() == "class_attribute"
            && base.get_node_text(&child).eq_ignore_ascii_case(modifier)
    })
}

/// Extract parameter attributes from a parameter definition
pub(super) fn extract_parameter_attributes(base: &BaseExtractor, node: Node) -> String {
    let node_text = base.get_node_text(&node);
    if let Some(captures) = PARAMETER_ATTR_RE.captures(&node_text) {
        captures
            .get(0)
            .map_or(String::new(), |m| m.as_str().to_string())
    } else {
        String::new()
    }
}

pub(super) fn extract_command_annotation_attributes(
    base: &BaseExtractor,
    node: Node,
) -> Vec<String> {
    let node_text = base.get_node_text(&node);
    let prefix = match find_param_keyword(&node_text) {
        Some(index) => &node_text[..index],
        None => node_text.as_str(),
    };

    extract_non_type_bracket_attributes(prefix)
}

pub(super) fn extract_parameter_annotation_attributes(
    base: &BaseExtractor,
    node: Node,
) -> Vec<String> {
    let node_text = base.get_node_text(&node);
    extract_non_type_bracket_attributes(&node_text)
}

fn find_param_keyword(text: &str) -> Option<usize> {
    PARAM_BLOCK_RE.find(text).map(|m| m.start())
}

fn extract_non_type_bracket_attributes(text: &str) -> Vec<String> {
    extract_bracket_segments(text)
        .into_iter()
        .filter(|segment| !is_type_annotation_bracket(segment))
        .collect()
}

fn extract_bracket_segments(text: &str) -> Vec<String> {
    let mut segments = Vec::new();
    let mut start = None;
    let mut depth = 0usize;
    let mut quote = None;
    let mut previous_was_escape = false;

    for (index, ch) in text.char_indices() {
        if update_quote_state(ch, &mut quote, &mut previous_was_escape) {
            continue;
        }
        if quote.is_some() {
            continue;
        }

        match ch {
            '[' => {
                if depth == 0 {
                    start = Some(index);
                }
                depth += 1;
            }
            ']' if depth > 0 => {
                depth -= 1;
                if depth == 0
                    && let Some(start_index) = start.take()
                {
                    segments.push(text[start_index..index + ch.len_utf8()].to_string());
                }
            }
            _ => {}
        }
    }

    segments
}

fn update_quote_state(ch: char, quote: &mut Option<char>, previous_was_escape: &mut bool) -> bool {
    if let Some(active_quote) = *quote {
        if *previous_was_escape {
            *previous_was_escape = false;
            return true;
        }
        if ch == '`' || ch == '\\' {
            *previous_was_escape = true;
            return true;
        }
        if ch == active_quote {
            *quote = None;
            return true;
        }
        return true;
    }

    if matches!(ch, '\'' | '"') {
        *quote = Some(ch);
        return true;
    }

    false
}

fn is_type_annotation_bracket(segment: &str) -> bool {
    let inner = segment
        .trim()
        .strip_prefix('[')
        .and_then(|value| value.strip_suffix(']'))
        .map(str::trim)
        .unwrap_or(segment.trim());

    if inner.is_empty() {
        return false;
    }

    let name = inner
        .split_once('(')
        .map(|(name, _)| name)
        .unwrap_or(inner)
        .trim();

    if is_known_attribute_name(name) {
        return false;
    }

    if inner.contains('(') || inner.contains('=') || inner.contains(',') {
        return false;
    }

    inner
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '.' | '[' | ']'))
}

fn is_known_attribute_name(name: &str) -> bool {
    let key = name
        .rsplit(['.', '\\'])
        .next()
        .unwrap_or(name)
        .trim()
        .trim_end_matches("Attribute")
        .to_ascii_lowercase();

    matches!(
        key.as_str(),
        "alias"
            | "allowemptycollection"
            | "allowemptystring"
            | "allownull"
            | "argumentcompleter"
            | "argumenttransformation"
            | "cmdletbinding"
            | "credential"
            | "outputtype"
            | "parameter"
            | "psdefaultvalue"
            | "supportswildcards"
            | "validatescript"
            | "validateset"
            | "validaterange"
            | "validatepattern"
            | "validatelength"
            | "validatecount"
            | "validatenotnull"
            | "validatenotnullorempty"
            | "validatedrive"
            | "validateuserdrive"
    )
}

/// The base type name nodes of a `class_statement`: every `simple_name` after
/// the class name (`class Repo : BaseRepo, IDisposable`).
pub(super) fn class_base_name_nodes<'a>(node: Node<'a>) -> Vec<Node<'a>> {
    let mut cursor = node.walk();
    node.children(&mut cursor)
        .filter(|child| child.kind() == "simple_name")
        .skip(1)
        .collect()
}

/// Extract property type annotation from a property definition
pub(super) fn extract_property_type(base: &BaseExtractor, node: Node) -> Option<String> {
    let node_text = base.get_node_text(&node);
    BRACKET_TYPE_RE
        .captures(&node_text)
        .and_then(|captures| captures.get(1).map(|m| format!("[{}]", m.as_str())))
}

/// Recursively find all nodes of a given type
#[allow(clippy::only_used_in_recursion)] // &self used in recursive calls
pub(super) fn find_nodes_by_type<'a>(node: Node<'a>, node_type: &str) -> Vec<Node<'a>> {
    find_nodes_by_type_at_depth(node, node_type, 0)
}

fn find_nodes_by_type_at_depth<'a>(node: Node<'a>, node_type: &str, depth: u32) -> Vec<Node<'a>> {
    if !should_visit_tree_depth(depth) {
        return Vec::new();
    }

    let mut result = Vec::new();
    let Some(child_depth) = child_tree_depth(depth) else {
        return result;
    };
    let mut cursor = node.walk();

    // Check direct children first
    for child in node.children(&mut cursor) {
        if child.kind() == node_type {
            result.push(child);
        }
        // Recursively search in children
        result.extend(find_nodes_by_type_at_depth(child, node_type, child_depth));
    }

    result
}

/// Extract function name from a param_block node (used for advanced functions)
pub(super) fn extract_function_name_from_param_block(
    base: &BaseExtractor,
    node: Node,
    function_name_re: &regex::Regex,
) -> Option<String> {
    let mut ancestor = node.parent();
    while let Some(n) = ancestor {
        if n.kind() == "function_statement" {
            return None;
        }
        ancestor = n.parent();
    }

    let mut current = Some(node);
    while let Some(n) = current {
        if n.kind() == "program" {
            break;
        }
        current = n.parent();
    }

    if let Some(program_node) = current {
        let mut cursor = program_node.walk();
        let preceding_error_name = program_node
            .children(&mut cursor)
            .filter(|child| child.kind() == "ERROR" && child.start_byte() < node.start_byte())
            .filter_map(|child| {
                let text = base.get_node_text(&child);
                function_name_re
                    .captures(&text)
                    .and_then(|captures| captures.get(1))
                    .map(|name| name.as_str().to_string())
            })
            .last();
        if preceding_error_name.is_some() {
            return preceding_error_name;
        }
    }

    // Fallback: look in parent nodes for any ERROR containing function
    let mut current = node.parent();
    while let Some(n) = current {
        if n.kind() == "ERROR" {
            let text = base.get_node_text(&n);
            if let Some(captures) = function_name_re.captures(&text)
                && let Some(func_name) = captures.get(1)
            {
                return Some(func_name.as_str().to_string());
            }
        }
        current = n.parent();
    }

    None
}
