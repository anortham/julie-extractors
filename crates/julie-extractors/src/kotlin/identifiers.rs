//! Identifier and reference extraction for Kotlin
//!
//! This module handles extraction of function calls, member access, and other
//! identifier usages for LSP-quality find_references support.

use crate::base::{
    BaseExtractor, ContainingSymbolIndex, Identifier, IdentifierKind, Symbol,
    extract_type_arguments,
};
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};
use tree_sitter::Node;

/// Extract all identifier usages from a Kotlin file
pub(super) fn extract_identifiers(
    base: &mut BaseExtractor,
    tree: &tree_sitter::Tree,
    detached_annotation_nodes: &[Node],
    symbols: &[Symbol],
) -> Vec<Identifier> {
    let containing_symbols = base.containing_symbol_index(symbols);

    walk_tree_for_identifiers(base, tree.root_node(), &containing_symbols, 0);
    for node in detached_annotation_nodes {
        walk_tree_for_identifiers(base, *node, &containing_symbols, 0);
    }

    base.identifiers.clone()
}

/// Recursively walk tree extracting identifiers from each node
fn walk_tree_for_identifiers(
    base: &mut BaseExtractor,
    node: Node,
    containing_symbols: &ContainingSymbolIndex<'_>,
    depth: u32,
) {
    if !should_visit_tree_depth(depth) {
        return;
    }

    extract_identifier_from_node(base, node, containing_symbols);

    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk_tree_for_identifiers(base, child, containing_symbols, child_depth);
    }
}

/// Extract identifier from a single node based on its kind
fn extract_identifier_from_node(
    base: &mut BaseExtractor,
    node: Node,
    containing_symbols: &ContainingSymbolIndex<'_>,
) {
    match node.kind() {
        // Function/method calls: foo(), bar.baz(), mutableListOf<User>()
        "call_expression" => {
            // Collect children once so we can find both the callee and type_arguments
            // without keeping the cursor borrow alive across mutable base calls.
            let children: Vec<_> = {
                let mut cursor = node.walk();
                node.children(&mut cursor).collect()
            };

            let type_args_node = children
                .iter()
                .find(|c| c.kind() == "type_arguments")
                .copied();

            // Simple call: identifier or simple_identifier is the first callee child.
            if let Some(child) = children
                .iter()
                .find(|c| c.kind() == "identifier" || c.kind() == "simple_identifier")
            {
                let arguments = type_args_node
                    .map(|ta| extract_type_arguments(base, ta, decompose_kotlin_type_arg));
                let name = identifier_name(base, child);
                let containing = find_containing_symbol_id(node, containing_symbols);
                let receiver_type = self_receiver_type(base, node);
                let identifier = base.create_identifier_with_receiver_type(
                    child,
                    name,
                    IdentifierKind::Call,
                    containing,
                    receiver_type,
                );
                if let Some(args) = arguments
                    && !args.is_empty()
                {
                    base.record_type_arguments(&identifier, args);
                }
            } else if let Some(nav_expr) = children
                .iter()
                .find(|c| c.kind() == "navigation_expression")
            {
                // Member call: obj.foo<T>()
                let nav_name = extract_rightmost_identifier(base, nav_expr);
                let arguments = type_args_node
                    .map(|ta| extract_type_arguments(base, ta, decompose_kotlin_type_arg));
                let containing = find_containing_symbol_id(node, containing_symbols);
                let receiver_type = self_receiver_type(base, node);
                if let Some((name_node, name)) = nav_name {
                    let identifier = base.create_identifier_with_receiver_type(
                        &name_node,
                        name,
                        IdentifierKind::Call,
                        containing,
                        receiver_type,
                    );
                    if let Some(args) = arguments
                        && !args.is_empty()
                    {
                        base.record_type_arguments(&identifier, args);
                    }
                }
            }
            // Phase 3b: capture string-literal call-arguments (config-free;
            // carrier classification + gate run later in the artifact language-policy pass).
            record_kotlin_call_arg_literals(base, node, containing_symbols);
        }

        // Type references in type positions: val x: Foo, fun f(a: Foo): Bar,
        // class Foo(service: Bar), typealias A = Foo
        // Kotlin uses `user_type` for type annotations. It contains an
        // `identifier` child for the type name. Unlike Scala/Java,
        // class/interface/object declaration names use `identifier`
        // directly (not inside `user_type`), so we don't need to filter
        // declaration names here.
        "user_type" => {
            // A qualified type `Outer.Inner` names its last segment; the
            // leading segments ride along as receiver metadata.
            let segments: Vec<Node> = node
                .children(&mut node.walk())
                .filter(|n| n.kind() == "identifier" || n.kind() == "simple_identifier")
                .collect();

            if let Some((name_node, qualifiers)) = segments.split_last() {
                let name_node = *name_node;
                let name = base.get_node_text(&name_node);

                if is_kotlin_noise_type(&name) {
                    return;
                }

                let containing = find_containing_symbol_id(node, containing_symbols);
                let mut metadata = std::collections::HashMap::new();
                if let Some((receiver, outer)) = qualifiers.split_last() {
                    metadata.insert(
                        "receiver".to_string(),
                        serde_json::Value::String(base.get_node_text(receiver)),
                    );
                    if !outer.is_empty() {
                        let qualifier: Vec<String> =
                            outer.iter().map(|n| base.get_node_text(n)).collect();
                        metadata.insert(
                            "receiver_qualifier".to_string(),
                            serde_json::Value::String(qualifier.join(".")),
                        );
                    }
                }
                let identifier = if metadata.is_empty() {
                    base.create_identifier(&name_node, name, IdentifierKind::TypeUsage, containing)
                } else {
                    base.create_identifier_with_metadata(
                        &name_node,
                        name,
                        IdentifierKind::TypeUsage,
                        containing,
                        metadata,
                    )
                };
                // If this user_type is the outermost generic use site (not nested
                // inside another type_arguments list), record its ordered type args.
                record_outermost_kotlin_type_arguments(base, node, &identifier);
            }
        }

        // Member access: object.property; `Type::member` references a function.
        "navigation_expression" => {
            // Only extract if it's NOT part of a call_expression
            if let Some(parent) = node.parent()
                && parent.kind() == "call_expression"
            {
                return;
            }

            // `Foo::class` is a class literal: a type usage of `Foo`.
            if let Some(type_name) = class_literal_type(base, node) {
                let name = identifier_name(base, &type_name);
                let containing_symbol_id = find_containing_symbol_id(node, containing_symbols);
                base.create_identifier(
                    &type_name,
                    name,
                    IdentifierKind::TypeUsage,
                    containing_symbol_id,
                );
                return;
            }

            // Extract the rightmost identifier (the member name)
            if let Some((name_node, name)) = extract_rightmost_identifier(base, &node) {
                let containing_symbol_id = find_containing_symbol_id(node, containing_symbols);
                let is_function_reference = name != "class"
                    && node
                        .children(&mut node.walk())
                        .any(|child| child.kind() == "::");
                let kind = if is_function_reference {
                    IdentifierKind::Call
                } else {
                    IdentifierKind::MemberAccess
                };

                base.create_identifier(&name_node, name, kind, containing_symbol_id);
            }
        }

        // `@Query("select …")`: the annotation's value string is a literal
        // whose carrier is the annotation name; the carrier gate keeps only
        // configured carriers.
        "annotation" => record_kotlin_annotation_value_literal(base, node, containing_symbols),

        // Infix call: `a plusTax 20` calls `plusTax`.
        "infix_expression" => {
            if let Some(operator) = node.child(1).filter(|child| child.kind() == "identifier") {
                let name = identifier_name(base, &operator);
                let containing = find_containing_symbol_id(node, containing_symbols);
                base.create_identifier(&operator, name, IdentifierKind::Call, containing);
            }
        }

        // Function reference: `::isValid`.
        "callable_reference" => {
            let member = node
                .children(&mut node.walk())
                .filter(|child| child.kind() == "identifier")
                .last();
            if let Some(member) = member {
                let name = identifier_name(base, &member);
                if name != "class" {
                    let containing = find_containing_symbol_id(node, containing_symbols);
                    base.create_identifier(&member, name, IdentifierKind::Call, containing);
                }
            }
        }

        // `variable_ref` complement arm (locked contract — see the reference
        // implementation doc comment in csharp/identifiers.rs): a bare `identifier`
        // used as a value or as the object/receiver of a navigation — the reads the
        // Call/MemberAccess/TypeUsage arms above do not own. Kotlin type positions
        // live inside `user_type`, which the predicate excludes.
        "identifier" if is_kotlin_value_read_identifier(base, node) => {
            let name = identifier_name(base, &node);
            // Rule 5: reuse the existing noise filter, plus the `it`/`field`
            // soft keywords (implicit lambda parameter / property backing
            // field), and the `null`/`true`/`false` literals, which all parse
            // as plain identifiers in kotlin-ng.
            if !is_kotlin_noise_type(&name)
                && !matches!(name.as_str(), "it" | "field" | "null" | "true" | "false")
            {
                let containing = find_containing_symbol_id(node, containing_symbols);
                base.create_identifier(&node, name, IdentifierKind::VariableRef, containing);
            }
        }

        _ => {
            // Skip other node types
        }
    }
}

/// Rule 1/4 predicate for the `variable_ref` arm: is this bare `identifier` a
/// value read or a navigation receiver (the complement of the Call/MemberAccess/
/// TypeUsage arms)? Node kinds and field names were verified empirically against
/// the vendored tree-sitter-kotlin-ng 1.1.0 grammar (which differs from older
/// kotlin grammars — e.g. simple `$name` string interpolation is lexed as
/// `string_content` and never reaches identifier extraction).
fn is_kotlin_value_read_identifier(base: &BaseExtractor, node: Node) -> bool {
    let Some(parent) = node.parent() else {
        return false;
    };
    let is_field = |name: &str| parent.child_by_field_name(name).map(|n| n.id()) == Some(node.id());

    match parent.kind() {
        // Rule 2: type positions are owned by the `user_type` (TypeUsage) arm.
        "user_type" => false,

        // Rule 2: the direct identifier child of a call is the callee (Call arm).
        "call_expression" => false,

        // Rule 1/2: only the leading receiver of a navigation is a read this arm
        // owns; the member name is owned by the MemberAccess/Call arms. The
        // type of a `Foo::class` literal is owned by the navigation arm.
        "navigation_expression" => {
            parent.child(0).map(|c| c.id()) == Some(node.id()) && !is_class_literal(base, parent)
        }

        // Rule 3: declaration names. `variable_declaration` also covers lambda
        // parameters and `for` loop variables in kotlin-ng.
        "variable_declaration"
        | "parameter"
        | "class_parameter"
        | "type_parameter"
        | "enum_entry"
        | "type_alias"
        | "function_declaration"
        | "class_declaration"
        | "object_declaration"
        | "companion_object"
        | "catch_block" => false,

        // Rule 3: package/import segments, aliases, and labels are not reads.
        "qualified_identifier"
        | "package_header"
        | "import"
        | "import_alias"
        | "import_list"
        | "label"
        | "labeled_expression" => false,

        // Rule 1: a named-argument LABEL (`bar` in `f(bar = seed)`) names a
        // PARAMETER, not a member — skip it; the argument VALUE is a read.
        "value_argument" => !is_kotlin_named_argument_label(parent, node),

        // Rule 2-adjacent: the middle identifier of an infix expression
        // (`until` in `0 until count`) is the infix FUNCTION, a callee — not a
        // value read. The operands (children 0 and 2) are reads.
        "infix_expression" => parent.child(1).map(|c| c.id()) != Some(node.id()),

        // The referenced function of `::name` is owned by the Call arm.
        "callable_reference" => false,

        // Rule 4: the LHS of a PLAIN assignment is write-only; a COMPOUND
        // operator (`+=`, …) reads. The RHS is always a read.
        "assignment" => {
            !is_field("left")
                || parent
                    .child_by_field_name("operator")
                    .map(|op| op.kind() != "=")
                    .unwrap_or(false)
        }

        // Every other position — argument value, operand, return value, `if`/
        // `when` branch, `${...}` interpolation, `for` collection, when-entry
        // constant — is a read.
        _ => true,
    }
}

/// `Foo::class`: a navigation of `identifier`, `::`, and the `class` keyword.
fn is_class_literal(base: &BaseExtractor, node: Node) -> bool {
    let mut cursor = node.walk();
    let children: Vec<Node> = node.children(&mut cursor).collect();
    matches!(
        children.as_slice(),
        [receiver, separator, member]
            if receiver.kind() == "identifier"
                && separator.kind() == "::"
                && member.kind() == "identifier"
                && base.get_node_text(member) == "class"
    )
}

/// The type name of a `Foo::class` literal.
fn class_literal_type<'a>(base: &BaseExtractor, node: Node<'a>) -> Option<Node<'a>> {
    is_class_literal(base, node)
        .then(|| node.child(0))
        .flatten()
}

/// Is `node` the label of a named argument (`bar` in `f(bar = seed)`)? The
/// kotlin-ng grammar has no field for it: a labeled `value_argument` holds
/// `[identifier, "=", expression]`, so the label is the first child when an
/// anonymous `=` token is present.
fn is_kotlin_named_argument_label(value_argument: Node, node: Node) -> bool {
    if value_argument.child(0).map(|c| c.id()) != Some(node.id()) {
        return false;
    }
    let mut cursor = value_argument.walk();
    value_argument
        .children(&mut cursor)
        .any(|c| !c.is_named() && c.kind() == "=")
}

/// Record outermost generic type arguments for a `user_type` node.
///
/// Fires when the `user_type` is an outermost generic use site (e.g. `List` in
/// `List<User>`), but not when it is nested inside another generic's
/// `type_arguments` (where it rides along as a `child`).
fn record_outermost_kotlin_type_arguments(
    base: &mut BaseExtractor,
    user_type_node: Node,
    identifier: &Identifier,
) {
    // Skip if this user_type is nested inside another type_arguments.
    if is_kotlin_user_type_nested(user_type_node) {
        return;
    }
    // Find the type_arguments child (e.g. `<User>` in `List<User>`).
    let children: Vec<_> = {
        let mut cursor = user_type_node.walk();
        user_type_node.children(&mut cursor).collect()
    };
    let Some(arg_list) = children.into_iter().find(|c| c.kind() == "type_arguments") else {
        return;
    };
    let arguments = extract_type_arguments(base, arg_list, decompose_kotlin_type_arg);
    base.record_type_arguments(identifier, arguments);
}

/// Returns true if `user_type` is nested inside a `type_projection` (i.e., it
/// is a type argument of some outer generic, not an outermost use site).
///
/// In Kotlin the nesting path is:
/// `outer_user_type > type_arguments > type_projection > [nullable_type >]* user_type`
fn is_kotlin_user_type_nested(user_type: Node) -> bool {
    let mut current = user_type;
    loop {
        let Some(parent) = current.parent() else {
            return false;
        };
        match parent.kind() {
            "type_projection" => return true,
            // Transparent type wrappers — keep climbing.
            "nullable_type" | "parenthesized_type" | "non_nullable_type" => {
                current = parent;
            }
            _ => return false,
        }
    }
}

/// Decompose a single child of a Kotlin `type_arguments` node.
///
/// Kotlin always wraps each argument in `type_projection`:
/// `type_arguments { type_projection { [variance_modifier,] type } ... }`
///
/// Returns `(type_name, Option<nested_type_arguments_node>)`.
fn decompose_kotlin_type_arg<'a>(
    base: &BaseExtractor,
    node: Node<'a>,
) -> Option<(String, Option<Node<'a>>)> {
    if !node.is_named() {
        return None; // skip commas and angle brackets
    }
    if node.kind() != "type_projection" {
        return None;
    }
    // Find the actual type node inside type_projection (skip variance_modifier).
    let type_node = {
        let children: Vec<Node<'a>> = {
            let mut cursor = node.walk();
            node.children(&mut cursor).collect()
        };
        children
            .into_iter()
            .find(|c| c.is_named() && c.kind() != "variance_modifier")
    };
    let Some(type_node) = type_node else {
        return Some(("*".to_string(), None)); // star projection
    };
    extract_kotlin_type_node_info(base, type_node)
}

/// Recursively extract `(type_name, nested_type_arguments)` from a type node.
fn extract_kotlin_type_node_info<'a>(
    base: &BaseExtractor,
    node: Node<'a>,
) -> Option<(String, Option<Node<'a>>)> {
    extract_kotlin_type_node_info_at_depth(base, node, 0)
}

fn extract_kotlin_type_node_info_at_depth<'a>(
    base: &BaseExtractor,
    node: Node<'a>,
    depth: u32,
) -> Option<(String, Option<Node<'a>>)> {
    if !should_visit_tree_depth(depth) {
        return None;
    }

    match node.kind() {
        "user_type" => {
            let children: Vec<Node<'a>> = {
                let mut cursor = node.walk();
                node.children(&mut cursor).collect()
            };
            let name = children
                .iter()
                .find(|c| c.kind() == "identifier" || c.kind() == "simple_identifier")
                .map(|n| base.get_node_text(n))
                .unwrap_or_else(|| base.get_node_text(&node));
            let nested = children.into_iter().find(|c| c.kind() == "type_arguments");
            Some((name, nested))
        }
        "nullable_type" => {
            // `Foo?` — unwrap the inner type and append "?".
            let mut cursor = node.walk();
            let inner = node.named_children(&mut cursor).next();
            if let Some(inner) = inner {
                let child_depth = child_tree_depth(depth)?;
                extract_kotlin_type_node_info_at_depth(base, inner, child_depth)
                    .map(|(name, nested)| (format!("{}?", name), nested))
            } else {
                Some((base.get_node_text(&node), None))
            }
        }
        _ => Some((base.get_node_text(&node), None)),
    }
}

/// Find the ID of the symbol that contains this node
fn find_containing_symbol_id(
    node: Node,
    containing_symbols: &ContainingSymbolIndex<'_>,
) -> Option<String> {
    containing_symbols.find(node).map(|s| s.id.clone())
}

// ============================================================================
// String-literal call-argument capture
// ============================================================================

/// Capture string-literal arguments of a Kotlin `call_expression` as `Literal`
/// records.
///
/// Config-free: `carrier` is the verbatim callee text; the URL/SQL
/// classification and the carrier gate run later in the artifact language-policy pass.
/// Kotlin call args live in a `value_arguments` child holding `value_argument`
/// nodes; a named argument (`url = "..."`) carries an extra `identifier` name,
/// so the value is the argument's last named child. `arg_position` is counted
/// over the full argument list. Kotlin string templates (`"$x"` / `"${x}"`)
/// decode to `{}` holes via the shared `interpolation`-aware decoder.
fn record_kotlin_call_arg_literals(
    base: &mut BaseExtractor,
    call_node: Node,
    containing_symbols: &ContainingSymbolIndex<'_>,
) {
    let children: Vec<Node> = {
        let mut cursor = call_node.walk();
        call_node.children(&mut cursor).collect()
    };
    let Some(value_args) = children
        .iter()
        .find(|c| c.kind() == "value_arguments")
        .copied()
    else {
        return;
    };
    let callee = children
        .iter()
        .find(|c| {
            matches!(
                c.kind(),
                "identifier" | "simple_identifier" | "navigation_expression"
            )
        })
        .copied();
    let carrier = callee.and_then(|c| kotlin_carrier(base, c));
    let containing_symbol_id = find_containing_symbol_id(call_node, containing_symbols);

    let args: Vec<Node> = {
        let mut cursor = value_args.walk();
        value_args.named_children(&mut cursor).collect()
    };
    for (pos, arg) in args.into_iter().enumerate() {
        if let Some(value) = kotlin_argument_value(arg)
            && let Some(text) = base.decode_string_literal(&value)
        {
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

fn record_kotlin_annotation_value_literal(
    base: &mut BaseExtractor,
    annotation: Node,
    containing_symbols: &ContainingSymbolIndex<'_>,
) {
    let Some(invocation) = annotation
        .children(&mut annotation.walk())
        .find(|child| child.kind() == "constructor_invocation")
    else {
        return;
    };
    let carrier = invocation
        .children(&mut invocation.walk())
        .find(|child| child.kind() == "user_type")
        .and_then(|user_type| {
            user_type
                .children(&mut user_type.walk())
                .filter(|child| child.kind() == "identifier")
                .last()
        })
        .map(|name| base.get_node_text(&name));
    let Some(arguments) = invocation
        .children(&mut invocation.walk())
        .find(|child| child.kind() == "value_arguments")
    else {
        return;
    };
    let value = arguments
        .named_children(&mut arguments.walk())
        .filter(|argument| argument.kind() == "value_argument")
        .find_map(|argument| {
            if !is_named_argument(argument) {
                return kotlin_argument_value(argument);
            }
            let label = argument.named_child(0)?;
            (base.get_node_text(&label) == "value")
                .then(|| kotlin_argument_value(argument))
                .flatten()
        });
    let Some(value) = value else {
        return;
    };
    let Some(text) = base.decode_string_literal(&value) else {
        return;
    };
    let containing_symbol_id = find_containing_symbol_id(annotation, containing_symbols);
    base.record_literal(&value, text, carrier, 0, containing_symbol_id);
}

fn is_named_argument(argument: Node) -> bool {
    argument
        .children(&mut argument.walk())
        .any(|child| !child.is_named() && child.kind() == "=")
}

/// The value expression of a Kotlin `value_argument`. A named argument
/// (`name = expr`) has the name as a leading `identifier`, so the value is the
/// last named child; a positional argument's single named child is the value.
fn kotlin_argument_value(arg: Node) -> Option<Node> {
    if arg.kind() != "value_argument" {
        return Some(arg);
    }
    let mut cursor = arg.walk();
    arg.named_children(&mut cursor).last()
}

/// Derive a Kotlin call's carrier from its callee.
///
/// Plain `identifier`/`simple_identifier` → its text (`fetch`). A
/// `navigation_expression` (`db.execute`, `client.get`) → the `receiver.member`
/// join so dotted client APIs match config (`client.get`) while bare DB verbs
/// (`execute`/`query`) match any receiver via the gate's last-segment rule.
fn kotlin_carrier(base: &BaseExtractor, callee: Node) -> Option<String> {
    match callee.kind() {
        "identifier" | "simple_identifier" => Some(base.get_node_text(&callee)),
        "navigation_expression" => {
            let named: Vec<Node> = {
                let mut cursor = callee.walk();
                callee.named_children(&mut cursor).collect()
            };
            let receiver = named.first().map(|n| base.get_node_text(n));
            let member = named.last().map(|n| base.get_node_text(n));
            match (receiver, member) {
                (Some(r), Some(m)) if named.len() >= 2 => Some(format!("{r}.{m}")),
                (_, Some(m)) => Some(m),
                _ => None,
            }
        }
        _ => {
            let text = base.get_node_text(&callee);
            if text.is_empty() { None } else { Some(text) }
        }
    }
}

/// Returns true for Kotlin types that are too common to be meaningful
/// type references for centrality scoring.
///
/// Includes:
/// - Single-letter type params (T, K, V, E, R) — generic type parameters
/// - Kotlin/JVM primitive and base types — ubiquitous in every file
fn is_kotlin_noise_type(name: &str) -> bool {
    // Single-letter uppercase names are almost always generic type parameters.
    if name.len() == 1 && name.chars().next().is_some_and(|c| c.is_ascii_uppercase()) {
        return true;
    }

    matches!(
        name,
        // Kotlin primitive types
        "Int"
            | "Long"
            | "Short"
            | "Byte"
            | "Float"
            | "Double"
            | "Char"
            | "Boolean"
            | "Unit"
            // Kotlin top types
            | "Any"
            | "Nothing"
            // JVM interop
            | "String"
            | "Object"
    )
}

/// Helper to extract the rightmost identifier in a navigation_expression
fn extract_rightmost_identifier<'a>(
    base: &BaseExtractor,
    node: &Node<'a>,
) -> Option<(Node<'a>, String)> {
    // Kotlin navigation_expression structure
    // For chained access like user.account.balance:
    // - We need to find the rightmost identifier

    // First, try to find identifier children (rightmost in chain)
    let identifiers: Vec<Node> = node
        .children(&mut node.walk())
        .filter(|n| n.kind() == "identifier" || n.kind() == "simple_identifier")
        .collect();

    if let Some(last_identifier) = identifiers.last() {
        let name = identifier_name(base, last_identifier);
        return Some((*last_identifier, name));
    }

    None
}

/// The reference name of a Kotlin identifier node, without escaping backticks.
///
/// A call site spells a backticked declaration the same way the declaration
/// does, so both sides must strip for the names to match.
fn identifier_name(base: &BaseExtractor, node: &Node) -> String {
    super::helpers::strip_backticks(&base.get_node_text(node)).to_string()
}

pub(super) fn self_receiver_type(base: &BaseExtractor, node: Node) -> Option<String> {
    self_receiver_type_at(base, node)
}

fn self_receiver_type_at(base: &BaseExtractor, node: Node) -> Option<String> {
    let nav = {
        let mut cursor = node.walk();
        node.children(&mut cursor)
            .find(|child| child.kind() == "navigation_expression")
    }?;
    let receiver = {
        let mut cursor = nav.walk();
        nav.named_children(&mut cursor).next()
    }?;
    match receiver.kind() {
        "this_expression" => enclosing_type_name(base, node),
        "super_expression" => declared_superclass_name(base, node),
        _ => None,
    }
}

/// The type `this` names at `node`: the receiver type of the nearest enclosing
/// extension function or property, else the nearest enclosing type.
fn enclosing_type_name(base: &BaseExtractor, node: Node) -> Option<String> {
    let mut current = node.parent();
    while let Some(candidate) = current {
        match candidate.kind() {
            "function_declaration" | "property_declaration" => {
                if let Some(receiver) = super::helpers::extract_receiver_type(base, &candidate) {
                    let base_name = receiver
                        .split('<')
                        .next()
                        .unwrap_or(&receiver)
                        .trim()
                        .trim_end_matches('?')
                        .to_string();
                    return Some(base_name);
                }
            }
            "class_declaration" | "object_declaration" | "companion_object" => {
                return super::helpers::declared_name(base, &candidate).map(|(name, _)| name);
            }
            _ => {}
        }
        current = candidate.parent();
    }
    None
}

fn declared_superclass_name(base: &BaseExtractor, node: Node) -> Option<String> {
    let mut current = node.parent();
    while let Some(candidate) = current {
        if matches!(candidate.kind(), "class_declaration" | "object_declaration") {
            let name = super::helpers::collect_base_type_names(base, &candidate)
                .into_iter()
                .next()?;
            let base_name = match name.split_once('<') {
                Some((head, _)) => head.trim().to_string(),
                None => name.trim().to_string(),
            };
            return Some(base_name);
        }
        current = candidate.parent();
    }
    None
}
