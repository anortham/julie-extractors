use super::type_facts::{self, ReturnTypeIndex};
use crate::base::{BaseExtractor, Symbol, SymbolKind, SymbolOptions, Visibility};
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};
use std::collections::HashSet;
use tree_sitter::Node;

/// Names of every class and structure declared anywhere in the file, so a
/// `New T()` initializer is typed even when `T` is declared further down.
pub(super) fn collect_type_names(base: &BaseExtractor, root: Node) -> HashSet<String> {
    let mut names = HashSet::new();
    collect_type_names_at(base, root, &mut names, 0);
    names
}

fn collect_type_names_at(
    base: &BaseExtractor,
    node: Node,
    names: &mut HashSet<String>,
    depth: u32,
) {
    if !should_visit_tree_depth(depth) {
        return;
    }
    if matches!(node.kind(), "class_block" | "structure_block")
        && let Some(name) = node.child_by_field_name("name")
    {
        names.insert(base.get_node_text(&name));
    }
    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_type_names_at(base, child, names, child_depth);
    }
}

/// `Dim a, b As Integer` shares the trailing `As` clause with every bare name
/// before it; a name with its own initializer ends the sharing run.
pub(super) fn extract_dim_statement(
    base: &mut BaseExtractor,
    node: Node,
    parent_id: Option<String>,
    same_file: &HashSet<String>,
    return_types: &ReturnTypeIndex,
    symbols: &[Symbol],
) -> Vec<Symbol> {
    let mut bindings: Vec<DimBinding> = Vec::new();
    for i in 0..node.child_count() {
        let Some(child) = node.child(i as u32) else {
            continue;
        };
        let field = node.field_name_for_child(i as u32);
        if field == Some("name") {
            bindings.push(DimBinding {
                name: Some(child),
                ..Default::default()
            });
            continue;
        }
        let Some(binding) = bindings.last_mut() else {
            continue;
        };
        match field {
            Some("value") if child.kind() == "new_expression" => {
                binding.type_node = child.child_by_field_name("type");
            }
            Some("initializer") => binding.initializer = Some(child),
            _ if child.kind() == "as_clause" => {
                binding.type_node = child.child_by_field_name("type");
            }
            _ if child.kind() == "array_rank_specifier" => binding.rank = Some(child),
            _ => {}
        }
    }
    share_trailing_types(&mut bindings);

    let statement_names: Vec<String> = bindings
        .iter()
        .filter_map(|binding| binding.name)
        .map(|name| type_facts::name_key(&base.get_node_text(&name)))
        .collect();
    let is_shadowed = |name: &str| {
        statement_names.iter().any(|local| local == name)
            || is_member_scope_name(symbols, parent_id.as_deref(), name)
    };
    let initializers = InitializerTypes {
        same_file,
        return_types,
        is_shadowed: &is_shadowed,
    };
    bindings
        .into_iter()
        .filter_map(|binding| flush_dim_binding(base, parent_id.clone(), &initializers, binding))
        .collect()
}

/// Whether a local, local `Const`, or parameter of the member, or the member
/// itself, has this name: VB then resolves `Name(...)` to that variable, not
/// a method.
fn is_member_scope_name(symbols: &[Symbol], member_id: Option<&str>, name: &str) -> bool {
    symbols.iter().any(|symbol| {
        let in_scope = (matches!(symbol.kind, SymbolKind::Variable | SymbolKind::Constant)
            && symbol.parent_id.as_deref() == member_id)
            || Some(symbol.id.as_str()) == member_id;
        in_scope && type_facts::name_key(&symbol.name) == name
    })
}

/// What an untyped local's initializer can be typed from: a same-file
/// constructor or a same-file call with a declared return type.
struct InitializerTypes<'a> {
    same_file: &'a HashSet<String>,
    return_types: &'a ReturnTypeIndex,
    is_shadowed: &'a dyn Fn(&str) -> bool,
}

impl InitializerTypes<'_> {
    fn record(&self, base: &mut BaseExtractor, symbol: &Symbol, initializer: Node) {
        record_constructed_type(base, symbol, initializer, self.same_file);
        type_facts::record_call_initializer_type(
            base,
            &symbol.id,
            initializer,
            self.return_types,
            self.is_shadowed,
        );
    }
}

fn share_trailing_types(bindings: &mut [DimBinding]) {
    let mut shared = None;
    for binding in bindings.iter_mut().rev() {
        if binding.type_node.is_some() {
            shared = binding.type_node;
        } else if binding.initializer.is_some() {
            shared = None;
        } else {
            binding.type_node = shared;
        }
    }
}

#[derive(Default, Clone, Copy)]
struct DimBinding<'a> {
    name: Option<Node<'a>>,
    type_node: Option<Node<'a>>,
    rank: Option<Node<'a>>,
    initializer: Option<Node<'a>>,
}

fn flush_dim_binding(
    base: &mut BaseExtractor,
    parent_id: Option<String>,
    initializers: &InitializerTypes,
    binding: DimBinding,
) -> Option<Symbol> {
    let DimBinding {
        name: name_node,
        type_node,
        rank,
        initializer,
    } = binding;
    let name_node = name_node?;
    let symbol = create_local(base, name_node, parent_id, type_node, rank);
    if type_node.is_none()
        && let Some(initializer) = initializer
    {
        initializers.record(base, &symbol, initializer);
    }
    Some(symbol)
}

fn record_constructed_type(
    base: &mut BaseExtractor,
    symbol: &Symbol,
    initializer: Node,
    same_file: &HashSet<String>,
) {
    if let Some(constructed) = type_facts::constructor_type_node(initializer)
        && let Some(class_name) = type_facts::simple_unqualified_name(base, constructed)
        && same_file.contains(&class_name)
    {
        type_facts::record_constructor_fact(base, &symbol.id, &class_name);
    }
}

fn create_local(
    base: &mut BaseExtractor,
    name_node: Node,
    parent_id: Option<String>,
    type_node: Option<Node>,
    rank: Option<Node>,
) -> Symbol {
    let name = base.get_node_text(&name_node);
    let mut signature = format!("Dim {name}");
    if let Some(rank) = rank {
        signature.push_str(&base.get_node_text(&rank));
    }
    if let Some(type_node) = type_node {
        signature.push_str(&type_facts::as_clause_suffix(base, type_node));
    }
    let symbol = base.create_symbol(
        &name_node,
        name,
        SymbolKind::Variable,
        SymbolOptions {
            signature: Some(signature),
            visibility: Some(Visibility::Private),
            parent_id,
            ..Default::default()
        },
    );
    if let Some(type_node) = type_node {
        type_facts::record_declared_type(base, &symbol.id, type_node, rank);
    }
    symbol
}

/// Locals declared by a block header: `For Each x As T`, `For i As T`,
/// `Using r As New T`, `Using r = New T()`, and `Catch ex As T`. A `For`
/// variable without an `As` clause declares a local only when no parameter
/// or local of the member already has that name.
pub(super) fn extract_block_local(
    base: &mut BaseExtractor,
    node: Node,
    parent_id: Option<String>,
    symbols: &[Symbol],
    same_file: &HashSet<String>,
    return_types: &ReturnTypeIndex,
) -> Option<Symbol> {
    match node.kind() {
        "for_each_statement" | "for_statement" => {
            let name_node = node.child_by_field_name("variable")?;
            if name_node.kind() != "identifier" {
                return None;
            }
            let type_node = as_clause_type(node);
            if type_node.is_none()
                && is_declared_in_member(base, name_node, parent_id.as_deref(), symbols)
            {
                return None;
            }
            Some(create_local(base, name_node, parent_id, type_node, None))
        }
        "using_statement" => {
            if let Some(name_node) = node.child_by_field_name("resource") {
                let type_node = as_clause_type(node).or_else(|| {
                    node.child_by_field_name("value")
                        .filter(|value| value.kind() == "new_expression")
                        .and_then(|value| value.child_by_field_name("type"))
                });
                return Some(create_local(base, name_node, parent_id, type_node, None));
            }
            let assignment = using_assignment(node)?;
            let name_node = assignment.child_by_field_name("left")?;
            let resource = type_facts::name_key(&base.get_node_text(&name_node));
            let is_shadowed = |name: &str| {
                name == resource || is_member_scope_name(symbols, parent_id.as_deref(), name)
            };
            let initializers = InitializerTypes {
                same_file,
                return_types,
                is_shadowed: &is_shadowed,
            };
            let symbol = create_local(base, name_node, parent_id.clone(), None, None);
            if let Some(value) = assignment.child_by_field_name("right") {
                initializers.record(base, &symbol, value);
            }
            Some(symbol)
        }
        "catch_block" => {
            let name_node = node.child_by_field_name("exception")?;
            let type_node = node.child_by_field_name("type");
            Some(create_local(base, name_node, parent_id, type_node, None))
        }
        _ => None,
    }
}

/// `Using r = New T()` parses as an index on `r = New T`; returns that
/// assignment when its left side is a bare name.
pub(super) fn using_assignment(node: Node) -> Option<Node> {
    let value = node.child_by_field_name("value")?;
    let assignment = match value.kind() {
        "binary_expression" => value,
        "element_access" | "invocation" => value
            .child_by_field_name("object")
            .or_else(|| value.child_by_field_name("target"))
            .filter(|object| object.kind() == "binary_expression")?,
        _ => return None,
    };
    let is_assignment = assignment
        .child_by_field_name("operator")
        .is_some_and(|op| op.kind() == "=");
    let left = assignment.child_by_field_name("left")?;
    (is_assignment && left.kind() == "identifier").then_some(assignment)
}

fn as_clause_type(node: Node) -> Option<Node> {
    let mut cursor = node.walk();
    node.children(&mut cursor)
        .find(|child| child.kind() == "as_clause")
        .and_then(|clause| clause.child_by_field_name("type"))
}

fn is_declared_in_member(
    base: &BaseExtractor,
    name_node: Node,
    member_id: Option<&str>,
    symbols: &[Symbol],
) -> bool {
    let name = base.get_node_text(&name_node);
    symbols.iter().any(|symbol| {
        symbol.kind == SymbolKind::Variable
            && symbol.parent_id.as_deref() == member_id
            && symbol.name.eq_ignore_ascii_case(&name)
    })
}
