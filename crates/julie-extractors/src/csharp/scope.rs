use crate::base::{ContainingSymbolIndex, Symbol, SymbolKind};
use std::collections::HashMap;
use tree_sitter::Node;

/// Declarations whose bodies own the calls and references inside them.
const MEMBER_NODE_KINDS: &[&str] = &[
    "method_declaration",
    "constructor_declaration",
    "destructor_declaration",
    "property_declaration",
    "indexer_declaration",
    "event_declaration",
    "operator_declaration",
    "conversion_operator_declaration",
    "local_function_statement",
    "lambda_expression",
    "anonymous_method_expression",
];

/// The one containing symbol for a reference site: the innermost member
/// declaration with a symbol (a property or indexer body counts, unlike the
/// span-priority index, which ranks a type above a property), else the
/// narrowest symbol the shared index finds.
pub(crate) struct MemberScope<'a> {
    members_by_start: HashMap<u32, &'a Symbol>,
    index: ContainingSymbolIndex<'a>,
}

impl<'a> MemberScope<'a> {
    pub(crate) fn new(symbols: &'a [Symbol], file_path: &str) -> Self {
        let members_by_start = symbols
            .iter()
            .filter(|symbol| is_member_kind(&symbol.kind))
            .map(|symbol| (symbol.start_byte, symbol))
            .collect();
        Self {
            members_by_start,
            index: ContainingSymbolIndex::new(symbols, file_path),
        }
    }

    pub(crate) fn find(&self, node: Node) -> Option<&'a Symbol> {
        let mut current = node.parent();
        while let Some(candidate) = current {
            if MEMBER_NODE_KINDS.contains(&candidate.kind())
                && let Some(symbol) = self.members_by_start.get(&(candidate.start_byte() as u32))
            {
                return Some(symbol);
            }
            current = candidate.parent();
        }
        self.index.find(node)
    }
}

fn is_member_kind(kind: &SymbolKind) -> bool {
    matches!(
        kind,
        SymbolKind::Method
            | SymbolKind::Constructor
            | SymbolKind::Destructor
            | SymbolKind::Property
            | SymbolKind::Event
            | SymbolKind::Operator
            | SymbolKind::Function
    )
}

/// True when `name` is a type parameter declared by a declaration enclosing
/// `node` (`new T()` inside `Make<T>()` constructs no known type).
pub(crate) fn is_type_parameter_in_scope(content: &str, node: Node, name: &str) -> bool {
    let mut current = node.parent();
    while let Some(candidate) = current {
        let mut cursor = candidate.walk();
        let declares = candidate
            .children(&mut cursor)
            .filter(|child| child.kind() == "type_parameter_list")
            .any(|list| {
                let mut list_cursor = list.walk();
                list.named_children(&mut list_cursor).any(|parameter| {
                    parameter
                        .child_by_field_name("name")
                        .and_then(|name_node| content.get(name_node.byte_range()))
                        == Some(name)
                })
            });
        if declares {
            return true;
        }
        current = candidate.parent();
    }
    false
}

/// The declared type a target-typed `new(...)` constructs: the type of the
/// local, field, or property it initializes, or the return type of the member
/// whose value it returns. `var` and nullable wrappers name no or the inner
/// type respectively; any other context yields `None`.
pub(crate) fn target_type_of_implicit_new(node: Node) -> Option<Node> {
    let parent = node.parent()?;
    let declared = match parent.kind() {
        "variable_declarator" => parent
            .parent()
            .filter(|declaration| declaration.kind() == "variable_declaration")?
            .child_by_field_name("type"),
        "property_declaration" => parent.child_by_field_name("type"),
        "arrow_expression_clause" | "return_statement" => enclosing_return_type(parent),
        _ => None,
    }?;
    let declared = if declared.kind() == "nullable_type" {
        declared.named_child(0)?
    } else {
        declared
    };
    matches!(
        declared.kind(),
        "identifier" | "generic_name" | "qualified_name"
    )
    .then_some(declared)
}

fn enclosing_return_type(node: Node) -> Option<Node> {
    let mut current = node.parent();
    while let Some(candidate) = current {
        match candidate.kind() {
            "method_declaration" => return candidate.child_by_field_name("returns"),
            "local_function_statement" | "property_declaration" | "indexer_declaration" => {
                return candidate.child_by_field_name("type");
            }
            "lambda_expression" | "anonymous_method_expression" => return None,
            _ => current = candidate.parent(),
        }
    }
    None
}
