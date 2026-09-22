use super::helpers::{declared_name, extract_method_name_from_call, is_self_directed_call};
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
            if let Some(target) = symbol_argument_call_target(extractor.base(), node) {
                record_call(extractor, node, target, context, relationships);
            }
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
    if let Some(superclass_node) = node.child_by_field_name("superclass") {
        let Some(class_name) = declared_name(base, node) else {
            return;
        };

        let superclass_name = base
            .get_node_text(&superclass_node)
            .replace('<', "")
            .trim()
            .to_string();

        let Some(from_symbol) = find_type_symbol(symbols, &class_name, node) else {
            return;
        };

        if let Some(to_symbol) = symbols.iter().find(|s| {
            is_type_symbol(s)
                && (s.name == superclass_name || qualified_name(s) == Some(&superclass_name))
        }) {
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
            let pending = extractor.base().create_pending_relationship(
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
        && let Some(module_node) = arg_node.children(&mut arg_node.walk()).next()
    {
        let module_name = base.get_node_text(&module_node);

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
                line_number: child.start_position().row as u32 + 1,
                span: Some(crate::base::NormalizedSpan::from_node(&child)),
                reference_site_is_exact: false,
                confidence: 1.0,
                metadata: None,
            });
        } else if let Some(from_symbol) = from_symbol {
            let pending = extractor.base().create_pending_relationship(
                from_symbol.id.clone(),
                unresolved_ruby_constant(module_name),
                RelationshipKind::Implements,
                &child,
                Some(from_symbol.id.clone()),
                Some(0.9),
            );
            extractor.add_structured_pending_relationship(pending);
        }
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

/// `send(:audit)`, `public_send(:audit)`, `__send__(:audit)`, `method(:audit)`,
/// and a `&:price` block argument name a method by symbol.
fn symbol_argument_call_target(
    base: &crate::base::BaseExtractor,
    node: Node,
) -> Option<CallTarget> {
    let method = base.get_node_text(&node.child_by_field_name("method")?);
    let arguments = node.child_by_field_name("arguments")?;
    let first = arguments.named_child(0)?;
    let symbol = match first.kind() {
        "simple_symbol"
            if matches!(
                method.as_str(),
                "send" | "public_send" | "__send__" | "method"
            ) =>
        {
            first
        }
        "block_argument" => first
            .named_child(0)
            .filter(|child| child.kind() == "simple_symbol")?,
        _ => return None,
    };
    let name = base
        .get_node_text(&symbol)
        .trim_start_matches(':')
        .to_string();
    let resolve_locally = first.kind() != "block_argument" && is_self_directed_call(node);
    Some(CallTarget {
        target: UnresolvedTarget::simple(name),
        resolve_locally,
        receiver_type: None,
    })
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

    if let Some(parent_id) = caller.parent_id.as_deref() {
        let same_parent_callables: Vec<&Symbol> = symbol_index
            .candidates_by_name(method_name)
            .filter(|symbol| {
                matches!(
                    symbol.kind,
                    SymbolKind::Function | SymbolKind::Method | SymbolKind::Constructor
                ) && symbol.parent_id.as_deref() == Some(parent_id)
            })
            .collect();

        match same_parent_callables.as_slice() {
            [symbol] => return LocalTargetResolution::Resolved(symbol),
            [] => {}
            _ => return LocalTargetResolution::Ambiguous,
        }
    }

    symbol_index.resolve_call_target(method_name, Some(caller), None)
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
