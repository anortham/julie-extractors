//! Identifier and reference extraction for Scala
//!
//! Extracts function calls, member access, and other identifier usages
//! for LSP-quality find_references support.

use crate::base::{BaseExtractor, Identifier, IdentifierKind, Symbol};
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};
use std::collections::HashMap;
use tree_sitter::Node;

/// Extract all identifier usages from a Scala file
pub(super) fn extract_identifiers(
    base: &mut BaseExtractor,
    tree: &tree_sitter::Tree,
    symbols: &[Symbol],
) -> Vec<Identifier> {
    let symbol_map: HashMap<String, &Symbol> = symbols.iter().map(|s| (s.id.clone(), s)).collect();

    walk_tree_for_identifiers(base, tree.root_node(), &symbol_map, 0);

    base.identifiers.clone()
}

/// Recursively walk tree extracting identifiers
fn walk_tree_for_identifiers(
    base: &mut BaseExtractor,
    node: Node,
    symbol_map: &HashMap<String, &Symbol>,
    depth: u32,
) {
    if !should_visit_tree_depth(depth) {
        return;
    }

    extract_identifier_from_node(base, node, symbol_map);

    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk_tree_for_identifiers(base, child, symbol_map, child_depth);
    }
}

/// Extract identifier from a single node
fn extract_identifier_from_node(
    base: &mut BaseExtractor,
    node: Node,
    symbol_map: &HashMap<String, &Symbol>,
) {
    match node.kind() {
        // Function/method calls
        "call_expression" => {
            // Phase 3b: capture string-literal call-arguments (config-free;
            // carrier classification + gate run later in the artifact language-policy pass).
            // Runs before the early-returning callee branches below.
            record_scala_call_arg_literals(base, node, symbol_map);
            for child in node.children(&mut node.walk()) {
                if child.kind() == "identifier" {
                    let name = base.get_node_text(&child);
                    let containing = find_containing_symbol_id(base, node, symbol_map);
                    base.create_identifier(&child, name, IdentifierKind::Call, containing);
                    return;
                } else if child.kind() == "field_expression" {
                    if let Some((name_node, name)) = extract_rightmost_identifier(base, &child) {
                        let containing = find_containing_symbol_id(base, node, symbol_map);
                        let receiver_type = self_receiver_type(base, child);
                        base.create_identifier_with_receiver_type(
                            &name_node,
                            name,
                            IdentifierKind::Call,
                            containing,
                            receiver_type,
                        );
                    }
                    return;
                } else if child.kind() == "generic_function" {
                    // Generic method call: foo[T](x) or obj.method[T](x)
                    if let Some(func) = child.child_by_field_name("function") {
                        let containing = find_containing_symbol_id(base, node, symbol_map);
                        let opt_identifier = if func.kind() == "identifier" {
                            let name = base.get_node_text(&func);
                            Some(base.create_identifier(
                                &func,
                                name,
                                IdentifierKind::Call,
                                containing,
                            ))
                        } else if func.kind() == "field_expression" {
                            extract_rightmost_identifier(base, &func).map(|(name_node, name)| {
                                let receiver_type = self_receiver_type(base, func);
                                base.create_identifier_with_receiver_type(
                                    &name_node,
                                    name,
                                    IdentifierKind::Call,
                                    containing,
                                    receiver_type,
                                )
                            })
                        } else {
                            None
                        };
                        if let (Some(identifier), Some(args_node)) =
                            (opt_identifier, child.child_by_field_name("type_arguments"))
                        {
                            let arguments = crate::base::extract_type_arguments(
                                base,
                                args_node,
                                decompose_scala_type_arg,
                            );
                            base.record_type_arguments(&identifier, arguments);
                        }
                    }
                    return;
                }
            }
        }

        // Type references in type positions: val x: Foo, def f(a: Foo): Bar,
        // class Foo extends Bar, type A = Foo
        // Scala uses `type_identifier` for both declaration names and references.
        // We filter out declaration names via parent context.
        "type_identifier" => {
            if is_type_declaration_name(&node) {
                return;
            }

            let name = base.get_node_text(&node);

            if is_scala_noise_type(&name) {
                return;
            }

            let containing = find_containing_symbol_id(base, node, symbol_map);
            let identifier =
                base.create_identifier(&node, name, IdentifierKind::TypeUsage, containing);
            record_outermost_scala_type_arguments(base, node, &identifier);
        }

        // Member access: obj.field
        "field_expression" => {
            // Only extract if NOT part of a call_expression
            if let Some(parent) = node.parent()
                && parent.kind() == "call_expression"
            {
                return;
            }

            if let Some((name_node, name)) = extract_rightmost_identifier(base, &node) {
                let containing = find_containing_symbol_id(base, node, symbol_map);
                base.create_identifier(&name_node, name, IdentifierKind::MemberAccess, containing);
            }
        }

        // `variable_ref` complement arm (locked contract — see the reference
        // implementation doc comment in csharp/identifiers.rs): a bare `identifier`
        // used as a value or as the object/receiver of a field access — the reads
        // the Call/MemberAccess/TypeUsage arms above do not own. Scala type
        // positions are `type_identifier` nodes, so they never reach this arm.
        "identifier" if is_scala_value_read_identifier(base, node) => {
            let name = base.get_node_text(&node);
            // Rule 5: reuse the existing noise filter. (`this`/`super`/`true`/
            // `null` are distinct grammar nodes, never `identifier`.)
            if !is_scala_noise_type(&name) {
                let containing = find_containing_symbol_id(base, node, symbol_map);
                base.create_identifier(&node, name, IdentifierKind::VariableRef, containing);
            }
        }

        _ => {}
    }
}

/// Rule 1/4 predicate for the `variable_ref` arm: is this bare `identifier` a
/// value read or a field-access receiver (the complement of the Call/
/// MemberAccess/TypeUsage arms)? Node kinds and field names were verified
/// empirically against the vendored tree-sitter-scala 0.26.0 grammar.
///
/// Pattern positions use Scala's own syntactic disambiguation rule: a
/// Capitalized identifier in a pattern is a STABLE REFERENCE (a read of an
/// existing value, e.g. `case First =>`), while a lowercase identifier BINDS a
/// new name (`case fallback =>`). This mirrors scalac; the rare backtick-quoted
/// lowercase stable pattern is not distinguishable here and is skipped.
fn is_scala_value_read_identifier(base: &BaseExtractor, node: Node) -> bool {
    let Some(parent) = node.parent() else {
        return false;
    };
    let is_field = |name: &str| parent.child_by_field_name(name).map(|n| n.id()) == Some(node.id());
    let starts_uppercase = || {
        base.get_node_text(&node)
            .chars()
            .next()
            .is_some_and(|c| c.is_uppercase())
    };

    match parent.kind() {
        // Rule 2: the callee is owned by the Call arm.
        "call_expression" => !is_field("function"),
        "generic_function" => false,

        // Rule 1/2: only the `value` receiver of a field access is a read; the
        // accessed `field` is owned by the MemberAccess/Call arms.
        "field_expression" => is_field("value"),

        // Rule 2-adjacent: a word operator (`a max b`) is a method reference,
        // not a value read; operands are reads.
        "infix_expression" => !is_field("operator"),

        // Rule 4: Scala `assignment_expression` is always PLAIN `=` (compound
        // `+=` parses as infix_expression), so its LHS is write-only. This also
        // skips named-argument labels (`bar` in `f(bar = seed)`), which name
        // parameters. The RHS is a read.
        "assignment_expression" => !is_field("left"),

        // Rule 3: declaration names/patterns. Initializer values fall through
        // as reads via their own parents.
        "val_definition" | "var_definition" | "val_declaration" | "var_declaration" => {
            !(is_field("pattern") || is_field("name"))
        }
        "function_definition"
        | "function_declaration"
        | "class_definition"
        | "object_definition"
        | "trait_definition"
        | "enum_definition"
        | "type_definition"
        | "given_definition"
        | "parameter"
        | "class_parameter"
        | "binding" => !is_field("name"),
        "lambda_expression" => !is_field("parameters"),
        "self_type" => false,

        // Rule 3: a for-comprehension enumerator binds its first child; the
        // generator collection (and guards) are reads.
        "enumerator" => parent.named_child(0).map(|c| c.id()) != Some(node.id()),

        // Rule 2: the interpolator (`s` in s"...") is a StringContext method,
        // not a value read; `${...}` bodies fall through as reads.
        "interpolated_string_expression" => !is_field("interpolator"),

        // Rule 3: package/import segments, selectors, and renames.
        "package_identifier"
        | "package_clause"
        | "import_declaration"
        | "export_declaration"
        | "namespace_selectors"
        | "arrow_renamed_identifier"
        | "namespace_wildcard" => false,

        // Patterns: capitalized = stable reference read; lowercase = binding.
        "case_clause" if is_field("pattern") => starts_uppercase(),
        "case_clause" => true,
        "tuple_pattern"
        | "case_class_pattern"
        | "infix_pattern"
        | "alternative_pattern"
        | "typed_pattern"
        | "capture_pattern"
        | "stable_identifier_pattern" => starts_uppercase(),

        // Every other position — argument, operand, return value, if/match
        // branch, interpolation body, generator collection — is a read.
        _ => true,
    }
}

/// Find the ID of the symbol that contains this node
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

/// Capture string-literal arguments of a Scala `call_expression` as `Literal`
/// records.
///
/// Config-free: `carrier` is the verbatim callee text; the URL/SQL
/// classification and the carrier gate run later in the artifact language-policy pass. Scala's
/// call has a `function` callee and an `arguments` node holding `expression`
/// children; `arg_position` is counted over the full argument list. Plain Scala
/// `string` nodes expose no content child, so they decode via the shared
/// delimiter-strip fallback. (Prefixed interpolators like Doobie's `sql"..."`
/// and sttp's `uri"..."` are NOT call-argument literals and are out of scope.)
fn record_scala_call_arg_literals(
    base: &mut BaseExtractor,
    call_node: Node,
    symbol_map: &HashMap<String, &Symbol>,
) {
    let Some(func) = call_node.child_by_field_name("function") else {
        return;
    };
    let Some(args) = call_node.child_by_field_name("arguments") else {
        return;
    };
    let carrier = scala_carrier(base, func);
    let containing_symbol_id = find_containing_symbol_id(base, call_node, symbol_map);

    let arg_nodes: Vec<Node> = {
        let mut cursor = args.walk();
        args.named_children(&mut cursor).collect()
    };
    for (pos, arg) in arg_nodes.into_iter().enumerate() {
        if let Some(text) = base.decode_string_literal(&arg) {
            base.record_literal(
                &arg,
                text,
                carrier.clone(),
                pos as u32,
                containing_symbol_id.clone(),
            );
        }
    }
}

/// Derive a Scala call's carrier from its (generic-unwrapped) callee.
///
/// Plain `identifier`/`operator_identifier` → its text (`SQL`, `greet`). A
/// `field_expression` (`requests.get`, `stmt.executeQuery`) → the `value.field`
/// join so dotted client APIs match config (`requests.get`) while bare DB verbs
/// (`executeQuery`/`SQL`) match any receiver via the gate's last-segment rule.
fn scala_carrier(base: &BaseExtractor, func: Node) -> Option<String> {
    let func = if func.kind() == "generic_function" {
        func.child_by_field_name("function").unwrap_or(func)
    } else {
        func
    };
    match func.kind() {
        "identifier" | "operator_identifier" => Some(base.get_node_text(&func)),
        "field_expression" => {
            let value = func
                .child_by_field_name("value")
                .map(|n| base.get_node_text(&n));
            let field = func
                .child_by_field_name("field")
                .map(|n| base.get_node_text(&n));
            match (value, field) {
                (Some(v), Some(f)) => Some(format!("{v}.{f}")),
                (None, Some(f)) => Some(f),
                _ => None,
            }
        }
        _ => {
            let text = base.get_node_text(&func);
            if text.is_empty() { None } else { Some(text) }
        }
    }
}

/// Check if a `type_identifier` node is a declaration name rather than a type reference.
///
/// In Scala, `type_identifier` appears as the `name` field of:
/// - `type_definition` → `type Foo = ...` (declaration)
///
/// Class/trait/object names use `identifier`, not `type_identifier`, so they
/// don't need to be filtered here.
fn is_type_declaration_name(node: &Node) -> bool {
    if let Some(parent) = node.parent()
        && let Some(name_node) = parent.child_by_field_name("name")
        && name_node.id() == node.id()
    {
        return parent.kind() == "type_definition";
    }
    false
}

/// Returns true for Scala types that are too common to be meaningful
/// type references for centrality scoring.
///
/// Includes:
/// - Single-letter type params (T, A, B, etc.) — generic type parameters used in scope
/// - Scala primitive/base types — ubiquitous in every file
fn is_scala_noise_type(name: &str) -> bool {
    // Single-letter uppercase names are almost always generic type parameters.
    if name.len() == 1 && name.chars().next().is_some_and(|c| c.is_ascii_uppercase()) {
        return true;
    }

    matches!(
        name,
        // Scala AnyVal types
        "Int"
            | "Long"
            | "Short"
            | "Byte"
            | "Float"
            | "Double"
            | "Char"
            | "Boolean"
            | "Unit"
            // Scala top types
            | "Any"
            | "AnyRef"
            | "AnyVal"
            | "Nothing"
            | "Null"
            // Java interop
            | "String"
            | "Object"
    )
}

/// Extract the rightmost identifier from a field_expression
fn extract_rightmost_identifier<'a>(
    base: &BaseExtractor,
    node: &Node<'a>,
) -> Option<(Node<'a>, String)> {
    let identifiers: Vec<Node> = node
        .children(&mut node.walk())
        .filter(|n| n.kind() == "identifier")
        .collect();

    identifiers.last().map(|n| (*n, base.get_node_text(n)))
}

pub(super) fn self_receiver_type(base: &BaseExtractor, node: Node) -> Option<String> {
    let field_expression = match node.kind() {
        "field_expression" => node,
        "call_expression" => {
            let mut cursor = node.walk();
            node.children(&mut cursor)
                .find(|child| child.kind() == "field_expression")?
        }
        _ => return None,
    };
    if !field_value_is_this(base, field_expression) {
        return None;
    }
    super::helpers::enclosing_type_name(base, &field_expression)
}

/// tree-sitter-scala aliases the `this` keyword to `identifier` inside
/// expressions, so the receiver check must read the token text.
fn field_value_is_this(base: &BaseExtractor, field_expression: Node) -> bool {
    field_expression
        .child_by_field_name("value")
        .is_some_and(|value| {
            matches!(value.kind(), "identifier" | "this") && base.get_node_text(&value) == "this"
        })
}

/// Record type arguments for the outermost generic use site.
///
/// Called from the `type_identifier` arm after creating the identifier.
/// Records only when:
/// - the `type_identifier`'s parent is `generic_type` (e.g. `List` in `List[Int]`)
/// - AND that `generic_type` is not itself nested inside `type_arguments`
///   (i.e. `List` in `Map[String, List[Int]]` is skipped — it rides as a nested child)
fn record_outermost_scala_type_arguments(
    base: &mut BaseExtractor,
    name_node: Node,
    identifier: &Identifier,
) {
    let Some(parent) = name_node.parent() else {
        return;
    };
    if parent.kind() != "generic_type" {
        return;
    }
    // Skip if this generic_type is itself nested inside type_arguments (it's not outermost)
    if parent
        .parent()
        .map(|p| p.kind() == "type_arguments")
        .unwrap_or(false)
    {
        return;
    }
    let Some(arg_list) = parent.child_by_field_name("type_arguments") else {
        return;
    };
    let arguments = crate::base::extract_type_arguments(base, arg_list, decompose_scala_type_arg);
    base.record_type_arguments(identifier, arguments);
}

/// Decompose a child of `type_arguments` into `(type_name, nested_arg_list)`.
///
/// Returns `None` for punctuation and node kinds with no meaningful name.
fn decompose_scala_type_arg<'a>(
    base: &BaseExtractor,
    node: Node<'a>,
) -> Option<(String, Option<Node<'a>>)> {
    if !node.is_named() {
        return None; // skip [ , ]
    }
    match node.kind() {
        "type_identifier" => Some((base.get_node_text(&node), None)),
        "generic_type" => {
            // Nested generic: e.g. `List[Int]` inside outer type_arguments.
            let name = node
                .child_by_field_name("type")
                .map(|t| base.get_node_text(&t))
                .unwrap_or_else(|| base.get_node_text(&node));
            let nested = node.child_by_field_name("type_arguments");
            Some((name, nested))
        }
        "stable_type_identifier" => {
            // Qualified type: `scala.collection.mutable.Map` — use full source text as name.
            Some((base.get_node_text(&node), None))
        }
        _ => {
            // function_type (`Int => Boolean`), tuple_type, infix_type, wildcard, etc.
            // Return the source text as a leaf so the ordinal slot is preserved.
            // A None here would cause later args to receive wrong ordinals because
            // extract_type_arguments only increments the ordinal counter on Some.
            let text = base.get_node_text(&node);
            if text.is_empty() {
                None
            } else {
                Some((text, None))
            }
        }
    }
}
