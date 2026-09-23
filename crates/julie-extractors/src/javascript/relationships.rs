//! Relationship extraction for JavaScript (calls, inheritance)
//!
//! This module handles extraction of relationships between symbols such as
//! function calls and class inheritance relationships.
//!
//! Adapted from TypeScript extractor (JavaScript and TypeScript share AST structure)

use crate::base::{
    LocalTargetResolution, Relationship, RelationshipKind, ScopedSymbolIndex, Symbol, SymbolKind,
    UnresolvedTarget, is_test_call_symbol,
};
use crate::ecmascript_imports::is_ecmascript_global_direct_target;
use crate::javascript::JavaScriptExtractor;
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};
use tree_sitter::{Node, Tree};

type HeritageData = (String, Vec<(UnresolvedTarget, u32)>, String);

/// Extract all relationships from the syntax tree
pub(crate) fn extract_relationships(
    extractor: &mut JavaScriptExtractor,
    tree: &Tree,
    symbols: &[Symbol],
) -> Vec<Relationship> {
    let mut relationships = Vec::new();
    let symbol_index = ScopedSymbolIndex::new(symbols);
    let owners = super::ecmascript_owner_index(extractor.base(), tree.root_node(), symbols);
    extract_call_relationships(
        extractor,
        tree.root_node(),
        symbols,
        &symbol_index,
        &owners,
        &mut relationships,
        0,
    );
    extract_new_expression_relationships(
        extractor,
        tree.root_node(),
        symbols,
        &symbol_index,
        &owners,
        &mut relationships,
        0,
    );
    extract_inheritance_relationships(extractor, tree.root_node(), symbols, &mut relationships, 0);
    relationships
}

fn extract_new_expression_relationships(
    extractor: &mut JavaScriptExtractor,
    node: Node,
    symbols: &[Symbol],
    symbol_index: &ScopedSymbolIndex<'_>,
    owners: &super::EcmaOwnerIndex<'_>,
    relationships: &mut Vec<Relationship>,
    depth: u32,
) {
    if !should_visit_tree_depth(depth) {
        return;
    }

    if node.kind() == "new_expression"
        && let Some(constructor_node) = node.child_by_field_name("constructor")
    {
        let target = extract_call_target(extractor, constructor_node);
        let caller = owners.find(node);
        if let Some(caller) = caller {
            let resolution = symbol_index.resolve_call_target(
                &target.terminal_name,
                Some(caller),
                target.receiver.as_deref(),
            );
            let constructable_symbol = match &resolution {
                LocalTargetResolution::Resolved(type_symbol)
                    if matches!(
                        type_symbol.kind,
                        SymbolKind::Class | SymbolKind::Type | SymbolKind::Interface
                    ) =>
                {
                    Some(*type_symbol)
                }
                LocalTargetResolution::Resolved(function)
                    if is_constructor_function(symbols, function) =>
                {
                    Some(*function)
                }
                _ if target.receiver.is_none() => {
                    unique_constructable_symbol(symbols, &target.terminal_name)
                }
                _ => None,
            };
            if let Some(type_symbol) = constructable_symbol {
                relationships.push(Relationship {
                    id: format!(
                        "{}_{}_{:?}_{}",
                        caller.id,
                        type_symbol.id,
                        RelationshipKind::Instantiates,
                        node.start_position().row
                    ),
                    from_symbol_id: caller.id.clone(),
                    to_symbol_id: type_symbol.id.clone(),
                    kind: RelationshipKind::Instantiates,
                    file_path: extractor.base().file_path.clone(),
                    line_number: (node.start_position().row + 1) as u32,
                    span: Some(crate::base::NormalizedSpan::from_node(&node)),
                    reference_site_is_exact: false,
                    confidence: 1.0,
                    metadata: None,
                });
            } else if !is_ecmascript_global_direct_target(&target.terminal_name) {
                let pending = extractor.base().create_pending_relationship(
                    caller.id.clone(),
                    target,
                    RelationshipKind::Instantiates,
                    &node,
                    Some(caller.id.clone()),
                    Some(0.9),
                );
                extractor.add_structured_pending_relationship(pending);
            }
        }
    }

    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        extract_new_expression_relationships(
            extractor,
            child,
            symbols,
            symbol_index,
            owners,
            relationships,
            child_depth,
        );
    }
}

/// A function with members assigned to it (`Queue.prototype.clear = ...`):
/// a pre-class constructor, so `new Queue()` instantiates it.
fn is_constructor_function(symbols: &[Symbol], function: &Symbol) -> bool {
    function.kind == SymbolKind::Function
        && symbols.iter().any(|symbol| {
            symbol.parent_id.as_deref() == Some(function.id.as_str())
                && symbol.kind == SymbolKind::Method
        })
}

fn unique_constructable_symbol<'a>(symbols: &'a [Symbol], name: &str) -> Option<&'a Symbol> {
    let mut matches = symbols.iter().filter(|symbol| {
        symbol.name == name
            && (matches!(
                symbol.kind,
                SymbolKind::Class | SymbolKind::Type | SymbolKind::Interface
            ) || is_constructor_function(symbols, symbol))
    });
    let symbol = matches.next()?;
    matches.next().is_none().then_some(symbol)
}

/// Extract function call relationships
fn extract_call_relationships(
    extractor: &mut JavaScriptExtractor,
    node: Node,
    symbols: &[Symbol],
    symbol_index: &ScopedSymbolIndex<'_>,
    owners: &super::EcmaOwnerIndex<'_>,
    relationships: &mut Vec<Relationship>,
    depth: u32,
) {
    if !should_visit_tree_depth(depth) {
        return;
    }

    if let Some(function_node) = call_site_callee(extractor, node)
        && !(extractor.test_dsl_active
            && super::test_symbols::is_test_dsl_call(extractor.base(), node))
    {
        let target = extract_call_target(extractor, function_node);

        // Find the calling function (containing function)
        if let Some(caller_symbol) = owners.find(node) {
            let resolved_symbol = match symbol_index.resolve_call_target(
                &target.terminal_name,
                Some(caller_symbol),
                target.receiver.as_deref(),
            ) {
                LocalTargetResolution::Resolved(symbol) => Some(symbol),
                _ if target.receiver.as_deref() == Some("this") => {
                    constructor_function_member(symbols, caller_symbol, &target.terminal_name)
                }
                _ if target.receiver.is_none() => {
                    unique_callable_symbol(symbols, &target.terminal_name)
                }
                _ => None,
            }
            .filter(|symbol| !is_test_call_symbol(symbol))
            .filter(|symbol| {
                target.receiver.is_some()
                    || !matches!(symbol.kind, SymbolKind::Method | SymbolKind::Constructor)
            });

            if let Some(called_symbol) = resolved_symbol {
                let relationship = Relationship {
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
                    file_path: extractor.base().file_path.clone(),
                    line_number: (node.start_position().row + 1) as u32,
                    span: Some(crate::base::NormalizedSpan::from_node(&node)),
                    reference_site_is_exact: false,
                    confidence: 1.0,
                    metadata: None,
                };
                relationships.push(relationship);
            }
        }
    }

    // Recursively process children
    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        extract_call_relationships(
            extractor,
            child,
            symbols,
            symbol_index,
            owners,
            relationships,
            child_depth,
        );
    }
}

/// `this.m()` inside a pre-class constructor function resolves to a method
/// assigned to that function's prototype.
fn constructor_function_member<'a>(
    symbols: &'a [Symbol],
    caller: &Symbol,
    name: &str,
) -> Option<&'a Symbol> {
    if caller.kind != SymbolKind::Function {
        return None;
    }
    let mut matches = symbols.iter().filter(|symbol| {
        symbol.kind == SymbolKind::Method
            && symbol.name == name
            && symbol.parent_id.as_deref() == Some(caller.id.as_str())
    });
    let symbol = matches.next()?;
    matches.next().is_none().then_some(symbol)
}

fn unique_callable_symbol<'a>(symbols: &'a [Symbol], name: &str) -> Option<&'a Symbol> {
    let mut matches = symbols.iter().filter(|symbol| {
        symbol.name == name
            && !is_test_call_symbol(symbol)
            && matches!(
                symbol.kind,
                SymbolKind::Function | SymbolKind::Method | SymbolKind::Constructor
            )
    });
    let symbol = matches.next()?;
    matches.next().is_none().then_some(symbol)
}

/// The callee of a call site: the `function` of a call expression, or the
/// name of a JSX element that renders a component (capitalized terminal name).
pub(super) fn call_site_callee<'tree>(
    extractor: &JavaScriptExtractor,
    node: Node<'tree>,
) -> Option<Node<'tree>> {
    match node.kind() {
        "call_expression" => node.child_by_field_name("function"),
        "jsx_opening_element" | "jsx_self_closing_element" => {
            let name = node.child_by_field_name("name")?;
            let terminal = match name.kind() {
                "identifier" => name,
                "member_expression" => name.child_by_field_name("property")?,
                _ => return None,
            };
            extractor
                .base()
                .get_node_text(&terminal)
                .starts_with(|first: char| first.is_ascii_uppercase())
                .then_some(name)
        }
        _ => None,
    }
}

/// Collects the identifier chain of `Q1.Q2 ... Qn.t` and its root node; `None`
/// when any link is not a plain identifier (a call result, an index). A
/// `this`/`super` root stays in a two-part chain (`this.m`) and is dropped from
/// a longer one, so `this.repo.save` names the `repo` receiver.
pub(super) fn member_chain<'tree>(
    extractor: &JavaScriptExtractor,
    node: Node<'tree>,
) -> Option<(Vec<String>, Node<'tree>)> {
    fn collect<'tree>(
        extractor: &JavaScriptExtractor,
        node: Node<'tree>,
        parts: &mut Vec<String>,
    ) -> Option<Node<'tree>> {
        match node.kind() {
            "identifier" | "this" | "super" => {
                parts.push(extractor.base().get_node_text(&node));
                Some(node)
            }
            "member_expression" => {
                let object = node.child_by_field_name("object")?;
                let property = node
                    .child_by_field_name("property")
                    .filter(|property| property.kind() == "property_identifier")?;
                let root = collect(extractor, object, parts)?;
                parts.push(extractor.base().get_node_text(&property));
                Some(root)
            }
            _ => None,
        }
    }

    let mut parts = Vec::new();
    let root = collect(extractor, node, &mut parts)?;
    if parts.len() > 2 && matches!(root.kind(), "this" | "super") {
        parts.remove(0);
    }
    Some((parts, root))
}

fn extract_call_target(extractor: &JavaScriptExtractor, function_node: Node) -> UnresolvedTarget {
    if function_node.kind() == "member_expression" {
        if let Some((parts, _)) = member_chain(extractor, function_node) {
            return UnresolvedTarget::from_chain(parts);
        }
        let receiver = function_node
            .child_by_field_name("object")
            .map(|node| extractor.base().get_node_text(&node));
        let terminal_name = function_node
            .child_by_field_name("property")
            .map(|node| extractor.base().get_node_text(&node))
            .unwrap_or_else(|| extractor.base().get_node_text(&function_node));
        let display_name = receiver
            .as_ref()
            .map(|receiver| format!("{receiver}.{terminal_name}"))
            .unwrap_or_else(|| terminal_name.clone());

        return UnresolvedTarget {
            display_name,
            terminal_name,
            receiver,
            namespace_path: Vec::new(),
            import_context: None,
        };
    }

    let terminal_name = extractor.base().get_node_text(&function_node);
    UnresolvedTarget::simple(terminal_name)
}

/// Extract inheritance relationships (extends)
fn extract_inheritance_relationships(
    extractor: &mut JavaScriptExtractor,
    node: Node,
    symbols: &[Symbol],
    relationships: &mut Vec<Relationship>,
    depth: u32,
) {
    if !should_visit_tree_depth(depth) {
        return;
    }

    // Phase 1: Collect data using immutable borrow
    let heritage_data = match node.kind() {
        "extends_clause" | "class_heritage" => collect_heritage_data(extractor, node, symbols),
        _ => None,
    };

    // Phase 2: Create relationships (may need &mut extractor for pending)
    if let Some((class_symbol_id, base_types, file_path)) = heritage_data {
        for (target, line_number) in base_types {
            let lookup_name = target.terminal_name.clone();
            // JS only has extends, check for Class (JS has no Interface kind)
            if let Some(base_symbol) = symbols.iter().find(|s| {
                s.name == lookup_name && matches!(s.kind, SymbolKind::Class | SymbolKind::Interface)
            }) {
                relationships.push(Relationship {
                    id: format!(
                        "{}_{}_{:?}_{}",
                        class_symbol_id,
                        base_symbol.id,
                        RelationshipKind::Extends,
                        line_number - 1
                    ),
                    from_symbol_id: class_symbol_id.clone(),
                    to_symbol_id: base_symbol.id.clone(),
                    kind: RelationshipKind::Extends,
                    file_path: file_path.clone(),
                    line_number,
                    span: Some(crate::base::NormalizedSpan::from_node(&node)),
                    reference_site_is_exact: false,
                    confidence: 1.0,
                    metadata: None,
                });
            } else {
                // Cross-file: superclass is defined in another file
                let mut pending = extractor.base().create_pending_relationship(
                    class_symbol_id.clone(),
                    target.clone(),
                    RelationshipKind::Extends,
                    &node,
                    Some(class_symbol_id.clone()),
                    Some(0.9),
                );
                pending.pending.callee_name = target.terminal_name;
                pending.pending.line_number = line_number;
                extractor.add_structured_pending_relationship(pending);
            }
        }
    }

    // Recursively process children
    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        extract_inheritance_relationships(extractor, child, symbols, relationships, child_depth);
    }
}

/// Collect heritage clause data without needing mutable access
fn collect_heritage_data(
    extractor: &JavaScriptExtractor,
    node: Node,
    symbols: &[Symbol],
) -> Option<HeritageData> {
    let mut parent = node.parent()?;
    while !matches!(parent.kind(), "class_declaration" | "class") {
        parent = parent.parent()?;
    }
    let class_symbol = symbols.iter().find(|s| {
        s.kind == SymbolKind::Class
            && s.start_byte == parent.start_byte() as u32
            && s.end_byte == parent.end_byte() as u32
    })?;

    let mut base_types = Vec::new();
    match node.kind() {
        "extends_clause" => collect_explicit_superclass_targets(extractor, node, &mut base_types),
        "class_heritage" => {
            let mut found_structured_clause = false;
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                if child.kind() == "extends_clause" {
                    found_structured_clause = true;
                }
            }

            if found_structured_clause {
                return None;
            }

            // Direct equivalent: some grammars model class_heritage without nested extends_clause.
            collect_explicit_superclass_targets(extractor, node, &mut base_types);
        }
        _ => return None,
    }

    Some((
        class_symbol.id.clone(),
        base_types,
        extractor.base().file_path.clone(),
    ))
}

fn extract_terminal_heritage_identifier(
    extractor: &JavaScriptExtractor,
    node: Node,
    depth: u32,
) -> Option<(UnresolvedTarget, u32)> {
    if !should_visit_tree_depth(depth) {
        return None;
    }

    match node.kind() {
        "identifier" | "type_identifier" | "property_identifier" => {
            let name = extractor.base().get_node_text(&node);
            let line = (node.start_position().row + 1) as u32;
            Some((UnresolvedTarget::simple(name), line))
        }
        "member_expression" => {
            let object = node
                .child_by_field_name("object")
                .or_else(|| node.child_by_field_name("left"))?;
            let property = node
                .child_by_field_name("property")
                .or_else(|| node.child_by_field_name("right"))?;

            // Restrict to explicit identifier/member chains.
            let child_depth = child_tree_depth(depth)?;
            extract_terminal_heritage_identifier(extractor, object, child_depth)?;
            let (_, line) = extract_terminal_heritage_identifier(extractor, property, child_depth)?;
            let display_name = extractor.base().get_node_text(&node).replace(' ', "");
            let segments: Vec<String> = display_name
                .split('.')
                .filter(|segment| !segment.is_empty())
                .map(|segment| segment.to_string())
                .collect();
            let terminal_name = segments.last()?.clone();
            let namespace_path = if segments.len() > 1 {
                segments[..segments.len() - 1].to_vec()
            } else {
                Vec::new()
            };
            Some((
                UnresolvedTarget {
                    display_name,
                    terminal_name,
                    receiver: None,
                    namespace_path,
                    import_context: None,
                },
                line,
            ))
        }
        "parenthesized_expression" => {
            let expression = node.child_by_field_name("expression")?;
            extract_terminal_heritage_identifier(extractor, expression, child_tree_depth(depth)?)
        }
        "call_expression" | "new_expression" => None,
        _ => {
            let child_depth = child_tree_depth(depth)?;
            let mut cursor = node.walk();
            for child in node.named_children(&mut cursor) {
                if let Some(candidate) =
                    extract_terminal_heritage_identifier(extractor, child, child_depth)
                {
                    return Some(candidate);
                }
            }
            None
        }
    }
}

fn collect_explicit_superclass_targets(
    extractor: &JavaScriptExtractor,
    node: Node,
    base_types: &mut Vec<(UnresolvedTarget, u32)>,
) {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if let Some((name, line)) = extract_terminal_heritage_identifier(extractor, child, 0) {
            base_types.push((name, line));
            break;
        }
    }
}
