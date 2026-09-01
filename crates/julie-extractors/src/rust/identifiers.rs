/// Rust identifier extraction for LSP-quality reference tracking
/// - Function calls
/// - Variable references
/// - Member access expressions
mod containing_symbols;
mod literals;

use containing_symbols::ContainingSymbolIndex;
use literals::{record_rust_call_arg_literals, record_rust_macro_arg_literals};

use crate::base::{BaseExtractor, Identifier, IdentifierKind, Symbol};
use crate::rust::RustExtractor;
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};
use tree_sitter::Tree;

/// Extract all identifiers (references/usages) for LSP-quality reference tracking
///
/// Phase 1 - basic extraction. We extract:
/// - Function calls (call_expression)
/// - Variable references (identifier nodes in certain contexts)
///
/// Identifiers are stored unresolved (target_symbol_id = None) and resolved
/// on-demand during queries for optimal incremental update performance.
pub(super) fn extract_identifiers(
    extractor: &mut RustExtractor,
    tree: &Tree,
    symbols: &[Symbol],
) -> Vec<Identifier> {
    let file_path = extractor.get_base_mut().file_path.clone();
    let containing_symbols = ContainingSymbolIndex::new(symbols, &file_path);

    walk_tree_for_identifiers(extractor, tree.root_node(), &containing_symbols, 0);

    // Return extracted identifiers from base extractor
    extractor.get_base_mut().identifiers.clone()
}

/// Walk the tree extracting identifiers
fn walk_tree_for_identifiers(
    extractor: &mut RustExtractor,
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

/// Extract identifier from a single node
fn extract_identifier_from_node(
    extractor: &mut RustExtractor,
    node: tree_sitter::Node,
    containing_symbols: &ContainingSymbolIndex<'_>,
) {
    match node.kind() {
        // Function calls: foo(), bar.baz(), foo::<T>() (turbofish)
        "call_expression" => {
            if let Some(func_node) = node.child_by_field_name("function") {
                // Unwrap turbofish: generic_function { function: ..., type_arguments: ... }
                // e.g. `foo::<String>()` or `self.collect::<Vec<u8>>()`
                let (inner_func, turbofish_arg_list) = if func_node.kind() == "generic_function" {
                    let inner = func_node
                        .child_by_field_name("function")
                        .unwrap_or(func_node);
                    let args = func_node.child_by_field_name("type_arguments");
                    (inner, args)
                } else {
                    (func_node, None)
                };

                let name = {
                    let base = extractor.get_base_mut();
                    if inner_func.kind() == "field_expression" {
                        // Method call: extract just the field name
                        if let Some(field_node) = inner_func.child_by_field_name("field") {
                            base.get_node_text(&field_node)
                        } else {
                            base.get_node_text(&inner_func)
                        }
                    } else if inner_func.kind() == "scoped_identifier" {
                        // Qualified call: crate::module::function() → extract "function"
                        if let Some(name_node) = inner_func.child_by_field_name("name") {
                            base.get_node_text(&name_node)
                        } else {
                            base.get_node_text(&inner_func)
                        }
                    } else {
                        // Regular function call (bare identifier)
                        base.get_node_text(&inner_func)
                    }
                };

                let identifier_node = if inner_func.kind() == "field_expression" {
                    if let Some(field_node) = inner_func.child_by_field_name("field") {
                        field_node
                    } else {
                        inner_func
                    }
                } else if inner_func.kind() == "scoped_identifier" {
                    if let Some(name_node) = inner_func.child_by_field_name("name") {
                        name_node
                    } else {
                        inner_func
                    }
                } else {
                    inner_func
                };

                // Find containing symbol (which function/method contains this call)
                let containing_symbol_id = find_containing_symbol_id(node, containing_symbols);

                let identifier = {
                    let receiver_type = self_receiver_type(extractor, node);
                    let base = extractor.get_base_mut();
                    base.create_identifier_with_receiver_type(
                        &identifier_node,
                        name,
                        IdentifierKind::Call,
                        containing_symbol_id,
                        receiver_type,
                    )
                };

                // Record turbofish type arguments (e.g. `foo::<String>()` → (0, "String"))
                if let Some(arg_list) = turbofish_arg_list {
                    let base = extractor.get_base_mut();
                    let arguments = crate::base::extract_type_arguments(
                        base,
                        arg_list,
                        decompose_rust_type_arg,
                    );
                    base.record_type_arguments(&identifier, arguments);
                }
            }
            // Phase 3b: capture string-literal call-arguments (config-free;
            // carrier classification + gate run later in the artifact language-policy pass).
            record_rust_call_arg_literals(extractor, node, containing_symbols);
        }

        // Variable/field references in specific contexts
        // We're conservative - only extract clear variable usages, not all identifiers
        "field_expression" => {
            // Skip if this field_expression is the function of a call_expression
            // (e.g., self.method() - we want "method" as Call, not MemberAccess)
            if let Some(parent) = node.parent()
                && parent.kind() == "call_expression"
                && let Some(func_child) = parent.child_by_field_name("function")
                && func_child.id() == node.id()
            {
                // This field_expression IS the function being called, skip it
                return;
            }

            // object.field - extract the field name (not part of a call)
            if let Some(field_node) = node.child_by_field_name("field") {
                let name = {
                    let base = extractor.get_base_mut();
                    base.get_node_text(&field_node)
                };
                let containing_symbol_id = find_containing_symbol_id(node, containing_symbols);

                {
                    let base = extractor.get_base_mut();
                    base.create_identifier(
                        &field_node,
                        name,
                        IdentifierKind::MemberAccess,
                        containing_symbol_id,
                    );
                }
            }
        }

        "scoped_identifier" | "scoped_type_identifier" => {
            if is_inside_call_function(node) {
                return;
            }

            if let Some(name_node) = node.child_by_field_name("name") {
                let name = {
                    let base = extractor.get_base_mut();
                    base.get_node_text(&name_node)
                };
                let containing_symbol_id = find_containing_symbol_id(node, containing_symbols);

                let identifier = {
                    let base = extractor.get_base_mut();
                    base.create_identifier(
                        &name_node,
                        name,
                        IdentifierKind::TypeUsage,
                        containing_symbol_id,
                    )
                };
                // Record type args when the scoped type is the `type` field of a
                // generic_type: e.g. `std::io::Error<T>` → node.parent() == generic_type
                record_outermost_rust_type_arguments_for_scoped(extractor, node, &identifier);
            }
        }

        "type_identifier" if !is_rust_declaration_type_name(node) => {
            let name = {
                let base = extractor.get_base_mut();
                base.get_node_text(&node)
            };
            let containing_symbol_id = find_containing_symbol_id(node, containing_symbols);

            let identifier = {
                let base = extractor.get_base_mut();
                base.create_identifier(&node, name, IdentifierKind::TypeUsage, containing_symbol_id)
            };
            // Record type args when this identifier is the base of an outermost
            // generic: e.g. `Vec` in `Vec<String>` → parent is generic_type
            record_outermost_rust_type_arguments(extractor, node, &identifier);
        }

        // Macro calls: sqlx `query!`/`query_as!`/`query_scalar!` carry SQL as a
        // string literal inside the macro's token-tree (the dominant Rust SQL
        // form, compile-time checked). Phase 3b captures those string tokens.
        "macro_invocation" => {
            record_rust_macro_arg_literals(extractor, node, containing_symbols);
        }

        // `variable_ref` complement arm: a bare `identifier` used as a value,
        // a field-access receiver, or a scoped-access path receiver (`X` in
        // `X::Y()`) — the reads the Call/MemberAccess/TypeUsage arms above do
        // not own. Rust keywords, `self`, `true`/`false`, and lifetimes are
        // distinct grammar kinds, so no name filter is needed (the TypeUsage
        // arms carry none either). See the LOCKED SEMANTIC CONTRACT doc
        // comment in `csharp/identifiers.rs`.
        "identifier" if is_rust_value_read_identifier(node) => {
            let name = {
                let base = extractor.get_base_mut();
                base.get_node_text(&node)
            };
            let containing_symbol_id = find_containing_symbol_id(node, containing_symbols);
            let base = extractor.get_base_mut();
            base.create_identifier(
                &node,
                name,
                IdentifierKind::VariableRef,
                containing_symbol_id,
            );
        }

        // Rule 1: a struct-literal field name (`bar` in `Sample { bar: seed }`)
        // is a member reference in an initializer context; no other arm owns it
        // (the MemberAccess arm only covers `field_expression`).
        "field_identifier" => {
            if let Some(parent) = node.parent()
                && parent.kind() == "field_initializer"
            {
                let name = {
                    let base = extractor.get_base_mut();
                    base.get_node_text(&node)
                };
                let containing_symbol_id = find_containing_symbol_id(node, containing_symbols);
                let base = extractor.get_base_mut();
                base.create_identifier(
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

/// Rule 1/4 predicate for Rust `variable_ref` emission: is this bare
/// `identifier` a value read, a field-access receiver, or a scoped-access path
/// receiver (the complement of the Call/MemberAccess/TypeUsage arms)? Node
/// kinds and field names were verified against the vendored tree-sitter-rust
/// grammar (see task probes): plain assignment is `assignment_expression`
/// while compound assignment is the separate `compound_assignment_expr` kind
/// (whose target therefore reads by default), patterns bind via dedicated
/// `*_pattern` kinds, and loop labels wrap their identifier in a `label` node.
fn is_rust_value_read_identifier(node: tree_sitter::Node) -> bool {
    let Some(parent) = node.parent() else {
        return false;
    };

    let is_field =
        |field: &str| parent.child_by_field_name(field).map(|f| f.id()) == Some(node.id());

    match parent.kind() {
        // Rule 1/2: in `X::Y` only the leftmost `path` segment is the receiver
        // read; the `name` is owned by the scoped TypeUsage arm (or by the
        // Call arm when the chain is a callee).
        "scoped_identifier" => is_field("path") && rust_scoped_path_is_read_context(parent),
        "scoped_type_identifier" => false,

        // Rule 2: callees are owned by the Call arm (including the inner
        // function of a turbofish `generic_function`).
        "call_expression" | "generic_function" => !is_field("function"),

        // Rule 2 (complement boundary): no arm owns a macro callee today; the
        // Call arm should, so it is NOT emitted here (see open-gaps note).
        // Token-tree interiors ARE reads when the tree belongs to a macro
        // invocation (`println!("{}", macro_only)`), but not in `macro_rules!`
        // bodies or attribute arguments.
        "macro_invocation" => false,
        "token_tree" => rust_token_tree_belongs_to_macro_invocation(node),

        // Rule 3: declaration names.
        "function_item"
        | "function_signature_item"
        | "mod_item"
        | "const_item"
        | "static_item"
        | "enum_variant"
        | "macro_definition"
        | "extern_crate_declaration"
        | "use_as_clause"
        | "label" => false,

        // Rule 3: pattern positions bind rather than read.
        "let_declaration" | "parameter" | "for_expression" => !is_field("pattern"),
        "closure_parameters"
        | "match_pattern"
        | "tuple_struct_pattern"
        | "tuple_pattern"
        | "slice_pattern"
        | "struct_pattern"
        | "reference_pattern"
        | "ref_pattern"
        | "mut_pattern"
        | "captured_pattern"
        | "or_pattern"
        | "range_pattern" => false,

        // Rule 3: import trees and attribute/meta positions are not reads.
        "use_declaration"
        | "use_list"
        | "scoped_use_list"
        | "use_wildcard"
        | "attribute"
        | "attribute_item"
        | "inner_attribute_item"
        | "visibility_modifier" => false,

        // Rule 4: plain-assignment LHS is write-only (compound assignment is
        // the separate `compound_assignment_expr` kind, which reads).
        "assignment_expression" => !is_field("left"),

        // Every other position — argument, binary operand, return value,
        // `field_expression` receiver (`value` field), array element,
        // `shorthand_field_initializer`, match value, index, await/try
        // operand — is a read.
        _ => true,
    }
}

/// Context check for the `path` receiver of a `scoped_identifier`: climb to
/// the outermost `::` chain; a chain that resolves into a type position
/// (`scoped_type_identifier`), an import (`use ...`), an attribute, or a
/// visibility modifier is not a value read. Call-callee chains DO keep their
/// receiver read (`Sample` in `Sample::default_bar()`), mirroring the C#
/// reference where the static-access receiver stays live.
fn rust_scoped_path_is_read_context(mut scoped: tree_sitter::Node) -> bool {
    while let Some(parent) = scoped.parent() {
        match parent.kind() {
            "scoped_identifier" => scoped = parent,
            "scoped_type_identifier" => return false,
            _ => break,
        }
    }
    let Some(owner) = scoped.parent() else {
        return false;
    };
    !matches!(
        owner.kind(),
        "use_declaration"
            | "use_list"
            | "scoped_use_list"
            | "use_as_clause"
            | "use_wildcard"
            | "attribute"
            | "attribute_item"
            | "inner_attribute_item"
            | "visibility_modifier"
            | "mod_item"
    )
}

/// True when a `token_tree` interior identifier belongs to a macro INVOCATION
/// (its tokens are call-site expressions and therefore reads), as opposed to a
/// `macro_rules!` body or an attribute argument list.
fn rust_token_tree_belongs_to_macro_invocation(node: tree_sitter::Node) -> bool {
    let mut current = node;
    while let Some(parent) = current.parent() {
        match parent.kind() {
            "token_tree" => current = parent,
            "macro_invocation" => return true,
            _ => return false,
        }
    }
    false
}

fn is_rust_declaration_type_name(node: tree_sitter::Node) -> bool {
    let Some(parent) = node.parent() else {
        return false;
    };

    if let Some(name_node) = parent.child_by_field_name("name")
        && name_node.id() == node.id()
    {
        return matches!(
            parent.kind(),
            "struct_item"
                | "enum_item"
                | "union_item"
                | "trait_item"
                | "type_item"
                | "impl_item"
                | "type_parameter"
        );
    }

    matches!(parent.kind(), "type_parameters")
}

fn is_inside_call_function(node: tree_sitter::Node) -> bool {
    let mut current = node;
    while let Some(parent) = current.parent() {
        if parent.kind() == "call_expression"
            && let Some(function_node) = parent.child_by_field_name("function")
        {
            return node.start_byte() >= function_node.start_byte()
                && node.end_byte() <= function_node.end_byte();
        }
        current = parent;
    }
    false
}

/// If `name_node` (a `type_identifier`) is the direct `type` child of an
/// *outermost* `generic_type` use site (e.g. the `Vec` of `Vec<String>`),
/// record that generic's ordered/nested applied type arguments against
/// `identifier`.
///
/// "Outermost" means the `generic_type` is NOT itself inside another
/// `type_arguments` list — nested generics ride along as `children` of the
/// enclosing usage and are never double-counted as separate rows.
fn record_outermost_rust_type_arguments(
    extractor: &mut RustExtractor,
    name_node: tree_sitter::Node,
    identifier: &Identifier,
) {
    let Some(parent) = name_node.parent() else {
        return;
    };
    // Both `generic_type` (type-position) and `generic_type_with_turbofish` (struct-literal
    // construction, e.g. `Repo::<String> { .. }`) carry a `type_arguments` field with the
    // same shape — handle both uniformly.
    if parent.kind() != "generic_type" && parent.kind() != "generic_type_with_turbofish" {
        return;
    }
    // Skip if this generic is itself nested inside another type_arguments.
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
    let base = extractor.get_base_mut();
    let arguments = crate::base::extract_type_arguments(base, arg_list, decompose_rust_type_arg);
    base.record_type_arguments(identifier, arguments);
}

/// Like `record_outermost_rust_type_arguments` but the anchor is a
/// `scoped_identifier` or `scoped_type_identifier` node — the node itself
/// (not a name child inside it) is the `type` field of the parent
/// `generic_type`.
fn record_outermost_rust_type_arguments_for_scoped(
    extractor: &mut RustExtractor,
    scoped_node: tree_sitter::Node,
    identifier: &Identifier,
) {
    let Some(parent) = scoped_node.parent() else {
        return;
    };
    // Mirror the type_identifier variant: accept both generic_type and
    // generic_type_with_turbofish for the same reasons.
    if parent.kind() != "generic_type" && parent.kind() != "generic_type_with_turbofish" {
        return;
    }
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
    let base = extractor.get_base_mut();
    let arguments = crate::base::extract_type_arguments(base, arg_list, decompose_rust_type_arg);
    base.record_type_arguments(identifier, arguments);
}

/// `TypeArgDecomposer` for Rust: maps a named child of a `type_arguments`
/// list to its applied argument.  Returns `None` to skip punctuation (`<`,
/// `,`, `>`) and lifetime parameters (`'a`, `'static`).  For a nested
/// `generic_type` returns the type name plus its inner `type_arguments` to
/// recurse into; for every other named type node (primitive types, references,
/// arrays, etc.) returns its source text as a leaf.
fn decompose_rust_type_arg<'a>(
    base: &BaseExtractor,
    node: tree_sitter::Node<'a>,
) -> Option<(String, Option<tree_sitter::Node<'a>>)> {
    if !node.is_named() {
        return None; // skip punctuation: < , >
    }
    match node.kind() {
        "lifetime" => None, // skip 'a, 'static, etc.
        "generic_type" => {
            // Nested generic such as `Vec<u8>` inside `HashMap<String, Vec<u8>>`
            let name = node
                .child_by_field_name("type")
                .map(|t| base.get_node_text(&t))
                .unwrap_or_else(|| base.get_node_text(&node));
            let nested = node.child_by_field_name("type_arguments");
            Some((name, nested))
        }
        _ => Some((base.get_node_text(&node), None)),
    }
}

/// Find the ID of the symbol that contains this node
fn find_containing_symbol_id(
    node: tree_sitter::Node,
    containing_symbols: &ContainingSymbolIndex<'_>,
) -> Option<String> {
    containing_symbols
        .find(node)
        .map(|symbol| symbol.id.clone())
}

pub(super) fn self_receiver_type(
    extractor: &RustExtractor,
    node: tree_sitter::Node,
) -> Option<String> {
    let callee = if node.kind() == "call_expression" {
        let function = node.child_by_field_name("function")?;
        if function.kind() == "generic_function" {
            function.child_by_field_name("function").unwrap_or(function)
        } else {
            function
        }
    } else if node.kind() == "generic_function" {
        node.child_by_field_name("function").unwrap_or(node)
    } else {
        node
    };

    let is_self_receiver = match callee.kind() {
        "field_expression" => {
            let value = callee.child_by_field_name("value")?;
            extractor.base.get_node_text(&value) == "self"
        }
        "scoped_identifier" => {
            let path = callee.child_by_field_name("path")?;
            extractor.base.get_node_text(&path) == "Self"
        }
        _ => false,
    };
    if !is_self_receiver {
        return None;
    }

    let start = node.start_byte();
    extractor
        .get_impl_blocks()
        .iter()
        .find(|block| start >= block.start_byte && start < block.end_byte)
        .map(|block| block.type_name.clone())
}
