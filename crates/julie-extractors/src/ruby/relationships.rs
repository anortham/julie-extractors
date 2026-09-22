use super::helpers::{
    declared_name, extract_method_name_from_call, is_self_directed_call, method_symbol_arguments,
};
use super::identifiers;
use super::locals::LocalBindings;
/// Relationship extraction for Ruby symbols
/// Handles inheritance, module inclusion, and other symbol relationships
use crate::base::{
    ContainingSymbolIndex, LocalTargetResolution, Relationship, RelationshipKind,
    ScopedSymbolIndex, Symbol, SymbolKind, UnresolvedTarget,
};
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};
use tree_sitter::Node;

/// Extract all relationships from a tree
pub(super) fn extract_relationships(
    extractor: &mut super::RubyExtractor,
    tree: &tree_sitter::Tree,
    symbols: &[Symbol],
) -> Vec<Relationship> {
    let mut relationships = Vec::new();
    let symbol_index = ScopedSymbolIndex::new(symbols);
    let containing_symbols = extractor.base().containing_symbol_index(symbols);
    let mut context = CallContext {
        symbol_index: &symbol_index,
        containing_symbols: &containing_symbols,
        locals: LocalBindings::default(),
    };

    extract_relationships_from_node(
        extractor,
        tree.root_node(),
        symbols,
        &mut context,
        &mut relationships,
        0,
    );
    relationships
}

struct CallContext<'a, 'b> {
    symbol_index: &'b ScopedSymbolIndex<'a>,
    containing_symbols: &'b ContainingSymbolIndex<'a>,
    locals: LocalBindings,
}

/// Recursively extract relationships from a node
fn extract_relationships_from_node(
    extractor: &mut super::RubyExtractor,
    node: Node,
    symbols: &[Symbol],
    context: &mut CallContext<'_, '_>,
    relationships: &mut Vec<Relationship>,
    depth: u32,
) {
    if !should_visit_tree_depth(depth) {
        return;
    }

    match node.kind() {
        "class" => {
            extract_inheritance_relationship(extractor, node, symbols, relationships);
            extract_module_inclusion_relationships(extractor, node, symbols, relationships);
        }
        "module" => {
            extract_module_inclusion_relationships(extractor, node, symbols, relationships);
        }
        "call" => {
            if let Some(target) = call_target(extractor.base(), node) {
                record_call(extractor, node, target, context, relationships);
            }
            for argument in method_symbol_arguments(extractor.base(), node) {
                let target = CallTarget {
                    target: UnresolvedTarget::simple(argument.name),
                    resolve_locally: argument.resolve_locally,
                    receiver_type: None,
                };
                record_call(extractor, argument.node, target, context, relationships);
            }
            record_shared_example_reference(extractor, node, symbols, context, relationships);
        }
        "assignment" => {
            extract_class_new_inheritance(extractor, node, symbols, relationships);
        }
        "super" if !is_call_method(node) => {
            if let Some(target) = super_call_target(extractor.base(), node) {
                record_call(extractor, node, target, context, relationships);
            }
        }
        "identifier" => {
            let content = extractor.base().content.clone();
            if identifiers::is_ruby_value_read_identifier(node)
                && context.locals.is_method_call(&content, node)
            {
                let target = CallTarget {
                    target: UnresolvedTarget::simple(extractor.base().get_node_text(&node)),
                    resolve_locally: true,
                    receiver_type: None,
                };
                record_call(extractor, node, target, context, relationships);
            }
        }
        _ => {}
    }

    // Recursively process children
    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        extract_relationships_from_node(
            extractor,
            child,
            symbols,
            context,
            relationships,
            child_depth,
        );
    }
}

/// Extract inheritance relationship from class definition
fn extract_inheritance_relationship(
    extractor: &mut super::RubyExtractor,
    node: Node,
    symbols: &[Symbol],
    relationships: &mut Vec<Relationship>,
) {
    let base = extractor.base();
    let Some(class_name) = declared_name(base, node) else {
        return;
    };
    let Some(superclass_name) = super::symbols::extract_superclass_name(base, node) else {
        return;
    };
    let Some(from_symbol) = find_type_symbol(symbols, &class_name, node) else {
        return;
    };
    record_extends(
        extractor,
        node,
        from_symbol,
        superclass_name,
        symbols,
        relationships,
    );
}

/// `Handler = Class.new(StandardError) do ... end` extends `StandardError`.
fn extract_class_new_inheritance(
    extractor: &mut super::RubyExtractor,
    node: Node,
    symbols: &[Symbol],
    relationships: &mut Vec<Relationship>,
) {
    let base = extractor.base();
    let Some(right) = node.child_by_field_name("right") else {
        return;
    };
    if !super::calls::is_class_new(base, right) {
        return;
    }
    let Some(superclass_name) = super::calls::class_new_superclass(base, right) else {
        return;
    };
    let start_byte = node.start_byte() as u32;
    let Some(from_symbol) = symbols
        .iter()
        .find(|symbol| symbol.kind == SymbolKind::Class && symbol.start_byte == start_byte)
    else {
        return;
    };
    record_extends(
        extractor,
        node,
        from_symbol,
        superclass_name,
        symbols,
        relationships,
    );
}

fn record_extends(
    extractor: &mut super::RubyExtractor,
    node: Node,
    from_symbol: &Symbol,
    superclass_name: String,
    symbols: &[Symbol],
    relationships: &mut Vec<Relationship>,
) {
    let base = extractor.base();
    if let Some(to_symbol) = lexical_type_symbol(symbols, from_symbol, &superclass_name) {
        relationships.push(Relationship {
            id: format!(
                "{}_{}_{:?}_{}",
                from_symbol.id,
                to_symbol.id,
                RelationshipKind::Extends,
                node.start_position().row
            ),
            from_symbol_id: from_symbol.id.clone(),
            to_symbol_id: to_symbol.id.clone(),
            kind: RelationshipKind::Extends,
            file_path: base.file_path.clone(),
            line_number: node.start_position().row as u32 + 1,
            span: Some(crate::base::NormalizedSpan::from_node(&node)),
            reference_site_is_exact: false,
            confidence: 1.0,
            metadata: None,
        });
    } else {
        let pending = base.create_pending_relationship(
            from_symbol.id.clone(),
            unresolved_ruby_constant(superclass_name),
            RelationshipKind::Extends,
            &node,
            Some(from_symbol.id.clone()),
            Some(0.9),
        );
        extractor.add_structured_pending_relationship(pending);
    }
}

/// The same-file class or module a constant names from where `from_symbol`
/// is declared, found the way Ruby looks constants up: the innermost
/// enclosing namespace first, then each outer one, then the top level. A
/// `::`-qualified name matches a compact declaration's `qualifiedName`.
fn lexical_type_symbol<'a>(
    symbols: &'a [Symbol],
    from_symbol: &Symbol,
    constant: &str,
) -> Option<&'a Symbol> {
    if constant.contains("::") {
        let constant = constant.trim_start_matches("::");
        return symbols
            .iter()
            .find(|s| is_type_symbol(s) && qualified_name(s) == Some(constant));
    }
    let mut scope = from_symbol.parent_id.clone();
    loop {
        if let Some(found) = symbols.iter().find(|s| {
            is_type_symbol(s)
                && s.name == constant
                && s.parent_id == scope
                && s.id != from_symbol.id
        }) {
            return Some(found);
        }
        let outer = scope
            .as_ref()
            .and_then(|id| symbols.iter().find(|s| &s.id == id))?;
        scope = outer.parent_id.clone();
    }
}

/// `it_behaves_like "a countable"` and its siblings run a shared group: a
/// `references` edge to the same-file group, else a pending reference by name.
fn record_shared_example_reference(
    extractor: &mut super::RubyExtractor,
    node: Node,
    symbols: &[Symbol],
    context: &CallContext<'_, '_>,
    relationships: &mut Vec<Relationship>,
) {
    let base = extractor.base();
    let Some(method) = extract_method_name_from_call(node, |n| base.get_node_text(n)) else {
        return;
    };
    if !matches!(
        method.as_str(),
        "it_behaves_like" | "it_should_behave_like" | "include_examples" | "include_context"
    ) || !is_self_directed_call(node)
    {
        return;
    }
    let Some(group_node) = node
        .child_by_field_name("arguments")
        .and_then(|arguments| arguments.named_child(0))
        .filter(|first| first.kind() == "string")
    else {
        return;
    };
    let Some(group_name) = base.decode_string_literal(&group_node) else {
        return;
    };
    let Some(caller) = context.containing_symbols.find(node) else {
        return;
    };
    if let Some(group) = symbols
        .iter()
        .find(|symbol| symbol.kind == SymbolKind::Namespace && symbol.name == group_name)
    {
        relationships.push(base.create_relationship(
            caller.id.clone(),
            group.id.clone(),
            RelationshipKind::References,
            &group_node,
            Some(0.9),
            None,
        ));
        return;
    }
    let pending = base.create_pending_relationship(
        caller.id.clone(),
        UnresolvedTarget::simple(group_name),
        RelationshipKind::References,
        &group_node,
        Some(caller.id.clone()),
        Some(0.8),
    );
    extractor.add_structured_pending_relationship(pending);
}

/// Extract module inclusion relationships (include, extend, prepend, using)
fn extract_module_inclusion_relationships(
    extractor: &mut super::RubyExtractor,
    node: Node,
    symbols: &[Symbol],
    relationships: &mut Vec<Relationship>,
) {
    let base = extractor.base();
    let Some(class_or_module_name) = declared_name(base, node) else {
        return;
    };

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "call" {
            // Direct call node
            process_include_extend_call(
                extractor,
                child,
                &class_or_module_name,
                symbols,
                relationships,
            );
        } else if child.kind() == "body_statement" {
            // Call might be inside a body_statement
            let mut body_cursor = child.walk();
            for body_child in child.children(&mut body_cursor) {
                if body_child.kind() == "call" {
                    process_include_extend_call(
                        extractor,
                        body_child,
                        &class_or_module_name,
                        symbols,
                        relationships,
                    );
                }
            }
        }
    }
}

/// Process a single include/extend call node
fn process_include_extend_call(
    extractor: &mut super::RubyExtractor,
    child: Node,
    class_or_module_name: &str,
    symbols: &[Symbol],
    relationships: &mut Vec<Relationship>,
) {
    let base = extractor.base();
    if is_self_directed_call(child)
        && let Some(method_name) = extract_method_name_from_call(child, |n| base.get_node_text(n))
        && matches!(
            method_name.as_str(),
            "include" | "extend" | "prepend" | "using"
        )
        && let Some(arg_node) = child.child_by_field_name("arguments")
    {
        let mut cursor = arg_node.walk();
        let modules: Vec<(Node, String)> = arg_node
            .named_children(&mut cursor)
            .filter(|argument| matches!(argument.kind(), "constant" | "scope_resolution"))
            .map(|argument| (argument, base.get_node_text(&argument)))
            .collect();
        for (module_node, module_name) in modules {
            record_inclusion(
                extractor,
                child,
                module_node,
                class_or_module_name,
                module_name,
                symbols,
                relationships,
            );
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn record_inclusion(
    extractor: &mut super::RubyExtractor,
    child: Node,
    module_node: Node,
    class_or_module_name: &str,
    module_name: String,
    symbols: &[Symbol],
    relationships: &mut Vec<Relationship>,
) {
    let base = extractor.base();

    let owner = child.parent().and_then(|parent| {
        if parent.kind() == "body_statement" {
            parent.parent()
        } else {
            Some(parent)
        }
    });
    let from_symbol =
        owner.and_then(|owner| find_type_symbol(symbols, class_or_module_name, owner));
    let to_symbol = symbols
        .iter()
        .find(|s| s.name == module_name && is_type_symbol(s));

    if let (Some(from_symbol), Some(to_symbol)) = (from_symbol, to_symbol) {
        relationships.push(Relationship {
            id: format!(
                "{}_{}_{:?}_{}",
                from_symbol.id,
                to_symbol.id,
                RelationshipKind::Implements,
                child.start_position().row
            ),
            from_symbol_id: from_symbol.id.clone(),
            to_symbol_id: to_symbol.id.clone(),
            kind: RelationshipKind::Implements,
            file_path: base.file_path.clone(),
            line_number: module_node.start_position().row as u32 + 1,
            span: Some(crate::base::NormalizedSpan::from_node(&module_node)),
            reference_site_is_exact: false,
            confidence: 1.0,
            metadata: None,
        });
    } else if let Some(from_symbol) = from_symbol {
        let pending = extractor.base().create_pending_relationship(
            from_symbol.id.clone(),
            unresolved_ruby_constant(module_name),
            RelationshipKind::Implements,
            &module_node,
            Some(from_symbol.id.clone()),
            Some(0.9),
        );
        extractor.add_structured_pending_relationship(pending);
    }
}

/// One call site's target, built from the AST.
struct CallTarget {
    target: UnresolvedTarget,
    /// A receiverless or `self` call may resolve to a same-file method.
    resolve_locally: bool,
    receiver_type: Option<String>,
}

fn record_call(
    extractor: &mut super::RubyExtractor,
    node: Node,
    call: CallTarget,
    context: &CallContext<'_, '_>,
    relationships: &mut Vec<Relationship>,
) {
    if call.target.terminal_name.is_empty() {
        return;
    }
    let Some(caller_symbol) = context.containing_symbols.find(node) else {
        return;
    };
    // A DSL call such as `test "x" do` or `setup do` declares the symbol that
    // contains it; the call is that declaration, not a call from it.
    if caller_symbol.start_byte == node.start_byte() as u32
        && caller_symbol.end_byte == node.end_byte() as u32
    {
        return;
    }
    let base = extractor.base();
    let resolution = if call.resolve_locally && call.target.namespace_path.is_empty() {
        resolve_ruby_call_target(
            context.symbol_index,
            &call.target.terminal_name,
            caller_symbol,
            call.target.receiver.as_deref(),
        )
    } else {
        LocalTargetResolution::Missing
    };
    if let LocalTargetResolution::Resolved(called_symbol) = resolution {
        relationships.push(Relationship {
            id: format!(
                "{}_{}_{:?}_{}",
                caller_symbol.id,
                called_symbol.id,
                RelationshipKind::Calls,
                node.start_position().row
            ),
            from_symbol_id: caller_symbol.id.clone(),
            to_symbol_id: called_symbol.id.clone(),
            kind: RelationshipKind::Calls,
            file_path: base.file_path.clone(),
            line_number: (node.start_position().row + 1) as u32,
            span: Some(crate::base::NormalizedSpan::from_node(&node)),
            reference_site_is_exact: false,
            confidence: 0.9,
            metadata: None,
        });
        return;
    }
    let receiver_type = call
        .receiver_type
        .or_else(|| super::type_facts::self_receiver_type(base, node));
    let pending = base
        .create_pending_relationship(
            caller_symbol.id.clone(),
            call.target,
            RelationshipKind::Calls,
            &node,
            Some(caller_symbol.id.clone()),
            Some(0.7),
        )
        .with_receiver_type(receiver_type);
    extractor.add_structured_pending_relationship(pending);
}

/// The target of a `call` node, read from its `receiver` and `method` fields.
/// A call receiver contributes its method chain without arguments or blocks
/// (`Mailer.with.receipt` for `Mailer.with(to: x).receipt`); a `::` scope
/// receiver contributes its path.
/// A writer target (`self.mode = x`) calls `mode=`.
fn call_target(base: &crate::base::BaseExtractor, node: Node) -> Option<CallTarget> {
    let method = node.child_by_field_name("method")?;
    if method.kind() == "super" {
        return super_call_target(base, node);
    }
    let mut method_name = base.get_node_text(&method);
    if method_name.is_empty() {
        return None;
    }
    let is_writer = node.parent().is_some_and(|parent| {
        matches!(parent.kind(), "assignment" | "operator_assignment")
            && parent
                .child_by_field_name("left")
                .is_some_and(|left| left.id() == node.id())
    });
    if is_writer {
        method_name.push('=');
    }
    let Some(receiver) = node.child_by_field_name("receiver") else {
        return Some(CallTarget {
            target: UnresolvedTarget::simple(method_name),
            resolve_locally: true,
            receiver_type: None,
        });
    };
    let mut parts = receiver_parts(base, receiver);
    let resolve_locally = receiver.kind() == "self";
    if parts.is_empty() {
        return Some(CallTarget {
            target: UnresolvedTarget::simple(method_name),
            resolve_locally: false,
            receiver_type: None,
        });
    }
    parts.push(method_name);
    Some(CallTarget {
        target: UnresolvedTarget::from_chain(parts),
        resolve_locally,
        receiver_type: None,
    })
}

fn receiver_parts(base: &crate::base::BaseExtractor, receiver: Node) -> Vec<String> {
    match receiver.kind() {
        "scope_resolution" => {
            let mut parts = receiver
                .child_by_field_name("scope")
                .map(|scope| receiver_parts(base, scope))
                .unwrap_or_default();
            if let Some(name) = receiver.child_by_field_name("name") {
                parts.push(base.get_node_text(&name));
            }
            parts
        }
        "call" => {
            let mut parts = receiver
                .child_by_field_name("receiver")
                .map(|inner| receiver_parts(base, inner))
                .unwrap_or_default();
            if let Some(method) = receiver.child_by_field_name("method") {
                parts.push(base.get_node_text(&method));
            }
            parts
        }
        "identifier" | "constant" | "self" | "instance_variable" | "class_variable"
        | "global_variable" => vec![base.get_node_text(&receiver)],
        _ => Vec::new(),
    }
}

/// `super` calls the enclosing method's name on the declared superclass.
fn super_call_target(base: &crate::base::BaseExtractor, node: Node) -> Option<CallTarget> {
    let mut current = node.parent();
    let mut method_name = None;
    while let Some(candidate) = current {
        match candidate.kind() {
            "method" | "singleton_method" if method_name.is_none() => {
                method_name = candidate
                    .child_by_field_name("name")
                    .map(|name| base.get_node_text(&name));
            }
            "class" => {
                let superclass = candidate
                    .child_by_field_name("superclass")
                    .and_then(|superclass| superclass.named_child(0))
                    .map(|superclass| base.get_node_text(&superclass));
                let method_name = method_name?;
                return Some(CallTarget {
                    target: UnresolvedTarget {
                        display_name: format!("super.{method_name}"),
                        terminal_name: method_name,
                        receiver: Some("super".to_string()),
                        namespace_path: Vec::new(),
                        import_context: None,
                    },
                    resolve_locally: false,
                    receiver_type: superclass,
                });
            }
            "module" => break,
            _ => {}
        }
        current = candidate.parent();
    }
    let method_name = method_name?;
    Some(CallTarget {
        target: UnresolvedTarget {
            display_name: format!("super.{method_name}"),
            terminal_name: method_name,
            receiver: Some("super".to_string()),
            namespace_path: Vec::new(),
            import_context: None,
        },
        resolve_locally: false,
        receiver_type: None,
    })
}

fn is_call_method(node: Node) -> bool {
    node.parent().is_some_and(|parent| {
        parent.kind() == "call"
            && parent
                .child_by_field_name("method")
                .is_some_and(|method| method.id() == node.id())
    })
}

/// The `qualifiedName` a compact declaration such as `class Api::V1::Base` records.
fn qualified_name(symbol: &Symbol) -> Option<&str> {
    symbol.metadata.as_ref()?.get("qualifiedName")?.as_str()
}

fn is_type_symbol(symbol: &Symbol) -> bool {
    matches!(symbol.kind, SymbolKind::Class | SymbolKind::Module)
}

/// The class or module symbol a `class`/`module` node declared.
fn find_type_symbol<'a>(symbols: &'a [Symbol], name: &str, node: Node) -> Option<&'a Symbol> {
    let start_byte = node.start_byte() as u32;
    symbols
        .iter()
        .find(|s| s.name == name && is_type_symbol(s) && s.start_byte == start_byte)
        .or_else(|| symbols.iter().find(|s| s.name == name && is_type_symbol(s)))
}

fn resolve_ruby_call_target<'a>(
    symbol_index: &ScopedSymbolIndex<'a>,
    method_name: &str,
    caller: &Symbol,
    receiver: Option<&str>,
) -> LocalTargetResolution<'a> {
    if receiver.is_some() {
        return symbol_index.resolve_call_target(method_name, Some(caller), receiver);
    }

    // A class-level macro (`before_action :set_post`) names a method of the
    // class itself.
    let owner_id = if matches!(caller.kind, SymbolKind::Class | SymbolKind::Module) {
        Some(caller.id.as_str())
    } else {
        caller.parent_id.as_deref()
    };
    if let Some(parent_id) = owner_id {
        let same_parent_callables: Vec<&Symbol> = symbol_index
            .candidates_by_name(method_name)
            .filter(|symbol| {
                matches!(
                    symbol.kind,
                    SymbolKind::Function | SymbolKind::Method | SymbolKind::Constructor
                ) && symbol.parent_id.as_deref() == Some(parent_id)
                    && !super::calls::is_hook_block_symbol(symbol)
            })
            .collect();

        match same_parent_callables.as_slice() {
            [symbol] => return LocalTargetResolution::Resolved(symbol),
            [] => {}
            _ => return LocalTargetResolution::Ambiguous,
        }
    }

    match symbol_index.resolve_call_target(method_name, Some(caller), None) {
        LocalTargetResolution::Resolved(symbol) if super::calls::is_hook_block_symbol(symbol) => {
            LocalTargetResolution::Missing
        }
        resolution => resolution,
    }
}

fn unresolved_ruby_constant(name: String) -> UnresolvedTarget {
    let parts: Vec<_> = name
        .split("::")
        .filter(|part| !part.is_empty())
        .map(str::to_string)
        .collect();

    let terminal_name = parts.last().cloned().unwrap_or_else(|| name.clone());
    let namespace_path = parts
        .get(..parts.len().saturating_sub(1))
        .unwrap_or(&[])
        .to_vec();

    UnresolvedTarget {
        display_name: name,
        terminal_name,
        receiver: None,
        namespace_path,
        import_context: None,
    }
}
