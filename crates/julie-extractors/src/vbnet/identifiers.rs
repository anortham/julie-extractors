use crate::base::{
    BaseExtractor, ContainingSymbolIndex, Identifier, IdentifierKind, Symbol,
    extract_type_arguments,
};
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};
use tree_sitter::{Node, Tree};

pub fn extract_identifiers(
    base: &mut BaseExtractor,
    tree: &Tree,
    symbols: &[Symbol],
) -> Vec<Identifier> {
    let containing_symbols = base.containing_symbol_index(symbols);
    walk_tree_for_identifiers(base, tree.root_node(), &containing_symbols, 0);
    base.identifiers.clone()
}

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

fn extract_identifier_from_node(
    base: &mut BaseExtractor,
    node: Node,
    containing_symbols: &ContainingSymbolIndex<'_>,
) {
    match node.kind() {
        "invocation_expression" | "invocation" => {
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                if child.kind() == "identifier" {
                    let name = base.get_node_text(&child);
                    let containing_symbol_id = find_containing_symbol_id(node, containing_symbols);
                    base.create_identifier(
                        &child,
                        name,
                        IdentifierKind::Call,
                        containing_symbol_id,
                    );
                    break;
                } else if child.kind() == "member_access_expression"
                    || child.kind() == "member_access"
                {
                    let name_node = child.child_by_field_name("member").or_else(|| {
                        let mut mc = child.walk();
                        let children: Vec<_> = child.children(&mut mc).collect();
                        children
                            .into_iter()
                            .rev()
                            .find(|c| c.kind() == "identifier")
                    });
                    if let Some(name_node) = name_node {
                        let name = base.get_node_text(&name_node);
                        let containing_symbol_id =
                            find_containing_symbol_id(node, containing_symbols);
                        let receiver_type = self_receiver_type(base, child);
                        base.create_identifier_with_receiver_type(
                            &name_node,
                            name,
                            IdentifierKind::Call,
                            containing_symbol_id,
                            receiver_type,
                        );
                    }
                    break;
                }
            }
            // Phase 3: capture string-literal call-arguments (config-free; the
            // carrier classification + gate happen in the artifact language-policy pass).
            record_vbnet_call_arg_literals(base, node, containing_symbols);
        }
        "member_access_expression" | "member_access" => {
            if let Some(parent) = node.parent()
                && (parent.kind() == "invocation_expression" || parent.kind() == "invocation")
            {
                return;
            }

            let mut cursor = node.walk();
            let children: Vec<_> = node.children(&mut cursor).collect();
            if let Some(name_node) = children.iter().rev().find(|c| c.kind() == "identifier") {
                let name = base.get_node_text(name_node);
                let containing_symbol_id = find_containing_symbol_id(node, containing_symbols);
                base.create_identifier(
                    name_node,
                    name,
                    IdentifierKind::MemberAccess,
                    containing_symbol_id,
                );
            }
        }

        // VB.NET generic type use site: `List(Of String)`, `Dictionary(Of String, Integer)`
        // Grammar: generic_type → namespace_name (base name) + type_argument_list (args)
        "generic_type" => {
            // Outermost-only rule: skip if this generic_type is a nested arg of another generic.
            if node
                .parent()
                .map(|p| p.kind() == "type_argument_list")
                .unwrap_or(false)
            {
                return;
            }
            let children: Vec<_> = {
                let mut cursor = node.walk();
                node.children(&mut cursor).collect()
            };
            let Some(name_node) = children.iter().find(|c| c.kind() == "namespace_name") else {
                return;
            };
            let name = base.get_node_text(name_node);
            let containing_symbol_id = find_containing_symbol_id(node, containing_symbols);
            let identifier = base.create_identifier(
                name_node,
                name,
                IdentifierKind::TypeUsage,
                containing_symbol_id,
            );
            if let Some(arg_list) = children.iter().find(|c| c.kind() == "type_argument_list") {
                let arguments = extract_type_arguments(base, *arg_list, decompose_vbnet_type_arg);
                base.record_type_arguments(&identifier, arguments);
            }
        }

        // `variable_ref` complement arm (locked contract in csharp/identifiers.rs):
        // a bare `identifier` used as a value or as the object/receiver of a member
        // access — the reads the Call/MemberAccess/TypeUsage arms above do not own.
        // The other arms match on PARENT node kinds (`invocation`, `member_access`,
        // `generic_type`), so this arm fires once per identifier node and the
        // predicate excludes every position those arms own (no duplicate rows).
        // VB keywords (`Me`, `MyBase`, `True`, `Nothing`) and primitive types
        // (`Integer`, `Object`) are distinct grammar node kinds — never `identifier`
        // — so rule 5 is satisfied structurally (verified against
        // tree-sitter-vb-dotnet rev 25dca4a).
        "identifier" if is_vbnet_value_read_identifier(node) => {
            let name = base.get_node_text(&node);
            let containing_symbol_id = find_containing_symbol_id(node, containing_symbols);
            base.create_identifier(
                &node,
                name,
                IdentifierKind::VariableRef,
                containing_symbol_id,
            );
        }

        _ => {}
    }
}

/// Rule 1/4 predicate: is this bare `identifier` a value read or a member-access
/// receiver (the complement of the Call/MemberAccess/TypeUsage arms)? Mirrors
/// `is_csharp_value_read_identifier`; node kinds and field names were verified
/// empirically against tree-sitter-vb-dotnet rev 25dca4a (probe evidence in the
/// Task 6 report): declaration names are `name` fields, plain assignment parses
/// as BOTH `call_statement > binary_expression(left, "=", right)` (statement
/// level) and `assignment_statement > left_hand_side` (nested level), and
/// compound assignment reuses `left_hand_side` under
/// `compound_assignment_statement`.
fn is_vbnet_value_read_identifier(node: Node) -> bool {
    let Some(parent) = node.parent() else {
        return false;
    };

    let is_name_field = parent.child_by_field_name("name").map(|n| n.id()) == Some(node.id());

    match parent.kind() {
        // Rule 2: the bare callee (`target`) of a call is owned by the Call arm.
        "invocation" | "invocation_expression" => false,
        // Rule 1/2: only the `object` receiver of a member access is a read; the
        // accessed `member` is owned by the MemberAccess/Call arms.
        "member_access" | "member_access_expression" => {
            parent.child_by_field_name("object").map(|o| o.id()) == Some(node.id())
        }
        // `.Member` inside a With block: a member name, never a bare variable read
        // (mirrors the C# member_binding_expression exclusion).
        "implicit_member_access" => false,

        // Rule 2/3: namespace/type name positions — imports, `As` clauses, `New T`,
        // generic bases and type arguments, qualified names, inheritance clauses.
        "namespace_name" | "qualified_name" | "generic_type" | "type_argument_list" => false,

        // Rule 3: declaration names. Their NON-name identifier children (an
        // initializer value, an enum member's value expression) fall through as
        // reads via `!is_name_field`.
        "class_block"
        | "module_block"
        | "structure_block"
        | "interface_block"
        | "enum_block"
        | "method_declaration"
        | "abstract_method_declaration"
        | "constructor_declaration"
        | "operator_declaration"
        | "property_declaration"
        | "event_declaration"
        | "delegate_declaration"
        | "declare_statement"
        | "variable_declarator"
        | "dim_statement"
        | "const_declaration"
        | "enum_member"
        | "parameter"
        | "lambda_parameter"
        | "type_parameter" => !is_name_field,

        // Rule 3: the `variable` of For/For Each is a definition; `collection`,
        // `start`, and `end` positions read.
        "for_each_statement" | "for_statement" => {
            parent.child_by_field_name("variable").map(|v| v.id()) != Some(node.id())
        }

        // Rule 3: labels and GoTo targets are not variables.
        "label_statement" | "goto_statement" => false,

        // Rule 1: an argument value is a read; the `name` of a named argument
        // (`Foo(bar:=5)`) is a parameter label — EXCEPT in an attribute, where the
        // named argument is a member reference (`<Foo(Baz:=1)>` reads Baz).
        "argument" => {
            if !is_name_field {
                return true;
            }
            parent
                .parent() // argument_list
                .and_then(|list| list.parent())
                .map(|owner| owner.kind() == "attribute")
                .unwrap_or(false)
        }
        // Rule 2: the attribute's own name is a type usage, not a value read.
        "attribute" => false,

        // Rule 1: a With-initializer member LHS is a read (`New Foo With {.Bar = 5}`
        // reads Bar); its `value` side reads too.
        "member_initializer" | "object_initializer" => true,

        // Rule 4: statement-level plain assignment parses as
        // `call_statement > binary_expression` with operator `=`; its LHS is
        // write-only. Every other binary_expression position (comparisons,
        // arithmetic, `If x = y` equality) is a read.
        "binary_expression" => {
            let is_left = parent.child_by_field_name("left").map(|n| n.id()) == Some(node.id());
            if !is_left {
                return true;
            }
            let is_statement_assignment = parent
                .parent()
                .map(|g| g.kind() == "call_statement")
                .unwrap_or(false);
            let is_plain_eq = parent
                .child_by_field_name("operator")
                .map(|op| op.kind() == "=")
                .unwrap_or(false);
            !(is_statement_assignment && is_plain_eq)
        }
        // Rule 4: `left_hand_side` is shared by plain `assignment_statement`
        // (write-only) and `compound_assignment_statement` (a read).
        "left_hand_side" => parent
            .parent()
            .map(|g| g.kind() == "compound_assignment_statement")
            .unwrap_or(false),

        // Every other expression/statement value slot — return / condition /
        // argument element / RaiseEvent / interpolation / unary operand / … — is
        // a read.
        _ => true,
    }
}

/// `TypeArgDecomposer` for VB.NET: maps a named child of `type_argument_list` to its
/// applied argument. Nested `generic_type` children recurse; everything else is a leaf.
fn decompose_vbnet_type_arg<'a>(
    base: &BaseExtractor,
    node: Node<'a>,
) -> Option<(String, Option<Node<'a>>)> {
    if !node.is_named() {
        return None; // skip punctuation (commas, "Of" keyword, parens)
    }
    match node.kind() {
        "generic_type" => {
            // Nested generic: e.g. `List(Of User)` inside `Dictionary(Of String, List(Of User))`.
            let children: Vec<_> = {
                let mut cursor = node.walk();
                node.children(&mut cursor).collect()
            };
            let name_node = children.iter().find(|c| c.kind() == "namespace_name")?;
            let name = base.get_node_text(name_node);
            let nested = children
                .into_iter()
                .find(|c| c.kind() == "type_argument_list");
            Some((name, nested))
        }
        _ => {
            // Leaf: namespace_name ("String", "User"), primitive_type ("Integer"), etc.
            Some((base.get_node_text(&node), None))
        }
    }
}

fn find_containing_symbol_id(
    node: Node,
    containing_symbols: &ContainingSymbolIndex<'_>,
) -> Option<String> {
    containing_symbols.find(node).map(|s| s.id.clone())
}

// ============================================================================
// String-literal call-argument capture (Miller bridge Phase 3)
// ============================================================================

/// Capture string-literal arguments of a VB.NET `invocation` as `Literal`
/// records. Config-free: `carrier` is the invoked method name (mirrors the C#
/// leg); the URL/SQL classification and the carrier gate run later in the
/// artifact language-policy pass. VB wraps each call argument in an `argument`
/// node, so the value expression is the argument's last named child (after any
/// `name:=` for a named argument). `arg_position` is counted over the full
/// argument list.
fn record_vbnet_call_arg_literals(
    base: &mut BaseExtractor,
    node: Node,
    containing_symbols: &ContainingSymbolIndex<'_>,
) {
    let Some(target) = node.child_by_field_name("target") else {
        return;
    };
    let Some(args) = node.child_by_field_name("arguments") else {
        return;
    };
    let carrier = vbnet_carrier(base, target);
    let containing_symbol_id = find_containing_symbol_id(node, containing_symbols);

    let mut cursor = args.walk();
    for (pos, arg) in args.named_children(&mut cursor).enumerate() {
        let value = if arg.kind() == "argument" {
            let mut vc = arg.walk();
            arg.named_children(&mut vc).last()
        } else {
            Some(arg)
        };
        if let Some(value) = value
            && let Some(text) = decode_vbnet_literal(base, &value)
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

/// Derive a VB.NET call's carrier: the invoked method name (generics, if any,
/// stripped). `target` is an `identifier` (bare call) or a `member_access`
/// whose `member` field is the method name. The receiver is dropped — .NET
/// HTTP/DB carriers are matched by bare method name via the gate's last-segment
/// rule (`conn.Execute` -> `execute`), and the receiver is usually a local var.
fn vbnet_carrier(base: &BaseExtractor, target: Node) -> Option<String> {
    let text = match target.kind() {
        "identifier" => base.get_node_text(&target),
        "member_access" | "member_access_expression" => target
            .child_by_field_name("member")
            .or_else(|| target.child_by_field_name("name"))
            .map(|n| base.get_node_text(&n))?,
        _ => base.get_node_text(&target),
    };
    let stripped = match text.find('<') {
        Some(i) => text[..i].to_string(),
        None => text,
    };
    if stripped.is_empty() {
        None
    } else {
        Some(stripped)
    }
}

/// Decode a VB.NET call-argument string for capture.
///
/// Plain strings (`"..."`) are a single flat `string_literal` token, handled by
/// the shared `decode_string_literal` delimiter-strip fallback. Interpolated
/// strings appear as an `interpolated_string_literal` (either directly as the
/// argument value or wrapped in a `string_literal` choice node). Their static
/// text segments are **anonymous** tokens the base decoder's named-children walk
/// cannot see, so they are decoded here to the shared `{}`-hole convention
/// (`$"u/{id}"` -> `u/{}`), with escaped `""`/`{{`/`}}` resolved.
fn decode_vbnet_literal(base: &BaseExtractor, value: &Node) -> Option<String> {
    let interp = if value.kind() == "interpolated_string_literal" {
        Some(*value)
    } else if value.kind() == "string_literal" {
        let mut cursor = value.walk();
        value
            .named_children(&mut cursor)
            .find(|n| n.kind() == "interpolated_string_literal")
    } else {
        None
    };
    if let Some(interp) = interp {
        return Some(decode_vbnet_interpolated(base, &interp));
    }
    base.decode_string_literal(value)
}

/// Decode a VB.NET `interpolated_string_literal` to the `{}`-hole convention by
/// reconstructing from source: the gaps between `interpolation` children are
/// filled verbatim from the file bytes and each interpolation becomes `{}`. This
/// is robust whether or not the grammar exposes the (anonymous) static text
/// segments as child nodes. The `$"` opener / `"` closer are then stripped and
/// `""`/`{{`/`}}` escapes resolved.
fn decode_vbnet_interpolated(base: &BaseExtractor, interp: &Node) -> String {
    let bytes = base.content.as_bytes();
    let total_end = interp.end_byte().min(bytes.len());
    let mut out = String::new();
    let mut pos = interp.start_byte().min(total_end);
    let mut cursor = interp.walk();
    for child in interp.named_children(&mut cursor) {
        if child.kind() != "interpolation" {
            continue;
        }
        let cs = child.start_byte().min(total_end);
        if cs > pos {
            out.push_str(&String::from_utf8_lossy(&bytes[pos..cs]));
        }
        out.push_str("{}");
        pos = child.end_byte().min(total_end);
    }
    if pos < total_end {
        out.push_str(&String::from_utf8_lossy(&bytes[pos..total_end]));
    }
    let mut s = out.as_str();
    s = s.strip_prefix("$\"").unwrap_or(s);
    s = s.strip_suffix('"').unwrap_or(s);
    s.replace("\"\"", "\"")
        .replace("{{", "{")
        .replace("}}", "}")
}

pub(super) fn self_receiver_type(base: &BaseExtractor, node: Node) -> Option<String> {
    let member_access = match node.kind() {
        "member_access" | "member_access_expression" => node,
        "invocation" | "invocation_expression" => {
            let target = node.child_by_field_name("target").or_else(|| {
                let mut cursor = node.walk();
                node.children(&mut cursor).next()
            })?;
            if matches!(target.kind(), "member_access" | "member_access_expression") {
                target
            } else {
                return None;
            }
        }
        _ => return None,
    };
    let object = member_access.child_by_field_name("object")?;
    if object.kind() != "me_expression" {
        return None;
    }
    let text = base.get_node_text(&object);
    if text.eq_ignore_ascii_case("Me") || text.eq_ignore_ascii_case("MyClass") {
        enclosing_type_name(base, member_access)
    } else if text.eq_ignore_ascii_case("MyBase") {
        declared_base_type_name(base, member_access)
    } else {
        None
    }
}

fn enclosing_type_name(base: &BaseExtractor, node: Node) -> Option<String> {
    let mut current = node.parent();
    while let Some(candidate) = current {
        if matches!(
            candidate.kind(),
            "class_block" | "structure_block" | "module_block"
        ) {
            return candidate
                .child_by_field_name("name")
                .map(|name_node| base.get_node_text(&name_node));
        }
        current = candidate.parent();
    }
    None
}

fn declared_base_type_name(base: &BaseExtractor, node: Node) -> Option<String> {
    let mut current = node.parent();
    while let Some(candidate) = current {
        if matches!(candidate.kind(), "class_block" | "structure_block") {
            let inherits = super::helpers::extract_inherits(base, &candidate);
            let first = inherits.into_iter().next()?;
            return first.rsplit('.').next().map(|name| name.trim().to_string());
        }
        current = candidate.parent();
    }
    None
}
