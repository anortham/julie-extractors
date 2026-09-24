use super::FSharpExtractor;
use super::call_types::ReturnTypeIndex;
use super::parameters;
use crate::base::types::TypeNameRules;
use crate::base::{BaseExtractor, Symbol, SymbolKind};
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};
use std::collections::HashMap;
use tree_sitter::Node;

pub(super) const FSHARP_TYPE_NAME_RULES: TypeNameRules = TypeNameRules {
    nullable_suffixes: &[],
    reference_prefixes: &["byref", "inref", "outref"],
    generic_open: &['<'],
};

pub(super) fn collect_types(
    extractor: &mut FSharpExtractor,
    root: Node,
    symbols: &[Symbol],
) -> HashMap<String, String> {
    let mut types = HashMap::new();
    extractor.base.type_info.clear();
    let return_types = ReturnTypeIndex::build(&extractor.base, root);
    let mut scope = TypeScope {
        symbols,
        return_types: &return_types,
        types: &mut types,
    };
    walk(&mut extractor.base, root, &mut scope, 0);
    for (symbol_id, type_info) in &extractor.base.type_info {
        types.insert(symbol_id.clone(), type_info.resolved_type.clone());
    }
    types
}

struct TypeScope<'s, 't> {
    symbols: &'s [Symbol],
    return_types: &'s ReturnTypeIndex<'t>,
    types: &'s mut HashMap<String, String>,
}

fn walk<'t>(base: &mut BaseExtractor, node: Node<'t>, scope: &mut TypeScope<'_, 't>, depth: u32) {
    if !should_visit_tree_depth(depth) {
        return;
    }
    let symbols = scope.symbols;
    match node.kind() {
        "function_or_value_defn" => {
            collect_definition_type(base, node, scope);
            parameters::record_parameter_facts(base, node, symbols);
        }
        "declaration_expression" => collect_use_type(base, node, scope),
        "record_field" | "union_type_field" => collect_field_type(base, node, symbols, scope.types),
        "member_defn" => {
            collect_member_type(base, node, symbols, scope.types);
            parameters::record_parameter_facts(base, node, symbols);
        }
        "anon_type_defn" => parameters::record_parameter_facts(base, node, symbols),
        "value_definition" | "member_signature" => {
            collect_signature_type(base, node, symbols, scope.types)
        }
        _ => {}
    }
    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk(base, child, scope, child_depth);
    }
}

fn collect_definition_type<'t>(
    base: &mut BaseExtractor,
    node: Node<'t>,
    scope: &mut TypeScope<'_, 't>,
) {
    let symbols = scope.symbols;
    let Some(left) = direct_child(node, "function_declaration_left")
        .or_else(|| direct_child(node, "value_declaration_left"))
    else {
        return;
    };
    let Some(name_node) = declaration_name(left) else {
        return;
    };
    let Some(symbol) = symbol_for_name(symbols, base, &name_node) else {
        return;
    };
    let pattern = if left.kind() == "value_declaration_left" {
        bound_pattern(left)
    } else {
        Some(BoundPattern::Name)
    };
    let written_on_pattern = match pattern {
        Some(BoundPattern::Typed(type_node)) => Some(type_node),
        _ => None,
    };
    let explicit = direct_type_child_after(node, left).or(written_on_pattern);
    if let Some(type_node) = explicit {
        insert_type(base, scope.types, symbol, type_node);
        return;
    }
    let (Some(_), Some(keyword), Some(body)) =
        (pattern, node.child(0), node.child_by_field_name("body"))
    else {
        return;
    };
    record_initializer_type(base, &symbol.id, keyword, body, scope);
}

enum BoundPattern<'t> {
    Name,
    Typed(Node<'t>),
}

/// The value pattern of `let x = ...` or `let (x: T) = ...`, with optional
/// parentheses. Tuple, list, array, cons, union-case, and `as` patterns bind
/// parts of the value, so they return `None`.
fn bound_pattern(left: Node) -> Option<BoundPattern> {
    let mut cursor = left.walk();
    let mut patterns = left.named_children(&mut cursor).filter(|child| {
        !matches!(child.kind(), "mutable" | "access_modifier") && !child.is_extra()
    });
    let (pattern, None) = (patterns.next()?, patterns.next()) else {
        return None;
    };
    let pattern = unparenthesized(pattern)?;
    if pattern.kind() == "typed_pattern" {
        let (inner, type_node) = parameters::typed_pattern_parts(pattern)?;
        return is_name_pattern(unparenthesized(inner)?).then_some(BoundPattern::Typed(type_node));
    }
    is_name_pattern(pattern).then_some(BoundPattern::Name)
}

fn unparenthesized(mut pattern: Node) -> Option<Node> {
    while pattern.kind() == "paren_pattern" {
        let mut cursor = pattern.walk();
        let mut inner = pattern
            .named_children(&mut cursor)
            .filter(|child| !child.is_extra());
        let (only, None) = (inner.next()?, inner.next()) else {
            return None;
        };
        pattern = only;
    }
    Some(pattern)
}

fn is_name_pattern(pattern: Node) -> bool {
    let mut cursor = pattern.walk();
    let children: Vec<Node> = pattern.named_children(&mut cursor).collect();
    pattern.kind() == "identifier_pattern"
        && matches!(children[..], [name] if name.kind() == "long_identifier_or_op"
            && name.named_child_count() == 1
            && name.named_child(0).is_some_and(|child| child.kind() == "identifier"))
}

/// `use name = expr` and `use! name = expr` have no `function_or_value_defn`.
fn collect_use_type<'t>(base: &mut BaseExtractor, node: Node<'t>, scope: &TypeScope<'_, 't>) {
    let Some(keyword) = direct_child(node, "use").or_else(|| direct_child(node, "use!")) else {
        return;
    };
    let mut cursor = node.walk();
    let children: Vec<Node> = node.children(&mut cursor).collect();
    if children.iter().any(is_type_node) {
        return;
    }
    let Some(value) = children
        .iter()
        .position(|child| child.kind() == "=")
        .and_then(|index| children.get(index + 1))
    else {
        return;
    };
    let Some(symbol) =
        direct_identifier(node).and_then(|name| symbol_for_name(scope.symbols, base, &name))
    else {
        return;
    };
    record_initializer_type(base, &symbol.id, keyword, *value, scope);
}

/// Literal and same-file constructor initializers, then same-file calls.
/// A `let!`/`use!` binds the computation's result, so only the call path,
/// which unwraps the builder's wrapper type, applies to it.
fn record_initializer_type<'t>(
    base: &mut BaseExtractor,
    symbol_id: &str,
    keyword: Node<'t>,
    value: Node<'t>,
    scope: &TypeScope<'_, 't>,
) {
    if !base.get_node_text(&keyword).ends_with('!') {
        if let Some(literal) = literal_type(value) {
            record_named_type(base, symbol_id, literal, literal, true);
            return;
        }
        if let Some(type_name) = same_file_constructor_type(base, value, scope.symbols) {
            record_named_type(base, symbol_id, &type_name, &type_name, true);
            return;
        }
    }
    if let Some((name, declared)) = scope.return_types.initializer_type(base, keyword, value) {
        record_named_type(base, symbol_id, &name, &declared, true);
    }
}

fn collect_field_type(
    base: &mut BaseExtractor,
    node: Node,
    symbols: &[Symbol],
    types: &mut HashMap<String, String>,
) {
    let Some(name_node) = direct_identifier(node) else {
        return;
    };
    let Some(type_node) = direct_type_child(node) else {
        return;
    };
    let Some(symbol) = symbol_for_name(symbols, base, &name_node) else {
        return;
    };
    insert_type(base, types, symbol, type_node);
}

fn collect_member_type(
    base: &mut BaseExtractor,
    node: Node,
    symbols: &[Symbol],
    types: &mut HashMap<String, String>,
) {
    let Some(definition) = direct_child(node, "method_or_prop_defn") else {
        return;
    };
    let Some(name) = definition.child_by_field_name("name") else {
        return;
    };
    let Some(name_node) = terminal_identifier(name) else {
        return;
    };
    let Some(symbol) = symbol_for_name(symbols, base, &name_node) else {
        return;
    };
    if let Some(type_node) =
        parameters::member_return_type(definition).or_else(|| direct_type_child(definition))
    {
        insert_type(base, types, symbol, type_node);
    }
}

/// `val pi: float` and `abstract Run: int -> string`: the type after the
/// last argument arrow is the declared value or return type.
fn collect_signature_type(
    base: &mut BaseExtractor,
    node: Node,
    symbols: &[Symbol],
    types: &mut HashMap<String, String>,
) {
    let name_node = match node.kind() {
        "value_definition" => {
            direct_child(node, "value_declaration_left").and_then(first_identifier)
        }
        _ => direct_identifier(node),
    };
    let Some(symbol) = name_node.and_then(|name| symbol_for_name(symbols, base, &name)) else {
        return;
    };
    let Some(spec) = direct_child(node, "curried_spec") else {
        return;
    };
    let mut cursor = spec.walk();
    let children: Vec<Node> = spec.named_children(&mut cursor).collect();
    if let Some(type_node) = children.last().filter(|child| is_type_node(child)) {
        insert_type(base, types, symbol, *type_node);
    }
}

fn insert_type(
    base: &mut BaseExtractor,
    types: &mut HashMap<String, String>,
    symbol: &Symbol,
    node: Node,
) {
    record_type_node(base, &symbol.id, node, false);
    if let Some(type_info) = base.type_info.get(&symbol.id) {
        types.insert(symbol.id.clone(), type_info.resolved_type.clone());
    }
}

pub(super) fn record_type_node(
    base: &mut BaseExtractor,
    symbol_id: &str,
    type_node: Node,
    is_inferred: bool,
) {
    let Some(base_name) = structural_base_name(base, type_node) else {
        return;
    };
    let declared = base.get_node_text(&type_node);
    record_named_type(base, symbol_id, &base_name, declared.trim(), is_inferred);
}

fn record_named_type(
    base: &mut BaseExtractor,
    symbol_id: &str,
    base_name: &str,
    declared: &str,
    is_inferred: bool,
) {
    base.record_declared_type_fact_with_declared(
        symbol_id,
        base_name,
        declared,
        &FSHARP_TYPE_NAME_RULES,
        is_inferred,
    );
}

pub(super) fn structural_base_name(base: &BaseExtractor, node: Node) -> Option<String> {
    let mut node = node;
    loop {
        match node.kind() {
            "identifier" | "long_identifier" | "simple_type" | "type_argument" => {
                let text = base.get_node_text(&node);
                let text = text.trim();
                if text.is_empty() {
                    return None;
                }
                return Some(text.to_string());
            }
            "paren_type" | "atomic_type" | "flexible_type" | "type_name" => {
                node = first_type_or_named_child(node)?;
            }
            "generic_type" => {
                let name_node = direct_child(node, "long_identifier")?;
                let name = base.get_node_text(&name_node);
                let name = name.trim();
                if matches!(name, "byref" | "inref" | "outref") {
                    node = generic_argument_type(node)?;
                    continue;
                }
                if name.is_empty() {
                    return None;
                }
                return Some(name.to_string());
            }
            "list_type" => {
                let text = base.get_node_text(&node);
                let text = text.trim();
                return (!text.is_empty()).then(|| text.to_string());
            }
            "postfix_type" => {
                let ident = last_named_child_of_kind(node, "long_identifier")?;
                let text = base.get_node_text(&ident);
                let text = text.trim();
                if text.is_empty() {
                    return None;
                }
                return Some(text.to_string());
            }
            "static_type" => {
                node = first_type_or_named_child(node)?;
            }
            _ => return None,
        }
    }
}

fn generic_argument_type(node: Node) -> Option<Node> {
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        if is_type_node(&child) {
            return Some(child);
        }
        if matches!(child.kind(), "type_attributes" | "types" | "type_attribute")
            && let Some(inner) = first_type_or_named_child(child)
        {
            return Some(inner);
        }
    }
    None
}

fn first_type_or_named_child(node: Node) -> Option<Node> {
    let mut cursor = node.walk();
    let children: Vec<_> = node.named_children(&mut cursor).collect();
    children
        .iter()
        .copied()
        .find(is_type_node)
        .or_else(|| children.into_iter().next())
}

fn last_named_child_of_kind<'a>(node: Node<'a>, kind: &str) -> Option<Node<'a>> {
    let mut cursor = node.walk();
    let children: Vec<_> = node.named_children(&mut cursor).collect();
    children
        .into_iter()
        .rev()
        .find(|child| child.kind() == kind)
}

fn same_file_constructor_type(
    base: &BaseExtractor,
    body: Node,
    symbols: &[Symbol],
) -> Option<String> {
    let application = match body.kind() {
        "application_expression" => body,
        _ => return None,
    };
    let head = first_named_child(application)?;
    if !matches!(head.kind(), "long_identifier" | "long_identifier_or_op") {
        return None;
    }
    let name = base.get_node_text(&head);
    let name = name.trim();
    if name.is_empty() || name.contains('.') {
        return None;
    }
    symbols
        .iter()
        .any(|symbol| {
            symbol.name == name
                && matches!(
                    symbol.kind,
                    SymbolKind::Class
                        | SymbolKind::Struct
                        | SymbolKind::Union
                        | SymbolKind::Interface
                        | SymbolKind::Enum
                        | SymbolKind::Type
                        | SymbolKind::Delegate
                )
        })
        .then(|| name.to_string())
}

fn literal_type(node: Node) -> Option<&'static str> {
    if node.kind() != "const" {
        return None;
    }
    let child = first_named_child(node)?;
    Some(match child.kind() {
        "string" | "triple_quoted_string" | "verbatim_string" => "string",
        "char" => "char",
        "int" => "int",
        "float" => "float",
        "decimal" => "decimal",
        "bool" => "bool",
        "unit" => "unit",
        "xint" | "int32" => "int",
        "int64" => "int64",
        "uint32" => "uint32",
        "uint64" => "uint64",
        "int16" => "int16",
        "uint16" => "uint16",
        "byte" => "byte",
        "sbyte" => "sbyte",
        "ieee32" => "float32",
        "ieee64" => "float",
        "nativeint" => "nativeint",
        "unativeint" => "unativeint",
        "bignum" => "bigint",
        _ => return None,
    })
}

fn symbol_for_name<'a>(
    symbols: &'a [Symbol],
    base: &BaseExtractor,
    name_node: &Node,
) -> Option<&'a Symbol> {
    let name_text = base.get_node_text(name_node);
    let name = name_text.trim();
    symbols
        .iter()
        .filter(|symbol| symbol.name == name)
        .filter(|symbol| {
            symbol.start_byte <= name_node.start_byte() as u32
                && symbol.end_byte >= name_node.end_byte() as u32
        })
        .min_by_key(|symbol| symbol.end_byte.saturating_sub(symbol.start_byte))
}

pub(super) fn direct_type_child_after<'a>(node: Node<'a>, left: Node<'a>) -> Option<Node<'a>> {
    let mut cursor = node.walk();
    let children: Vec<_> = node.children(&mut cursor).collect();
    children
        .into_iter()
        .skip_while(|child| child.id() != left.id())
        .skip(1)
        .find(is_type_node)
}

pub(super) fn direct_type_child(node: Node) -> Option<Node> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor).find(is_type_node)
}

fn declaration_name(node: Node) -> Option<Node> {
    first_identifier(node)
}

fn direct_identifier(node: Node) -> Option<Node> {
    let mut cursor = node.walk();
    node.children(&mut cursor)
        .find(|child| child.kind() == "identifier")
}

pub(super) fn first_identifier(node: Node) -> Option<Node> {
    if node.kind() == "identifier" {
        return Some(node);
    }
    let mut cursor = node.walk();
    node.named_children(&mut cursor).find_map(first_identifier)
}

pub(super) fn direct_child<'a>(node: Node<'a>, kind: &str) -> Option<Node<'a>> {
    let mut cursor = node.walk();
    node.children(&mut cursor)
        .find(|child| child.kind() == kind)
}

fn is_type_node(node: &Node) -> bool {
    matches!(
        node.kind(),
        "simple_type"
            | "generic_type"
            | "atomic_type"
            | "compound_type"
            | "constrained_type"
            | "flexible_type"
            | "function_type"
            | "list_type"
            | "paren_type"
            | "postfix_type"
            | "static_type"
            | "struct_type"
            | "tuple_type"
            | "type_name"
            | "types"
    )
}

fn first_named_child(node: Node) -> Option<Node> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor).next()
}

pub(super) fn terminal_identifier(node: Node) -> Option<Node> {
    if node.kind() == "identifier" {
        return Some(node);
    }
    let mut cursor = node.walk();
    let children: Vec<_> = node.named_children(&mut cursor).collect();
    children.into_iter().rev().find_map(terminal_identifier)
}
