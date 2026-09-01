//! C++ identifier extraction for LSP find_references functionality
//!
//! Extracts function calls, member access, and other identifier usages
//! from C++ source code for precise code navigation.

use crate::base::{BaseExtractor, Identifier, IdentifierKind, Symbol};
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};
use std::collections::HashMap;
use tree_sitter::Node;

use super::CppExtractor;
use super::helpers;

impl CppExtractor {
    /// Walk the tree and extract identifiers
    pub(super) fn walk_tree_for_identifiers(
        &mut self,
        node: Node,
        symbol_map: &HashMap<String, &Symbol>,
        depth: u32,
    ) {
        if !should_visit_tree_depth(depth) {
            return;
        }

        // Extract identifier from this node if applicable
        self.extract_identifier_from_node(node, symbol_map);

        // Recursively walk children
        let Some(child_depth) = child_tree_depth(depth) else {
            return;
        };
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.walk_tree_for_identifiers(child, symbol_map, child_depth);
        }
    }

    /// Extract identifier from a single node based on its kind
    fn extract_identifier_from_node(&mut self, node: Node, symbol_map: &HashMap<String, &Symbol>) {
        match node.kind() {
            // Function calls: foo(), bar.baz(), make_shared<Foo>()
            "call_expression" => {
                // Phase 3: capture string-literal call-arguments (config-free; the
                // carrier classification + gate happen in the artifact language-policy pass). Done
                // first so it also covers template calls (`query<T>("SELECT ...")`),
                // which the identifier logic below returns early for.
                self.record_call_arg_literals(node, symbol_map);
                if let Some(func_node) = node.child_by_field_name("function") {
                    // Template function call: make_shared<Foo>(), invoke<T>(), etc.
                    if func_node.kind() == "template_function" {
                        if let Some(name_node) = func_node.child_by_field_name("name") {
                            let name = self.base.get_node_text(&name_node);
                            let containing_symbol_id =
                                self.find_containing_symbol_id(node, symbol_map);
                            let identifier = self.base.create_identifier(
                                &name_node,
                                name,
                                IdentifierKind::Call,
                                containing_symbol_id,
                            );
                            if let Some(arg_list) = func_node.child_by_field_name("arguments") {
                                let arguments = crate::base::extract_type_arguments(
                                    &self.base,
                                    arg_list,
                                    decompose_cpp_type_arg,
                                );
                                self.base.record_type_arguments(&identifier, arguments);
                            }
                        }
                        return;
                    }

                    let (identifier_node, name) = if func_node.kind() == "field_expression" {
                        if let Some(field_node) = func_node.child_by_field_name("field") {
                            (field_node, self.base.get_node_text(&field_node))
                        } else {
                            (func_node, self.base.get_node_text(&func_node))
                        }
                    } else {
                        (func_node, self.base.get_node_text(&func_node))
                    };

                    let containing_symbol_id = self.find_containing_symbol_id(node, symbol_map);
                    let receiver_type = this_receiver_type(&self.base, node);
                    self.base.create_identifier_with_receiver_type(
                        &identifier_node,
                        name,
                        IdentifierKind::Call,
                        containing_symbol_id,
                        receiver_type,
                    );
                }
            }

            // Member access: object.field, object->field
            "field_expression" => {
                // Extract the field name
                if let Some(field_node) = node.child_by_field_name("field") {
                    let name = self.base.get_node_text(&field_node);
                    let containing_symbol_id = self.find_containing_symbol_id(node, symbol_map);

                    self.base.create_identifier(
                        &field_node,
                        name,
                        IdentifierKind::MemberAccess,
                        containing_symbol_id,
                    );
                }
            }

            // Type references: MyClass x, void f(MyStruct param), Container<MyClass>
            // C++ tree-sitter uses `type_identifier` for BOTH declaration names
            // (class MyClass, struct Foo, enum Bar) AND reference positions.
            // We only want references — declarations are filtered by parent context.
            "type_identifier" => {
                if helpers::is_type_declaration_name(&node) {
                    return;
                }

                let name = self.base.get_node_text(&node);

                if helpers::is_noise_type(&name) {
                    return;
                }

                let containing_symbol_id = self.find_containing_symbol_id(node, symbol_map);

                let identifier = self.base.create_identifier(
                    &node,
                    name,
                    IdentifierKind::TypeUsage,
                    containing_symbol_id,
                );
                record_outermost_cpp_type_arguments(&mut self.base, node, &identifier);
            }

            // `variable_ref` complement arm: a bare `identifier` used as a
            // value or as a qualified static-member value read — the reads the
            // Call/MemberAccess/TypeUsage arms above do not own. Type positions
            // never reach here (C++ uses the distinct `type_identifier` kind),
            // member names are `field_identifier`, and `this` is its own node
            // kind. See the LOCKED SEMANTIC CONTRACT doc comment in
            // `csharp/identifiers.rs`.
            "identifier" if is_cpp_value_read_identifier(node) => {
                let name = self.base.get_node_text(&node);
                // Rule 5: reuse the TypeUsage arm's noise filter, plus the
                // pre-C++11 NULL macro (parses as a plain identifier).
                if !helpers::is_noise_type(&name) && name != "NULL" {
                    let containing_symbol_id = self.find_containing_symbol_id(node, symbol_map);
                    self.base.create_identifier(
                        &node,
                        name,
                        IdentifierKind::VariableRef,
                        containing_symbol_id,
                    );
                }
            }

            // Rule 1: the scope receiver of a static/qualified VALUE access
            // (`GraphTraversal` in `GraphTraversal::reach()` /
            // `GraphTraversal::limit`) — `X` in `X::Y`. The grammar gives scope
            // segments the distinct `namespace_identifier` kind, so they need
            // their own arm; type-context chains (terminal `type_identifier` /
            // `template_type`) stay with the TypeUsage arm.
            "namespace_identifier" if is_cpp_scope_receiver_read(node) => {
                let name = self.base.get_node_text(&node);
                if !helpers::is_noise_type(&name) {
                    let containing_symbol_id = self.find_containing_symbol_id(node, symbol_map);
                    self.base.create_identifier(
                        &node,
                        name,
                        IdentifierKind::VariableRef,
                        containing_symbol_id,
                    );
                }
            }

            // Rule 1: a designated-initializer member LHS (`.x` in
            // `Point pt = { .x = seed }`) is a member reference in an
            // initializer context; no other arm owns it.
            "field_identifier" => {
                if let Some(parent) = node.parent()
                    && parent.kind() == "field_designator"
                {
                    let name = self.base.get_node_text(&node);
                    let containing_symbol_id = self.find_containing_symbol_id(node, symbol_map);
                    self.base.create_identifier(
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

    /// Find the ID of the symbol that contains this node
    /// CRITICAL FIX: Only search symbols from THIS FILE, not all files
    fn find_containing_symbol_id(
        &self,
        node: Node,
        symbol_map: &HashMap<String, &Symbol>,
    ) -> Option<String> {
        self.base
            .find_containing_symbol_from_map(&node, symbol_map)
            .map(|s| s.id.clone())
    }

    /// Capture string-literal arguments of a C++ `call_expression` as `Literal`
    /// records. Config-free: `carrier` is the called function name (or
    /// `recv.method` for a member/qualified call); the URL/SQL classification and
    /// the carrier gate run later in the artifact language-policy pass. C++ wraps arguments in
    /// an `argument_list` with no per-argument name wrapper, so each named child
    /// is decoded directly. `arg_position` is counted over the full argument list,
    /// so e.g. the URL in `curl_easy_setopt(h, CURLOPT_URL, "https://...")`
    /// reports position 2.
    fn record_call_arg_literals(&mut self, node: Node, symbol_map: &HashMap<String, &Symbol>) {
        let Some(func_node) = node.child_by_field_name("function") else {
            return;
        };
        let Some(args) = node.child_by_field_name("arguments") else {
            return;
        };
        let carrier = cpp_carrier(&self.base, func_node);
        let containing_symbol_id = self.find_containing_symbol_id(node, symbol_map);

        let mut cursor = args.walk();
        for (pos, arg) in args.named_children(&mut cursor).enumerate() {
            if let Some(text) = self.base.decode_string_literal(&arg) {
                self.base.record_literal(
                    &arg,
                    text,
                    carrier.clone(),
                    pos as u32,
                    containing_symbol_id.clone(),
                );
            }
        }
    }
}

pub(super) fn this_receiver_type(base: &BaseExtractor, node: Node) -> Option<String> {
    let field_expr = match node.kind() {
        "field_expression" => node,
        "call_expression" => {
            let function = node.child_by_field_name("function")?;
            if function.kind() == "field_expression" {
                function
            } else {
                return None;
            }
        }
        _ => return None,
    };
    if !is_this_receiver(field_expr) {
        return None;
    }
    enclosing_type_name(base, field_expr).or_else(|| out_of_line_type_name(base, field_expr))
}

fn is_this_receiver(field_expr: Node) -> bool {
    let Some(argument) = field_expr.child_by_field_name("argument") else {
        return false;
    };
    let argument = peel_parentheses(argument);
    match argument.kind() {
        "this" => true,
        "pointer_expression" => {
            let starred = argument
                .child_by_field_name("operator")
                .is_some_and(|operator| operator.kind() == "*");
            let inner = argument
                .child_by_field_name("argument")
                .map(peel_parentheses);
            let this_arg = inner.is_some_and(|inner| inner.kind() == "this");
            starred && this_arg
        }
        _ => false,
    }
}

fn peel_parentheses(mut node: Node) -> Node {
    while node.kind() == "parenthesized_expression" {
        if let Some(inner) = node.named_child(0) {
            node = inner;
        } else {
            break;
        }
    }
    node
}

fn enclosing_type_name(base: &BaseExtractor, node: Node) -> Option<String> {
    let mut current = node.parent();
    while let Some(candidate) = current {
        if matches!(candidate.kind(), "class_specifier" | "struct_specifier") {
            return candidate
                .children(&mut candidate.walk())
                .find(|child| child.kind() == "type_identifier")
                .map(|name| base.get_node_text(&name));
        }
        current = candidate.parent();
    }
    None
}

fn out_of_line_type_name(base: &BaseExtractor, node: Node) -> Option<String> {
    let mut current = node.parent();
    while let Some(candidate) = current {
        if candidate.kind() == "function_definition" {
            let declarator = candidate.child_by_field_name("declarator")?;
            return qualified_declarator_scope(base, declarator);
        }
        current = candidate.parent();
    }
    None
}

fn qualified_declarator_scope(base: &BaseExtractor, node: Node) -> Option<String> {
    let mut current = node;
    loop {
        match current.kind() {
            "qualified_identifier" => {
                let name = current.child_by_field_name("name")?;
                if name.kind() == "qualified_identifier" {
                    current = name;
                    continue;
                }
                let scope = current.child_by_field_name("scope")?;
                return scope_segment_name(base, scope);
            }
            "function_declarator" | "pointer_declarator" | "reference_declarator" => {
                current = current.child_by_field_name("declarator")?;
            }
            _ => return None,
        }
    }
}

fn scope_segment_name(base: &BaseExtractor, scope: Node) -> Option<String> {
    match scope.kind() {
        "namespace_identifier" => Some(base.get_node_text(&scope)),
        "template_type" => scope
            .child_by_field_name("name")
            .map(|name| base.get_node_text(&name)),
        _ => None,
    }
}

/// Rule 1/4 predicate for C++ `variable_ref` emission: is this bare
/// `identifier` a value read (the complement of the Call/MemberAccess/TypeUsage
/// arms)? Node kinds and field names were verified against the vendored
/// tree-sitter-cpp grammar (see task probes): declarators carry a `declarator`
/// field, `assignment_expression` carries an anonymous `operator` field, scope
/// segments are `namespace_identifier`, and labels are `statement_identifier`.
fn is_cpp_value_read_identifier(node: Node) -> bool {
    let Some(parent) = node.parent() else {
        return false;
    };

    // Rule 3: any declarator-position identifier is a declaration name
    // (`init_declarator`, `function_declarator`, `array_declarator`,
    // `parameter_declaration`, bare `declaration declarator:`).
    if parent.child_by_field_name("declarator").map(|d| d.id()) == Some(node.id()) {
        return false;
    }

    match parent.kind() {
        // Rule 3: declarator wrappers whose inner identifier carries no field
        // (`*p`, `&item`, `auto [u, v]`).
        "pointer_declarator"
        | "reference_declarator"
        | "structured_binding_declarator"
        | "variadic_declarator" => false,

        // Rule 2: the callee is owned by the Call arm; a `template_function`
        // name (`foo<int>`) is likewise Call/type material, never a bare read.
        "call_expression" => {
            parent.child_by_field_name("function").map(|f| f.id()) != Some(node.id())
        }
        "template_function" => false,

        // Rule 2/3: a qualified name's terminal `identifier` is a value read
        // (`GraphTraversal::limit`) unless the chain is an out-of-line
        // declarator, a call callee (the Call arm records the full qualified
        // text), or an import-like context.
        "qualified_identifier" => {
            parent.child_by_field_name("name").map(|n| n.id()) == Some(node.id())
                && cpp_qualified_chain_is_value_read(parent)
        }

        // Rule 3: `#define NAME`, macro params, enum constants, namespaces,
        // aliases, and imports are declaration/meta positions.
        "preproc_def" | "preproc_function_def" | "enumerator" => {
            parent.child_by_field_name("name").map(|n| n.id()) != Some(node.id())
        }
        "preproc_params"
        | "namespace_definition"
        | "namespace_alias_definition"
        | "using_declaration" => false,

        // Meta positions: `[[nodiscard]]` attribute names and the arguments of
        // `__attribute__((...))` are not value reads.
        "attribute" => false,
        "argument_list" => parent
            .parent()
            .map(|gp| gp.kind() != "attribute_specifier")
            .unwrap_or(true),

        // Rule 4: plain-assignment LHS is write-only; compound assignment
        // reads its target. `x++`/`x--` are `update_expression` (reads).
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

        // Every other position — argument, initializer element, return value,
        // binary operand, lambda capture, subscript, `delete p`, receiver
        // (`argument` of field_expression) — is a read.
        _ => true,
    }
}

/// Walk from a `qualified_identifier` to the outermost node of its `::` chain.
fn outermost_cpp_qualified(mut node: Node) -> Node {
    while let Some(parent) = node.parent() {
        if parent.kind() == "qualified_identifier" {
            node = parent;
        } else {
            break;
        }
    }
    node
}

/// Context check shared by the qualified-name and scope-receiver predicates:
/// the outermost `X::Y::Z` chain is a VALUE read only when it is not an
/// out-of-line declarator (`int Widget::grow(...)`), not a call callee (owned
/// by the Call arm as full qualified text), and not a `using` import.
fn cpp_qualified_chain_is_value_read(scoped: Node) -> bool {
    let outer = outermost_cpp_qualified(scoped);
    let Some(owner) = outer.parent() else {
        return false;
    };
    if owner.child_by_field_name("declarator").map(|d| d.id()) == Some(outer.id()) {
        return false;
    }
    if owner.kind() == "call_expression"
        && owner.child_by_field_name("function").map(|f| f.id()) == Some(outer.id())
    {
        return false;
    }
    !matches!(
        owner.kind(),
        "using_declaration" | "namespace_alias_definition" | "function_declarator"
    )
}

/// Rule 1 predicate for the scope receiver of a qualified access: `X` in
/// `X::Y` / `X::Y()` emits as a read (mirrors the C# reference where the
/// static-access receiver keeps the accessed type/namespace alive). Type
/// context is detected by the chain's terminal name kind: `identifier` means a
/// value/callee chain, while `type_identifier`/`template_type` chains belong to
/// the TypeUsage arm.
fn is_cpp_scope_receiver_read(node: Node) -> bool {
    let Some(parent) = node.parent() else {
        return false;
    };
    if parent.kind() != "qualified_identifier"
        || parent.child_by_field_name("scope").map(|s| s.id()) != Some(node.id())
    {
        return false;
    }

    let outer = outermost_cpp_qualified(parent);

    // Terminal name: descend nested qualified chains to the innermost name.
    let mut terminal = outer.child_by_field_name("name");
    while let Some(t) = terminal {
        if t.kind() == "qualified_identifier" {
            terminal = t.child_by_field_name("name");
        } else {
            break;
        }
    }
    if terminal.map(|t| t.kind()) != Some("identifier") {
        return false;
    }

    let Some(owner) = outer.parent() else {
        return false;
    };
    if owner.child_by_field_name("declarator").map(|d| d.id()) == Some(outer.id()) {
        return false;
    }
    // Unlike the terminal name, the scope receiver reads even in a call-callee
    // chain (`GraphTraversal::reach()`), so only declaration/import contexts
    // are excluded here.
    !matches!(
        owner.kind(),
        "using_declaration" | "namespace_alias_definition" | "function_declarator"
    )
}

/// Truncate a callee segment at its first `<` so generic arguments don't leak
/// into the carrier (`query<User>` -> `query`). Mirrors the C# leg's generic
/// strip and keeps the gate's last-segment match working for template methods.
fn strip_cpp_generics(text: &str) -> String {
    match text.find('<') {
        Some(i) => text[..i].to_string(),
        None => text.to_string(),
    }
}

/// Derive a C++ call's carrier. Plain `identifier` → its text (`PQexec`);
/// `field_expression` (`db.exec(...)`, `repo.query<User>(...)`) → the
/// `object.field` join (generics stripped from the field) so the gate's
/// last-segment rule can match a bare config; `template_function`
/// (`query<T>(...)`) → the `name` field; `qualified_identifier` (`ns::fn(...)`)
/// → the trailing `name` segment.
fn cpp_carrier(base: &BaseExtractor, func_node: Node) -> Option<String> {
    match func_node.kind() {
        "identifier" => Some(base.get_node_text(&func_node)),
        "field_expression" => {
            let object = func_node
                .child_by_field_name("argument")
                .map(|n| base.get_node_text(&n));
            let field = func_node
                .child_by_field_name("field")
                .map(|n| strip_cpp_generics(&base.get_node_text(&n)));
            match (object, field) {
                (Some(o), Some(f)) => Some(format!("{o}.{f}")),
                (None, Some(f)) => Some(f),
                _ => None,
            }
        }
        "template_function" => func_node
            .child_by_field_name("name")
            .map(|n| strip_cpp_generics(&base.get_node_text(&n))),
        "qualified_identifier" => func_node
            .child_by_field_name("name")
            .map(|n| base.get_node_text(&n))
            .or_else(|| Some(base.get_node_text(&func_node))),
        _ => {
            let text = base.get_node_text(&func_node);
            if text.is_empty() { None } else { Some(text) }
        }
    }
}

// ============================================================================
// Type-argument capture helpers (Miller bridge Phase 2)
// ============================================================================

/// Record type arguments for the outermost `template_type` generic use site.
///
/// Called from the `type_identifier` arm after creating the identifier.  Records
/// only when:
/// - the `type_identifier`'s parent is a `template_type` (e.g. `Box` in `Box<Item>`)
/// - AND that `template_type` is not itself nested inside a `type_descriptor` (which
///   places it inside another template's `template_argument_list`)
///
/// The qualified-identifier case (`std::vector<T>`) is handled by also checking
/// one level further: if the parent of `template_type` is a `qualified_identifier`
/// which is itself inside a `type_descriptor`, it's still nested.
fn record_outermost_cpp_type_arguments(
    base: &mut BaseExtractor,
    name_node: Node,
    identifier: &Identifier,
) {
    let Some(parent) = name_node.parent() else {
        return;
    };
    if parent.kind() != "template_type" {
        return;
    }
    // "Outermost" means the template_type is not nested inside another
    // template's type_descriptor argument wrapper.
    let template_parent = parent.parent();
    let is_nested = template_parent
        .map(|tp| {
            tp.kind() == "type_descriptor"
                || (tp.kind() == "qualified_identifier"
                    && tp
                        .parent()
                        .map(|gp| gp.kind() == "type_descriptor")
                        .unwrap_or(false))
        })
        .unwrap_or(false);
    if is_nested {
        return;
    }
    let Some(arg_list) = parent.child_by_field_name("arguments") else {
        return;
    };
    let arguments = crate::base::extract_type_arguments(base, arg_list, decompose_cpp_type_arg);
    base.record_type_arguments(identifier, arguments);
}

/// Decompose a child of `template_argument_list` into `(type_name, nested_arg_list)`.
///
/// C++ template arguments are wrapped in `type_descriptor` nodes. We unwrap the
/// `type` field of the descriptor:
/// - `template_type` → nested generic: name from `name` field, recurse into `arguments`
/// - Anything else (`primitive_type`, `type_identifier`, `qualified_identifier`, …) → leaf
///
/// Non-type template arguments (`expression` children) are skipped.
fn decompose_cpp_type_arg<'a>(
    base: &BaseExtractor,
    node: Node<'a>,
) -> Option<(String, Option<Node<'a>>)> {
    if !node.is_named() {
        return None; // skip < , >
    }
    match node.kind() {
        "type_descriptor" => {
            let type_node = node.child_by_field_name("type")?;
            match type_node.kind() {
                "template_type" => {
                    // Nested generic
                    let name = type_node
                        .child_by_field_name("name")
                        .map(|n| base.get_node_text(&n))
                        .unwrap_or_else(|| base.get_node_text(&type_node));
                    let nested = type_node.child_by_field_name("arguments");
                    Some((name, nested))
                }
                _ => {
                    // Leaf: primitive_type, type_identifier, qualified_identifier, etc.
                    Some((base.get_node_text(&type_node), None))
                }
            }
        }
        _ => {
            // Non-type template argument (e.g. `5` in `array<int, 5>`).
            // Capture the raw source text as a leaf; dropping these shifts ordinals.
            Some((base.get_node_text(&node), None))
        }
    }
}
