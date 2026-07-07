// PHP Extractor - Identifier extraction (function calls, member access, type usage)

use super::PhpExtractor;
use crate::base::{BaseExtractor, IdentifierKind, Symbol};
use std::collections::HashMap;
use tree_sitter::Node;

/// Extract identifier from a single node based on its kind
pub(super) fn extract_identifier_from_node(
    extractor: &mut PhpExtractor,
    node: Node,
    symbol_map: &HashMap<String, &Symbol>,
) {
    match node.kind() {
        // Direct function calls: print_r(), array_map()
        "function_call_expression" => {
            // The function field contains the function being called
            if let Some(function_node) = node.child_by_field_name("function") {
                let name = extractor.get_base().get_node_text(&function_node);
                let containing_symbol_id = find_containing_symbol_id(extractor, node, symbol_map);

                extractor.get_base_mut().create_identifier(
                    &function_node,
                    name,
                    IdentifierKind::Call,
                    containing_symbol_id,
                );
            }
            // Phase 3b: capture string-literal call-arguments config-free.
            record_php_call_arg_literals(extractor, node, symbol_map);
        }

        // Method calls: $this->add(), $obj->method()
        "member_call_expression" => {
            // Extract the method name from the name field
            if let Some(name_node) = node.child_by_field_name("name") {
                let name = extractor.get_base().get_node_text(&name_node);
                let containing_symbol_id = find_containing_symbol_id(extractor, node, symbol_map);

                extractor.get_base_mut().create_identifier(
                    &name_node,
                    name,
                    IdentifierKind::Call,
                    containing_symbol_id,
                );
            }
            // Phase 3b: capture string-literal call-arguments config-free.
            record_php_call_arg_literals(extractor, node, symbol_map);
        }

        // Static method calls: Http::get(), DB::select(), Model::where()
        "scoped_call_expression" => {
            // Extract the method name from the name field
            if let Some(name_node) = node.child_by_field_name("name") {
                let name = extractor.get_base().get_node_text(&name_node);
                let containing_symbol_id = find_containing_symbol_id(extractor, node, symbol_map);

                extractor.get_base_mut().create_identifier(
                    &name_node,
                    name,
                    IdentifierKind::Call,
                    containing_symbol_id,
                );
            }
            // Phase 3b: capture string-literal call-arguments config-free.
            record_php_call_arg_literals(extractor, node, symbol_map);
        }

        // Member access: $obj->property
        "member_access_expression" => {
            // Skip if parent is a call expression (handled above)
            if let Some(parent) = node.parent()
                && (parent.kind() == "function_call_expression"
                    || parent.kind() == "member_call_expression")
            {
                return; // Skip - handled by call expressions
            }

            // Extract the member name (rightmost identifier)
            if let Some(name_node) = node.child_by_field_name("name") {
                let name = extractor.get_base().get_node_text(&name_node);
                let containing_symbol_id = find_containing_symbol_id(extractor, node, symbol_map);

                extractor.get_base_mut().create_identifier(
                    &name_node,
                    name,
                    IdentifierKind::MemberAccess,
                    containing_symbol_id,
                );
            }
        }

        // Type annotations: parameter types, return types, property types.
        // PHP tree-sitter uses `named_type` for class/interface type references
        // (e.g., Request, Response, App) and `primitive_type` for builtins
        // (e.g., int, string, void). We only create type_usage for named_type.
        //
        // named_type appears in:
        //   - Parameter types:  function handle(Request $req)
        //   - Return types:     function handle(): Response
        //   - Property types:   public Request $request
        //   - Union types:      string|Request  (named_type inside union_type)
        //   - Optional types:   ?Request        (named_type inside optional_type)
        "named_type" => {
            let name = extractor.get_base().get_node_text(&node);

            // Skip single-letter type params (rare in PHP, but possible)
            if name.len() <= 1 {
                return;
            }

            let containing_symbol_id = find_containing_symbol_id(extractor, node, symbol_map);

            extractor.get_base_mut().create_identifier(
                &node,
                name,
                IdentifierKind::TypeUsage,
                containing_symbol_id,
            );
        }

        // instanceof expressions: $obj instanceof Router
        // PHP tree-sitter represents this as binary_expression with an
        // "instanceof" anonymous child. The type name after instanceof is
        // a `name` node.
        "binary_expression" => {
            let mut cursor = node.walk();
            let mut found_instanceof = false;
            for child in node.children(&mut cursor) {
                if found_instanceof && child.is_named() {
                    let name = extractor.get_base().get_node_text(&child);

                    // Skip single-letter names
                    if name.len() <= 1 {
                        return;
                    }

                    let containing_symbol_id =
                        find_containing_symbol_id(extractor, node, symbol_map);

                    extractor.get_base_mut().create_identifier(
                        &child,
                        name,
                        IdentifierKind::TypeUsage,
                        containing_symbol_id,
                    );
                    return;
                }
                if child.kind() == "instanceof" {
                    found_instanceof = true;
                }
            }
        }

        // `variable_ref` complement arm, `$variable` half (locked contract — see
        // the doc comment in csharp/identifiers.rs): a `variable_name` used as a
        // value or as the object/receiver of a member access — the reads the
        // Call/MemberAccess/TypeUsage arms above do not own. The row uses the
        // sigil-free inner `name` text (`$total` -> `total`), matching
        // `extract_variable_assignment` symbol naming so name-liveness matches.
        "variable_name" => {
            if is_php_value_read_variable(node) {
                let name = php_variable_bare_name(extractor.get_base(), node);
                // Rule 5: `$this` is a receiver convention, never a symbol name.
                if let Some(name) = name
                    && name != "this"
                {
                    let containing_symbol_id =
                        find_containing_symbol_id(extractor, node, symbol_map);
                    extractor.get_base_mut().create_identifier(
                        &node,
                        name,
                        IdentifierKind::VariableRef,
                        containing_symbol_id,
                    );
                }
            }
        }

        // `variable_ref` complement arm, bare-`name` half: constants in value
        // position (`echo VISIBILITY_UNKNOWN`) and class receivers of static
        // access (`GraphTraversal` in `GraphTraversal::reach()`) — names the
        // Call/MemberAccess/TypeUsage arms above do not own.
        "name" if is_php_value_read_name(node) => {
            let name = extractor.get_base().get_node_text(&node);
            let containing_symbol_id = find_containing_symbol_id(extractor, node, symbol_map);
            extractor.get_base_mut().create_identifier(
                &node,
                name,
                IdentifierKind::VariableRef,
                containing_symbol_id,
            );
        }

        _ => {
            // Skip other node types for now
        }
    }
}

/// The sigil-free name of a `variable_name` node (`$total` -> `total`).
fn php_variable_bare_name(base: &BaseExtractor, node: Node) -> Option<String> {
    let mut cursor = node.walk();
    let name_node = node
        .named_children(&mut cursor)
        .find(|c| c.kind() == "name");
    match name_node {
        Some(n) => Some(base.get_node_text(&n)),
        None => {
            let text = base.get_node_text(&node);
            let trimmed = text.trim_start_matches('$');
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            }
        }
    }
}

/// Rule 1/4 predicate for the `variable_ref` arm, `$variable` half: is this
/// `variable_name` a value read or a receiver read? Inclusive by default with
/// enumerated exclusions, mirroring `is_csharp_value_read_identifier`. Node
/// kinds and field names verified against the vendored tree-sitter-php 0.24.2
/// grammar.
fn is_php_value_read_variable(node: Node) -> bool {
    let Some(parent) = node.parent() else {
        return false;
    };
    let is_name_field = parent.child_by_field_name("name").map(|n| n.id()) == Some(node.id());

    match parent.kind() {
        // Rule 4: a plain assignment LHS is write-only (also PHP's declaration
        // form for locals); a compound assignment reads both sides.
        "assignment_expression" => {
            parent.child_by_field_name("left").map(|l| l.id()) != Some(node.id())
        }
        "augmented_assignment_expression" => true,
        // Rule 1/2: only the object/scope receiver of a member/scoped access is
        // our read; the accessed member `name` is owned by the Call/MemberAccess
        // arms (or is member-shaped for scoped property access).
        "member_access_expression"
        | "member_call_expression"
        | "nullsafe_member_access_expression"
        | "nullsafe_member_call_expression" => {
            parent.child_by_field_name("object").map(|o| o.id()) == Some(node.id())
        }
        "scoped_call_expression" | "scoped_property_access_expression" => {
            parent.child_by_field_name("scope").map(|s| s.id()) == Some(node.id())
        }
        // Rule 3: declarations — parameters, properties, static/global binders,
        // catch binders.
        "simple_parameter"
        | "variadic_parameter"
        | "property_promotion_parameter"
        | "property_element"
        | "static_variable_declaration"
        | "global_declaration"
        | "catch_clause" => false,
        // Rule 4: `[$a, $b] = …` / `list($a, $b) = …` destructuring targets.
        "list_literal" => false,
        // Rule 4: `foreach ($items as $item)` — the source before `as` is a
        // read; bound variables after `as` are writes.
        "foreach_statement" => php_precedes_as_keyword(parent, node),
        "pair" | "by_ref" => {
            // A `$k => $v` pair (or by-ref binder) directly under foreach binds;
            // pairs in array literals are value reads.
            !parent
                .parent()
                .map(|gp| gp.kind() == "foreach_statement")
                .unwrap_or(false)
        }
        // Dynamic member names (`$obj->$prop`) ride in a `name` field — the
        // member side is rule-2 territory; receivers were handled above.
        _ if is_name_field => !matches!(parent.kind(), "member_access_expression"),
        // Every other value slot — argument, array element, echo/print operand,
        // condition, return value, interpolation, use-clause capture — is a read.
        _ => true,
    }
}

/// Rule 1/4 predicate for the `variable_ref` arm, bare-`name` half. PHP `name`
/// nodes appear in many syntactic positions; only genuine value reads and
/// static-access receivers qualify. Verified against tree-sitter-php 0.24.2.
fn is_php_value_read_name(node: Node) -> bool {
    let Some(parent) = node.parent() else {
        return false;
    };

    match parent.kind() {
        // Rule 2: the inner `name` of a `$variable` is owned by the
        // `variable_name` arm — never emit it twice.
        "variable_name" => false,
        // Rule 2: callees and accessed member names are owned by the Call/
        // MemberAccess arms; only the static `scope` receiver is our read.
        "function_call_expression"
        | "member_call_expression"
        | "member_access_expression"
        | "nullsafe_member_call_expression"
        | "nullsafe_member_access_expression" => false,
        "scoped_call_expression" | "scoped_property_access_expression" => {
            parent.child_by_field_name("scope").map(|s| s.id()) == Some(node.id())
        }
        // `Foo::BAR`: the first named child is the class receiver (a read); the
        // accessed constant is member-shaped (rule 2, unowned today).
        "class_constant_access_expression" => {
            let mut cursor = parent.walk();
            parent.named_children(&mut cursor).next().map(|c| c.id()) == Some(node.id())
        }
        // Rule 2: type positions (owned by the TypeUsage arm or type-shaped).
        "named_type"
        | "object_creation_expression"
        | "base_clause"
        | "class_interface_clause"
        | "attribute" => false,
        // Rule 3: namespace/import machinery and aliases.
        "namespace_name"
        | "qualified_name"
        | "namespace_use_clause"
        | "namespace_aliasing_clause"
        | "namespace_definition"
        | "namespace_use_group"
        | "use_declaration" => false,
        // Rule 3: declaration names.
        "class_declaration"
        | "interface_declaration"
        | "trait_declaration"
        | "enum_declaration"
        | "enum_case"
        | "function_definition"
        | "method_declaration"
        | "const_element"
        | "const_declaration" => false,
        // Per plan: named-argument LABELS (`foo(bar: 5)`) are parameter refs — skip.
        "argument" => parent.child_by_field_name("name").map(|n| n.id()) != Some(node.id()),
        // Rule 2: the `instanceof` RHS is owned by the binary_expression
        // TypeUsage arm; other binary operands are reads.
        "binary_expression" => {
            let is_instanceof_rhs = parent
                .child_by_field_name("operator")
                .map(|op| op.kind() == "instanceof")
                .unwrap_or(false)
                && parent.child_by_field_name("right").map(|r| r.id()) == Some(node.id());
            !is_instanceof_rhs
        }
        // Every other value slot — echo operand, array element, condition,
        // argument value, return value — is a read.
        _ => true,
    }
}

/// True when `node` starts before the `as` keyword of a `foreach` statement —
/// i.e. it is the iterated source (a read), not a bound loop variable.
fn php_precedes_as_keyword(foreach_node: Node, node: Node) -> bool {
    let mut cursor = foreach_node.walk();
    let as_start = foreach_node
        .children(&mut cursor)
        .find(|c| c.kind() == "as")
        .map(|c| c.start_byte());
    match as_start {
        Some(as_start) => node.start_byte() < as_start,
        None => true,
    }
}

/// Find the ID of the symbol that contains this node
/// CRITICAL: Only search symbols from THIS FILE (file-scoped filtering)
fn find_containing_symbol_id(
    extractor: &PhpExtractor,
    node: Node,
    symbol_map: &HashMap<String, &Symbol>,
) -> Option<String> {
    extractor
        .get_base()
        .find_containing_symbol_from_map(&node, symbol_map)
        .map(|s| s.id.clone())
}

// ============================================================================
// String-literal call-argument capture (Miller bridge Phase 3b)
// ============================================================================

/// Capture string-literal arguments of a PHP call (`function_call_expression`,
/// `member_call_expression`, `scoped_call_expression`) as `Literal` records.
///
/// Config-free: `carrier` is the verbatim callee — the bare function `name` for a
/// function call, the `object.name` join for a method call (`$client.get`), or the
/// `scope.name` join for a static call (`Http.get`). `kind` stays `Other`; the
/// `src/` carrier gate sets the authoritative kind and drops non-carrier literals.
/// `arg_position` counts over the full argument list. Named-argument labels are
/// skipped via `php_argument_value` (the value is the argument's last named child).
fn record_php_call_arg_literals(
    extractor: &mut PhpExtractor,
    call_node: Node,
    symbol_map: &HashMap<String, &Symbol>,
) {
    let Some(args_node) = call_node.child_by_field_name("arguments") else {
        return;
    };
    let carrier = php_carrier(extractor.get_base(), call_node);
    let containing_symbol_id = find_containing_symbol_id(extractor, call_node, symbol_map);

    let mut cursor = args_node.walk();
    for (pos, arg) in args_node.named_children(&mut cursor).enumerate() {
        let Some(value) = php_argument_value(arg) else {
            continue;
        };
        if let Some(text) = extractor.get_base().decode_string_literal(&value) {
            extractor.get_base_mut().record_literal(
                &value,
                text,
                carrier.clone(),
                pos as u32,
                containing_symbol_id.clone(),
            );
        }
    }
}

/// Resolve a PHP `argument` node to its value node.
///
/// A positional argument wraps its value directly (`(argument (encapsed_string …))`);
/// a named argument carries a `name:` label first (`foo(label: "v")`), so the value
/// is the *last* named child. Reference (`&$x`) and spread (`...$a`) modifiers ride
/// as anonymous tokens, so the last named child is still the value expression.
fn php_argument_value(arg: Node) -> Option<Node> {
    if arg.kind() != "argument" {
        return None;
    }
    let mut cursor = arg.walk();
    arg.named_children(&mut cursor).last()
}

/// Derive a PHP call's carrier from its callee shape.
///
/// `function_call_expression` → the bare `function` text (`mysqli_query`).
/// `member_call_expression` → `object.name` (`$pdo.query`) so a local-variable
/// receiver still matches a bare method config (`query`, `prepare`) via the gate's
/// last-segment rule. `scoped_call_expression` → `scope.name` (`Http.get`) so a
/// dotted facade config matches exactly.
fn php_carrier(base: &BaseExtractor, call_node: Node) -> Option<String> {
    match call_node.kind() {
        "function_call_expression" => call_node
            .child_by_field_name("function")
            .map(|n| base.get_node_text(&n)),
        "member_call_expression" => {
            let object = call_node
                .child_by_field_name("object")
                .map(|n| base.get_node_text(&n));
            let name = call_node
                .child_by_field_name("name")
                .map(|n| base.get_node_text(&n));
            match (object, name) {
                (Some(o), Some(n)) => Some(format!("{o}.{n}")),
                (None, Some(n)) => Some(n),
                _ => None,
            }
        }
        "scoped_call_expression" => {
            let scope = call_node
                .child_by_field_name("scope")
                .map(|n| base.get_node_text(&n));
            let name = call_node
                .child_by_field_name("name")
                .map(|n| base.get_node_text(&n));
            match (scope, name) {
                (Some(s), Some(n)) => Some(format!("{s}.{n}")),
                (None, Some(n)) => Some(n),
                _ => None,
            }
        }
        _ => None,
    }
}
