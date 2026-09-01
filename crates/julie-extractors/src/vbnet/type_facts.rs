use crate::base::BaseExtractor;
use crate::base::types::TypeNameRules;
use tree_sitter::Node;

pub(super) const VBNET_TYPE_NAME_RULES: TypeNameRules = TypeNameRules {
    nullable_suffixes: &["?"],
    reference_prefixes: &[],
    generic_open: &['('],
};

/// Array base names such as `Worker()` are reduced structurally, so the
/// `(` generic opener must not cut the array suffix off again.
const VBNET_ARRAY_TYPE_NAME_RULES: TypeNameRules = TypeNameRules {
    nullable_suffixes: &[],
    reference_prefixes: &[],
    generic_open: &[],
};

struct ReducedType {
    base_name: String,
    is_array: bool,
}

pub(super) fn record_declared_type(
    base: &mut BaseExtractor,
    symbol_id: &str,
    type_node: Node,
    declarator_rank: Option<Node>,
) {
    record_type_node(base, symbol_id, type_node, declarator_rank, false);
}

pub(super) fn record_constructor_fact(base: &mut BaseExtractor, symbol_id: &str, class_name: &str) {
    base.record_declared_type_fact(symbol_id, class_name, &VBNET_TYPE_NAME_RULES, true);
}

pub(super) fn declared_type_node(node: Node) -> Option<Node> {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "as_clause" {
            return child.child_by_field_name("type");
        }
    }
    node.child_by_field_name("value")
        .filter(|value| value.kind() == "new_expression")
        .and_then(|value| value.child_by_field_name("type"))
}

pub(super) fn declarator_rank_node(node: Node) -> Option<Node> {
    named_child_of_kind(node, "array_rank_specifier")
}

pub(super) fn constructor_type_node(initializer: Node) -> Option<Node> {
    match initializer.kind() {
        "new_expression" => initializer.child_by_field_name("type"),
        "element_access" => {
            let object = initializer.child_by_field_name("object")?;
            if object.kind() == "new_expression" {
                object.child_by_field_name("type")
            } else {
                None
            }
        }
        _ => None,
    }
}

pub(super) fn simple_unqualified_name(base: &BaseExtractor, type_node: Node) -> Option<String> {
    if !is_simple_constructed_type(type_node) {
        return None;
    }
    let name_node = base_type_name_node(type_node)?;
    Some(base.get_node_text(&name_node))
}

fn is_simple_constructed_type(node: Node) -> bool {
    match node.kind() {
        "identifier" | "primitive_type" => true,
        "namespace_name" => single_identifier(node).is_some(),
        "array_type" => node
            .child_by_field_name("element")
            .is_some_and(is_simple_constructed_type),
        "nullable_type" => node.named_child(0).is_some_and(is_simple_constructed_type),
        _ => false,
    }
}

fn record_type_node(
    base: &mut BaseExtractor,
    symbol_id: &str,
    type_node: Node,
    declarator_rank: Option<Node>,
    is_inferred: bool,
) {
    let type_node = without_constructor_argument_list(base, type_node);
    let Some(reduced) = reduce_type(base, type_node) else {
        return;
    };
    let rank_suffix = declarator_rank
        .map(|rank| rank_suffix(base, rank))
        .unwrap_or_default();
    let base_name = format!("{}{}", reduced.base_name, rank_suffix);
    let declared = format!("{}{}", base.get_node_text(&type_node), rank_suffix);
    let rules = if reduced.is_array || !rank_suffix.is_empty() {
        &VBNET_ARRAY_TYPE_NAME_RULES
    } else {
        &VBNET_TYPE_NAME_RULES
    };
    base.record_declared_type_fact_with_declared(
        symbol_id,
        &base_name,
        &declared,
        rules,
        is_inferred,
    );
}

fn reduce_type(base: &BaseExtractor, node: Node) -> Option<ReducedType> {
    if node.kind() == "array_type" {
        let element = reduce_type(base, node.child_by_field_name("element")?)?;
        let rank = node.child_by_field_name("rank")?;
        return Some(ReducedType {
            base_name: format!("{}{}", element.base_name, rank_suffix(base, rank)),
            is_array: true,
        });
    }
    let name_node = base_type_name_node(node)?;
    Some(ReducedType {
        base_name: base.get_node_text(&name_node),
        is_array: false,
    })
}

/// `New Foo()` parses `Foo()` as an array type, but the empty parentheses are
/// the constructor argument list, so the element type is the declared type.
fn without_constructor_argument_list<'a>(base: &BaseExtractor, type_node: Node<'a>) -> Node<'a> {
    let in_new_expression = type_node
        .parent()
        .is_some_and(|parent| parent.kind() == "new_expression");
    if !in_new_expression || type_node.kind() != "array_type" {
        return type_node;
    }
    match (
        type_node.child_by_field_name("element"),
        type_node.child_by_field_name("rank"),
    ) {
        (Some(element), Some(rank)) if rank_suffix(base, rank) == "()" => element,
        _ => type_node,
    }
}

fn rank_suffix(base: &BaseExtractor, rank: Node) -> String {
    let sizes = rank.named_child_count();
    let commas = if sizes > 0 {
        sizes - 1
    } else {
        base.get_node_text(&rank).matches(',').count()
    };
    format!("({})", ",".repeat(commas))
}

fn base_type_name_node(node: Node) -> Option<Node> {
    let mut node = node;
    loop {
        match node.kind() {
            "identifier" | "primitive_type" | "namespace_name" => return Some(node),
            "generic_type" => {
                node = named_child_of_kind(node, "namespace_name")?;
            }
            "nullable_type" => {
                node = node.named_child(0)?;
            }
            "array_type" => {
                node = node.child_by_field_name("element")?;
            }
            "new_expression" => {
                node = node.child_by_field_name("type")?;
            }
            _ => return None,
        }
    }
}

fn single_identifier(node: Node) -> Option<Node> {
    let mut cursor = node.walk();
    let mut identifiers = node
        .children(&mut cursor)
        .filter(|child| child.kind() == "identifier");
    let first = identifiers.next()?;
    if identifiers.next().is_some() {
        return None;
    }
    Some(first)
}

fn named_child_of_kind<'a>(node: Node<'a>, kind: &str) -> Option<Node<'a>> {
    let mut cursor = node.walk();
    node.children(&mut cursor)
        .find(|child| child.kind() == kind)
}
