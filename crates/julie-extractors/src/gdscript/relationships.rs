//! Relationship extraction for GDScript
//! Handles function call relationships (including cross-file pending relationships)

use super::super::base::{
    BaseExtractor, LocalTargetResolution, Relationship, RelationshipKind, ScopedSymbolIndex,
    StructuredPendingRelationship, Symbol, SymbolKind, UnresolvedTarget,
};
use super::GDScriptExtractor;
use super::helpers::DeclarationIndex;
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};
use std::collections::HashMap;
use tree_sitter::{Node, Tree};

/// Extract relationships from GDScript code
pub(super) fn extract_relationships(
    extractor: &mut GDScriptExtractor,
    tree: &Tree,
    symbols: &[Symbol],
) -> Vec<Relationship> {
    let mut relationships = Vec::new();
    let scoped_index = ScopedSymbolIndex::new(symbols);
    let containing_symbols = DeclarationIndex::new(&extractor.base, symbols);
    let class_members = ClassMembers::new(symbols);

    extract_metadata_inheritance_relationships(extractor, symbols, &mut relationships);

    // Recursively visit all nodes to extract relationships
    let resolver = Resolver {
        containing_symbols: &containing_symbols,
        scoped_index: &scoped_index,
        class_members: &class_members,
    };
    visit_node_for_relationships(
        extractor,
        tree.root_node(),
        &resolver,
        &mut relationships,
        0,
    );

    relationships
}

fn extract_metadata_inheritance_relationships(
    extractor: &mut GDScriptExtractor,
    symbols: &[Symbol],
    relationships: &mut Vec<Relationship>,
) {
    for class_symbol in symbols
        .iter()
        .filter(|symbol| symbol.kind == SymbolKind::Class)
    {
        let Some(base_class) = class_symbol
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.get("baseClass"))
            .and_then(|value| value.as_str())
            .filter(|base_class| !base_class.is_empty())
        else {
            continue;
        };

        if is_builtin_gdscript_base_class(base_class) {
            continue;
        }

        if let Some(base_symbol) = symbols.iter().find(|symbol| {
            symbol.id != class_symbol.id
                && symbol.name == base_class
                && symbol.kind == SymbolKind::Class
        }) {
            relationships.push(Relationship {
                id: format!(
                    "{}_{}_{:?}_{}",
                    class_symbol.id,
                    base_symbol.id,
                    RelationshipKind::Extends,
                    class_symbol.start_line
                ),
                from_symbol_id: class_symbol.id.clone(),
                to_symbol_id: base_symbol.id.clone(),
                kind: RelationshipKind::Extends,
                file_path: extractor.base.file_path.clone(),
                line_number: class_symbol.start_line,
                span: base_class_span(&extractor.base.content, class_symbol, base_class),
                reference_site_is_exact: false,
                confidence: 0.95,
                metadata: None,
            });
        } else {
            let mut pending = StructuredPendingRelationship::new(
                class_symbol.id.clone(),
                UnresolvedTarget::simple(base_class.to_string()),
                Some(class_symbol.id.clone()),
                RelationshipKind::Extends,
                extractor.base.file_path.clone(),
                class_symbol.start_line,
                0.8,
            );
            pending.span = base_class_span(&extractor.base.content, class_symbol, base_class);
            extractor.add_structured_pending_relationship(pending);
        }
    }
}

/// The base-class name on the `extends` line of a class header. A script's
/// `extends` can sit on the line before or after its `class_name`.
fn base_class_span(
    content: &str,
    class_symbol: &Symbol,
    base_class: &str,
) -> Option<crate::base::NormalizedSpan> {
    let line = class_symbol.start_line;
    [
        Some(line),
        Some(line + 1),
        line.checked_sub(1),
        Some(line + 2),
    ]
    .into_iter()
    .flatten()
    .filter(|&candidate| {
        content
            .lines()
            .nth(candidate.saturating_sub(1) as usize)
            .is_some_and(|text| text.contains("extends"))
    })
    .find_map(|candidate| {
        crate::base::NormalizedSpan::from_line_occurrence(content, candidate, base_class)
    })
}

fn is_builtin_gdscript_base_class(name: &str) -> bool {
    matches!(
        name,
        "Object"
            | "RefCounted"
            | "Resource"
            | "Node"
            | "Node2D"
            | "Node3D"
            | "Control"
            | "CanvasItem"
            | "CanvasLayer"
            | "Area2D"
            | "Area3D"
            | "CharacterBody2D"
            | "CharacterBody3D"
            | "RigidBody2D"
            | "RigidBody3D"
            | "StaticBody2D"
            | "StaticBody3D"
            | "Sprite2D"
            | "Sprite3D"
            | "Camera2D"
            | "Camera3D"
            | "Label"
            | "Button"
            | "Panel"
            | "AnimationPlayer"
            | "AudioStreamPlayer"
            | "Timer"
    )
}

/// Visit a node and extract relationships from it
fn visit_node_for_relationships(
    extractor: &mut GDScriptExtractor,
    node: Node,
    resolver: &Resolver<'_>,
    relationships: &mut Vec<Relationship>,
    depth: u32,
) {
    if !should_visit_tree_depth(depth) {
        return;
    }

    match node.kind() {
        "call" => {
            let mut cursor = node.walk();
            let callee = node
                .children(&mut cursor)
                .find(|child| child.kind() == "identifier");
            if let Some(callee) = callee {
                let target = UnresolvedTarget::simple(extractor.base.get_node_text(&callee));
                extract_call_relationship(
                    extractor,
                    CallSite {
                        node,
                        target,
                        receiver_type: None,
                    },
                    resolver,
                    relationships,
                );
            }
        }
        "getter" | "setter" => {
            let target = UnresolvedTarget::simple(extractor.base.get_node_text(&node));
            extract_call_relationship(
                extractor,
                CallSite {
                    node,
                    target,
                    receiver_type: None,
                },
                resolver,
                relationships,
            );
        }
        "attribute" => {
            for call_site in attribute_call_sites(&extractor.base, node) {
                extract_call_relationship(extractor, call_site, resolver, relationships);
            }
        }
        _ => {}
    }

    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        visit_node_for_relationships(extractor, child, resolver, relationships, child_depth);
    }
}

struct Resolver<'a> {
    containing_symbols: &'a DeclarationIndex<'a>,
    scoped_index: &'a ScopedSymbolIndex<'a>,
    class_members: &'a ClassMembers<'a>,
}

/// The callable members of each class, so a bare `name()` call inside a class
/// resolves to that class's own method: GDScript calls it on `self`.
struct ClassMembers<'a> {
    by_id: HashMap<&'a str, &'a Symbol>,
    callables: HashMap<(&'a str, &'a str), &'a Symbol>,
}

impl<'a> ClassMembers<'a> {
    fn new(symbols: &'a [Symbol]) -> Self {
        let by_id = symbols
            .iter()
            .map(|symbol| (symbol.id.as_str(), symbol))
            .collect();
        let callables = symbols
            .iter()
            .filter(|symbol| {
                matches!(
                    symbol.kind,
                    SymbolKind::Method | SymbolKind::Function | SymbolKind::Constructor
                )
            })
            .filter_map(|symbol| {
                Some(((symbol.parent_id.as_deref()?, symbol.name.as_str()), symbol))
            })
            .collect();
        Self { by_id, callables }
    }

    fn implicit_self_target(&self, caller: &Symbol, name: &str) -> Option<&'a Symbol> {
        let mut owner = caller.parent_id.as_deref();
        while let Some(id) = owner {
            let symbol = self.by_id.get(id)?;
            if symbol.kind == SymbolKind::Class {
                return self.callables.get(&(id, name)).copied();
            }
            owner = symbol.parent_id.as_deref();
        }
        None
    }
}

struct CallSite<'tree> {
    node: Node<'tree>,
    target: UnresolvedTarget,
    receiver_type: Option<String>,
}

/// One call site per `attribute_call` segment of a flat chain. The receiver is
/// the source up to the previous segment, never text from the arguments.
fn attribute_call_sites<'tree>(base: &BaseExtractor, node: Node<'tree>) -> Vec<CallSite<'tree>> {
    let mut cursor = node.walk();
    let children: Vec<Node> = node.named_children(&mut cursor).collect();
    let mut sites = Vec::new();
    for index in 1..children.len() {
        let segment = children[index];
        if segment.kind() != "attribute_call" {
            continue;
        }
        let mut call_cursor = segment.walk();
        let Some(name_node) = segment
            .children(&mut call_cursor)
            .find(|child| child.kind() == "identifier")
        else {
            continue;
        };
        let start = node.start_byte();
        let receiver = base.content[start..children[index - 1].end_byte()].to_string();
        let display_name = base.content[start..segment.end_byte()].to_string();
        let receiver_type = if index == 1 {
            super::identifiers::attribute_receiver_type(base, node)
        } else {
            None
        };
        sites.push(CallSite {
            node: segment,
            target: qualified_target(receiver, base.get_node_text(&name_node), display_name),
            receiver_type,
        });
    }
    sites
}

/// Emit a same-file `calls` relationship, or a pending one, from the
/// declaration that owns the call site.
fn extract_call_relationship(
    extractor: &mut GDScriptExtractor,
    call_site: CallSite<'_>,
    resolver: &Resolver<'_>,
    relationships: &mut Vec<Relationship>,
) {
    let CallSite {
        node,
        target,
        receiver_type,
    } = call_site;
    if target.terminal_name.is_empty() {
        return;
    }
    let Some(caller_symbol) = resolver.containing_symbols.find(node).filter(|symbol| {
        matches!(
            symbol.kind,
            SymbolKind::Function
                | SymbolKind::Method
                | SymbolKind::Constructor
                | SymbolKind::Field
                | SymbolKind::Constant
        )
    }) else {
        return;
    };

    let implicit_self = (target.receiver.is_none() && target.namespace_path.is_empty())
        .then(|| {
            resolver
                .class_members
                .implicit_self_target(caller_symbol, &target.terminal_name)
        })
        .flatten();
    let resolution = match implicit_self {
        Some(member) => LocalTargetResolution::Resolved(member),
        None => resolver.scoped_index.resolve_call_target(
            &target.terminal_name,
            Some(caller_symbol),
            target.receiver.as_deref(),
        ),
    };
    match resolution {
        LocalTargetResolution::Resolved(called_symbol) => {
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
                file_path: extractor.base.file_path.clone(),
                line_number: (node.start_position().row + 1) as u32,
                span: Some(crate::base::NormalizedSpan::from_node(&node)),
                reference_site_is_exact: false,
                confidence: 0.9,
                metadata: None,
            });
        }
        LocalTargetResolution::Import(_)
        | LocalTargetResolution::Ambiguous
        | LocalTargetResolution::Missing
        | LocalTargetResolution::ReceiverQualified => {
            let pending = extractor
                .base
                .create_pending_relationship(
                    caller_symbol.id.clone(),
                    target,
                    RelationshipKind::Calls,
                    &node,
                    Some(caller_symbol.id.clone()),
                    Some(0.7),
                )
                .with_receiver_type(receiver_type);
            extractor.add_structured_pending_relationship(pending);
        }
    }
}

/// Splits a plain-identifier chain of three or more parts into receiver and
/// namespace. A two-part or non-identifier receiver keeps the node text as
/// its display name, which the golden fixtures record with the call suffix.
fn qualified_target(
    receiver: String,
    terminal_name: String,
    display_name: String,
) -> UnresolvedTarget {
    UnresolvedTarget::from_qualified_text(&format!("{receiver}.{terminal_name}"), &["."])
        .filter(|target| !target.namespace_path.is_empty())
        .unwrap_or(UnresolvedTarget {
            display_name,
            terminal_name,
            receiver: Some(receiver),
            namespace_path: Vec::new(),
            import_context: None,
        })
}
