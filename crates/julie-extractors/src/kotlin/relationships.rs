//! Relationship extraction for Kotlin (inheritance, implementation, calls)
//!
//! This module handles extraction of inheritance, interface implementation,
//! and method/function call relationships.

use crate::base::{
    BaseExtractor, LocalTargetResolution, Relationship, RelationshipKind, ScopedSymbolIndex,
    Symbol, SymbolKind, UnresolvedTarget,
};
use crate::kotlin::KotlinExtractor;
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};
use serde_json::Value;
use std::collections::HashMap;
use tree_sitter::Node;

struct BaseTypeEntry {
    target: UnresolvedTarget,
    is_constructor_invocation: bool,
}

/// Extract inheritance and implementation relationships from a Kotlin type.
/// Targets drop type arguments and split qualified names: `Repository<User>`
/// targets `Repository`, and `JsonAdapter.Factory` targets `Factory` with
/// receiver `JsonAdapter`.
pub(super) fn extract_inheritance_relationships(
    extractor: &mut KotlinExtractor,
    node: &Node,
    symbols: &[Symbol],
    relationships: &mut Vec<Relationship>,
) {
    let Some(class_symbol) = find_class_symbol(extractor.base(), node, symbols) else {
        return;
    };

    let base_type_entries = collect_base_type_entries(extractor.base(), node);
    let file_path = extractor.base().file_path.clone();
    let line_number = (node.start_position().row + 1) as u32;

    for base_type_entry in base_type_entries {
        let target = base_type_entry.target;
        let base_type_symbol = target
            .receiver
            .is_none()
            .then(|| {
                symbols.iter().find(|s| {
                    s.name == target.terminal_name
                        && s.file_path == file_path
                        && matches!(
                            s.kind,
                            SymbolKind::Class | SymbolKind::Interface | SymbolKind::Struct
                        )
                })
            })
            .flatten();

        if let Some(base_type_symbol) = base_type_symbol {
            let relationship_kind = if base_type_symbol.kind == SymbolKind::Interface {
                RelationshipKind::Implements
            } else {
                RelationshipKind::Extends
            };

            relationships.push(Relationship {
                id: format!(
                    "{}_{}_{:?}_{}",
                    class_symbol.id,
                    base_type_symbol.id,
                    relationship_kind,
                    node.start_position().row
                ),
                from_symbol_id: class_symbol.id.clone(),
                to_symbol_id: base_type_symbol.id.clone(),
                kind: relationship_kind,
                file_path: file_path.clone(),
                line_number,
                span: Some(crate::base::NormalizedSpan::from_node(node)),
                reference_site_is_exact: false,
                confidence: 1.0,
                metadata: Some(HashMap::from([(
                    "baseType".to_string(),
                    Value::String(target.terminal_name),
                )])),
            });
        } else {
            // An interface extends its supertypes, and a constructor invocation
            // (`BaseModel()`) names a superclass; every other supertype is an
            // implemented interface.
            let pending_kind = if class_symbol.kind == SymbolKind::Interface
                || base_type_entry.is_constructor_invocation
            {
                RelationshipKind::Extends
            } else {
                RelationshipKind::Implements
            };

            let pending = extractor.base().create_pending_relationship(
                class_symbol.id.clone(),
                target,
                pending_kind,
                node,
                Some(class_symbol.id.clone()),
                Some(0.9),
            );
            extractor.add_structured_pending_relationship(pending);
        }
    }
}

/// One entry per delegation specifier: `Base()`, `Iface`, and `Iface by impl`.
fn collect_base_type_entries(base: &BaseExtractor, node: &Node) -> Vec<BaseTypeEntry> {
    let container = node
        .children(&mut node.walk())
        .find(|n| n.kind() == "delegation_specifiers")
        .unwrap_or(*node);
    let specifiers: Vec<Node> = container
        .children(&mut container.walk())
        .filter(|n| n.kind() == "delegation_specifier")
        .collect();

    specifiers
        .into_iter()
        .filter_map(|specifier| {
            let type_node = specifier.children(&mut specifier.walk()).find(|n| {
                matches!(
                    n.kind(),
                    "type"
                        | "user_type"
                        | "identifier"
                        | "constructor_invocation"
                        | "explicit_delegation"
                        | "delegated_super_type"
                )
            })?;
            let is_constructor_invocation = type_node.kind() == "constructor_invocation";
            let named = match type_node.kind() {
                "constructor_invocation" | "explicit_delegation" | "delegated_super_type" => {
                    type_node
                        .children(&mut type_node.walk())
                        .find(|n| matches!(n.kind(), "user_type" | "type" | "identifier"))?
                }
                _ => type_node,
            };
            Some(BaseTypeEntry {
                target: type_target(base, named),
                is_constructor_invocation,
            })
        })
        .collect()
}

/// The target of a type reference: the `identifier` segments of a `user_type`
/// without type arguments or backticks.
fn type_target(base: &BaseExtractor, type_node: Node) -> UnresolvedTarget {
    UnresolvedTarget::from_chain(type_target_parts(base, type_node))
}

fn type_target_parts(base: &BaseExtractor, type_node: Node) -> Vec<String> {
    if type_node.kind() == "user_type" {
        type_node
            .children(&mut type_node.walk())
            .filter(|n| n.kind() == "identifier")
            .map(|n| call_name(base, &n))
            .collect()
    } else {
        vec![call_name(base, &type_node)]
    }
}

/// The symbol declared by this class, interface, enum or object node, matched
/// by name and end byte so same-named nested types keep their own edges.
fn find_class_symbol<'a>(
    base: &BaseExtractor,
    node: &Node,
    symbols: &'a [Symbol],
) -> Option<&'a Symbol> {
    let (class_name, _) = super::helpers::declared_name(base, node)?;

    symbols.iter().find(|s| {
        s.name == class_name
            && s.end_byte as usize == node.end_byte()
            && matches!(
                s.kind,
                SymbolKind::Class | SymbolKind::Interface | SymbolKind::Enum | SymbolKind::Struct
            )
            && s.file_path == base.file_path
    })
}

/// Extract function/method call relationships
///
/// Creates resolved Relationship when target is a local function.
/// Creates PendingRelationship when target is:
/// - Not found in local symbol_map (e.g., method on imported type)
pub(super) fn extract_call_relationships(
    extractor: &mut KotlinExtractor,
    node: Node,
    symbols: &[Symbol],
    relationships: &mut Vec<Relationship>,
    depth: u32,
) {
    let symbol_index = ScopedSymbolIndex::new(symbols);

    // Find call expression nodes in this subtree
    walk_tree_for_calls(
        extractor,
        node,
        &symbol_index,
        symbols,
        relationships,
        depth,
    );
}

fn walk_tree_for_calls(
    extractor: &mut KotlinExtractor,
    node: Node,
    symbol_index: &ScopedSymbolIndex<'_>,
    all_symbols: &[Symbol],
    relationships: &mut Vec<Relationship>,
    depth: u32,
) {
    if !should_visit_tree_depth(depth) {
        return;
    }

    let call = match node.kind() {
        "call_expression" => call_expression_target(extractor, node),
        "infix_expression" => infix_call_target(extractor, node),
        "callable_reference" | "navigation_expression" => {
            function_reference_target(extractor, node)
        }
        _ => None,
    };
    if let Some(call) = call {
        emit_call(
            extractor,
            node,
            call,
            symbol_index,
            all_symbols,
            relationships,
        );
    }

    // Recursively process children
    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk_tree_for_calls(
            extractor,
            child,
            symbol_index,
            all_symbols,
            relationships,
            child_depth,
        );
    }
}

struct CallSite {
    target: UnresolvedTarget,
    /// The receiver is an expression result (`a().b()`, `list.map {}.c()`),
    /// so the callee cannot be a same-file function found by bare name.
    receiver_is_expression: bool,
}

fn call_expression_target(extractor: &KotlinExtractor, node: Node) -> Option<CallSite> {
    let base = extractor.base();
    let mut function_name = None;
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "identifier" || child.kind() == "simple_identifier" {
            function_name = Some(call_name(base, &child));
            break;
        }
        if child.kind() == "navigation_expression" {
            let mut nav_cursor = child.walk();
            let last_id = child
                .children(&mut nav_cursor)
                .filter(|nav_child| {
                    nav_child.kind() == "identifier" || nav_child.kind() == "simple_identifier"
                })
                .last()
                .map(|nav_child| call_name(base, &nav_child));
            if last_id.is_some() {
                function_name = last_id;
                break;
            }
        }
    }
    let function_name = function_name?;
    let (target, receiver_is_expression) = unresolved_call_target(extractor, node, &function_name);
    Some(CallSite {
        target,
        receiver_is_expression,
    })
}

/// `a plusTax 20` calls `plusTax` on `a`.
fn infix_call_target(extractor: &KotlinExtractor, node: Node) -> Option<CallSite> {
    let base = extractor.base();
    let operator = node.child(1).filter(|child| child.kind() == "identifier")?;
    let name = call_name(base, &operator);
    let left = node.child(0)?;
    Some(match left.kind() {
        "identifier" => CallSite {
            target: UnresolvedTarget::from_chain(vec![call_name(base, &left), name]),
            receiver_is_expression: false,
        },
        "this_expression" | "super_expression" => CallSite {
            target: UnresolvedTarget::simple(name),
            receiver_is_expression: false,
        },
        _ => CallSite {
            target: UnresolvedTarget::simple(name),
            receiver_is_expression: true,
        },
    })
}

/// `::isValid` and `Type::member` reference a function; `Type::class` does not.
fn function_reference_target(extractor: &KotlinExtractor, node: Node) -> Option<CallSite> {
    let base = extractor.base();
    let is_reference = node.kind() == "callable_reference"
        || node
            .children(&mut node.walk())
            .any(|child| child.kind() == "::");
    if !is_reference {
        return None;
    }
    let mut cursor = node.walk();
    let named: Vec<Node> = node.named_children(&mut cursor).collect();
    let member = named
        .last()
        .filter(|member| member.kind() == "identifier")?;
    let name = call_name(base, member);
    if name == "class" {
        return None;
    }
    let target = match named.as_slice() {
        [receiver, _] if receiver.kind() == "identifier" => {
            UnresolvedTarget::from_chain(vec![call_name(base, receiver), name])
        }
        [receiver, _] if receiver.kind() == "user_type" => {
            let mut parts = type_target_parts(base, *receiver);
            parts.push(name);
            UnresolvedTarget::from_chain(parts)
        }
        _ => UnresolvedTarget::simple(name),
    };
    Some(CallSite {
        target,
        receiver_is_expression: false,
    })
}

fn emit_call(
    extractor: &mut KotlinExtractor,
    node: Node,
    call: CallSite,
    symbol_index: &ScopedSymbolIndex<'_>,
    all_symbols: &[Symbol],
    relationships: &mut Vec<Relationship>,
) {
    let Some(caller) = extractor.base().find_containing_symbol(&node, all_symbols) else {
        return;
    };

    if symbol_spans_node(caller, &node) {
        return;
    }

    let CallSite {
        target,
        receiver_is_expression,
    } = call;
    let line_number = node.start_position().row as u32 + 1;
    let file_path = extractor.base().file_path.clone();
    let receiver_type = super::identifiers::self_receiver_type(extractor.base(), node);

    let resolution = if receiver_is_expression {
        LocalTargetResolution::ReceiverQualified
    } else {
        match symbol_index.resolve_call_target(
            target.terminal_name.as_str(),
            Some(caller),
            target.receiver.as_deref(),
        ) {
            LocalTargetResolution::ReceiverQualified => {
                match extension_target(extractor.base(), &target, caller, all_symbols) {
                    Some(extension) => LocalTargetResolution::Resolved(extension),
                    None => LocalTargetResolution::ReceiverQualified,
                }
            }
            resolution => resolution,
        }
    };

    let confidence = match resolution {
        LocalTargetResolution::Resolved(called_symbol)
            if !extractor.is_dsl_call_symbol(&called_symbol.id) =>
        {
            relationships.push(Relationship {
                id: format!(
                    "{}_{}_{:?}_{}",
                    caller.id,
                    called_symbol.id,
                    RelationshipKind::Calls,
                    node.start_position().row
                ),
                from_symbol_id: caller.id.clone(),
                to_symbol_id: called_symbol.id.clone(),
                kind: RelationshipKind::Calls,
                file_path,
                line_number,
                span: Some(crate::base::NormalizedSpan::from_node(&node)),
                reference_site_is_exact: false,
                confidence: 0.9,
                metadata: None,
            });
            return;
        }
        LocalTargetResolution::Import(_) => 0.8,
        _ => 0.7,
    };
    let pending = extractor
        .base()
        .create_pending_relationship(
            caller.id.clone(),
            target,
            RelationshipKind::Calls,
            &node,
            Some(caller.id.clone()),
            Some(confidence),
        )
        .with_receiver_type(receiver_type);
    extractor.add_structured_pending_relationship(pending);
}

/// `value.ext()` resolves to the same-file extension function `ext` whose
/// receiver type is the declared type of `value`, a parameter or local of the
/// caller or a top-level property.
fn extension_target<'a>(
    base: &BaseExtractor,
    target: &UnresolvedTarget,
    caller: &Symbol,
    all_symbols: &'a [Symbol],
) -> Option<&'a Symbol> {
    let receiver = target.receiver.as_deref()?;
    if !target.namespace_path.is_empty() {
        return None;
    }
    let receiver_symbol = all_symbols
        .iter()
        .filter(|symbol| {
            symbol.name == receiver
                && matches!(symbol.kind, SymbolKind::Variable | SymbolKind::Property)
        })
        .find(|symbol| symbol.parent_id.as_deref() == Some(caller.id.as_str()))
        .or_else(|| {
            all_symbols.iter().find(|symbol| {
                symbol.name == receiver
                    && symbol.parent_id.is_none()
                    && matches!(symbol.kind, SymbolKind::Property | SymbolKind::Constant)
            })
        })?;
    let receiver_type = &base.type_info.get(&receiver_symbol.id)?.resolved_type;
    let mut candidates = all_symbols.iter().filter(|symbol| {
        symbol.name == target.terminal_name
            && matches!(symbol.kind, SymbolKind::Function | SymbolKind::Method)
            && symbol
                .metadata
                .as_ref()
                .and_then(|metadata| metadata.get("extendedType"))
                .and_then(Value::as_str)
                .is_some_and(|extended| type_base_name(extended) == receiver_type)
    });
    let extension = candidates.next()?;
    candidates.next().is_none().then_some(extension)
}

/// `List<User>?` reduces to `List`.
fn type_base_name(type_text: &str) -> &str {
    type_text
        .split(['<', '?'])
        .next()
        .unwrap_or(type_text)
        .trim()
}

/// Is `symbol` the symbol this very call node produced?
///
/// A Kotest or Spek lifecycle hook is written `beforeEach { }`, so the call
/// adapter names the symbol after the callee. The containing-symbol lookup then
/// returns that same symbol for the call node it was built from, and resolving
/// the callee finds it again — a `beforeEach` calls `beforeEach` edge that
/// describes nothing. A declaration never shares a span with a call expression,
/// so a recursive function keeps its real self-call edge.
fn symbol_spans_node(symbol: &Symbol, node: &Node) -> bool {
    let spans = |node: &Node| {
        symbol.start_byte as usize == node.start_byte()
            && symbol.end_byte as usize == node.end_byte()
    };
    // `test("name") { }` parses as a call whose callee is the call `test("name")`.
    spans(node)
        || node.parent().is_some_and(|parent| {
            parent.kind() == "call_expression"
                && parent.named_child(0).map(|callee| callee.id()) == Some(node.id())
                && spans(&parent)
        })
}

fn call_name(base: &BaseExtractor, node: &Node) -> String {
    super::helpers::strip_backticks(&base.get_node_text(node)).to_string()
}

fn unresolved_call_target(
    extractor: &KotlinExtractor,
    node: Node,
    fallback_name: &str,
) -> (UnresolvedTarget, bool) {
    let base = extractor.base();
    let mut parts = Vec::new();
    let mut receiver_is_expression = false;
    let mut current = node.named_child(0);
    while let Some(expression) = current {
        match expression.kind() {
            "identifier" | "simple_identifier" => {
                parts.push(call_name(base, &expression));
                break;
            }
            "navigation_expression" => {
                let mut cursor = expression.walk();
                let named: Vec<Node> = expression.named_children(&mut cursor).collect();
                let (Some(receiver), Some(member)) = (named.first(), named.last()) else {
                    break;
                };
                if named.len() != 2 || !matches!(member.kind(), "identifier" | "simple_identifier")
                {
                    break;
                }
                parts.push(call_name(base, member));
                current = Some(*receiver);
            }
            "this_expression" | "super_expression" => break,
            _ => {
                receiver_is_expression = !parts.is_empty();
                break;
            }
        }
    }
    parts.reverse();

    if parts.len() >= 2 && !receiver_is_expression {
        return (UnresolvedTarget::from_chain(parts), false);
    }

    (
        UnresolvedTarget::simple(fallback_name.to_string()),
        receiver_is_expression,
    )
}
