//! Relationship extraction for Scala (inheritance, calls)
//!
//! Handles extends/implements relationships and function call relationships.

use crate::base::{
    BaseExtractor, LocalTargetResolution, Relationship, RelationshipKind, ScopedSymbolIndex,
    Symbol, SymbolKind, UnresolvedTarget,
};
use crate::scala::ScalaExtractor;
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};
use serde_json::Value;
use std::collections::HashMap;
use tree_sitter::Node;

/// Extract inheritance relationships from extends clauses, and from the type
/// a given instance with a template body implements.
pub(super) fn extract_inheritance_relationships(
    extractor: &mut ScalaExtractor,
    node: &Node,
    symbols: &[Symbol],
    relationships: &mut Vec<Relationship>,
) {
    let base = extractor.base();
    let Some(class_symbol) = find_type_symbol(base, node, symbols) else {
        return;
    };

    let base_types = collect_extends_types(extractor.base(), node);
    let file_path = extractor.base().file_path.clone();
    let line_number = (node.start_position().row + 1) as u32;

    for (target, syntax_relationship_kind) in base_types {
        let base_type_symbol = symbols.iter().find(|s| {
            target.receiver.is_none()
                && s.name == target.terminal_name
                && matches!(
                    s.kind,
                    SymbolKind::Class
                        | SymbolKind::Trait
                        | SymbolKind::Interface
                        | SymbolKind::Enum
                )
        });

        if let Some(base_type_symbol) = base_type_symbol {
            let relationship_kind = if base_type_symbol.kind == SymbolKind::Trait {
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
                    Value::String(target.terminal_name.clone()),
                )])),
            });
        } else {
            let pending_kind = if class_symbol.kind == SymbolKind::Trait {
                RelationshipKind::Extends
            } else {
                syntax_relationship_kind
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

/// The supertypes of a definition, without type arguments, split into
/// terminal name, receiver and namespace: `play.api.libs.json.Reads[Money]`
/// targets `Reads`. A given with a template body (`given Show[Int] with`)
/// implements its type; an alias given (`given ec: EC = ...`) implements
/// nothing.
fn collect_extends_types(
    base: &BaseExtractor,
    node: &Node,
) -> Vec<(UnresolvedTarget, RelationshipKind)> {
    let target = |type_node: Node| {
        let type_node = if type_node.kind() == "generic_type" {
            type_node.child_by_field_name("type").unwrap_or(type_node)
        } else {
            type_node
        };
        let text = base.get_node_text(&type_node);
        UnresolvedTarget::from_qualified_text(&text, &["."])
            .unwrap_or_else(|| UnresolvedTarget::simple(text))
    };

    if node.kind() == "given_definition" {
        let implements = node
            .child_by_field_name("body")
            .is_some_and(|body| body.kind() == "with_template_body");
        return node
            .child_by_field_name("return_type")
            .filter(|_| implements)
            .map(|given_type| vec![(target(given_type), RelationshipKind::Implements)])
            .unwrap_or_default();
    }

    let mut types = Vec::new();
    let extends_clause = node
        .children(&mut node.walk())
        .find(|n| n.kind() == "extends_clause");

    if let Some(ec) = extends_clause {
        let mut next_kind = RelationshipKind::Extends;
        for child in ec.children(&mut ec.walk()) {
            if child.kind() == "with" {
                next_kind = RelationshipKind::Implements;
            } else if matches!(
                child.kind(),
                "type_identifier" | "generic_type" | "stable_type_identifier"
            ) {
                types.push((target(child), next_kind.clone()));
                next_kind = RelationshipKind::Implements;
            }
        }
    }

    types
}

/// The symbol a definition node declared: the same name at the same start
/// byte, so a companion object and its class keep their own edges.
fn find_type_symbol<'a>(
    base: &BaseExtractor,
    node: &Node,
    symbols: &'a [Symbol],
) -> Option<&'a Symbol> {
    symbols.iter().find(|s| {
        s.start_byte as usize == node.start_byte()
            && s.file_path == base.file_path
            && matches!(
                s.kind,
                SymbolKind::Class
                    | SymbolKind::Trait
                    | SymbolKind::Interface
                    | SymbolKind::Enum
                    | SymbolKind::EnumMember
                    | SymbolKind::Variable
            )
    })
}

/// Extract function/method call relationships
pub(super) fn extract_call_relationships(
    extractor: &mut ScalaExtractor,
    node: Node,
    symbols: &[Symbol],
    relationships: &mut Vec<Relationship>,
    depth: u32,
) {
    let symbol_index = ScopedSymbolIndex::new(symbols);

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
    extractor: &mut ScalaExtractor,
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
        "call_expression" => call_target(extractor, node),
        "instance_expression" => instance_target(extractor, node),
        "infix_expression" => infix_target(extractor, node),
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

enum CallSite {
    /// A method call. `receiver_is_expression` marks a receiver that is a call
    /// result or other expression, so the callee cannot be a same-file method
    /// found by its bare name.
    Method {
        target: UnresolvedTarget,
        receiver_is_expression: bool,
    },
    /// `new Type(...)` calls the constructor of `Type`.
    Constructor(UnresolvedTarget),
}

/// The callee of `f(x)`, `a.b.f(x)` or `f[T](x)`. A call whose callee is
/// itself a call (`f(a)(b)`, `new Box(1)(2)`) names no new method.
fn call_target(extractor: &ScalaExtractor, node: Node) -> Option<CallSite> {
    let base = extractor.base();
    let mut function = node.child_by_field_name("function")?;
    if function.kind() == "generic_function" {
        function = function.child_by_field_name("function")?;
    }
    match function.kind() {
        // `def this() = this(0)` delegates to the primary constructor, which
        // is the class itself.
        "identifier" if base.get_node_text(&function) == "this" => {
            super::helpers::enclosing_type_name(base, &node)
                .map(|class_name| CallSite::Constructor(UnresolvedTarget::simple(class_name)))
        }
        "identifier" => Some(CallSite::Method {
            target: UnresolvedTarget::simple(base.get_node_text(&function)),
            receiver_is_expression: false,
        }),
        "field_expression" => {
            let terminal_name = base.get_node_text(&function.child_by_field_name("field")?);
            if super::identifiers::field_value_keyword(base, function) == Some("this") {
                return Some(CallSite::Method {
                    target: UnresolvedTarget {
                        display_name: format!("this.{terminal_name}"),
                        terminal_name,
                        receiver: Some("this".to_string()),
                        namespace_path: Vec::new(),
                        import_context: None,
                    },
                    receiver_is_expression: false,
                });
            }
            let mut parts = vec![terminal_name.clone()];
            let mut value = function.child_by_field_name("value");
            let mut receiver_is_expression = false;
            while let Some(current) = value {
                match current.kind() {
                    "identifier" => {
                        parts.push(base.get_node_text(&current));
                        break;
                    }
                    "field_expression" => {
                        parts.push(base.get_node_text(&current.child_by_field_name("field")?));
                        value = current.child_by_field_name("value");
                    }
                    _ => {
                        receiver_is_expression = true;
                        break;
                    }
                }
            }
            parts.reverse();
            Some(CallSite::Method {
                target: if receiver_is_expression {
                    UnresolvedTarget::simple(terminal_name)
                } else {
                    UnresolvedTarget::from_chain(parts)
                },
                receiver_is_expression,
            })
        }
        _ => None,
    }
}

/// `a plus b` calls `plus` on `a`; `xs map f filter g` calls `filter` on an
/// expression. Symbolic operators are not method calls here.
fn infix_target(extractor: &ScalaExtractor, node: Node) -> Option<CallSite> {
    let base = extractor.base();
    let operator = node
        .child_by_field_name("operator")
        .filter(|operator| operator.kind() == "identifier")?;
    let method = base.get_node_text(&operator);
    let left = node.child_by_field_name("left")?;
    Some(if left.kind() == "identifier" {
        CallSite::Method {
            target: UnresolvedTarget::from_chain(vec![base.get_node_text(&left), method]),
            receiver_is_expression: false,
        }
    } else {
        CallSite::Method {
            target: UnresolvedTarget::simple(method),
            receiver_is_expression: true,
        }
    })
}

fn instance_target(extractor: &ScalaExtractor, node: Node) -> Option<CallSite> {
    let base = extractor.base();
    let mut type_node = node.named_children(&mut node.walk()).find(|child| {
        matches!(
            child.kind(),
            "type_identifier" | "stable_type_identifier" | "generic_type"
        )
    })?;
    if type_node.kind() == "generic_type" {
        type_node = type_node.child_by_field_name("type")?;
    }
    let text = base.get_node_text(&type_node);
    Some(CallSite::Constructor(
        UnresolvedTarget::from_qualified_text(&text, &["."])
            .unwrap_or_else(|| UnresolvedTarget::simple(text)),
    ))
}

fn emit_call(
    extractor: &mut ScalaExtractor,
    node: Node,
    call: CallSite,
    symbol_index: &ScopedSymbolIndex<'_>,
    all_symbols: &[Symbol],
    relationships: &mut Vec<Relationship>,
) {
    let Some(caller) = find_caller(node, all_symbols) else {
        return;
    };
    if is_test_clause_callee(caller, node) {
        return;
    }
    let receiver_type = super::identifiers::self_receiver_type(extractor.base(), node);

    let (target, resolution) = match call {
        CallSite::Method {
            target,
            receiver_is_expression: true,
        } => (target, LocalTargetResolution::ReceiverQualified),
        CallSite::Method { target, .. } => {
            let resolution = symbol_index.resolve_call_target(
                &target.terminal_name,
                Some(caller),
                target.receiver.as_deref(),
            );
            (target, resolution)
        }
        CallSite::Constructor(target) => {
            let mut classes = all_symbols.iter().filter(|symbol| {
                target.receiver.is_none()
                    && symbol.name == target.terminal_name
                    && symbol.kind == SymbolKind::Class
            });
            let resolution = match (classes.next(), classes.next()) {
                (Some(class), None) => LocalTargetResolution::Resolved(class),
                _ => LocalTargetResolution::Missing,
            };
            (target, resolution)
        }
    };

    let confidence = match resolution {
        LocalTargetResolution::Resolved(called_symbol) => {
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
                file_path: extractor.base().file_path.clone(),
                line_number: node.start_position().row as u32 + 1,
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

/// The innermost callable around a call, else the innermost symbol. A local
/// `val` inside a method therefore never owns the calls in its initializer,
/// while a class-level or top-level `val` does.
fn find_caller<'a>(node: Node, symbols: &'a [Symbol]) -> Option<&'a Symbol> {
    let innermost = |include: fn(&Symbol) -> bool| {
        symbols
            .iter()
            .filter(|symbol| {
                include(symbol)
                    && node.start_byte() >= symbol.start_byte as usize
                    && node.end_byte() <= symbol.end_byte as usize
            })
            .min_by_key(|symbol| symbol.end_byte - symbol.start_byte)
    };
    innermost(|symbol| {
        matches!(
            symbol.kind,
            SymbolKind::Function | SymbolKind::Method | SymbolKind::Constructor
        )
    })
    .or_else(|| innermost(|_| true))
}

/// Is `node` the DSL call that produced `caller`? `test("n") { }` becomes a
/// test symbol spanning the outer call, and its callee is the inner call
/// `test("n")`; a call edge from the case to `test` describes nothing.
fn is_test_clause_callee(caller: &Symbol, node: Node) -> bool {
    let spans = |node: Node| {
        caller.start_byte as usize == node.start_byte()
            && caller.end_byte as usize == node.end_byte()
    };
    spans(node)
        || node.parent().is_some_and(|parent| {
            let callee_field = match parent.kind() {
                "call_expression" => "function",
                "infix_expression" => "left",
                _ => return false,
            };
            parent.child_by_field_name(callee_field).map(|f| f.id()) == Some(node.id())
                && spans(parent)
        })
}
