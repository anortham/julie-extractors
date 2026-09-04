use super::helpers;
use super::type_facts;
/// Identifier extraction for LSP-quality find_references
///
/// Extracts all identifier usages:
/// - Function calls: `foo()`, `require("module")`
/// - Method calls with colon syntax: `obj:method()`
/// - Member access: `obj.field`, `obj.field.nested`
use crate::base::{BaseExtractor, ContainingSymbolIndex, Identifier, IdentifierKind, Symbol};
use crate::lua::LuaExtractor;
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};
use tree_sitter::{Node, Tree};

/// Extract all identifier usages (function calls, member access, etc.)
/// Following the Rust extractor reference implementation pattern
pub(super) fn extract_identifiers(
    extractor: &mut LuaExtractor,
    tree: &Tree,
    symbols: &[Symbol],
) -> Vec<Identifier> {
    let containing_symbols = extractor.base().containing_symbol_index(symbols);

    // Walk the tree and extract identifiers
    walk_tree_for_identifiers(extractor, tree.root_node(), &containing_symbols, 0);

    // Return the collected identifiers
    extractor.base().identifiers.clone()
}

/// Recursively walk tree extracting identifiers from each node
fn walk_tree_for_identifiers(
    extractor: &mut LuaExtractor,
    node: Node,
    containing_symbols: &ContainingSymbolIndex<'_>,
    depth: u32,
) {
    if !should_visit_tree_depth(depth) {
        return;
    }

    // Extract identifier from this node if applicable
    extract_identifier_from_node(extractor, node, containing_symbols);

    // Recursively walk children
    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk_tree_for_identifiers(extractor, child, containing_symbols, child_depth);
    }
}

/// Extract identifier from a single node based on its kind
fn extract_identifier_from_node(
    extractor: &mut LuaExtractor,
    node: Node,
    containing_symbols: &ContainingSymbolIndex<'_>,
) {
    match node.kind() {
        // Function calls: foo(), require("module")
        "function_call" => {
            // Try to get the function name from the identifier child
            if let Some(name_node) = helpers::find_child_by_type(&node, "identifier") {
                let name = extractor.base().get_node_text(&name_node);
                let containing_symbol_id = find_containing_symbol_id(node, containing_symbols);

                extractor.base_mut().create_identifier(
                    &name_node,
                    name,
                    IdentifierKind::Call,
                    containing_symbol_id,
                );
            }
            // If no direct identifier, check for dot_index_expression (like math.sqrt())
            else if let Some(dot_index) =
                helpers::find_child_by_type(&node, "dot_index_expression")
            {
                // Extract the rightmost identifier (the method name)
                if let Some(_method_node) = helpers::find_child_by_type(&dot_index, "identifier") {
                    // Get all identifiers and use the last one (rightmost)
                    let mut cursor = dot_index.walk();
                    let identifiers: Vec<Node> = dot_index
                        .children(&mut cursor)
                        .filter(|c| c.kind() == "identifier")
                        .collect();

                    if let Some(last_identifier) = identifiers.last() {
                        let name = extractor.base().get_node_text(last_identifier);
                        let containing_symbol_id =
                            find_containing_symbol_id(node, containing_symbols);
                        let receiver_type = type_facts::call_receiver_type(extractor.base(), node);

                        extractor.base_mut().create_identifier_with_receiver_type(
                            last_identifier,
                            name,
                            IdentifierKind::Call,
                            containing_symbol_id,
                            receiver_type,
                        );
                    }
                }
            }
            // Phase 3b: capture string-literal call-arguments config-free; the
            // carrier classification + bloat gate run later in the artifact language-policy pass.
            record_lua_call_arg_literals(extractor, node, containing_symbols);
        }

        // Method calls with colon syntax: obj:method()
        "method_index_expression" => {
            // Extract the method name (rightmost identifier)
            let mut cursor = node.walk();
            let identifiers: Vec<Node> = node
                .children(&mut cursor)
                .filter(|c| c.kind() == "identifier")
                .collect();

            if let Some(method_node) = identifiers.last() {
                let name = extractor.base().get_node_text(method_node);
                let containing_symbol_id = find_containing_symbol_id(node, containing_symbols);
                let receiver_type = type_facts::call_receiver_type(extractor.base(), node);

                extractor.base_mut().create_identifier_with_receiver_type(
                    method_node,
                    name,
                    IdentifierKind::Call,
                    containing_symbol_id,
                    receiver_type,
                );
            }
        }

        // Member access with dot: obj.field, obj.field.nested
        "dot_index_expression" => {
            // Only extract if it's NOT part of a function_call or method_index_expression
            // (we handle those in the cases above)
            if let Some(parent) = node.parent()
                && (parent.kind() == "function_call" || parent.kind() == "method_index_expression")
            {
                return; // Skip - handled by function/method call
            }

            // Extract the rightmost identifier (the member name)
            let mut cursor = node.walk();
            let identifiers: Vec<Node> = node
                .children(&mut cursor)
                .filter(|c| c.kind() == "identifier")
                .collect();

            if let Some(member_node) = identifiers.last() {
                let name = extractor.base().get_node_text(member_node);
                let containing_symbol_id = find_containing_symbol_id(node, containing_symbols);

                extractor.base_mut().create_identifier(
                    member_node,
                    name,
                    IdentifierKind::MemberAccess,
                    containing_symbol_id,
                );
            }
        }

        // `variable_ref` complement arm (locked contract — see the doc comment
        // in csharp/identifiers.rs): a bare `identifier` used as a value or as
        // the table receiver of a dot/method index — the reads the Call/
        // MemberAccess arms above do not own. `nil`/`true`/`false` are distinct
        // grammar nodes and never reach here.
        "identifier" if is_lua_value_read_identifier(node) => {
            let name = extractor.base().get_node_text(&node);
            // Rule 5: `self` is a receiver convention, never a symbol name.
            if name != "self" {
                let containing_symbol_id = find_containing_symbol_id(node, containing_symbols);
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
            // Future: type usage, import statements, etc.
        }
    }
}

/// Rule 1/4 predicate for the `variable_ref` arm: is this bare `identifier` a
/// value read or a table-receiver read (the complement of the Call/MemberAccess
/// arms)? Inclusive by default with enumerated exclusions, mirroring
/// `is_csharp_value_read_identifier`. Node kinds and field names verified
/// against the vendored tree-sitter-lua 0.5.0 grammar.
fn is_lua_value_read_identifier(node: Node) -> bool {
    let Some(parent) = node.parent() else {
        return false;
    };
    let is_name_field = parent.child_by_field_name("name").map(|n| n.id()) == Some(node.id());

    match parent.kind() {
        // Rule 2: the callee is owned by the Call arm; call arguments sit inside
        // an `arguments` node, not directly under `function_call`.
        "function_call" => false,
        // Rule 1/2: only the `table` receiver of a dot/method index is our read;
        // the accessed field/method name is owned by the Call/MemberAccess arms.
        // This also fires for `function Obj.helper()` declarations, where `Obj`
        // is a genuine table read that keeps the table name-live.
        "dot_index_expression" | "method_index_expression" => {
            parent.child_by_field_name("table").map(|t| t.id()) == Some(node.id())
        }
        // Rule 4: `variable_list` is the LHS of assignment_statement — Lua's
        // plain write AND its declaration form (`local x = 5`). Lua has no
        // compound assignment, so every direct LHS identifier is write-only.
        // (Generic-for loop variables also sit in a `variable_list`.)
        "variable_list" => false,
        // Rule 3: parameter and function declaration names.
        "parameters" => false,
        "function_declaration" | "function_definition" => false,
        // Rule 4: the numeric-for loop variable binds; start/end/step are reads.
        "for_numeric_clause" => !is_name_field,
        // Rule 3: labels are not values.
        "goto_statement" | "label_statement" => false,
        // A table-constructor field: `key = v` has a syntactic key (skip); a
        // computed `[k] = v` key is a read; the value side is always a read.
        "field" if is_name_field => node
            .prev_sibling()
            .map(|s| s.kind() == "[")
            .unwrap_or(false),
        // Every other value slot — expression_list element, argument, binary
        // operand, condition, return value — is a read.
        _ => true,
    }
}

/// Find the ID of the symbol that contains this node
/// CRITICAL: Only search symbols from THIS FILE (file-scoped filtering)
fn find_containing_symbol_id(
    node: Node,
    containing_symbols: &ContainingSymbolIndex<'_>,
) -> Option<String> {
    containing_symbols.find(node).map(|s| s.id.clone())
}

// ============================================================================
// String-literal call-argument capture (Miller bridge Phase 3b)
// ============================================================================

/// Capture string-literal arguments of a Lua `function_call` as `Literal`
/// records.
///
/// Config-free: `carrier` is the verbatim callee — a bare `identifier`
/// (`load`), or the `table.field`/`table.method` join for a
/// `dot_index_expression` (`http.request`) / `method_index_expression`
/// (`conn:execute` → `conn.execute`). `kind` stays `Other`; the `src/` carrier
/// gate sets the authoritative kind and drops non-carrier literals.
/// `arg_position` counts over the full argument list.
fn record_lua_call_arg_literals(
    extractor: &mut LuaExtractor,
    call_node: Node,
    containing_symbols: &ContainingSymbolIndex<'_>,
) {
    let Some(args_node) = call_node.child_by_field_name("arguments") else {
        return;
    };
    let carrier = lua_carrier(extractor.base(), call_node);
    let containing_symbol_id = find_containing_symbol_id(call_node, containing_symbols);

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

/// Derive a Lua `function_call`'s carrier from its `name` field.
///
/// `identifier` → bare name. `dot_index_expression` (`http.request`) →
/// `table.field`. `method_index_expression` (`conn:execute`) → `table.method`
/// (joined with `.` so the gate's last-segment rule matches a bare `execute`
/// config and a dotted `http.request` config matches exactly).
fn lua_carrier(base: &BaseExtractor, call_node: Node) -> Option<String> {
    let name = call_node.child_by_field_name("name")?;
    match name.kind() {
        "identifier" => Some(base.get_node_text(&name)),
        "dot_index_expression" => join_receiver_member(
            name.child_by_field_name("table")
                .map(|n| base.get_node_text(&n)),
            name.child_by_field_name("field")
                .map(|n| base.get_node_text(&n)),
        ),
        "method_index_expression" => join_receiver_member(
            name.child_by_field_name("table")
                .map(|n| base.get_node_text(&n)),
            name.child_by_field_name("method")
                .map(|n| base.get_node_text(&n)),
        ),
        _ => {
            let text = base.get_node_text(&name);
            if text.is_empty() { None } else { Some(text) }
        }
    }
}

/// Join a `receiver` and `member` into a `receiver.member` carrier, tolerating a
/// missing receiver.
fn join_receiver_member(receiver: Option<String>, member: Option<String>) -> Option<String> {
    match (receiver, member) {
        (Some(r), Some(m)) => Some(format!("{r}.{m}")),
        (None, Some(m)) => Some(m),
        _ => None,
    }
}
