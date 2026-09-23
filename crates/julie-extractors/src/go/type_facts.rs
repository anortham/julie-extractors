//! Declared-type fact recording for Go.

use crate::base::BaseExtractor;
use crate::base::types::TypeNameRules;
use tree_sitter::Node;

const TYPE_NAME_RULES: TypeNameRules = TypeNameRules {
    nullable_suffixes: &[],
    reference_prefixes: &["*"],
    generic_open: &[],
};

/// `[` opens both generic argument lists and array types in Go, so this rules
/// set applies only to nodes already proven to be `generic_type` with a
/// `type_identifier` base; array, slice, and map types never reach it.
const GENERIC_INSTANTIATION_RULES: TypeNameRules = TypeNameRules {
    nullable_suffixes: &[],
    reference_prefixes: &["*"],
    generic_open: &['['],
};

/// Record a declared-type fact for `symbol_id` when `type_node` names a base
/// type the consumer can bind: a plain or qualified type name, a generic
/// instantiation with an identifier base, or a pointer to one of those.
pub(super) fn record_type_node_fact(
    base: &mut BaseExtractor,
    symbol_id: &str,
    type_node: Node,
    is_inferred: bool,
) {
    let Some(rules) = binding_rules(type_node) else {
        return;
    };
    let declared = base.get_node_text(&type_node);
    base.record_declared_type_fact(symbol_id, &declared, rules, is_inferred);
}

pub(super) fn binds_base_type(type_node: Node) -> bool {
    binding_rules(type_node).is_some()
}

fn binding_rules(type_node: Node) -> Option<&'static TypeNameRules> {
    match type_node.kind() {
        "type_identifier" | "qualified_type" => Some(&TYPE_NAME_RULES),
        "generic_type" => generic_rules(type_node),
        "pointer_type" => match type_node.named_child(0)?.kind() {
            "type_identifier" | "qualified_type" => Some(&TYPE_NAME_RULES),
            "generic_type" => generic_rules(type_node.named_child(0)?),
            _ => None,
        },
        _ => None,
    }
}

fn generic_rules(generic_node: Node) -> Option<&'static TypeNameRules> {
    let base_node = generic_node.child_by_field_name("type")?;
    (base_node.kind() == "type_identifier").then_some(&GENERIC_INSTANTIATION_RULES)
}

/// Record the type a value initializes a variable with (`is_inferred=true`):
/// a `Foo{...}` / `&Foo{...}` literal, `new(Foo)`, or result `result_index` of
/// a same-file function call (`u, err := LoadUser(1)`, `s := NewSet[string]()`).
/// Predeclared and unnamed result types record nothing.
pub(super) fn record_inferred_value_type(
    base: &mut BaseExtractor,
    symbol_id: &str,
    value: Node,
    result_index: usize,
) {
    if result_index == 0
        && let Some(literal_type) = composite_literal_type_node(value)
    {
        record_type_node_fact(base, symbol_id, literal_type, true);
        return;
    }
    let function = match value.kind() {
        "call_expression" => value.child_by_field_name("function"),
        "type_conversion_expression" => value.child_by_field_name("type"),
        _ => None,
    };
    let Some(function) = function else {
        return;
    };
    if result_index == 0
        && function.kind() == "identifier"
        && base.get_node_text(&function) == "new"
    {
        record_new_argument_type(base, symbol_id, value);
        return;
    }
    let callee = match function.kind() {
        "identifier" => function,
        "generic_type" => match function.child_by_field_name("type") {
            Some(name) if name.kind() == "type_identifier" => name,
            _ => return,
        },
        _ => return,
    };
    let name = base.get_node_text(&callee);
    let Some(result_type) = same_file_function_declaration(value, &name, base)
        .and_then(|declaration| declaration.child_by_field_name("result"))
        .and_then(|result| nth_result_type(result, result_index))
    else {
        return;
    };
    if names_declared_type(base, result_type) {
        record_type_node_fact(base, symbol_id, result_type, true);
    }
}

/// Record a function's first declared result type (`is_inferred=false`).
/// Function-typed and other unnamed result types record nothing.
pub(super) fn record_return_type(base: &mut BaseExtractor, symbol_id: &str, function: Node) {
    if let Some(result_type) = function
        .child_by_field_name("result")
        .and_then(|result| nth_result_type(result, 0))
    {
        record_type_node_fact(base, symbol_id, result_type, false);
    }
}

/// The type of result `index` of a result list, counting each name of a
/// grouped declaration (`(a, b int, err error)`).
fn nth_result_type(result: Node, index: usize) -> Option<Node> {
    if result.kind() != "parameter_list" {
        return (index == 0).then_some(result);
    }
    let mut position = 0;
    for declaration in result.named_children(&mut result.walk()) {
        if declaration.kind() != "parameter_declaration" {
            continue;
        }
        let names = declaration
            .children_by_field_name("name", &mut declaration.walk())
            .count()
            .max(1);
        if index < position + names {
            return declaration.child_by_field_name("type");
        }
        position += names;
    }
    None
}

/// A named, non-predeclared type (or a pointer to one) the consumer can bind.
fn names_declared_type(base: &BaseExtractor, type_node: Node) -> bool {
    let named = match type_node.kind() {
        "pointer_type" => type_node.named_child(0),
        _ => Some(type_node),
    };
    let base_name = named.and_then(|named| match named.kind() {
        "type_identifier" => Some(named),
        "generic_type" => named.child_by_field_name("type"),
        "qualified_type" => named.child_by_field_name("name"),
        _ => None,
    });
    base_name.is_some_and(|name| !is_predeclared_type(&base.get_node_text(&name)))
        && binds_base_type(type_node)
}

/// `new(User)` / `new(models.User)`: the argument is the allocated type.
fn record_new_argument_type(base: &mut BaseExtractor, symbol_id: &str, call: Node) {
    let Some(argument) = call
        .child_by_field_name("arguments")
        .and_then(|arguments| arguments.named_child(0))
    else {
        return;
    };
    match argument.kind() {
        "type_identifier" | "qualified_type" | "generic_type" => {
            if names_declared_type(base, argument) {
                record_type_node_fact(base, symbol_id, argument, true);
            }
        }
        "identifier" | "selector_expression" => {
            let text = base.get_node_text(&argument);
            if !is_predeclared_type(&text) {
                base.record_declared_type_fact(symbol_id, &text, &TYPE_NAME_RULES, true);
            }
        }
        _ => {}
    }
}

/// The type node of a `Foo{...}` or `&Foo{...}` initializer, when present.
pub(super) fn composite_literal_type_node(value_node: Node) -> Option<Node> {
    let literal = match value_node.kind() {
        "composite_literal" => value_node,
        "unary_expression" => {
            let operator = value_node.child_by_field_name("operator")?;
            let operand = value_node.child_by_field_name("operand")?;
            (operator.kind() == "&" && operand.kind() == "composite_literal").then_some(operand)?
        }
        _ => return None,
    };
    literal.child_by_field_name("type")
}

/// The name node of `F` in an explicit instantiation `F[T](x)`, which the
/// grammar reads as a type conversion, when the file declares no type `F`.
pub(super) fn instantiated_function_name<'a>(
    base: &BaseExtractor,
    conversion: Node<'a>,
) -> Option<Node<'a>> {
    if conversion.kind() != "type_conversion_expression" {
        return None;
    }
    let generic = conversion
        .child_by_field_name("type")
        .filter(|node| node.kind() == "generic_type")?;
    let base_type = generic.child_by_field_name("type")?;
    let name = match base_type.kind() {
        "type_identifier" => base_type,
        "qualified_type" => base_type.child_by_field_name("name")?,
        _ => return None,
    };
    if base_type.kind() == "type_identifier"
        && same_file_type_declaration(conversion, &base.get_node_text(&name), base)
    {
        return None;
    }
    Some(name)
}

fn same_file_type_declaration(node: Node, name: &str, base: &BaseExtractor) -> bool {
    let root = file_root(node);
    let mut cursor = root.walk();
    root.children(&mut cursor)
        .filter(|child| child.kind() == "type_declaration")
        .any(|declaration| {
            declaration
                .named_children(&mut declaration.walk())
                .any(|spec| {
                    spec.child_by_field_name("name")
                        .is_some_and(|spec_name| base.get_node_text(&spec_name) == name)
                })
        })
}

fn same_file_function_declaration<'a>(
    node: Node<'a>,
    name: &str,
    base: &BaseExtractor,
) -> Option<Node<'a>> {
    let root = file_root(node);
    let mut cursor = root.walk();
    root.children(&mut cursor).find(|child| {
        child.kind() == "function_declaration"
            && child
                .child_by_field_name("name")
                .is_some_and(|name_node| base.get_node_text(&name_node) == name)
    })
}

fn file_root(mut node: Node) -> Node {
    while let Some(parent) = node.parent() {
        node = parent;
    }
    node
}

fn is_predeclared_type(name: &str) -> bool {
    matches!(
        name,
        "any"
            | "bool"
            | "byte"
            | "comparable"
            | "complex64"
            | "complex128"
            | "error"
            | "float32"
            | "float64"
            | "int"
            | "int8"
            | "int16"
            | "int32"
            | "int64"
            | "rune"
            | "string"
            | "uint"
            | "uint8"
            | "uint16"
            | "uint32"
            | "uint64"
            | "uintptr"
    )
}
