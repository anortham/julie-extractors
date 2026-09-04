//! Identifier extraction for function calls, member access, and type references
//!
//! This module handles extraction of identifier usages within C code, such as function calls,
//! member/field access operations, and type_identifier references (TypeUsage).

use crate::base::{ContainingSymbolIndex, Identifier, IdentifierKind, Symbol};
use crate::c::CExtractor;
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};

/// Extract all identifiers from the syntax tree
pub(super) fn extract_identifiers(
    extractor: &mut CExtractor,
    tree: &tree_sitter::Tree,
    symbols: &[Symbol],
) -> Vec<Identifier> {
    let containing_symbols = extractor.base.containing_symbol_index(symbols);
    walk_tree_for_identifiers(extractor, tree.root_node(), &containing_symbols, 0);
    extractor.base.identifiers.clone()
}

/// Recursively walk tree extracting identifiers from each node
fn walk_tree_for_identifiers(
    extractor: &mut CExtractor,
    node: tree_sitter::Node,
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
    extractor: &mut CExtractor,
    node: tree_sitter::Node,
    containing_symbols: &ContainingSymbolIndex<'_>,
) {
    match node.kind() {
        // Function calls: add(), printf()
        "call_expression" => {
            if let Some(func_node) = node.child_by_field_name("function") {
                let name = extractor.base.get_node_text(&func_node);

                // Find containing symbol (which function contains this call)
                let containing_symbol_id = find_containing_symbol_id(node, containing_symbols);

                // Create identifier for this function call
                extractor.base.create_identifier(
                    &func_node,
                    name,
                    IdentifierKind::Call,
                    containing_symbol_id,
                );
            }
            // Phase 3: capture string-literal call-arguments (config-free; the
            // carrier classification + gate happen in the artifact language-policy pass).
            record_c_call_arg_literals(extractor, node, containing_symbols);
        }

        // Type references: typedef names, struct tags, enum tags in type positions.
        // C's tree-sitter grammar uses `type_identifier` for user-defined types
        // appearing in declarations, parameters, field types, casts, sizeof, etc.
        "type_identifier" => {
            if let Some(parent) = node.parent() {
                let is_definition_site = match parent.kind() {
                    // `struct Foo { ... }` — "Foo" is the tag being defined
                    // But `struct Foo*` in a parameter is a USAGE (struct_specifier
                    // without a body/field_declaration_list child).
                    "struct_specifier" | "union_specifier" => {
                        // It's a definition if the struct/union has a body
                        parent.child_by_field_name("body").is_some()
                    }
                    // `enum Color { ... }` — "Color" is the tag being defined
                    "enum_specifier" => parent.child_by_field_name("body").is_some(),
                    // `typedef int MyInt;` — "MyInt" is the alias being defined.
                    // In C's tree-sitter grammar, the typedef alias is the
                    // `declarator` field of `type_definition`.
                    "type_definition" => {
                        // The type_identifier is the declarator (the new name)
                        node.parent()
                            .and_then(|p| p.child_by_field_name("declarator"))
                            .is_some_and(|d| d.id() == node.id())
                    }
                    _ => false,
                };

                if !is_definition_site {
                    let name = extractor.base.get_node_text(&node);
                    let containing_symbol_id = find_containing_symbol_id(node, containing_symbols);
                    extractor.base.create_identifier(
                        &node,
                        name,
                        IdentifierKind::TypeUsage,
                        containing_symbol_id,
                    );
                }
            }
        }

        // Member/field access: p->x, obj.field
        "field_expression" => {
            // Skip if parent is a call_expression (will be handled as function call)
            if let Some(parent) = node.parent()
                && parent.kind() == "call_expression"
            {
                return;
            }

            // Extract field name from field_expression
            if let Some(field_node) = node.child_by_field_name("field") {
                let name = extractor.base.get_node_text(&field_node);
                let containing_symbol_id = find_containing_symbol_id(node, containing_symbols);

                extractor.base.create_identifier(
                    &field_node,
                    name,
                    IdentifierKind::MemberAccess,
                    containing_symbol_id,
                );
            }
        }

        // `variable_ref` complement arm: a bare `identifier` used as a value or
        // as the object/receiver of a member access — the reads the Call/
        // MemberAccess/TypeUsage arms above do not own. Type positions never
        // reach here (C uses the distinct `type_identifier` kind), and field
        // names are the distinct `field_identifier` kind. See the LOCKED
        // SEMANTIC CONTRACT doc comment in `csharp/identifiers.rs`.
        "identifier" if is_c_value_read_identifier(node) => {
            let name = extractor.base.get_node_text(&node);
            // Rule 5: C keywords (`sizeof`, `return`, ...) and C23 `true`/
            // `false`/`nullptr` are distinct grammar tokens, so the only
            // builtin-flavored names that parse as plain identifiers are the
            // classic stdlib macros filtered here.
            if !is_c_builtin_value_name(&name) {
                let containing_symbol_id = find_containing_symbol_id(node, containing_symbols);
                extractor.base.create_identifier(
                    &node,
                    name,
                    IdentifierKind::VariableRef,
                    containing_symbol_id,
                );
            }
        }

        // Rule 1: a designated-initializer member LHS (`.x` in
        // `struct Point p = { .x = seed }`) is a member reference in an
        // initializer context. The grammar wraps it as
        // `field_designator (field_identifier)`; no other arm owns it.
        "field_identifier" => {
            if let Some(parent) = node.parent()
                && parent.kind() == "field_designator"
            {
                let name = extractor.base.get_node_text(&node);
                let containing_symbol_id = find_containing_symbol_id(node, containing_symbols);
                extractor.base.create_identifier(
                    &node,
                    name,
                    IdentifierKind::VariableRef,
                    containing_symbol_id,
                );
            }
        }

        _ => {}
    }
}

/// Rule 1/4 predicate for C `variable_ref` emission: is this bare `identifier`
/// a value read or a member-access receiver (the complement of the Call/
/// MemberAccess/TypeUsage arms)? Node kinds and field names were verified
/// against the vendored tree-sitter-c grammar (see task probes): declarators
/// carry a `declarator` field, `assignment_expression` carries an anonymous
/// `operator` field (`=`, `+=`, ...), and labels/goto targets are the distinct
/// `statement_identifier` kind so they never reach this predicate.
fn is_c_value_read_identifier(node: tree_sitter::Node) -> bool {
    let Some(parent) = node.parent() else {
        return false;
    };

    // Rule 3: any declarator-position identifier is a declaration name. This
    // one field check covers `init_declarator`, `function_declarator`,
    // `array_declarator`, `pointer_declarator`, `parameter_declaration`, and a
    // bare `declaration declarator: (identifier)` uniformly.
    if parent.child_by_field_name("declarator").map(|d| d.id()) == Some(node.id()) {
        return false;
    }

    match parent.kind() {
        // Rule 2: the callee is owned by the Call arm; arguments live under the
        // separate `argument_list` node and stay reads.
        "call_expression" => {
            parent.child_by_field_name("function").map(|f| f.id()) != Some(node.id())
        }

        // Rule 3: `#define NAME ...` / `#define NAME(args) ...` define NAME and
        // its macro parameters; `enumerator` defines the enum constant. Their
        // non-name children (e.g. an enumerator's explicit value) stay reads.
        "preproc_def" | "preproc_function_def" | "enumerator" => {
            parent.child_by_field_name("name").map(|n| n.id()) != Some(node.id())
        }
        "preproc_params" => false,

        // Meta positions: `[[nodiscard]]` attribute names and the arguments of
        // `__attribute__((...))` are not value reads.
        "attribute" => false,
        "argument_list" => parent
            .parent()
            .map(|gp| gp.kind() != "attribute_specifier")
            .unwrap_or(true),

        // Rule 4: the LHS of a PLAIN assignment (`x = 5`) is write-only; a
        // compound assignment (`x += 1`) reads its target. `x++`/`x--` are the
        // separate `update_expression` kind and fall through as reads.
        "assignment_expression" => {
            let is_left = parent.child_by_field_name("left").map(|n| n.id()) == Some(node.id());
            if !is_left {
                return true;
            }
            parent
                .child_by_field_name("operator")
                .map(|op| op.kind() != "=")
                .unwrap_or(false)
        }

        // Every other position — argument, initializer value, return value,
        // binary operand, subscript, receiver (`argument` of field_expression),
        // sizeof operand, `#if`/`#ifdef` macro condition — is a read.
        _ => true,
    }
}

/// Rule 5 filter: C's TypeUsage arm needs no name filter (builtin types are the
/// distinct `primitive_type` kind), so the only value-position builtins to
/// exclude are the stdlib macro spellings that parse as plain identifiers in
/// pre-C23 code.
fn is_c_builtin_value_name(name: &str) -> bool {
    matches!(name, "NULL" | "true" | "false")
}

/// Find the ID of the symbol that contains this node
/// CRITICAL: Only search symbols from THIS FILE (file-scoped filtering)
fn find_containing_symbol_id(
    node: tree_sitter::Node,
    containing_symbols: &ContainingSymbolIndex<'_>,
) -> Option<String> {
    containing_symbols.find(node).map(|s| s.id.clone())
}

// ============================================================================
// String-literal call-argument capture (Miller bridge Phase 3)
// ============================================================================

/// Capture string-literal arguments of a C `call_expression` as `Literal`
/// records. Config-free: `carrier` is the called function name (or `recv.field`
/// for a function-pointer member call); the URL/SQL classification and the
/// carrier gate run later in the artifact language-policy pass. C has no named-argument
/// wrappers, so each `argument_list` named child is decoded directly.
/// `arg_position` is counted over the full argument list, so e.g. the URL in
/// `curl_easy_setopt(h, CURLOPT_URL, "https://...")` reports position 2.
fn record_c_call_arg_literals(
    extractor: &mut CExtractor,
    node: tree_sitter::Node,
    containing_symbols: &ContainingSymbolIndex<'_>,
) {
    let Some(func_node) = node.child_by_field_name("function") else {
        return;
    };
    let Some(args) = node.child_by_field_name("arguments") else {
        return;
    };
    let carrier = c_carrier(extractor, func_node);
    let containing_symbol_id = find_containing_symbol_id(node, containing_symbols);

    let mut cursor = args.walk();
    for (pos, arg) in args.named_children(&mut cursor).enumerate() {
        if let Some(text) = extractor.base.decode_string_literal(&arg) {
            extractor.base.record_literal(
                &arg,
                text,
                carrier.clone(),
                pos as u32,
                containing_symbol_id.clone(),
            );
        }
    }
}

/// Derive a C call's carrier. Plain `identifier` → its text (`sqlite3_exec`);
/// `field_expression` (`p->fn`, `obj.fn` via function pointer) → the
/// `object.field` join so the gate's last-segment rule can match a bare config.
fn c_carrier(extractor: &CExtractor, func_node: tree_sitter::Node) -> Option<String> {
    match func_node.kind() {
        "identifier" => Some(extractor.base.get_node_text(&func_node)),
        "field_expression" => {
            let object = func_node
                .child_by_field_name("argument")
                .map(|n| extractor.base.get_node_text(&n));
            let field = func_node
                .child_by_field_name("field")
                .map(|n| extractor.base.get_node_text(&n));
            match (object, field) {
                (Some(o), Some(f)) => Some(format!("{o}.{f}")),
                (None, Some(f)) => Some(f),
                _ => None,
            }
        }
        _ => {
            let text = extractor.base.get_node_text(&func_node);
            if text.is_empty() { None } else { Some(text) }
        }
    }
}
