/// Identifier extraction for LSP-quality find_references
/// Tracks function calls, member access, and other identifier usages
use super::PythonExtractor;
use super::helpers;
use super::type_arguments::record_outermost_python_type_arguments;
use crate::base::{BaseExtractor, ContainingSymbolIndex, Identifier, IdentifierKind, Symbol};
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};
use tree_sitter::{Node, Tree};

/// Extract all identifier usages (function calls, member access, etc.)
/// Following the Rust extractor reference implementation pattern
pub fn extract_identifiers(
    extractor: &mut PythonExtractor,
    tree: &Tree,
    symbols: &[Symbol],
) -> Vec<Identifier> {
    let containing_symbols = extractor.base().containing_symbol_index(symbols);

    // Walk the tree and extract identifiers
    walk_tree_for_identifiers(extractor, tree.root_node(), &containing_symbols, 0);

    // Return the collected identifiers
    extractor.base_mut().identifiers.clone()
}

/// Recursively walk tree extracting identifiers from each node
fn walk_tree_for_identifiers(
    extractor: &mut PythonExtractor,
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
    extractor: &mut PythonExtractor,
    node: Node,
    containing_symbols: &ContainingSymbolIndex<'_>,
) {
    match node.kind() {
        // Function/method calls: foo(), bar.baz()
        // Python uses "call" node type
        "call" => {
            // The function being called is in the "function" field
            if let Some(function_node) = node.child_by_field_name("function") {
                match function_node.kind() {
                    "identifier" => {
                        // Simple function call: foo()
                        let name = extractor.base_mut().get_node_text(&function_node);
                        let containing_symbol_id =
                            find_containing_symbol_id(node, containing_symbols);

                        extractor.base_mut().create_identifier(
                            &function_node,
                            name,
                            IdentifierKind::Call,
                            containing_symbol_id,
                        );
                    }
                    "attribute" => {
                        // Member call: object.method()
                        // Extract the rightmost identifier (the method name)
                        if let Some(attr_node) = function_node.child_by_field_name("attribute") {
                            let name = extractor.base_mut().get_node_text(&attr_node);
                            let containing_symbol_id =
                                find_containing_symbol_id(node, containing_symbols);
                            let receiver_type = helpers::self_or_cls_receiver_type(
                                extractor.base(),
                                &function_node,
                            );

                            extractor.base_mut().create_identifier_with_receiver_type(
                                &attr_node,
                                name,
                                IdentifierKind::Call,
                                containing_symbol_id,
                                receiver_type,
                            );
                        }
                    }
                    _ => {
                        // Other cases like subscript expressions
                        // Skip for now
                    }
                }
            }
            // Phase 3: capture string-literal call-arguments config-free; the
            // carrier classification + bloat gate run later in the artifact language-policy pass.
            record_python_call_arg_literals(extractor, node, containing_symbols);
        }

        // Member access: object.property
        // Python uses "attribute" node type
        "attribute" => {
            if is_python_type_usage_node(node)
                || is_isinstance_class_argument(extractor.base(), node)
            {
                if let Some(attr_node) = node.child_by_field_name("attribute") {
                    let name = extractor.base_mut().get_node_text(&attr_node);
                    if !is_python_builtin_type(&name) {
                        let containing_symbol_id =
                            find_containing_symbol_id(node, containing_symbols);

                        let identifier = extractor.base_mut().create_identifier(
                            &attr_node,
                            name,
                            IdentifierKind::TypeUsage,
                            containing_symbol_id,
                        );
                        // `node` is the whole `a.B` attribute expression; if it is
                        // the `value` field of a subscript (e.g. `typing.Optional[X]`)
                        // we record the ordered type arguments against the identifier.
                        record_outermost_python_type_arguments(extractor, node, &identifier);
                    }
                }
                return;
            }

            // Only extract if it's NOT part of a call
            // (we handle those in the call case above)
            if let Some(parent) = node.parent()
                && parent.kind() == "call"
            {
                // Check if this attribute is the function being called
                if let Some(function_node) = parent.child_by_field_name("function")
                    && function_node.id() == node.id()
                {
                    return; // Skip - handled by call
                }
            }

            // Extract the attribute name
            if let Some(attr_node) = node.child_by_field_name("attribute") {
                let name = extractor.base_mut().get_node_text(&attr_node);
                let containing_symbol_id = find_containing_symbol_id(node, containing_symbols);

                extractor.base_mut().create_identifier(
                    &attr_node,
                    name,
                    IdentifierKind::MemberAccess,
                    containing_symbol_id,
                );
            }
        }

        "identifier"
            if is_python_type_usage_identifier(node)
                || is_isinstance_class_argument(extractor.base(), node) =>
        {
            let name = extractor.base_mut().get_node_text(&node);
            if !is_python_builtin_type(&name) || is_generic_head(node) {
                let containing_symbol_id = find_containing_symbol_id(node, containing_symbols);

                let identifier = extractor.base_mut().create_identifier(
                    &node,
                    name,
                    IdentifierKind::TypeUsage,
                    containing_symbol_id,
                );
                // If this identifier is the `value` of an outermost subscript
                // (e.g. `Optional` in `Optional[User]`), record the ordered
                // type arguments.  Nested generics are skipped here — their
                // args ride along as `children` of the enclosing usage.
                record_outermost_python_type_arguments(extractor, node, &identifier);
            }
        }

        // `variable_ref` complement arm (locked contract — see the doc comment in
        // csharp/identifiers.rs): a bare `identifier` used as a value or as the
        // object/receiver of an attribute access — the reads the Call/MemberAccess/
        // TypeUsage arms above do not own. Evaluated only after the TypeUsage guard,
        // so type positions never reach here (single row per node; no duplicates).
        "identifier" if is_python_value_read_identifier(node) => {
            let name = extractor.base_mut().get_node_text(&node);
            // Rule 5: reuse the builtin filter (`True`/`False`/`None` are distinct
            // grammar nodes and never reach this arm). `self`/`cls` receiver
            // conventions and `__name__`-style dunders are pure noise for
            // name-liveness, so they are filtered here as well.
            if !is_python_builtin_type(&name)
                && name != "self"
                && name != "cls"
                && !(name.starts_with("__") && name.ends_with("__"))
            {
                let containing_symbol_id = find_containing_symbol_id(node, containing_symbols);
                extractor.base_mut().create_identifier(
                    &node,
                    name,
                    IdentifierKind::VariableRef,
                    containing_symbol_id,
                );
            }
        }

        "string" if is_python_type_usage_node(node) => {
            record_forward_reference_usages(extractor, node, containing_symbols);
        }

        "dotted_name" => record_pattern_references(extractor, node, containing_symbols),

        _ => {}
    }
}

/// A builtin generic such as `list` in `list[User]`: the head of a generic
/// type or subscript, so its type arguments have an identifier to join to.
fn is_generic_head(node: Node) -> bool {
    let Some(parent) = node.parent() else {
        return false;
    };
    match parent.kind() {
        "generic_type" => parent.named_child(0).map(|head| head.id()) == Some(node.id()),
        "subscript" => parent.child_by_field_name("value").map(|head| head.id()) == Some(node.id()),
        _ => false,
    }
}

/// Type usages for the names inside a forward-reference annotation string
/// (`"Repo"`, `"User | None"`), each at its exact span. Strings that are
/// `Literal[...]` values or `Annotated[...]` metadata are not types.
fn record_forward_reference_usages(
    extractor: &mut PythonExtractor,
    string_node: Node,
    containing_symbols: &ContainingSymbolIndex<'_>,
) {
    if !is_annotation_type_string(extractor.base(), string_node) {
        return;
    }
    let mut cursor = string_node.walk();
    let Some(content) = string_node
        .named_children(&mut cursor)
        .find(|child| child.kind() == "string_content")
    else {
        return;
    };
    let text = extractor.base().get_node_text(&content);
    let base_offset = content.start_byte();
    let containing_symbol_id = find_containing_symbol_id(string_node, containing_symbols);
    for (start, end) in forward_reference_terminal_names(&text) {
        let name = text[start..end].to_string();
        if is_python_builtin_type(&name) {
            continue;
        }
        let Some(span) = crate::base::NormalizedSpan::from_content_range(
            &extractor.base().content,
            base_offset + start,
            base_offset + end,
        ) else {
            continue;
        };
        extractor.base_mut().create_identifier_at_span(
            span,
            name,
            IdentifierKind::TypeUsage,
            containing_symbol_id.clone(),
            None,
        );
    }
}

/// The byte ranges of the terminal segment of each dotted name in a
/// forward-reference string: `User` and `None` in `"User | None"`, `Order`
/// in `"app.models.Order"`.
fn forward_reference_terminal_names(text: &str) -> Vec<(usize, usize)> {
    let bytes = text.as_bytes();
    let mut ranges = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        let byte = bytes[index];
        if !(byte.is_ascii_alphabetic() || byte == b'_') {
            index += 1;
            continue;
        }
        let mut segment_start = index;
        while index < bytes.len()
            && (bytes[index].is_ascii_alphanumeric() || matches!(bytes[index], b'_' | b'.'))
        {
            if bytes[index] == b'.' {
                segment_start = index + 1;
            }
            index += 1;
        }
        if segment_start < index {
            ranges.push((segment_start, index));
        }
    }
    ranges
}

/// A plain (not f-, b-, or r-prefixed) string in a type position that is not
/// a `Literal[...]` value or `Annotated[...]` metadata.
pub(super) fn is_annotation_type_string(base: &BaseExtractor, string_node: Node) -> bool {
    let text = base.get_node_text(&string_node);
    if !text.starts_with(['"', '\'']) {
        return false;
    }
    let mut current = string_node;
    while let Some(parent) = current.parent() {
        match parent.kind() {
            "type" => current = parent,
            "type_parameter" => {
                let Some(generic) = parent.parent() else {
                    return true;
                };
                let Some(head) = generic.named_child(0) else {
                    return true;
                };
                let head_text = base.get_node_text(&head);
                let head_name = head_text.rsplit('.').next().unwrap_or(&head_text);
                if head_name == "Literal" {
                    return false;
                }
                if head_name == "Annotated" {
                    let mut cursor = parent.walk();
                    let first_arg = parent.named_children(&mut cursor).next();
                    return first_arg.map(|arg| arg.id()) == Some(current.id());
                }
                current = generic;
            }
            "generic_type" | "union_type" | "binary_operator" => current = parent,
            _ => return true,
        }
    }
    true
}

/// References in `match` patterns. A class pattern head (`Point(...)`) is a
/// type usage. A dotted value pattern (`Color.RED`) reads its first name and
/// accesses the rest. A bare name is a capture and stays silent.
fn record_pattern_references(
    extractor: &mut PythonExtractor,
    node: Node,
    containing_symbols: &ContainingSymbolIndex<'_>,
) {
    let is_class_head = node
        .parent()
        .is_some_and(|parent| parent.kind() == "class_pattern");
    if !is_class_head && (node.named_child_count() < 2 || !is_in_case_pattern(node)) {
        return;
    }
    let mut cursor = node.walk();
    let names: Vec<Node> = node
        .named_children(&mut cursor)
        .filter(|child| child.kind() == "identifier")
        .collect();
    let containing_symbol_id = find_containing_symbol_id(node, containing_symbols);
    let last = names.len().saturating_sub(1);
    for (index, name_node) in names.into_iter().enumerate() {
        let kind = if is_class_head && index == last {
            IdentifierKind::TypeUsage
        } else if index == 0 {
            IdentifierKind::VariableRef
        } else {
            IdentifierKind::MemberAccess
        };
        let name = extractor.base().get_node_text(&name_node);
        extractor.base_mut().create_identifier(
            &name_node,
            name,
            kind,
            containing_symbol_id.clone(),
        );
    }
}

fn is_in_case_pattern(node: Node) -> bool {
    let mut current = node;
    while let Some(parent) = current.parent() {
        match parent.kind() {
            "case_pattern" => return true,
            "case_clause" | "block" | "module" => return false,
            _ => current = parent,
        }
    }
    false
}

/// Rule 1/4 predicate for the `variable_ref` arm: is this bare `identifier` a
/// value read or an attribute-receiver read (the complement of the Call/
/// MemberAccess/TypeUsage arms)? Inclusive by default with enumerated
/// exclusions, mirroring `is_csharp_value_read_identifier`. Node kinds and
/// field names verified against the vendored tree-sitter-python 0.25.0 grammar.
fn is_python_value_read_identifier(node: Node) -> bool {
    // Rule 3: never a class/function/type-alias definition name.
    if is_python_declaration_name(node) {
        return false;
    }
    let Some(parent) = node.parent() else {
        return false;
    };
    let is_name_field = parent.child_by_field_name("name").map(|n| n.id()) == Some(node.id());

    match parent.kind() {
        // Rule 2: the callee is owned by the Call arm; argument identifiers sit
        // inside `argument_list`, not directly under `call`.
        "call" => parent.child_by_field_name("function").map(|f| f.id()) != Some(node.id()),
        // Rule 1/2: only the object (receiver) of an attribute access is a read;
        // the accessed `.name` is owned by the MemberAccess/Call/TypeUsage arms.
        "attribute" => parent.child_by_field_name("object").map(|o| o.id()) == Some(node.id()),
        // Rule 4: a plain assignment LHS is write-only — and it is also how
        // Python declares locals/globals, so this covers rule 3 for variables.
        "assignment" => parent.child_by_field_name("left").map(|l| l.id()) != Some(node.id()),
        // Rule 4: an augmented assignment (`x += 1`) reads both sides.
        "augmented_assignment" => true,
        // The walrus (`n := seed`) binds its `name`; the value is a read.
        "named_expression" => !is_name_field,
        // Per plan: keyword-argument NAMES (`foo(bar=5)`) are parameter refs —
        // skip them; keyword-argument VALUES are reads.
        "keyword_argument" => !is_name_field,
        // Rule 3: parameter declarations (a default VALUE is a read).
        "parameters" | "lambda_parameters" | "typed_parameter" => false,
        "default_parameter" | "typed_default_parameter" => !is_name_field,
        // Rule 4: destructuring / splat patterns are write targets.
        "pattern_list"
        | "tuple_pattern"
        | "list_pattern"
        | "list_splat_pattern"
        | "dictionary_splat_pattern" => false,
        // Rule 4: the for-loop target binds; the iterated collection is a read.
        "for_statement" => parent.child_by_field_name("left").map(|l| l.id()) != Some(node.id()),
        // `with … as X` / `except E as X` bind X.
        "as_pattern_target" => false,
        // Rule 3/4: match-statement pattern BINDINGS (PEP 634 — a bare name in a
        // case pattern is always a capture). Probed shapes: `[*items]`/`**rest`
        // put the name under `splat_pattern`; `case _ as handler` puts the
        // binder DIRECTLY under `as_pattern` (only in case context — with/except
        // binders ride in `as_pattern_target`, handled above, and their
        // as_pattern-child value stays a read); `Point(x=…)` attribute names sit
        // under `keyword_pattern`. Bare captures (`case other:`) are wrapped in
        // `dotted_name` and were already excluded below. Dotted VALUE references
        // (`case Color.RED:`) also ride in `dotted_name` and stay non-emitting —
        // the safe (miss, not false-alive) direction.
        "splat_pattern" | "keyword_pattern" => false,
        "as_pattern" => !parent
            .parent()
            .map(|gp| gp.kind() == "case_pattern")
            .unwrap_or(false),
        // Rule 3: import paths and aliases are declarations, not reads.
        "dotted_name"
        | "aliased_import"
        | "import_statement"
        | "import_from_statement"
        | "relative_import" => false,
        // `global`/`nonlocal`/`del` mention names without reading their values.
        "global_statement" | "nonlocal_statement" | "delete_statement" => false,
        // Every other expression/statement value slot — return / operand /
        // argument / condition / collection element / subscript / decorator — is
        // a read.
        _ => true,
    }
}

fn is_python_type_usage_identifier(node: Node) -> bool {
    if let Some(parent) = node.parent()
        && parent.kind() == "attribute"
    {
        return false;
    }

    is_python_type_usage_node(node)
}

fn is_python_type_usage_node(node: Node) -> bool {
    if is_python_declaration_name(node) {
        return false;
    }

    let mut current = node;
    while let Some(parent) = current.parent() {
        match parent.kind() {
            "type" | "generic_type" | "union_type" => return true,
            "call" | "return_statement" | "block" | "module" => return false,
            // `argument_list` is a stopping node for regular call arguments.
            // Exception: class superclasses sit in an `argument_list` whose parent
            // is `class_definition`. Allow that context so heritage subscripts like
            // `class Repo(Mapping[str, int])` are captured as type-usage positions.
            "argument_list" => {
                let parent_is_class = parent
                    .parent()
                    .map(|gp| gp.kind() == "class_definition")
                    .unwrap_or(false);
                return parent_is_class;
            }
            _ => {}
        }

        current = parent;
    }

    false
}

/// A class that `isinstance(value, Class)` or `issubclass(cls, Class)` checks, alone or in a
/// tuple: a runtime type check names its classes as types.
fn is_isinstance_class_argument(base: &BaseExtractor, node: Node) -> bool {
    let argument = match node.parent() {
        Some(tuple) if tuple.kind() == "tuple" => tuple,
        _ => node,
    };
    let Some(arguments) = argument.parent().filter(|p| p.kind() == "argument_list") else {
        return false;
    };
    if arguments.named_child(1).map(|second| second.id()) != Some(argument.id()) {
        return false;
    }
    arguments
        .parent()
        .and_then(|call| call.child_by_field_name("function"))
        .is_some_and(|function| {
            function.kind() == "identifier"
                && matches!(
                    base.get_node_text(&function).as_str(),
                    "isinstance" | "issubclass"
                )
        })
}

fn is_python_declaration_name(node: Node) -> bool {
    let Some(parent) = node.parent() else {
        return false;
    };
    if is_type_parameter_declaration(node) {
        return true;
    }

    if let Some(name_node) = parent.child_by_field_name("name") {
        return name_node.id() == node.id()
            && matches!(
                parent.kind(),
                "class_definition" | "function_definition" | "type_alias_statement"
            );
    }

    false
}

/// The name in the `left` of a `type X[T] = ...` statement, or a type
/// parameter declared by a class, function, or type alias (`T` in `[T]`).
fn is_type_parameter_declaration(node: Node) -> bool {
    let mut current = node;
    while let Some(parent) = current.parent() {
        match parent.kind() {
            "type_alias_statement" => {
                return parent.child_by_field_name("left").map(|left| left.id())
                    == Some(current.id());
            }
            "class_definition" | "function_definition" => {
                return parent
                    .child_by_field_name("type_parameters")
                    .map(|params| params.id())
                    == Some(current.id());
            }
            "type" | "generic_type" | "type_parameter" => current = parent,
            _ => return false,
        }
    }
    false
}

fn is_python_builtin_type(name: &str) -> bool {
    matches!(
        name,
        "bool"
            | "bytes"
            | "complex"
            | "dict"
            | "float"
            | "frozenset"
            | "int"
            | "list"
            | "None"
            | "object"
            | "set"
            | "str"
            | "tuple"
            | "type"
    )
}

/// Find the ID of the symbol that contains this node
/// CRITICAL: Only search symbols from THIS FILE (file-scoped filtering)
fn find_containing_symbol_id(
    node: Node,
    containing_symbols: &ContainingSymbolIndex<'_>,
) -> Option<String> {
    let anchor = helpers::decorated_definition_name(&node).unwrap_or(node);
    containing_symbols.find(anchor).map(|s| s.id.clone())
}

// ============================================================================
// String-literal call-argument capture
// ============================================================================

/// Capture string-literal arguments of a Python `call` as `Literal` records.
///
/// Config-free: `carrier` is the verbatim callee text; the URL/SQL
/// classification and the carrier gate run later in the artifact language-policy pass.
/// Records one literal per string-like argument, with `arg_position` counted
/// over the full (named) argument list. Keyword arguments (`url="..."`) descend
/// to their `value` so `requests.get(url="/api")` is captured too.
fn record_python_call_arg_literals(
    extractor: &mut PythonExtractor,
    call_node: Node,
    containing_symbols: &ContainingSymbolIndex<'_>,
) {
    let Some(function_node) = call_node.child_by_field_name("function") else {
        return;
    };
    let Some(args_node) = call_node.child_by_field_name("arguments") else {
        return;
    };
    let carrier = python_carrier(extractor.base(), function_node);
    let containing_symbol_id = find_containing_symbol_id(call_node, containing_symbols);

    let mut cursor = args_node.walk();
    for (pos, arg) in args_node.named_children(&mut cursor).enumerate() {
        // Keyword args (`name=value`) hold the literal in their `value` field.
        let value = if arg.kind() == "keyword_argument" {
            arg.child_by_field_name("value")
        } else {
            Some(arg)
        };
        if let Some(value) = value
            && let Some(text) = extractor.base().decode_string_literal(&value)
        {
            extractor.base_mut().record_literal(
                &value,
                text,
                carrier.clone(),
                pos as u32,
                containing_symbol_id.clone(),
            );
        }
    }
}

/// Derive a Python call's carrier from its callee.
///
/// Plain `identifier` → its text (`open`). `attribute` (`requests.get`,
/// `cursor.execute`) → the `object.attribute` join so dotted client APIs match
/// config (`requests.get`) and local-variable receivers still match a bare
/// method config (`execute`) via the gate's last-segment rule.
fn python_carrier(base: &BaseExtractor, function_node: Node) -> Option<String> {
    match function_node.kind() {
        "identifier" => Some(base.get_node_text(&function_node)),
        "attribute" => {
            let object = function_node
                .child_by_field_name("object")
                .map(|n| base.get_node_text(&n));
            let attribute = function_node
                .child_by_field_name("attribute")
                .map(|n| base.get_node_text(&n));
            match (object, attribute) {
                (Some(o), Some(a)) => Some(format!("{o}.{a}")),
                (None, Some(a)) => Some(a),
                _ => None,
            }
        }
        _ => {
            let text = base.get_node_text(&function_node);
            if text.is_empty() { None } else { Some(text) }
        }
    }
}
