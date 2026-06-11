/// Identifier extraction for Elixir — LSP-quality find_references support.
///
/// Walks the tree to find: function calls, module references (aliases),
/// and qualified calls (Module.function).
use crate::base::{BaseExtractor, Identifier, IdentifierKind, Symbol, extract_type_arguments};
use std::collections::HashMap;
use tree_sitter::{Node, Tree};

use super::helpers::{find_child_by_type, is_elixir_parameterized_type_call};

/// Extract all identifier usages from parsed Elixir source
pub(super) fn extract_identifiers(
    base: &mut BaseExtractor,
    tree: &Tree,
    symbols: &[Symbol],
) -> Vec<Identifier> {
    let symbol_map: HashMap<String, &Symbol> = symbols.iter().map(|s| (s.id.clone(), s)).collect();
    walk_tree_for_identifiers(base, tree.root_node(), &symbol_map);
    walk_tree_for_typespec_type_arguments(base, tree.root_node(), &symbol_map);
    base.identifiers.clone()
}

fn walk_tree_for_identifiers(
    base: &mut BaseExtractor,
    node: Node,
    symbol_map: &HashMap<String, &Symbol>,
) {
    extract_identifier_from_node(base, node, symbol_map);

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk_tree_for_identifiers(base, child, symbol_map);
    }
}

fn extract_identifier_from_node(
    base: &mut BaseExtractor,
    node: Node,
    symbol_map: &HashMap<String, &Symbol>,
) {
    match node.kind() {
        "call" => {
            // Check if this is a definition macro — skip those
            if let Some(target) = node.child_by_field_name("target")
                && target.kind() == "identifier" {
                    let name = base.get_node_text(&target);
                    if is_definition_keyword(&name) {
                        return;
                    }
                    // Regular function call
                    let containing = find_containing_symbol_id(base, node, symbol_map);
                    base.create_identifier(&target, name, IdentifierKind::Call, containing);
                }
            // Phase 3b: capture string-literal call-arguments config-free; the
            // carrier classification + bloat gate run later in the artifact language-policy pass.
            record_elixir_call_arg_literals(base, node, symbol_map);
        }
        "dot" => {
            // Qualified call: Module.function
            // The dot node has a left (module) and right (function) child
            if let (Some(left), Some(right)) = (
                node.child_by_field_name("left"),
                node.child_by_field_name("right"),
            ) {
                // Module reference
                if left.kind() == "alias" {
                    let module_name = base.get_node_text(&left);
                    let containing = find_containing_symbol_id(base, node, symbol_map);
                    base.create_identifier(
                        &left,
                        module_name,
                        IdentifierKind::TypeUsage,
                        containing.clone(),
                    );

                    // Function reference
                    if right.kind() == "identifier" {
                        let fn_name = base.get_node_text(&right);
                        base.create_identifier(
                            &right,
                            fn_name,
                            IdentifierKind::MemberAccess,
                            containing,
                        );
                    }
                }
            }
        }
        "alias"
            // Standalone module reference (not part of a definition)
            if !is_in_definition_context(&node) => {
                let name = base.get_node_text(&node);
                let containing = find_containing_symbol_id(base, node, symbol_map);
                base.create_identifier(&node, name, IdentifierKind::TypeUsage, containing);
            }
        _ => {}
    }
}

fn is_definition_keyword(name: &str) -> bool {
    matches!(
        name,
        "defmodule"
            | "def"
            | "defp"
            | "defmacro"
            | "defmacrop"
            | "defprotocol"
            | "defimpl"
            | "defstruct"
            | "defguard"
            | "defguardp"
            | "defdelegate"
            | "defexception"
            | "defoverridable"
            | "import"
            | "use"
            | "alias"
            | "require"
    )
}

fn is_in_definition_context(node: &Node) -> bool {
    let mut current = Some(*node);
    while let Some(n) = current {
        if n.kind() == "call"
            && let Some(target) = n.child_by_field_name("target")
            && target.kind() == "identifier"
        {
            // Check if the alias is a direct argument of a definition call
            let parent_is_args = node.parent().is_some_and(|p| {
                p.kind() == "arguments" && p.parent().is_some_and(|pp| pp.id() == n.id())
            });
            if parent_is_args {
                return true;
            }
        }
        current = n.parent();
    }
    false
}

fn find_containing_symbol_id(
    base: &BaseExtractor,
    node: Node,
    symbol_map: &HashMap<String, &Symbol>,
) -> Option<String> {
    base.find_containing_symbol_from_map(&node, symbol_map)
        .map(|s| s.id.clone())
}

// ============================================================================
// String-literal call-argument capture (Miller bridge Phase 3b)
// ============================================================================

/// Capture string-literal arguments of an Elixir `call` as `Literal` records.
///
/// Config-free: `carrier` is the verbatim callee — the bare function name for an
/// `identifier` target (`query`), or the `Module.function` join for a `dot`
/// target (`HTTPoison.get`, `Repo.query`). `kind` stays `Other`; the `src/`
/// carrier gate sets the authoritative kind and drops non-carrier literals.
/// `arg_position` counts over the full argument list. Definition macros
/// (`def`/`defp`/…) are skipped — their "arguments" are heads/bodies, not call
/// args.
fn record_elixir_call_arg_literals(
    base: &mut BaseExtractor,
    call_node: Node,
    symbol_map: &HashMap<String, &Symbol>,
) {
    let Some(target) = call_node.child_by_field_name("target") else {
        return;
    };
    if target.kind() == "identifier" && is_definition_keyword(&base.get_node_text(&target)) {
        return;
    }
    // The argument list is a `arguments` CHILD node of the call (not a field).
    let args_node = {
        let mut cursor = call_node.walk();
        call_node
            .named_children(&mut cursor)
            .find(|n| n.kind() == "arguments")
    };
    let Some(args_node) = args_node else {
        return;
    };
    let carrier = elixir_carrier(base, target);
    let containing_symbol_id = find_containing_symbol_id(base, call_node, symbol_map);

    let mut cursor = args_node.walk();
    for (pos, arg) in args_node.named_children(&mut cursor).enumerate() {
        // Keyword args hold the literal in a `value` field; positional string
        // args have no `value` field, so fall back to the arg itself.
        let value = arg.child_by_field_name("value").unwrap_or(arg);
        if let Some(text) = base.decode_string_literal(&value) {
            base.record_literal(
                &value,
                text,
                carrier.clone(),
                pos as u32,
                containing_symbol_id.clone(),
            );
        }
    }
}

/// Derive an Elixir call's carrier from its `target`.
///
/// `identifier` target → bare function name. `dot` target → `Module.function`
/// (`left.right`) so qualified client APIs match config exactly
/// (`HTTPoison.get`) and module receivers still match a bare method config
/// (`query`) via the gate's last-segment rule (`Repo.query` → `query`).
fn elixir_carrier(base: &BaseExtractor, target: Node) -> Option<String> {
    match target.kind() {
        "identifier" => Some(base.get_node_text(&target)),
        "dot" => {
            let left = target
                .child_by_field_name("left")
                .map(|n| base.get_node_text(&n));
            let right = target
                .child_by_field_name("right")
                .map(|n| base.get_node_text(&n));
            match (left, right) {
                (Some(l), Some(r)) => Some(format!("{l}.{r}")),
                (None, Some(r)) => Some(r),
                _ => None,
            }
        }
        _ => {
            let text = base.get_node_text(&target);
            if text.is_empty() { None } else { Some(text) }
        }
    }
}

// ============================================================================
// Typespec type-argument capture (Miller bridge Phase 2)
// ============================================================================

/// Walk module attributes and record ordered/nested type-argument usages from
/// `@type` / `@typep` / `@opaque` / `@spec` / `@callback` typespec trees.
fn walk_tree_for_typespec_type_arguments(
    base: &mut BaseExtractor,
    node: Node,
    symbol_map: &HashMap<String, &Symbol>,
) {
    if node.kind() == "unary_operator" {
        extract_typespec_type_arguments_from_attribute(base, node, symbol_map);
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk_tree_for_typespec_type_arguments(base, child, symbol_map);
    }
}

fn extract_typespec_type_arguments_from_attribute(
    base: &mut BaseExtractor,
    attr_node: Node,
    symbol_map: &HashMap<String, &Symbol>,
) {
    let Some(operator) = attr_node.child_by_field_name("operator") else {
        return;
    };
    if base.get_node_text(&operator) != "@" {
        return;
    }
    let Some(operand) = attr_node.child_by_field_name("operand") else {
        return;
    };
    if operand.kind() != "call" {
        return;
    }
    let Some(target) = operand.child_by_field_name("target") else {
        return;
    };
    if target.kind() != "identifier" {
        return;
    }

    let Some(args) = find_child_by_type(&operand, "arguments") else {
        return;
    };

    match base.get_node_text(&target).as_str() {
        "type" | "typep" | "opaque" => walk_elixir_type_alias_typespec(base, args, symbol_map),
        "spec" | "callback" => walk_elixir_spec_typespec(base, args, symbol_map),
        _ => {}
    }
}

fn walk_elixir_type_alias_typespec(
    base: &mut BaseExtractor,
    node: Node,
    symbol_map: &HashMap<String, &Symbol>,
) {
    if let Some(body) = find_typespec_body_node(node) {
        if let Some(right) = body.child_by_field_name("right") {
            walk_elixir_typespec_type_expr(base, right, symbol_map);
        }
        return;
    }
    walk_elixir_typespec_type_expr(base, node, symbol_map);
}

fn walk_elixir_spec_typespec(
    base: &mut BaseExtractor,
    node: Node,
    symbol_map: &HashMap<String, &Symbol>,
) {
    if let Some(body) = find_typespec_body_node(node) {
        if let Some(left) = body.child_by_field_name("left") {
            walk_elixir_spec_function_head(base, left, symbol_map);
        }
        if let Some(right) = body.child_by_field_name("right") {
            walk_elixir_typespec_type_expr(base, right, symbol_map);
        }
        return;
    }
    walk_elixir_typespec_type_expr(base, node, symbol_map);
}

fn find_typespec_body_node<'a>(node: Node<'a>) -> Option<Node<'a>> {
    if node.kind() == "binary_operator" {
        return Some(node);
    }
    let mut cursor = node.walk();
    node.named_children(&mut cursor)
        .find(|child| child.kind() == "binary_operator")
}

/// Walk a `@spec` / `@callback` function head without recording the head call
/// itself; parameter types inside the head may still contain generic forms.
fn walk_elixir_spec_function_head(
    base: &mut BaseExtractor,
    node: Node,
    symbol_map: &HashMap<String, &Symbol>,
) {
    if node.kind() == "call" {
        if let Some(args) = find_child_by_type(&node, "arguments") {
            walk_elixir_typespec_type_expr_children(base, args, symbol_map);
        }
        return;
    }
    walk_elixir_typespec_type_expr(base, node, symbol_map);
}

fn walk_elixir_typespec_type_expr(
    base: &mut BaseExtractor,
    node: Node,
    symbol_map: &HashMap<String, &Symbol>,
) {
    if node.kind() == "call"
        && is_elixir_parameterized_type_call(&node)
        && !is_nested_in_type_application_args(&node)
    {
        record_elixir_type_arguments(base, node, symbol_map);
        return;
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk_elixir_typespec_type_expr(base, child, symbol_map);
    }
}

fn walk_elixir_typespec_type_expr_children(
    base: &mut BaseExtractor,
    node: Node,
    symbol_map: &HashMap<String, &Symbol>,
) {
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        walk_elixir_typespec_type_expr(base, child, symbol_map);
    }
}

fn is_nested_in_type_application_args(node: &Node) -> bool {
    let Some(args) = node.parent() else {
        return false;
    };
    if args.kind() != "arguments" {
        return false;
    }
    let Some(parent_call) = args.parent() else {
        return false;
    };
    parent_call.kind() == "call"
        && parent_call.id() != node.id()
        && is_elixir_parameterized_type_call(&parent_call)
}

fn record_elixir_type_arguments(
    base: &mut BaseExtractor,
    call_node: Node,
    symbol_map: &HashMap<String, &Symbol>,
) {
    let Some(target) = call_node.child_by_field_name("target") else {
        return;
    };
    if target.kind() != "identifier" {
        return;
    }
    let Some(args) = find_child_by_type(&call_node, "arguments") else {
        return;
    };

    let name = base.get_node_text(&target);
    let containing_symbol_id = find_containing_symbol_id(base, call_node, symbol_map);
    let identifier = base.create_identifier(
        &target,
        name,
        IdentifierKind::TypeUsage,
        containing_symbol_id,
    );
    let arguments = extract_type_arguments(base, args, decompose_elixir_type_arg);
    base.record_type_arguments(&identifier, arguments);
}

fn decompose_elixir_type_arg<'a>(
    base: &BaseExtractor,
    node: Node<'a>,
) -> Option<(String, Option<Node<'a>>)> {
    if !node.is_named() {
        return None;
    }
    match node.kind() {
        "call" => {
            let target = node.child_by_field_name("target")?;
            let name = base.get_node_text(&target);
            let nested = find_child_by_type(&node, "arguments");
            Some((name, nested))
        }
        _ => Some((base.get_node_text(&node), None)),
    }
}
