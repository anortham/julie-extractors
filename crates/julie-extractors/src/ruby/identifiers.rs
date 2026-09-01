use super::helpers::{extract_method_name_from_call, is_assignment_target};
use super::type_facts;
use crate::base::{BaseExtractor, Identifier, IdentifierKind, Symbol};
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};
use std::collections::HashMap;
use tree_sitter::{Node, Tree};

/// Extract all identifier usages (function calls, member access, etc.)
/// Following the Rust extractor reference implementation pattern
pub(super) fn extract_identifiers(
    base: &mut BaseExtractor,
    tree: &Tree,
    symbols: &[Symbol],
) -> Vec<Identifier> {
    // Create symbol map for fast lookup
    let symbol_map: HashMap<String, &Symbol> = symbols.iter().map(|s| (s.id.clone(), s)).collect();

    // Walk the tree and extract identifiers
    walk_tree_for_identifiers(base, tree.root_node(), &symbol_map, 0);

    // Return the collected identifiers
    base.identifiers.clone()
}

/// Recursively walk tree extracting identifiers from each node
fn walk_tree_for_identifiers(
    base: &mut BaseExtractor,
    node: Node,
    symbol_map: &HashMap<String, &Symbol>,
    depth: u32,
) {
    if !should_visit_tree_depth(depth) {
        return;
    }

    // Extract identifier from this node if applicable
    extract_identifier_from_node(base, node, symbol_map);

    // Recursively walk children
    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk_tree_for_identifiers(base, child, symbol_map, child_depth);
    }
}

/// Extract identifier from a single node based on its kind
/// Ruby-specific: "call" nodes are used for both function calls and member access
fn extract_identifier_from_node(
    base: &mut BaseExtractor,
    node: Node,
    symbol_map: &HashMap<String, &Symbol>,
) {
    match node.kind() {
        // Ruby uses "call" for both function calls and member access
        // The difference is whether there's a receiver field
        "call" => {
            // Check if this call has a receiver (member access)
            if let Some(_receiver) = node.child_by_field_name("receiver") {
                if let Some(method_node) = node.child_by_field_name("method") {
                    let name = base.get_node_text(&method_node);
                    let containing_symbol_id = find_containing_symbol_id(base, node, symbol_map);
                    let receiver_type = type_facts::self_receiver_type(base, node);

                    base.create_identifier_with_receiver_type(
                        &method_node,
                        name,
                        IdentifierKind::MemberAccess,
                        containing_symbol_id,
                        receiver_type,
                    );
                }
            } else {
                // This is a simple function call (no receiver)
                // Extract the method/function name
                if let Some(name) = extract_method_name_from_call(node, |n| base.get_node_text(n)) {
                    // Find the identifier node for proper location
                    let mut cursor = node.walk();
                    for child in node.children(&mut cursor) {
                        if child.kind() == "identifier" {
                            let containing_symbol_id =
                                find_containing_symbol_id(base, node, symbol_map);

                            base.create_identifier(
                                &child,
                                name.clone(),
                                IdentifierKind::Call,
                                containing_symbol_id,
                            );
                            break;
                        }
                    }
                }
            }
            // Phase 3b: capture string-literal call-arguments config-free; the
            // carrier classification + bloat gate run later in the artifact language-policy pass.
            record_ruby_call_arg_literals(base, node, symbol_map);
        }

        // Type references: superclass, scope_resolution, include/extend args, etc.
        // Ruby constants are always PascalCase class/module names, so any constant
        // in a reference position (not a declaration name or assignment target) is
        // a type usage that contributes to centrality scoring.
        "constant" => {
            // Skip declaration names: the `name` field of class/module nodes
            if is_constant_declaration_name(&node) {
                return;
            }
            // Skip assignment LHS: `CONST = value` defines, not references
            if is_assignment_target(&node) {
                return;
            }

            let name = base.get_node_text(&node);
            let containing_symbol_id = find_containing_symbol_id(base, node, symbol_map);

            base.create_identifier(&node, name, IdentifierKind::TypeUsage, containing_symbol_id);
        }

        // `variable_ref` complement arm (locked contract — see the doc comment in
        // csharp/identifiers.rs): a bare `identifier` used as a value or as the
        // receiver of a call — the reads the Call/MemberAccess/TypeUsage arms
        // above do not own.
        //
        // Ruby boundary: a bare lowercase identifier in value position can be a
        // local read OR a receiverless zero-arg method call. tree-sitter-ruby
        // yields a `call` node only when parens/args/blocks/receivers are
        // present (owned by the Call arm above); the bare-`identifier`
        // complement lands here, making both meanings name-visible. Constants
        // are `constant` nodes and stay owned by the TypeUsage arm; `self`/
        // `nil`/`true`/`false` are distinct grammar nodes and never reach here.
        "identifier" if is_ruby_value_read_identifier(node) => {
            let name = base.get_node_text(&node);
            // Rule 5: Ruby has no builtin-type filter to reuse; the only
            // keyword-like bare identifiers worth filtering are the visibility
            // modifiers, which appear in statement position in nearly every
            // class and are never symbol names.
            if !matches!(
                name.as_str(),
                "private" | "protected" | "public" | "module_function"
            ) {
                let containing_symbol_id = find_containing_symbol_id(base, node, symbol_map);

                base.create_identifier(
                    &node,
                    name,
                    IdentifierKind::VariableRef,
                    containing_symbol_id,
                );
            }
        }

        _ => {
            // Skip other node types for now
        }
    }
}

/// Rule 1/4 predicate for the `variable_ref` arm: is this bare `identifier` a
/// value read or a call-receiver read (the complement of the Call/MemberAccess/
/// TypeUsage arms)? Inclusive by default with enumerated exclusions, mirroring
/// `is_csharp_value_read_identifier`. Node kinds and field names verified
/// against the vendored tree-sitter-ruby 0.23.1 grammar.
fn is_ruby_value_read_identifier(node: Node) -> bool {
    let Some(parent) = node.parent() else {
        return false;
    };
    let is_name_field = parent.child_by_field_name("name").map(|n| n.id()) == Some(node.id());

    match parent.kind() {
        // Rule 1/2: only the receiver of a `call` is our read; the `method`
        // name is owned by the Call/MemberAccess arms.
        "call" => parent.child_by_field_name("receiver").map(|r| r.id()) == Some(node.id()),
        // Rule 4: a plain assignment LHS is write-only — and it is also how Ruby
        // declares locals, so this covers rule 3 for variables.
        "assignment" => parent.child_by_field_name("left").map(|l| l.id()) != Some(node.id()),
        // Rule 4: a compound assignment (`x += 1`, `y ||= total`) reads both sides.
        "operator_assignment" => true,
        // Rule 4: multiple-assignment targets are writes.
        "left_assignment_list" | "destructured_left_assignment" | "rest_assignment" => false,
        // Rule 3: parameter declarations (an optional/keyword default VALUE is a read).
        "method_parameters"
        | "block_parameters"
        | "lambda_parameters"
        | "splat_parameter"
        | "hash_splat_parameter"
        | "block_parameter" => false,
        "optional_parameter" | "keyword_parameter" => !is_name_field,
        // Rule 3: method definition names.
        "method" | "singleton_method" => !is_name_field,
        // Rule 4: the `for` loop pattern binds; the iterated value is a read.
        "for" => parent.child_by_field_name("pattern").map(|p| p.id()) != Some(node.id()),
        // `rescue … => err` binds err.
        "exception_variable" => false,
        // Rule 3: alias/undef operate on method-name positions, not values.
        "alias" | "undef" => false,
        // Every other value slot — argument, array element, ternary/condition
        // operand, binary operand, return value, interpolation — is a read.
        _ => true,
    }
}

/// Returns true if this constant node is the declaration name of a class or module.
/// Example: `class Foo` or `module Bar` — the `Foo`/`Bar` constant is a declaration,
/// not a reference. All other constant positions are type references.
fn is_constant_declaration_name(node: &Node) -> bool {
    if let Some(parent) = node.parent()
        && let Some(name_node) = parent.child_by_field_name("name")
        && name_node.id() == node.id()
    {
        return matches!(parent.kind(), "class" | "module");
    }
    false
}

/// Find the ID of the symbol that contains this node
/// CRITICAL: Only search symbols from THIS FILE (file-scoped filtering)
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

/// Capture string-literal arguments of a Ruby `call` as `Literal` records.
///
/// Config-free: `carrier` is the verbatim callee — the bare `method` name for a
/// receiverless call (`execute("…")`), or the `receiver.method` join for a
/// member call (`Net::HTTP.get`, `conn.execute`). `kind` stays `Other`; the
/// `src/` carrier gate sets the authoritative kind and drops non-carrier
/// literals. `arg_position` counts over the full argument list. Keyword/hash
/// args (`url: "…"`) are `pair` nodes, so the loop descends to a `value` field
/// when present.
fn record_ruby_call_arg_literals(
    base: &mut BaseExtractor,
    call_node: Node,
    symbol_map: &HashMap<String, &Symbol>,
) {
    let Some(args_node) = call_node.child_by_field_name("arguments") else {
        return;
    };
    let carrier = ruby_carrier(base, call_node);
    let containing_symbol_id = find_containing_symbol_id(base, call_node, symbol_map);

    let mut cursor = args_node.walk();
    for (pos, arg) in args_node.named_children(&mut cursor).enumerate() {
        // Keyword/hash args (`key: value`) hold the literal in their `value`
        // field; positional string args have no `value` field, so use the arg.
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

/// Derive a Ruby call's carrier from its `receiver`/`method` fields.
///
/// Plain receiverless call → bare `method` text (`execute`). Member call →
/// `receiver.method` join so dotted client APIs match config exactly
/// (`Net::HTTP.get`) and local-variable receivers still match a bare method
/// config (`execute`) via the gate's last-segment rule (`conn.execute` →
/// `execute`).
fn ruby_carrier(base: &BaseExtractor, call_node: Node) -> Option<String> {
    let method = call_node
        .child_by_field_name("method")
        .map(|n| base.get_node_text(&n));
    let receiver = call_node
        .child_by_field_name("receiver")
        .map(|n| base.get_node_text(&n));
    match (receiver, method) {
        (Some(r), Some(m)) => Some(format!("{r}.{m}")),
        (None, Some(m)) => Some(m),
        _ => None,
    }
}
