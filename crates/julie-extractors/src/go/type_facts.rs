//! Declared-type fact recording for Go.

use super::helpers::receiver_base_type_node;
use crate::base::BaseExtractor;
use crate::base::types::TypeNameRules;
use std::collections::{HashMap, HashSet};
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
/// a call to a same-file function (`u, err := LoadUser(1)`,
/// `s := NewSet[string]()`) or to a same-file method called on the enclosing
/// method's receiver (`c := s.config()`) or on a same-file composite literal
/// (`p := (&Parser{}).parse()`). Predeclared, unnamed, and type-parameter
/// result types record nothing.
pub(super) fn record_inferred_value_type(
    base: &mut BaseExtractor,
    symbol_id: &str,
    value: Node,
    result_index: usize,
    result_types: &ResultTypeIndex,
) {
    let Some(value) = without_parentheses(value) else {
        return;
    };
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
    let callee_name = match function.kind() {
        "identifier" => Some(function),
        "generic_type" => function
            .child_by_field_name("type")
            .filter(|name| name.kind() == "type_identifier"),
        "index_expression" => function
            .child_by_field_name("operand")
            .filter(|name| name.kind() == "identifier"),
        _ => None,
    }
    .map(|name| base.get_node_text(&name))
    .filter(|name| !result_types.binds_locally(value, name));
    if result_index == 0 && function.kind() == "identifier" && callee_name.as_deref() == Some("new")
    {
        record_new_argument_type(base, symbol_id, value);
        return;
    }
    let candidates = match function.kind() {
        "identifier" | "generic_type" | "index_expression" => {
            callee_name.and_then(|name| result_types.functions.get(&name))
        }
        "selector_expression" => method_owner(base, function, result_types).and_then(|owner| {
            let method = base.get_node_text(&function.child_by_field_name("field")?);
            result_types.methods.get(&(owner, method))
        }),
        _ => None,
    };
    if let Some(result_type) = agreed_result(candidates, result_index) {
        base.record_declared_type_fact(symbol_id, &result_type.declared, result_type.rules, true);
    }
}

/// Declared result types of the file's functions and methods, and the
/// receiver binding of each method, for initializer inference.
#[derive(Debug, Default)]
pub(super) struct ResultTypeIndex {
    functions: HashMap<String, Vec<Results>>,
    /// Keyed by receiver base type name, then method name.
    methods: HashMap<(String, String), Vec<Results>>,
    /// Receiver name and receiver base type name by method node id, kept only
    /// for methods whose body never declares another binding with that name.
    receivers: HashMap<usize, (String, String)>,
    /// Names each top-level declaration binds inside itself (parameters,
    /// type parameters, results, locals, local types), by declaration node id. A call to one
    /// of these names may not reach the package-level declaration.
    local_names: HashMap<usize, HashSet<String>>,
}

/// The bindable type at each result position; `None` where the result is
/// unnamed, predeclared, or a type parameter. A result whose type arguments
/// name a type parameter (`*Stack[T]`) keeps only its base name (`Stack`),
/// because the call site binds `T` to a type this index does not know.
type Results = Vec<Option<ResultType>>;

#[derive(Debug)]
struct ResultType {
    declared: String,
    rules: &'static TypeNameRules,
}

impl ResultTypeIndex {
    pub(super) fn build(base: &BaseExtractor, root: Node) -> Self {
        let mut index = Self::default();
        for declaration in root.named_children(&mut root.walk()) {
            let mut local_names = declared_names(base, declaration);
            if declaration.kind() == "method_declaration" {
                local_names.extend(method_receiver_type_parameter_names(base, declaration));
            }
            index.local_names.insert(declaration.id(), local_names);
            match declaration.kind() {
                "function_declaration" => {
                    let Some(name) = declaration.child_by_field_name("name") else {
                        continue;
                    };
                    let generics = type_parameter_names(base, declaration);
                    index
                        .functions
                        .entry(base.get_node_text(&name))
                        .or_default()
                        .push(declared_results(base, declaration, &generics));
                }
                "method_declaration" => index.add_method(base, declaration),
                _ => {}
            }
        }
        index
    }

    /// Whether the top-level declaration holding `node` binds `name` itself.
    fn binds_locally(&self, node: Node, name: &str) -> bool {
        self.local_names
            .get(&top_level_declaration(node).id())
            .is_some_and(|names| names.contains(name))
    }

    fn add_method(&mut self, base: &BaseExtractor, method: Node) {
        let Some(receiver) = method_receiver(method) else {
            return;
        };
        let Some(receiver_type) = receiver.child_by_field_name("type") else {
            return;
        };
        let (Some(owner), Some(name)) = (
            receiver_base_type_node(receiver_type),
            method.child_by_field_name("name"),
        ) else {
            return;
        };
        let owner = base.get_node_text(&owner);
        let generics = receiver_type_parameter_names(base, receiver_type);
        self.methods
            .entry((owner.clone(), base.get_node_text(&name)))
            .or_default()
            .push(declared_results(base, method, &generics));
        let Some(receiver_name) = receiver
            .child_by_field_name("name")
            .map(|name| base.get_node_text(&name))
            .filter(|name| name != "_")
        else {
            return;
        };
        let rebound = method
            .child_by_field_name("body")
            .is_some_and(|body| declared_names(base, body).contains(&receiver_name));
        if !rebound {
            self.receivers.insert(method.id(), (receiver_name, owner));
        }
    }
}

fn declared_results(base: &BaseExtractor, callable: Node, generics: &[String]) -> Results {
    let Some(result) = callable.child_by_field_name("result") else {
        return Vec::new();
    };
    (0..)
        .map_while(|index| nth_result_type(result, index))
        .map(|result_type| bindable_result(base, result_type, generics))
        .collect()
}

fn bindable_result(
    base: &BaseExtractor,
    type_node: Node,
    generics: &[String],
) -> Option<ResultType> {
    if !names_declared_type(base, type_node) {
        return None;
    }
    if !mentions_type_parameter(base, type_node, generics) {
        return Some(ResultType {
            declared: base.get_node_text(&type_node),
            rules: binding_rules(type_node)?,
        });
    }
    let named = match type_node.kind() {
        "pointer_type" => type_node.named_child(0)?,
        _ => type_node,
    };
    let name = match named.kind() {
        "generic_type" => named.child_by_field_name("type")?,
        _ => named,
    };
    let name = base.get_node_text(&name);
    (!generics.contains(&name)).then_some(ResultType {
        declared: name,
        rules: &TYPE_NAME_RULES,
    })
}

/// The one result type every same-named candidate agrees on at `index`.
fn agreed_result(candidates: Option<&Vec<Results>>, index: usize) -> Option<&ResultType> {
    let mut types = candidates?
        .iter()
        .map(|results| results.get(index).and_then(Option::as_ref));
    let first = types.next()??;
    types
        .all(|other| other.is_some_and(|other| other.declared == first.declared))
        .then_some(first)
}

fn type_parameter_names(base: &BaseExtractor, function: Node) -> Vec<String> {
    let Some(parameters) = function.child_by_field_name("type_parameters") else {
        return Vec::new();
    };
    parameters
        .named_children(&mut parameters.walk())
        .filter(|parameter| parameter.kind() == "type_parameter_declaration")
        .flat_map(|parameter| {
            parameter
                .children_by_field_name("name", &mut parameter.walk())
                .map(|name| base.get_node_text(&name))
                .collect::<Vec<_>>()
        })
        .collect()
}

fn method_receiver(method: Node) -> Option<Node> {
    let list = method.child_by_field_name("receiver")?;
    list.named_children(&mut list.walk())
        .find(|child| child.kind() == "parameter_declaration")
}

fn method_receiver_type_parameter_names(base: &BaseExtractor, method: Node) -> Vec<String> {
    method_receiver(method)
        .and_then(|receiver| receiver.child_by_field_name("type"))
        .map(|receiver_type| receiver_type_parameter_names(base, receiver_type))
        .unwrap_or_default()
}

/// The type parameter names a generic receiver binds: `T` in `*Stack[T]`.
fn receiver_type_parameter_names(base: &BaseExtractor, receiver_type: Node) -> Vec<String> {
    let named = match receiver_type.kind() {
        "pointer_type" => receiver_type.named_child(0),
        _ => Some(receiver_type),
    };
    let Some(arguments) = named
        .filter(|named| named.kind() == "generic_type")
        .and_then(|generic| generic.child_by_field_name("type_arguments"))
    else {
        return Vec::new();
    };
    arguments
        .named_children(&mut arguments.walk())
        .filter_map(|argument| match argument.kind() {
            "type_elem" => argument.named_child(0),
            _ => Some(argument),
        })
        .filter(|argument| argument.kind() == "type_identifier")
        .map(|argument| base.get_node_text(&argument))
        .collect()
}

/// Whether `T` in `T`, `*Stack[T]`, or `Box[[]T]` is one of `generics`.
fn mentions_type_parameter(base: &BaseExtractor, type_node: Node, generics: &[String]) -> bool {
    let mut stack = vec![type_node];
    while let Some(node) = stack.pop() {
        if node.kind() == "type_identifier" && generics.contains(&base.get_node_text(&node)) {
            return true;
        }
        stack.extend(node.named_children(&mut node.walk()));
    }
    false
}

/// The names of every binding declared under `node`.
fn declared_names(base: &BaseExtractor, node: Node) -> HashSet<String> {
    let mut names = HashSet::new();
    let mut stack = vec![node];
    while let Some(node) = stack.pop() {
        if is_declared_name(node) {
            names.insert(base.get_node_text(&node));
        }
        stack.extend(node.named_children(&mut node.walk()));
    }
    names
}

fn is_declared_name(name: Node) -> bool {
    let Some(parent) = name.parent() else {
        return false;
    };
    match name.kind() {
        "identifier" => {}
        "type_identifier" => {
            return matches!(parent.kind(), "type_spec" | "type_alias")
                && parent.child_by_field_name("name") == Some(name);
        }
        _ => return false,
    }
    match parent.kind() {
        "var_spec"
        | "const_spec"
        | "parameter_declaration"
        | "variadic_parameter_declaration"
        | "type_parameter_declaration" => true,
        "expression_list" => parent.parent().is_some_and(|holder| {
            matches!(
                holder.kind(),
                "short_var_declaration"
                    | "range_clause"
                    | "receive_statement"
                    | "type_switch_statement"
            ) && holder
                .child_by_field_name("left")
                .or_else(|| holder.child_by_field_name("alias"))
                == Some(parent)
        }),
        _ => false,
    }
}

/// The receiver base type a `x.m` selector calls into: the enclosing
/// method's receiver type when `x` is its receiver, or the type of a
/// same-file `T{...}` / `&T{...}` literal.
fn method_owner(
    base: &BaseExtractor,
    selector: Node,
    result_types: &ResultTypeIndex,
) -> Option<String> {
    let operand = without_parentheses(selector.child_by_field_name("operand")?)?;
    if let Some(literal_type) = composite_literal_type_node(operand) {
        let name = match literal_type.kind() {
            "generic_type" => literal_type.child_by_field_name("type")?,
            _ => literal_type,
        };
        let name = (name.kind() == "type_identifier").then(|| base.get_node_text(&name))?;
        return (!result_types.binds_locally(selector, &name)).then_some(name);
    }
    if operand.kind() != "identifier" {
        return None;
    }
    let mut current = operand;
    let method = loop {
        current = current.parent()?;
        if current.kind() == "method_declaration" {
            break current;
        }
    };
    let (receiver, owner) = result_types.receivers.get(&method.id())?;
    (base.get_node_text(&operand) == *receiver).then(|| owner.clone())
}

fn without_parentheses(mut node: Node) -> Option<Node> {
    while node.kind() == "parenthesized_expression" {
        node = node.named_child(0)?;
    }
    Some(node)
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

fn top_level_declaration(mut node: Node) -> Node {
    while let Some(parent) = node.parent() {
        if parent.parent().is_none() {
            break;
        }
        node = parent;
    }
    node
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
