//! Call sites and caller scope shared by F# relationships and identifiers.

use crate::base::{Symbol, SymbolKind};
use tree_sitter::Node;

/// The callee expression of a call site, or `None` when `node` is not one.
///
/// A call site is the outermost `application_expression` of a chain, or a
/// pipe (`x |> f`, `f <| x`) whose function operand is a bare reference. The
/// grammar parses `a + f x` and `xs |> List.map f` as an application whose
/// head is the infix expression, so the callee is the infix's right operand.
pub(super) fn call_callee<'a>(node: Node<'a>, source: &str) -> Option<Node<'a>> {
    match node.kind() {
        "application_expression" if !is_nested_application(node) => {
            callee_of_head(first_named_child(node)?)
        }
        "infix_expression" => pipe_function(node, source),
        _ => None,
    }
}

/// True when `node` sits inside the callee expression of any enclosing call
/// site.
pub(super) fn is_within_callee(node: Node, source: &str) -> bool {
    let mut current = node.parent();
    while let Some(candidate) = current {
        if call_callee(candidate, source).is_some_and(|callee| contains_node(callee, node)) {
            return true;
        }
        current = candidate.parent();
    }
    false
}

/// The `types` node of a generic instantiation (`f<T>`) wrapping `callee`.
pub(super) fn callee_type_arguments(callee: Node) -> Option<Node> {
    let parent = callee
        .parent()
        .filter(|parent| parent.kind() == "typed_expression")?;
    let mut cursor = parent.walk();
    parent
        .named_children(&mut cursor)
        .find(|child| child.kind() == "types")
}

fn callee_of_head(head: Node) -> Option<Node> {
    match head.kind() {
        "application_expression" | "typed_expression" => callee_of_head(first_named_child(head)?),
        "infix_expression" => callee_of_head(right_operand(head)?),
        "dot_expression" | "long_identifier_or_op" | "long_identifier" => Some(head),
        _ => None,
    }
}

fn pipe_function<'a>(infix: Node<'a>, source: &str) -> Option<Node<'a>> {
    let op = infix_operator(infix, source)?;
    let is_application_head = infix.parent().is_some_and(|parent| {
        parent.kind() == "application_expression"
            && first_named_child(parent).is_some_and(|head| head.id() == infix.id())
    });
    let operand = match op.as_str() {
        "|>" | "||>" | "|||>" if !is_application_head => right_operand(infix)?,
        "<|" | "<||" | "<|||" => first_named_child(infix)?,
        _ => return None,
    };
    match operand.kind() {
        "dot_expression" | "long_identifier_or_op" | "long_identifier" => Some(operand),
        "typed_expression" => callee_of_head(operand),
        _ => None,
    }
}

fn infix_operator(infix: Node, source: &str) -> Option<String> {
    let mut cursor = infix.walk();
    let op = infix
        .named_children(&mut cursor)
        .find(|child| child.kind() == "infix_op")?;
    Some(
        source
            .get(op.start_byte()..op.end_byte())?
            .trim()
            .to_string(),
    )
}

fn right_operand(infix: Node) -> Option<Node> {
    let mut cursor = infix.walk();
    infix
        .named_children(&mut cursor)
        .filter(|child| child.kind() != "infix_op")
        .last()
}

fn is_nested_application(node: Node) -> bool {
    node.parent()
        .is_some_and(|parent| parent.kind() == "application_expression")
}

fn first_named_child(node: Node) -> Option<Node> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor).next()
}

fn contains_node(outer: Node, inner: Node) -> bool {
    outer.start_byte() <= inner.start_byte() && outer.end_byte() >= inner.end_byte()
}

/// Caller and containment scope for F#: the smallest enclosing declaration,
/// so code in a property, a module value, or a nested module belongs to that
/// declaration and not to the outer namespace. Parameters, local values,
/// record fields, and union cases are never scopes: their type references
/// belong to the enclosing type, the same as the `uses` edge they produce.
pub(super) struct Scope<'a> {
    candidates: Vec<&'a Symbol>,
}

impl<'a> Scope<'a> {
    pub(super) fn new(symbols: &'a [Symbol]) -> Self {
        let kind_of = |id: &str| symbols.iter().find(|s| s.id == id).map(|s| s.kind.clone());
        let candidates = symbols
            .iter()
            .filter(|symbol| {
                let is_parameter = symbol
                    .metadata
                    .as_ref()
                    .and_then(|metadata| metadata.get("role"))
                    .is_some_and(|role| role == "parameter");
                let is_local = symbol.kind == SymbolKind::Variable
                    && symbol
                        .parent_id
                        .as_deref()
                        .and_then(kind_of)
                        .is_some_and(|kind| {
                            matches!(
                                kind,
                                SymbolKind::Function
                                    | SymbolKind::Method
                                    | SymbolKind::Property
                                    | SymbolKind::Constructor
                                    | SymbolKind::Variable
                            )
                        });
                let is_type_member =
                    matches!(symbol.kind, SymbolKind::Field | SymbolKind::EnumMember);
                !is_parameter && !is_local && !is_type_member
            })
            .collect();
        Self { candidates }
    }

    // ponytail: linear scan per lookup; sort by start byte if large files get slow.
    pub(super) fn find(&self, node: Node) -> Option<&'a Symbol> {
        let start = node.start_byte() as u32;
        let end = node.end_byte() as u32;
        self.candidates
            .iter()
            .filter(|symbol| symbol.start_byte <= start && symbol.end_byte >= end)
            .min_by_key(|symbol| symbol.end_byte - symbol.start_byte)
            .copied()
    }
}
