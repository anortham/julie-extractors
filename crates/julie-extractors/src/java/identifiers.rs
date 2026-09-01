/// Identifier extraction for LSP-quality find_references
use crate::base::{BaseExtractor, Identifier, IdentifierKind, Symbol, extract_type_arguments};
use crate::java::JavaExtractor;
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};
use std::collections::HashMap;
use tree_sitter::{Node, Tree};

use super::helpers;

/// Extract all identifier usages (function calls, member access, etc.)
/// Following the Rust extractor reference implementation pattern
pub(super) fn extract_identifiers(
    extractor: &mut JavaExtractor,
    tree: &Tree,
    symbols: &[Symbol],
) -> Vec<Identifier> {
    // Create symbol map for fast lookup
    let symbol_map: HashMap<String, &Symbol> = symbols.iter().map(|s| (s.id.clone(), s)).collect();

    // Walk the tree and extract identifiers
    walk_tree_for_identifiers(extractor, tree.root_node(), &symbol_map, 0);

    // Return the collected identifiers
    extractor.base().identifiers.clone()
}

/// Recursively walk tree extracting identifiers from each node
fn walk_tree_for_identifiers(
    extractor: &mut JavaExtractor,
    node: Node,
    symbol_map: &HashMap<String, &Symbol>,
    depth: u32,
) {
    if !should_visit_tree_depth(depth) {
        return;
    }

    // Extract identifier from this node if applicable
    extract_identifier_from_node(extractor, node, symbol_map);

    // Recursively walk children
    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk_tree_for_identifiers(extractor, child, symbol_map, child_depth);
    }
}

/// Extract identifier from a single node based on its kind
fn extract_identifier_from_node(
    extractor: &mut JavaExtractor,
    node: Node,
    symbol_map: &HashMap<String, &Symbol>,
) {
    match node.kind() {
        // Method calls: foo(), bar.baz(), System.out.println()
        "method_invocation" => {
            // Try to get the method name from the "name" field (standard tree-sitter pattern)
            if let Some(name_node) = node.child_by_field_name("name") {
                let name = extractor.base().get_node_text(&name_node);
                let containing_symbol_id = find_containing_symbol_id(extractor, node, symbol_map);
                let receiver_type = self_receiver_type(extractor.base(), node);

                let identifier = extractor.base_mut().create_identifier_with_receiver_type(
                    &name_node,
                    name,
                    IdentifierKind::Call,
                    containing_symbol_id,
                    receiver_type,
                );
                // Generic method calls: `list.<String>stream()` carry a `type_arguments`
                // field directly on the method_invocation node.
                if let Some(type_args) = node.child_by_field_name("type_arguments") {
                    let arguments = extract_type_arguments(
                        extractor.base(),
                        type_args,
                        decompose_java_type_arg,
                    );
                    extractor
                        .base_mut()
                        .record_type_arguments(&identifier, arguments);
                }
            } else {
                // Fallback: look for identifier children
                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    if child.kind() == "identifier" {
                        let name = extractor.base().get_node_text(&child);
                        let containing_symbol_id =
                            find_containing_symbol_id(extractor, node, symbol_map);
                        let receiver_type = self_receiver_type(extractor.base(), node);

                        extractor.base_mut().create_identifier_with_receiver_type(
                            &child,
                            name,
                            IdentifierKind::Call,
                            containing_symbol_id,
                            receiver_type,
                        );
                        break;
                    }
                }
            }
            // Phase 3b: capture string-literal call-arguments config-free; the
            // carrier classification + bloat gate run later in the artifact language-policy pass.
            record_java_call_arg_literals(extractor, node, symbol_map);
        }

        // Field access: object.field
        //
        // This fires for a field_access ANYWHERE, including as the `object`
        // (receiver chain) of a method_invocation: in
        // `com.acme.GraphTraversal.reach()` the terminal receiver
        // `GraphTraversal` is the `field` of that object chain and this arm is
        // the only thing that makes it name-visible (the Call arm emits only
        // the invocation's `name`, never the receiver chain, so there is no
        // double-emission). An earlier `parent == method_invocation` early
        // return dropped exactly that row, making a class referenced ONLY via
        // fully-qualified static calls look dead to name-liveness (fix round 1,
        // adversarial-review finding). This also mirrors the C# reference arm,
        // where the `name` of a receiver member_access_expression is emitted.
        "field_access" => {
            // Extract the rightmost identifier (the field name)
            if let Some(name_node) = node.child_by_field_name("field") {
                let name = extractor.base().get_node_text(&name_node);
                let containing_symbol_id = find_containing_symbol_id(extractor, node, symbol_map);

                extractor.base_mut().create_identifier(
                    &name_node,
                    name,
                    IdentifierKind::MemberAccess,
                    containing_symbol_id,
                );
            }
        }

        // Type references: Gson gson, TypeAdapter<T> adapter, List<JsonElement>, etc.
        // Java tree-sitter uses `type_identifier` for BOTH declaration names
        // (class Foo, interface Foo) AND reference positions (Gson gson).
        // We only want references — declarations are filtered by parent context.
        "type_identifier" => {
            // Skip if this is a declaration name, not a type reference.
            if is_type_declaration_name(&node) {
                return;
            }

            let name = extractor.base().get_node_text(&node);

            // Skip single-letter generics — they carry no cross-file signal.
            if is_java_noise_type(&name) {
                return;
            }

            let containing_symbol_id = find_containing_symbol_id(extractor, node, symbol_map);

            let identifier = extractor.base_mut().create_identifier(
                &node,
                name,
                IdentifierKind::TypeUsage,
                containing_symbol_id,
            );
            // If this type_identifier is the name of a `generic_type` use site
            // (e.g. `List` in `List<String>`), record the ordered type arguments.
            // Nested generics are skipped here — they ride along as `children`.
            record_outermost_java_type_arguments(extractor, node, &identifier);
        }

        // `variable_ref` complement arm (locked contract — see the reference
        // implementation doc comment in csharp/identifiers.rs): a bare `identifier`
        // used as a value or as the object/receiver of a member access — the reads
        // the Call/MemberAccess/TypeUsage arms above do not own. Java type positions
        // are `type_identifier` nodes, so they never reach this arm.
        "identifier" if is_java_value_read_identifier(node) => {
            let name = extractor.base().get_node_text(&node);
            // Rule 5: reuse the existing noise filter. (`this`/`super`/`true`/
            // `null` and primitive types are distinct grammar nodes, never
            // `identifier`, so they are structurally excluded.)
            if !is_java_noise_type(&name) {
                let containing_symbol_id = find_containing_symbol_id(extractor, node, symbol_map);
                extractor.base_mut().create_identifier(
                    &node,
                    name,
                    IdentifierKind::VariableRef,
                    containing_symbol_id,
                );
            }
        }

        _ => {
            // Skip other node types for now
            // Future: constructor calls, etc.
        }
    }
}

/// Rule 1/4 predicate for the `variable_ref` arm: is this bare `identifier` a
/// value read or a member-access receiver (the complement of the Call/
/// MemberAccess/TypeUsage arms)? Node kinds and field names were verified
/// empirically against the vendored tree-sitter-java 0.23.5 grammar.
fn is_java_value_read_identifier(node: Node) -> bool {
    let Some(parent) = node.parent() else {
        return false;
    };
    let is_field = |name: &str| parent.child_by_field_name(name).map(|n| n.id()) == Some(node.id());

    match parent.kind() {
        // Rule 2: the callee `name` of a call is owned by the Call arm and the
        // accessed `field` by the MemberAccess arm; only the `object` receiver
        // is a read this arm owns.
        "method_invocation" | "field_access" => is_field("object"),

        // Rule 3: declaration names. Their NON-name identifier children (a
        // declarator initializer value, the enhanced-for collection) fall
        // through as reads.
        "class_declaration"
        | "interface_declaration"
        | "enum_declaration"
        | "annotation_type_declaration"
        | "record_declaration"
        | "method_declaration"
        | "constructor_declaration"
        | "compact_constructor_declaration"
        | "annotation_type_element_declaration"
        | "variable_declarator"
        | "formal_parameter"
        | "spread_parameter"
        | "catch_formal_parameter"
        | "enhanced_for_statement"
        | "enum_constant"
        | "type_parameter"
        | "module_declaration" => !is_field("name"),

        // Rule 3: lambda parameters are definitions; the body falls through as
        // reads via other parents.
        "lambda_expression" => !is_field("parameters"),
        "inferred_parameters" => false,

        // Rule 3: `instanceof` pattern bindings (`x instanceof Foo f`).
        "instanceof_expression" => !is_field("name"),

        // Rule 3: package/import segments and qualified names are not reads.
        "scoped_identifier" | "package_declaration" | "import_declaration" => false,

        // Rule 2: an annotation's name is a type-ish usage, not a value read.
        // (`element_value_pair` keys — annotation named args — fall through to
        // the default read arm: they ARE member references per the contract.)
        "annotation" | "marker_annotation" => false,

        // Rule 3: labels are not variables.
        "labeled_statement" | "break_statement" | "continue_statement" => false,

        // Rule 4: the LHS of a PLAIN assignment is write-only; a COMPOUND
        // operator (`+=`, `|=`, …) reads. The RHS is always a read.
        "assignment_expression" => {
            !is_field("left")
                || parent
                    .child_by_field_name("operator")
                    .map(|op| op.kind() != "=")
                    .unwrap_or(false)
        }

        // Every other position — argument, operand, return value, ternary arm,
        // array element/index, switch label constant, method-reference member,
        // update expression (`i++` reads), annotation named-arg key — is a read.
        _ => true,
    }
}

/// Check if a `type_identifier` node is a declaration name rather than a type reference.
///
/// In Java tree-sitter, `type_identifier` appears as the `name` field of:
/// - `class_declaration` → `class Foo {}` (declaration)
/// - `interface_declaration` → `interface Foo {}` (declaration)
/// - `enum_declaration` → `enum Foo {}` (declaration)
/// - `annotation_type_declaration` → `@interface Foo {}` (declaration)
/// - `type_parameter` → `<T extends Base>` (the `T` is a declaration)
fn is_type_declaration_name(node: &Node) -> bool {
    if let Some(parent) = node.parent() {
        // Check if this node is the `name` field of a declaration or type param
        if let Some(name_node) = parent.child_by_field_name("name")
            && name_node.id() == node.id()
        {
            return matches!(
                parent.kind(),
                "class_declaration"
                    | "interface_declaration"
                    | "enum_declaration"
                    | "annotation_type_declaration"
                    | "type_parameter"
            );
        }
    }
    false
}

/// Returns true for Java types that are too common/noisy to be meaningful
/// type references for centrality scoring.
///
/// Only filters single-letter generics (T, K, V, E, R, etc.) which carry no
/// cross-file signal. Does NOT filter standard library types (String, Integer,
/// List, Map, etc.) because:
/// 1. User-defined types with those names must be trackable
/// 2. Builtin references to non-existent symbols cause zero centrality impact
///    anyway (Step 1b only boosts symbols in the symbols table)
fn is_java_noise_type(name: &str) -> bool {
    // Single-letter names are almost always generic type parameters used in scope.
    // Even when they appear as references (e.g. `: T`), they carry no cross-file signal.
    name.len() == 1 && name.chars().next().is_some_and(|c| c.is_ascii_uppercase())
}

/// Record outermost generic type arguments for a `type_identifier` node.
///
/// Fires when `name_node` is the type-name child of a `generic_type` node
/// (e.g. `List` in `List<String>`), but only if that `generic_type` is not
/// itself nested inside a `type_arguments` list (i.e. it is the outermost use
/// site). Nested generics like `List` in `Map<String, List<Integer>>` are
/// captured as `children` of the outer usage, not as separate rows.
fn record_outermost_java_type_arguments(
    extractor: &mut JavaExtractor,
    name_node: Node,
    identifier: &Identifier,
) {
    let Some(generic_type) = name_node.parent() else {
        return;
    };
    if generic_type.kind() != "generic_type" {
        return;
    }
    // A `generic_type` whose parent is `type_arguments` is itself nested inside
    // another generic — its args ride along under the outer usage as `children`.
    if generic_type
        .parent()
        .map(|p| p.kind() == "type_arguments")
        .unwrap_or(false)
    {
        return;
    }
    let Some(arg_list) = type_arguments_child(generic_type) else {
        return;
    };
    let arguments = extract_type_arguments(extractor.base(), arg_list, decompose_java_type_arg);
    extractor
        .base_mut()
        .record_type_arguments(identifier, arguments);
}

/// Decompose a single child of a Java `type_arguments` node into a
/// `(type_name, optional_nested_arg_list)` pair for `extract_type_arguments`.
///
/// Java `type_arguments` children may be:
/// - `type_identifier` — a simple reference type (String, Integer, …)
/// - `generic_type`    — a nested generic (List<Integer>)
/// - `wildcard`        — `? extends Foo`, `? super Bar`
/// - primitive/array types — rare as explicit generic args
fn decompose_java_type_arg<'a>(
    base: &BaseExtractor,
    node: Node<'a>,
) -> Option<(String, Option<Node<'a>>)> {
    if !node.is_named() {
        return None; // skip commas and punctuation
    }
    match node.kind() {
        "generic_type" => {
            // Nested generic: name comes from the `type_identifier` child.
            let name = {
                let mut cursor = node.walk();
                node.children(&mut cursor)
                    .find(|c| c.kind() == "type_identifier")
                    .map(|n| base.get_node_text(&n))
                    .unwrap_or_else(|| base.get_node_text(&node))
            };
            Some((name, type_arguments_child(node)))
        }
        _ => {
            // type_identifier, wildcard, integral_type, floating_point_type,
            // array_type, scoped_type_identifier, etc. — use full text.
            Some((base.get_node_text(&node), None))
        }
    }
}

/// Find the `type_arguments` child of a `generic_type` node.
fn type_arguments_child(node: Node<'_>) -> Option<Node<'_>> {
    let mut cursor = node.walk();
    node.children(&mut cursor)
        .find(|c| c.kind() == "type_arguments")
}

/// Find the ID of the symbol that contains this node
/// CRITICAL: Only search symbols from THIS FILE (file-scoped filtering)
fn find_containing_symbol_id(
    extractor: &JavaExtractor,
    node: Node,
    symbol_map: &HashMap<String, &Symbol>,
) -> Option<String> {
    extractor
        .base()
        .find_containing_symbol_from_map(&node, symbol_map)
        .map(|s| s.id.clone())
}

// ============================================================================
// String-literal call-argument capture (Miller bridge Phase 3b)
// ============================================================================

/// Capture string-literal arguments of a Java `method_invocation` as `Literal`
/// records.
///
/// Config-free: `carrier` is the verbatim callee — the bare `name` for a
/// receiverless call, or the `object.name` join for a member call
/// (`restTemplate.getForObject`, `st.execute`). `kind` stays `Other`; the
/// `src/` carrier gate sets the authoritative kind and drops non-carrier
/// literals. `arg_position` counts over the full argument list.
fn record_java_call_arg_literals(
    extractor: &mut JavaExtractor,
    call_node: Node,
    symbol_map: &HashMap<String, &Symbol>,
) {
    let Some(args_node) = call_node.child_by_field_name("arguments") else {
        return;
    };
    let carrier = java_carrier(extractor.base(), call_node);
    let containing_symbol_id = find_containing_symbol_id(extractor, call_node, symbol_map);

    let mut cursor = args_node.walk();
    for (pos, arg) in args_node.named_children(&mut cursor).enumerate() {
        if let Some(text) = extractor.base().decode_string_literal(&arg) {
            extractor.base_mut().record_literal(
                &arg,
                text,
                carrier.clone(),
                pos as u32,
                containing_symbol_id.clone(),
            );
        }
    }
}

/// Derive a Java `method_invocation`'s carrier from its `object`/`name` fields.
///
/// No `object` (bare call) → the `name` text. With `object` → `object.name` so
/// dotted client APIs match config (`URI.create`) and local-variable receivers
/// still match a bare method config (`execute`, `getForObject`) via the gate's
/// last-segment rule (`st.execute` → `execute`).
fn java_carrier(base: &BaseExtractor, call_node: Node) -> Option<String> {
    let name = call_node
        .child_by_field_name("name")
        .map(|n| base.get_node_text(&n));
    let object = call_node
        .child_by_field_name("object")
        .map(|n| base.get_node_text(&n));
    match (object, name) {
        (Some(o), Some(n)) => Some(format!("{o}.{n}")),
        (None, Some(n)) => Some(n),
        _ => None,
    }
}

pub(super) fn self_receiver_type(base: &BaseExtractor, node: Node) -> Option<String> {
    let object = node.child_by_field_name("object")?;
    match object.kind() {
        "this" => enclosing_type_name(base, node),
        "super" => declared_superclass_name(base, node),
        _ => None,
    }
}

fn enclosing_type_name(base: &BaseExtractor, node: Node) -> Option<String> {
    let mut current = node.parent();
    while let Some(candidate) = current {
        if matches!(
            candidate.kind(),
            "class_declaration"
                | "interface_declaration"
                | "enum_declaration"
                | "record_declaration"
                | "annotation_type_declaration"
        ) {
            return candidate
                .child_by_field_name("name")
                .or_else(|| {
                    candidate
                        .children(&mut candidate.walk())
                        .find(|child| child.kind() == "identifier")
                })
                .map(|name_node| base.get_node_text(&name_node));
        }
        current = candidate.parent();
    }
    None
}

fn declared_superclass_name(base: &BaseExtractor, node: Node) -> Option<String> {
    let mut current = node.parent();
    while let Some(candidate) = current {
        if matches!(
            candidate.kind(),
            "class_declaration" | "enum_declaration" | "record_declaration"
        ) {
            return helpers::extract_superclass(base, candidate);
        }
        current = candidate.parent();
    }
    None
}
