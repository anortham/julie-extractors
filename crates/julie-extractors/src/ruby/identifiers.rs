use super::helpers::{
    extract_method_name_from_call, is_assignment_target, method_symbol_arguments,
};
use super::locals::LocalBindings;
use super::type_facts;
use crate::base::{BaseExtractor, ContainingSymbolIndex, Identifier, IdentifierKind, Symbol};
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};
use std::collections::HashSet;
use tree_sitter::{Node, Tree};

/// Extract all identifier usages (function calls, member access, etc.)
/// Following the Rust extractor reference implementation pattern
pub(super) fn extract_identifiers(
    base: &mut BaseExtractor,
    tree: &Tree,
    symbols: &[Symbol],
) -> Vec<Identifier> {
    let containing_symbols = base.containing_symbol_index(symbols);
    let mut locals = LocalBindings::default();
    let declarations: HashSet<(u32, &str)> = symbols
        .iter()
        .map(|symbol| (symbol.start_byte, symbol.name.as_str()))
        .collect();
    let context = IdentifierContext {
        containing_symbols: &containing_symbols,
        declarations: &declarations,
    };

    walk_tree_for_identifiers(base, tree.root_node(), &context, &mut locals, 0);

    // Return the collected identifiers
    base.identifiers.clone()
}

struct IdentifierContext<'a, 'b> {
    containing_symbols: &'b ContainingSymbolIndex<'a>,
    /// `(start_byte, name)` of every symbol, to tell the assignment that
    /// declares a field from a later write to it.
    declarations: &'b HashSet<(u32, &'a str)>,
}

/// Recursively walk tree extracting identifiers from each node
fn walk_tree_for_identifiers(
    base: &mut BaseExtractor,
    node: Node,
    context: &IdentifierContext<'_, '_>,
    locals: &mut LocalBindings,
    depth: u32,
) {
    if !should_visit_tree_depth(depth) {
        return;
    }

    extract_identifier_from_node(base, node, context, locals);

    // Recursively walk children
    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk_tree_for_identifiers(base, child, context, locals, child_depth);
    }
}

/// Extract identifier from a single node based on its kind
/// Ruby-specific: "call" nodes are used for both function calls and member access
fn extract_identifier_from_node(
    base: &mut BaseExtractor,
    node: Node,
    context: &IdentifierContext<'_, '_>,
    locals: &mut LocalBindings,
) {
    let containing_symbols = context.containing_symbols;
    match node.kind() {
        // Ruby uses "call" for both function calls and member access
        // The difference is whether there's a receiver field
        "call" => {
            // Check if this call has a receiver (member access)
            if let Some(_receiver) = node.child_by_field_name("receiver") {
                if let Some(method_node) = node.child_by_field_name("method") {
                    let name = base.get_node_text(&method_node);
                    let containing_symbol_id = find_containing_symbol_id(node, containing_symbols);
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
                                find_containing_symbol_id(node, containing_symbols);

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
            for argument in method_symbol_arguments(base, node) {
                let containing_symbol_id =
                    find_containing_symbol_id(argument.node, containing_symbols);
                base.create_identifier(
                    &argument.node,
                    argument.name,
                    IdentifierKind::Call,
                    containing_symbol_id,
                );
            }
            // Phase 3b: capture string-literal call-arguments config-free; the
            // carrier classification + bloat gate run later in the artifact language-policy pass.
            record_ruby_call_arg_literals(base, node, containing_symbols);
        }

        // `@repo`, `@@count`, and `$stdout` reads and writes reference their
        // variable. The assignment that declares a field is its definition.
        "instance_variable" | "class_variable" | "global_variable" => {
            let name = base.get_node_text(&node);
            if is_declaring_assignment_target(node, &name, context.declarations) {
                return;
            }
            let containing_symbol_id = find_containing_symbol_id(node, containing_symbols);
            base.create_identifier(
                &node,
                name,
                IdentifierKind::VariableRef,
                containing_symbol_id,
            );
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
            let containing_symbol_id = find_containing_symbol_id(node, containing_symbols);

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
                let containing_symbol_id = find_containing_symbol_id(node, containing_symbols);
                let kind = if locals.is_method_call(&base.content, node) {
                    IdentifierKind::Call
                } else {
                    IdentifierKind::VariableRef
                };

                base.create_identifier(&node, name, kind, containing_symbol_id);
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
pub(super) fn is_ruby_value_read_identifier(node: Node) -> bool {
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

fn is_declaring_assignment_target(
    node: Node,
    name: &str,
    declarations: &HashSet<(u32, &str)>,
) -> bool {
    node.parent().is_some_and(|assignment| {
        matches!(assignment.kind(), "assignment" | "operator_assignment")
            && assignment
                .child_by_field_name("left")
                .is_some_and(|left| left.id() == node.id())
            && declarations.contains(&(assignment.start_byte() as u32, name))
    })
}

/// Returns true if this constant node is the declaration name of a class or module.
/// Example: `class Foo` or `module Bar` — the `Foo`/`Bar` constant is a declaration,
/// not a reference. All other constant positions are type references.
fn is_constant_declaration_name(node: &Node) -> bool {
    let is_name_of = |parent: Node, child: Node| {
        parent
            .child_by_field_name("name")
            .is_some_and(|name| name.id() == child.id())
    };
    let Some(parent) = node.parent() else {
        return false;
    };
    if !is_name_of(parent, *node) {
        return false;
    }
    match parent.kind() {
        "class" | "module" => true,
        "scope_resolution" => parent.parent().is_some_and(|declaration| {
            matches!(declaration.kind(), "class" | "module") && is_name_of(declaration, parent)
        }),
        _ => false,
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
// String-literal call-argument capture
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
    containing_symbols: &ContainingSymbolIndex<'_>,
) {
    let Some(args_node) = call_node.child_by_field_name("arguments") else {
        return;
    };
    let carrier = ruby_carrier(base, call_node);
    let containing_symbol_id = find_containing_symbol_id(call_node, containing_symbols);
    let sql_fragment_builder = call_node
        .child_by_field_name("method")
        .is_some_and(|method| is_sql_fragment_builder(&base.get_node_text(&method)));

    let mut cursor = args_node.walk();
    for (pos, arg) in args_node.named_children(&mut cursor).enumerate() {
        // `where("created_at > ?", t)` takes SQL only as its first positional
        // argument; its keyword values are data, not SQL.
        if sql_fragment_builder && (pos > 0 || arg.kind() == "pair") {
            continue;
        }
        if let Some(heredoc_body) = heredoc_argument_body(arg) {
            let text = decode_heredoc(base, heredoc_body);
            base.record_literal(
                &heredoc_body,
                text,
                carrier.clone(),
                pos as u32,
                containing_symbol_id.clone(),
            );
            continue;
        }
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

/// ActiveRecord query builders whose first string argument is a SQL fragment.
fn is_sql_fragment_builder(method: &str) -> bool {
    matches!(method, "where" | "order" | "joins" | "having" | "reorder")
}

/// The body of a heredoc passed as an argument: `execute(<<~SQL)` or
/// `find_by_sql(<<-SQL.squish)`. The body follows the statement that opens
/// it; with several heredocs on one line, bodies come in opening order.
fn heredoc_argument_body(argument: Node<'_>) -> Option<Node<'_>> {
    let beginning = match argument.kind() {
        "heredoc_beginning" => argument,
        "call" => argument
            .child_by_field_name("receiver")
            .filter(|receiver| receiver.kind() == "heredoc_beginning")?,
        _ => return None,
    };
    let mut statement = beginning;
    loop {
        let parent = statement.parent()?;
        let mut sibling = statement.next_sibling();
        let mut bodies = Vec::new();
        while let Some(body) = sibling.filter(|node| node.kind() == "heredoc_body") {
            bodies.push(body);
            sibling = body.next_sibling();
        }
        if !bodies.is_empty() {
            let index = heredoc_beginnings_before(statement, beginning);
            return bodies.get(index).copied();
        }
        statement = parent;
    }
}

/// How many heredocs open inside `statement` before `beginning`.
fn heredoc_beginnings_before(statement: Node, beginning: Node) -> usize {
    let mut count = 0;
    let mut stack = vec![statement];
    let mut steps = 0usize;
    while let Some(node) = stack.pop() {
        steps += 1;
        if steps > 10_000 {
            break;
        }
        if node.kind() == "heredoc_beginning" && node.start_byte() < beginning.start_byte() {
            count += 1;
        }
        let mut cursor = node.walk();
        let children: Vec<Node> = node.named_children(&mut cursor).collect();
        stack.extend(children.into_iter().rev());
    }
    count
}

/// The heredoc body's text with interpolations as `{}`, its common
/// indentation removed, and surrounding blank lines trimmed.
fn decode_heredoc(base: &BaseExtractor, body: Node) -> String {
    let mut text = String::new();
    let mut cursor = body.walk();
    for child in body.named_children(&mut cursor) {
        match child.kind() {
            "heredoc_content" | "escape_sequence" => text.push_str(&base.get_node_text(&child)),
            "interpolation" => text.push_str("{}"),
            _ => {}
        }
    }
    let indent = text
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| line.len() - line.trim_start().len())
        .min()
        .unwrap_or(0);
    text.lines()
        .map(|line| line.get(indent..).unwrap_or(line.trim_start()))
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string()
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
