// Dart Extractor - Identifiers Extraction
//
// Methods for extracting identifier usages (function calls, member access, etc.)

use super::helpers::{find_child_by_type, get_node_text};
use crate::base::{BaseExtractor, Identifier, IdentifierKind, OwnerIndex};
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};
use tree_sitter::Node;

/// Walk the entire tree extracting identifier usages
pub(super) fn walk_tree_for_identifiers(
    base: &mut BaseExtractor,
    node: Node,
    containing_symbols: &OwnerIndex<'_>,
    depth: u32,
) {
    if !should_visit_tree_depth(depth) {
        return;
    }

    // Extract identifier from this node if applicable
    extract_identifier_from_node(base, node, containing_symbols);

    // Recursively walk children
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
    containing_symbols: &OwnerIndex<'_>,
) {
    match node.kind() {
        "call_expression" => {
            let function = node.child_by_field_name("function");
            if let Some(callee) = function.and_then(call_callee) {
                let target_node = callee.name;
                let name = get_node_text(&target_node);
                let containing_symbol_id = find_containing_symbol_id(node, containing_symbols);
                let receiver_type = self_receiver_type(base, node);
                let identifier = base.create_identifier_with_receiver_type(
                    &target_node,
                    name,
                    IdentifierKind::Call,
                    containing_symbol_id,
                    receiver_type,
                );
                if let Some(arguments) = function
                    .filter(|function| function.kind() == "instantiation_expression")
                    .and_then(|function| function.child_by_field_name("type_arguments"))
                {
                    record_call_type_arguments(base, arguments, &identifier);
                }
            }
            // Phase 3b: capture string-literal call-arguments (config-free;
            // carrier classification + gate run later in the artifact language-policy pass).
            record_dart_call_arg_literals(base, node, containing_symbols);
        }

        "const_object_expression" | "new_expression" | "constructor_invocation" => {
            if let Some(constructor) = node.child_by_field_name("constructor") {
                let name = get_node_text(&constructor);
                let containing_symbol_id = find_containing_symbol_id(node, containing_symbols);
                base.create_identifier(
                    &constructor,
                    name,
                    IdentifierKind::Call,
                    containing_symbol_id,
                );
            }
        }

        "member_expression" | "null_aware_member_expression" => {
            if is_call_function_node(node) {
                return;
            }

            if let Some(property_node) = node.child_by_field_name("property") {
                let name = get_node_text(&property_node);
                let containing_symbol_id = find_containing_symbol_id(node, containing_symbols);
                base.create_identifier(
                    &property_node,
                    name,
                    IdentifierKind::MemberAccess,
                    containing_symbol_id,
                );
            }
        }

        "member_access" => {
            if let Some(id_node) = find_child_by_type(&node, "identifier") {
                let name = get_node_text(&id_node);

                let is_call = if let Some(selector_node) = find_child_by_type(&node, "selector") {
                    find_child_by_type(&selector_node, "argument_part").is_some()
                } else {
                    false
                };

                let containing_symbol_id = find_containing_symbol_id(node, containing_symbols);
                let kind = if is_call {
                    crate::base::IdentifierKind::Call
                } else {
                    crate::base::IdentifierKind::MemberAccess
                };

                base.create_identifier(&id_node, name, kind, containing_symbol_id);
            }
        }

        // Type references: field types, parameter types, return types, generic args,
        // extends, implements, with clauses, mixin "on" constraints.
        // Dart tree-sitter uses `type_identifier` for type names. In Dart, class/enum/
        // mixin/extension declarations use `identifier` for their name (not type_identifier),
        // so the only declaration context where type_identifier IS the name is `type_alias`.
        "type_identifier" => {
            if is_type_declaration_name(&node) {
                return;
            }

            let name = get_node_text(&node);

            // Skip single-letter generic type parameters (T, K, V, E, S, R, etc.)
            if name.len() == 1 && name.chars().next().is_some_and(|c| c.is_ascii_uppercase()) {
                return;
            }

            let kind = if is_instantiation_callee(node) {
                IdentifierKind::Call
            } else {
                IdentifierKind::TypeUsage
            };
            let containing_symbol_id = find_containing_symbol_id(node, containing_symbols);
            let identifier = base.create_identifier(&node, name, kind, containing_symbol_id);
            record_outermost_dart_type_arguments(base, node, &identifier);
        }

        // `..clear()`: a call on the cascade target.
        "cascade_call_expression" => {
            if let Some(property) = node.child_by_field_name("property") {
                let containing_symbol_id = find_containing_symbol_id(node, containing_symbols);
                base.create_identifier(
                    &property,
                    get_node_text(&property),
                    IdentifierKind::Call,
                    containing_symbol_id,
                );
            }
        }

        // `..color = x`: a member of the cascade target.
        "cascade_selector" => {
            if let Some(property) = find_child_by_type(&node, "identifier") {
                let containing_symbol_id = find_containing_symbol_id(node, containing_symbols);
                base.create_identifier(
                    &property,
                    get_node_text(&property),
                    IdentifierKind::MemberAccess,
                    containing_symbol_id,
                );
            }
        }

        // `List<int>.filled(...)`: the generic type named before a static member.
        "identifier" if is_generic_type_receiver(node) => {
            let containing_symbol_id = find_containing_symbol_id(node, containing_symbols);
            let identifier = base.create_identifier(
                &node,
                get_node_text(&node),
                IdentifierKind::TypeUsage,
                containing_symbol_id,
            );
            if let Some(arguments) = node
                .parent()
                .and_then(|instantiation| instantiation.child_by_field_name("type_arguments"))
            {
                record_call_type_arguments(base, arguments, &identifier);
            }
        }

        "unconditional_assignable_selector" => {
            if let Some(id_node) = find_child_by_type(&node, "identifier") {
                let name = get_node_text(&id_node);
                let containing_symbol_id = find_containing_symbol_id(node, containing_symbols);

                base.create_identifier(
                    &id_node,
                    name,
                    crate::base::IdentifierKind::MemberAccess,
                    containing_symbol_id,
                );
            }
        }

        // `variable_ref` complement arm (locked contract — see the reference
        // implementation doc comment in csharp/identifiers.rs): a bare `identifier`
        // used as a value or as the object/receiver of a member access — the reads
        // the Call/MemberAccess/TypeUsage arms above do not own. Dart type
        // positions are `type_identifier` nodes, so they never reach this arm.
        // (`this`/`super`/`true`/`null` are distinct grammar tokens, never
        // `identifier`, so rule 5 is structurally satisfied.)
        "identifier" if is_dart_value_read_identifier(node) => {
            let name = get_node_text(&node);
            let containing_symbol_id = find_containing_symbol_id(node, containing_symbols);
            base.create_identifier(
                &node,
                name,
                IdentifierKind::VariableRef,
                containing_symbol_id,
            );
        }

        // Rule 1: a simple `$name` string interpolation is a value read, but the
        // tree-sitter-dart grammar lexes it as a distinct `identifier_dollar_escaped`
        // node (only `${...}` holds ordinary expression identifiers), so it needs
        // its own arm. The node text is the bare name without the `$`.
        "identifier_dollar_escaped"
            if node
                .parent()
                .is_some_and(|p| p.kind() == "template_substitution") =>
        {
            let name = get_node_text(&node);
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

/// Rule 1/4 predicate for the `variable_ref` arm: is this bare `identifier` a
/// value read or a member-access receiver (the complement of the Call/
/// MemberAccess/TypeUsage arms)? Node kinds and field names were verified
/// empirically against the vendored tree-sitter-dart 0.2.0 grammar.
fn is_dart_value_read_identifier(node: Node) -> bool {
    let Some(parent) = node.parent() else {
        return false;
    };
    let is_field = |name: &str| parent.child_by_field_name(name).map(|n| n.id()) == Some(node.id());

    match parent.kind() {
        // Rule 2: the callee `function` of a call is owned by the Call arm.
        "call_expression" => !is_field("function"),

        // Rule 2: `() => f()` parses as a call of `() => f`; its callee is
        // owned by the Call arm.
        "function_expression_body" => !parent
            .parent()
            .is_some_and(|closure| is_call_function_node(closure)),

        // Rule 2: a named constructor (`new Box.named()`) is owned by the Call arm.
        "const_object_expression" | "new_expression" | "constructor_invocation" => false,

        // Rule 1/2: only the `object` receiver of a member access is a read;
        // the accessed `property` is owned by the MemberAccess/Call arms.
        "member_expression" | "null_aware_member_expression" => is_field("object"),

        // Rule 2: cascade/selector member positions are member references owned
        // by the selector arms (or plain write targets); the cascade RECEIVER
        // is an ordinary expression child elsewhere and falls through as a read.
        "cascade_call_expression"
        | "cascade_selector"
        | "member_access"
        | "unconditional_assignable_selector" => false,

        // Rule 4: the LHS wrapper of an assignment. A PLAIN `=` target is
        // write-only; a COMPOUND operator (`+=`, …) reads its target. An
        // `assignable_expression` outside an assignment LHS (e.g. `x++`) reads.
        "assignable_expression" => {
            let Some(assignment) = parent.parent() else {
                return true;
            };
            if assignment.kind() != "assignment_expression"
                || assignment.child_by_field_name("left").map(|l| l.id()) != Some(parent.id())
            {
                return true;
            }
            assignment
                .child_by_field_name("operator")
                .map(|op| op.kind() != "=")
                .unwrap_or(false)
        }

        // Rule 3: declaration names. Their `value` initializer children are
        // reads; everything else under these parents is a definition name.
        "initialized_identifier"
        | "initialized_variable_definition"
        | "static_final_declaration" => is_field("value"),
        "class_declaration"
        | "enum_declaration"
        | "enum_constant"
        | "mixin_declaration"
        | "extension_declaration"
        | "function_signature"
        | "constructor_signature"
        | "factory_constructor_signature"
        | "constant_constructor_signature"
        | "redirecting_factory_constructor_signature"
        | "extension_type_name"
        | "extension_type_representation"
        | "part_of_directive"
        | "getter_signature"
        | "setter_signature"
        | "type_alias"
        | "formal_parameter"
        | "constructor_param"
        | "normal_parameter_type" => false,

        // Rule 3: pattern bindings (`String name`, `final (a, b) = ...`).
        "variable_pattern" => !is_field("name"),
        "constant_pattern" => !super::locals::is_declaring_pattern(parent),

        // Rule 3: a for-in loop binds `name`; the `value` collection reads.
        "for_statement" => !is_field("name"),

        // Rule 3: catch parameters (`catch (e, st)`) are definitions.
        "catch_clause" => false,

        // Rule 1: a named-argument LABEL (`bar:` in `f(bar: seed)`) names a
        // PARAMETER, not a member; statement labels are not variables either.
        "label" => false,

        // Rule 2: an annotation's name (`@override`, `@Deprecated(...)`) is a
        // type-ish usage, not a value read.
        "annotation" | "marker_annotation" => false,

        // Rule 3: import prefixes/aliases and library names.
        "import_specification" | "library_name" | "dotted_identifier_list" => false,

        // Every other position — argument, operand, return value, ternary arm,
        // `${...}` interpolation, switch constant, cascade receiver — is a read.
        _ => true,
    }
}

/// If `name_node` is the `type_identifier` of an *outermost* generic use site,
/// records that generic's ordered/nested applied type arguments against `identifier`.
///
/// ## Grammar details
///
/// Dart represents generic types in two structurally different ways depending on
/// context:
///
/// **Annotation / nested-arg context** (`parent.kind() == "type"`):
/// A `type` wrapper node contains `type_identifier` (the base name) and a
/// `type_arguments` named child: `type { type_identifier, type_arguments { … } }`.
/// The outermost check: if the `type` wrapper is itself inside a `type_arguments`
/// node, it is a nested arg and must not produce a separate usage row.
///
/// **Construction / heritage context** (`grandparent.kind()` ∈
/// `{new_expression, superclass, interfaces, mixins, mixin_application}`):
/// The grammar splits the generic into TWO sibling `type` nodes:
/// - First `type { type_identifier("Foo") }` — the base type name
/// - Second `type { < type { … } , type { … } > }` — the angle-bracket arg list
///
/// There is NO `type_arguments` node here; instead the sibling `type` node IS
/// the arg container and its named children are individual `type` arg-wrappers.
/// `decompose_dart_type_arg` expects exactly that layout (it handles `type`
/// wrapper children), so we can reuse it unchanged.
fn record_outermost_dart_type_arguments(
    base: &mut BaseExtractor,
    name_node: Node,
    identifier: &Identifier,
) {
    let Some(parent) = name_node.parent() else {
        return;
    };
    if parent.kind() != "type" {
        return; // type_identifier not in a type wrapper — unexpected context
    }
    let Some(grandparent) = parent.parent() else {
        return;
    };

    match grandparent.kind() {
        // ── Nested arg: rides as child of outer usage ────────────────────────
        "type_arguments" => (),

        // ── Construction / Heritage ─────────────────────────────────────────
        // The arg list is the NEXT named sibling `type` node (the `<...>` part).
        "new_expression" | "superclass" | "interfaces" | "mixins" | "mixin_application" => {
            let Some(args_container) = parent.next_named_sibling() else {
                return; // non-generic — no sibling
            };
            if args_container.kind() != "type" {
                return; // sibling is arguments/class_body/etc. — not generic
            }
            // The args_container is the `type { < type{…} , type{…} > }` node.
            // Its named children are the individual arg-wrapper `type` nodes.
            let arguments =
                crate::base::extract_type_arguments(base, args_container, decompose_dart_type_arg);
            base.record_type_arguments(identifier, arguments);
        }

        // ── Standard annotation ──────────────────────────────────────────────
        // The `type` wrapper contains `type_identifier` + `type_arguments` sibling.
        _ => {
            let mut cursor = parent.walk();
            let Some(arg_list) = parent
                .named_children(&mut cursor)
                .find(|c| c.kind() == "type_arguments")
            else {
                return; // non-generic annotation
            };
            let arguments =
                crate::base::extract_type_arguments(base, arg_list, decompose_dart_type_arg);
            base.record_type_arguments(identifier, arguments);
        }
    }
}

/// The explicit type arguments of a generic call or type literal.
fn record_call_type_arguments(base: &mut BaseExtractor, arguments: Node, identifier: &Identifier) {
    let arguments = crate::base::extract_type_arguments(base, arguments, decompose_dart_type_arg);
    base.record_type_arguments(identifier, arguments);
}

/// `List` in `List<int>.filled(...)`: the function of an instantiation that is
/// the object of a member access.
fn is_generic_type_receiver(node: Node) -> bool {
    let Some(instantiation) = node
        .parent()
        .filter(|parent| parent.kind() == "instantiation_expression")
    else {
        return false;
    };
    instantiation.child_by_field_name("function") == Some(node)
        && instantiation.parent().is_some_and(|member| {
            matches!(
                member.kind(),
                "member_expression" | "null_aware_member_expression"
            ) && member.child_by_field_name("object") == Some(instantiation)
        })
}

/// The expression a cascade section applies to: the nearest earlier sibling
/// that is not itself a cascade section.
pub(super) fn cascade_target(section: Node) -> Option<Node> {
    let mut current = section.prev_named_sibling();
    while let Some(sibling) = current {
        if sibling.kind() != "cascade_section" {
            return Some(sibling);
        }
        current = sibling.prev_named_sibling();
    }
    None
}

/// `TypeArgDecomposer` for Dart: maps a child of a `type_arguments` node to its
/// applied argument. Dart's `type_arguments` children are `type` wrapper nodes
/// (each containing a `type_identifier` and optionally nested `type_arguments`).
/// Unnamed punctuation (`<`, `,`, `>`) is skipped by the `!is_named()` guard.
fn decompose_dart_type_arg<'a>(
    base: &BaseExtractor,
    node: Node<'a>,
) -> Option<(String, Option<Node<'a>>)> {
    if !node.is_named() {
        return None; // skip punctuation: <, >, ,
    }
    if node.kind() != "type" {
        return None; // defensive skip
    }
    // Find the type_identifier child for the type name.
    let mut cursor1 = node.walk();
    let type_id = node
        .named_children(&mut cursor1)
        .find(|c| c.kind() == "type_identifier")?;
    let name = base.get_node_text(&type_id);
    // Find optional type_arguments child to recurse into for nested generics.
    let mut cursor2 = node.walk();
    let nested = node
        .named_children(&mut cursor2)
        .find(|c| c.kind() == "type_arguments");
    Some((name, nested))
}

/// Check if a `type_identifier` node is a declaration name rather than a type reference.
///
/// In Dart's tree-sitter grammar, most declarations (class, enum, mixin, extension)
/// use `identifier` for their name, NOT `type_identifier`. The only declaration
/// context where `type_identifier` is the name is `type_alias`:
///
///   typedef Callback = void Function(Event event);
///          ^^^^^^^^ type_identifier (declaration name - skip)
///
/// Other type_identifier appearances are references (superclass, field types,
/// parameter types, generic args, etc.) and should be extracted as TypeUsage.
fn is_type_declaration_name(node: &Node) -> bool {
    if let Some(parent) = node.parent() {
        // type_alias: `typedef Callback = ...` - the first type_identifier is the name
        if parent.kind() == "type_alias" {
            // Check if this is the first type_identifier child of the type_alias
            let mut cursor = parent.walk();
            for child in parent.children(&mut cursor) {
                if child.kind() == "type_identifier" {
                    return child.id() == node.id();
                }
            }
        }
    }
    false
}

/// The called name of a call and the byte range of its receiver.
pub(super) struct Callee<'tree> {
    pub(super) name: Node<'tree>,
    pub(super) receiver: Option<(usize, usize)>,
}

/// Reads the callee of a `call_expression` `function` field. The grammar
/// parses `() => f()` as a call of `() => f`, and `(e) => A.b(e)` as a call
/// whose receiver chain starts with `(e) => A`; both read as the call inside
/// the closure body.
pub(super) fn call_callee(function: Node) -> Option<Callee> {
    match function.kind() {
        "identifier" => Some(Callee {
            name: function,
            receiver: None,
        }),
        "member_expression" | "null_aware_member_expression" => {
            let mut object = function.child_by_field_name("object")?;
            if object.kind() == "instantiation_expression"
                && let Some(generic) = object.child_by_field_name("function")
            {
                object = generic;
            }
            let start = arrow_body_start(object).unwrap_or(object.start_byte());
            Some(Callee {
                name: function.child_by_field_name("property")?,
                receiver: Some((start, object.end_byte())),
            })
        }
        "cascade_call_expression" => {
            let target = function
                .parent()
                .filter(|section| section.kind() == "cascade_section")
                .and_then(cascade_target)?;
            Some(Callee {
                name: function.child_by_field_name("property")?,
                receiver: Some((target.start_byte(), target.end_byte())),
            })
        }
        "instantiation_expression" => call_callee(function.child_by_field_name("function")?),
        "function_expression" => call_callee(arrow_body(function)?),
        _ => None,
    }
}

fn arrow_body(closure: Node) -> Option<Node> {
    let body = closure.child_by_field_name("body")?;
    (body.kind() == "function_expression_body")
        .then(|| body.named_child(0))
        .flatten()
}

fn arrow_body_start(receiver: Node) -> Option<usize> {
    let mut current = receiver;
    loop {
        current = match current.kind() {
            "function_expression" => return arrow_body(current).map(|body| body.start_byte()),
            "member_expression" | "null_aware_member_expression" => {
                current.child_by_field_name("object")?
            }
            "call_expression" | "instantiation_expression" => {
                current.child_by_field_name("function")?
            }
            _ => return None,
        };
    }
}

/// The class and optional named constructor of `const T(...)`,
/// `new T.named(...)` or `T.named(...)` as a `constructor_invocation`.
pub(super) fn instantiation_target(
    base: &BaseExtractor,
    node: Node,
) -> Option<(String, Option<String>)> {
    let type_node = node.child_by_field_name("type")?;
    let names = instantiated_type_names(type_node);
    let constructor = node
        .child_by_field_name("constructor")
        .map(|constructor| base.get_node_text(&constructor));
    match (names.as_slice(), constructor) {
        ([.., last], Some(constructor)) => Some((base.get_node_text(last), Some(constructor))),
        ([type_name, constructor], None) if is_uppercase_identifier(*type_name) => Some((
            base.get_node_text(type_name),
            Some(base.get_node_text(constructor)),
        )),
        ([.., last], None) => Some((base.get_node_text(last), None)),
        ([], _) => None,
    }
}

fn instantiated_type_names(type_node: Node) -> Vec<Node> {
    if type_node.kind() == "type_identifier" {
        return vec![type_node];
    }
    let mut cursor = type_node.walk();
    type_node
        .named_children(&mut cursor)
        .filter(|child| child.kind() == "type_identifier")
        .collect()
}

/// Is this `type_identifier` the called name of an instantiation: the class
/// in `const T()`, or the constructor in `const T.named()`?
fn is_instantiation_callee(node: Node) -> bool {
    let Some(type_node) = node.parent().filter(|parent| parent.kind() == "type") else {
        return false;
    };
    let Some(expression) = type_node.parent().filter(|parent| {
        matches!(
            parent.kind(),
            "const_object_expression" | "new_expression" | "constructor_invocation"
        )
    }) else {
        return false;
    };
    if expression.child_by_field_name("type").map(|t| t.id()) != Some(type_node.id())
        || expression.child_by_field_name("constructor").is_some()
    {
        return false;
    }
    let names = instantiated_type_names(type_node);
    let is_named_constructor = names.len() == 2 && is_uppercase_identifier(names[0]);
    match names.as_slice() {
        [_, constructor] if is_named_constructor => constructor.id() == node.id(),
        [.., last] => last.id() == node.id(),
        [] => false,
    }
}

fn is_uppercase_identifier(node: Node) -> bool {
    get_node_text(&node)
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_uppercase())
}

pub(super) fn self_receiver_type(base: &BaseExtractor, node: Node) -> Option<String> {
    let function = if node.kind() == "call_expression" {
        node.child_by_field_name("function")?
    } else {
        node
    };
    if !matches!(
        function.kind(),
        "member_expression" | "null_aware_member_expression"
    ) {
        return None;
    }
    let object = function.child_by_field_name("object")?;
    match object.kind() {
        "this" => enclosing_class_name(base, node),
        "super" => super::relationships::first_extends_name(node),
        _ => None,
    }
}

fn enclosing_class_name(base: &BaseExtractor, node: Node) -> Option<String> {
    let mut current = node.parent();
    while let Some(candidate) = current {
        if matches!(candidate.kind(), "class_definition" | "class_declaration") {
            return find_child_by_type(&candidate, "identifier")
                .map(|name_node| base.get_node_text(&name_node));
        }
        current = candidate.parent();
    }
    None
}

fn is_call_function_node(node: Node) -> bool {
    let Some(parent) = node.parent() else {
        return false;
    };

    if parent.kind() != "call_expression" {
        return false;
    }

    parent
        .child_by_field_name("function")
        .is_some_and(|function_node| function_node.id() == node.id())
}

fn find_containing_symbol_id(node: Node, containing_symbols: &OwnerIndex<'_>) -> Option<String> {
    containing_symbols.find(node).map(|s| s.id.clone())
}

// ============================================================================
// String-literal call-argument capture
// ============================================================================

/// Capture string-literal arguments of a Dart `call_expression` as `Literal`
/// records.
///
/// Config-free: `carrier` is the verbatim callee text; the URL/SQL
/// classification and the carrier gate run later in the artifact language-policy pass. The
/// call has a `function` callee and an `arguments` node; a `named_argument`
/// (`body: "..."`) carries a leading `label`, so the value is its last non-label
/// child. `arg_position` is counted over the full argument list.
///
/// NOTE: Dart string interpolation (`$x` / `${x}`) nests its text as
/// `template_chars_*` content, which the shared `decode_string_literal` does not
/// recognize, so interpolated literals decode via the delimiter-strip fallback
/// (text preserved verbatim, no `{}` normalization). Plain string literals — the
/// common URL/SQL case — decode correctly. Flagged to the lead.
fn record_dart_call_arg_literals(
    base: &mut BaseExtractor,
    call_node: Node,
    containing_symbols: &OwnerIndex<'_>,
) {
    let Some(function_node) = call_node.child_by_field_name("function") else {
        return;
    };
    let Some(args_node) = call_node.child_by_field_name("arguments") else {
        return;
    };
    let carrier = dart_carrier(function_node);
    let containing_symbol_id = find_containing_symbol_id(call_node, containing_symbols);

    let arg_nodes: Vec<Node> = {
        let mut cursor = args_node.walk();
        args_node.named_children(&mut cursor).collect()
    };
    for (pos, arg) in arg_nodes.into_iter().enumerate() {
        // Named args (`name: value`) carry a leading `label`; the value is the
        // last non-label child.
        let value = if arg.kind() == "named_argument" {
            dart_named_arg_value(arg)
        } else {
            Some(arg)
        };
        if let Some(value) = value
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

/// The value expression of a Dart `named_argument` (`label: value`): the last
/// named child that is not the `label`.
fn dart_named_arg_value(arg: Node) -> Option<Node> {
    let mut cursor = arg.walk();
    arg.named_children(&mut cursor)
        .filter(|c| c.kind() != "label")
        .last()
}

/// Derive a Dart call's carrier from its callee.
///
/// Plain `identifier` → its text (`fetch`). A `member_expression` /
/// `null_aware_member_expression` (`dio.get`, `db.rawQuery`) → the
/// `object.property` join so dotted client APIs match config (`dio.get`) while
/// bare DB verbs (`rawQuery`/`execute`) match any receiver via the gate's
/// last-segment rule. `instantiation_expression` (`foo<T>(...)`) unwraps to its
/// inner callee.
fn dart_carrier(function_node: Node) -> Option<String> {
    match function_node.kind() {
        "identifier" => Some(get_node_text(&function_node)),
        "member_expression" | "null_aware_member_expression" => {
            let object = function_node
                .child_by_field_name("object")
                .map(|n| get_node_text(&n));
            let property = function_node
                .child_by_field_name("property")
                .map(|n| get_node_text(&n));
            match (object, property) {
                (Some(o), Some(p)) => Some(format!("{o}.{p}")),
                (None, Some(p)) => Some(p),
                _ => None,
            }
        }
        "instantiation_expression" => function_node
            .child_by_field_name("function")
            .and_then(dart_carrier),
        _ => {
            let text = get_node_text(&function_node);
            if text.is_empty() { None } else { Some(text) }
        }
    }
}
