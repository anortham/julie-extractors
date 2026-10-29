//! The static type of each argument of a Scala call or `new` expression, so
//! a reader can tell which overload the call binds: `run(1)` binds
//! `run(a: Int)`, not `run(a: String)`. A named argument also gets its name
//! in `arg_names`.

use std::collections::HashMap;

use tree_sitter::{Node, Tree};

use crate::base::argument_types::{
    self, CallArguments, Declarations, Found, enclosing, nearest_binding, node_text,
    suffixed_float_type, suffixed_integer_type,
};
use crate::base::types::strip_type_decorations;
use crate::base::{Identifier, Symbol, TypeInfo};
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};

use super::type_facts::{SCALA_TYPE_NAME_RULES, base_type_name_node};

const CALL_KINDS: [&str; 2] = ["call_expression", "instance_expression"];
const TYPE_DEFINITIONS: [&str; 4] = [
    "class_definition",
    "object_definition",
    "trait_definition",
    "enum_definition",
];
const STRING_INTERPOLATORS: [&str; 3] = ["s", "f", "raw"];

/// Adds `arg_types` to each Scala call identifier that has `arg_count`, and
/// `arg_names` when an argument is named.
pub(crate) fn enrich(
    tree: &Tree,
    source: &str,
    symbols: &[Symbol],
    types: &HashMap<String, TypeInfo>,
    identifiers: &mut [Identifier],
) {
    let reader = Reader {
        source,
        declarations: Declarations::new(symbols, types),
    };
    argument_types::annotate_calls("scala", tree, identifiers, |node| {
        let call = enclosing(node, &CALL_KINDS)?;
        let list = call.child_by_field_name("arguments")?;
        if list.kind() != "arguments" {
            return Some(CallArguments {
                types: vec![None],
                names: None,
            });
        }
        let mut cursor = list.walk();
        let arguments: Vec<Node> = list
            .named_children(&mut cursor)
            .filter(|argument| !argument.is_extra())
            .collect();
        let mut types = Vec::new();
        let mut names = Vec::new();
        for argument in arguments {
            let named = (argument.kind() == "assignment_expression")
                .then(|| argument.child_by_field_name("left"))
                .flatten()
                .filter(|left| left.kind() == "identifier");
            let value = match named {
                Some(_) => argument.child_by_field_name("right")?,
                None => argument,
            };
            names.push(named.map(|left| strip_backticks(node_text(source, left)).to_string()));
            types.push(reader.type_of(value, 0));
        }
        Some(CallArguments {
            types,
            names: names.iter().any(Option::is_some).then_some(names),
        })
    });
}

struct Reader<'a> {
    source: &'a str,
    declarations: Declarations<'a>,
}

impl Reader<'_> {
    fn type_of(&self, node: Node, depth: u32) -> Option<String> {
        if !should_visit_tree_depth(depth) {
            return None;
        }
        let text = node_text(self.source, node);
        match node.kind() {
            "parenthesized_expression" => {
                self.type_of(node.named_child(0)?, child_tree_depth(depth)?)
            }
            "integer_literal" => Some(suffixed_integer_type(text, "Long", "Int").to_string()),
            "floating_point_literal" => {
                Some(suffixed_float_type(text, "Float", "Double").to_string())
            }
            "character_literal" => Some("Char".to_string()),
            "string" => Some("String".to_string()),
            "boolean_literal" => Some("Boolean".to_string()),
            "interpolated_string_expression" => {
                let interpolator = node.child_by_field_name("interpolator")?;
                STRING_INTERPOLATORS
                    .contains(&self.source.get(interpolator.byte_range())?)
                    .then(|| "String".to_string())
            }
            "prefix_expression" => {
                let operand = node.named_child(0)?;
                (matches!(node.child(0).map(|op| op.kind()), Some("-" | "+"))
                    && matches!(operand.kind(), "integer_literal" | "floating_point_literal"))
                .then(|| self.type_of(operand, child_tree_depth(depth)?))
                .flatten()
            }
            "infix_expression" => {
                let operator = node.child_by_field_name("operator")?;
                let left = node.child_by_field_name("left")?;
                (self.source.get(operator.byte_range())? == "+"
                    && self.type_of(left, child_tree_depth(depth)?).as_deref() == Some("String"))
                .then(|| "String".to_string())
            }
            "instance_expression" => {
                let mut cursor = node.walk();
                let children: Vec<Node> = node
                    .named_children(&mut cursor)
                    .filter(|child| !child.is_extra() && child.kind() != "arguments")
                    .collect();
                match children.as_slice() {
                    [type_node] => self.type_name(*type_node),
                    _ => None,
                }
            }
            "ascription_expression" => {
                let type_node = node.named_child(1)?;
                (type_node.kind() != "repeated_parameter_type")
                    .then(|| self.type_name(type_node))
                    .flatten()
            }
            "generic_function" => {
                let function = node.child_by_field_name("function")?;
                let type_arguments = node.child_by_field_name("type_arguments")?;
                let mut cursor = type_arguments.walk();
                let arguments: Vec<Node> = type_arguments
                    .named_children(&mut cursor)
                    .filter(|child| !child.is_extra())
                    .collect();
                let [type_argument] = arguments.as_slice() else {
                    return None;
                };
                match function.kind() {
                    "identifier" if node_text(self.source, function) == "classOf" => {
                        Some("java.lang.Class".to_string())
                    }
                    "field_expression"
                        if function.child_by_field_name("field").is_some_and(|field| {
                            node_text(self.source, field) == "asInstanceOf"
                        }) =>
                    {
                        self.type_name(*type_argument)
                    }
                    _ => None,
                }
            }
            "field_expression" => {
                let value = node.child_by_field_name("value")?;
                let field = node.child_by_field_name("field")?;
                if value.kind() != "identifier" || node_text(self.source, value) != "this" {
                    return None;
                }
                let body = this_body(value)?;
                let name = strip_backticks(node_text(self.source, field));
                self.type_of_found(name, member(self.source, body, name))
            }
            "identifier" if text == "this" => {
                let body = this_body(node)?;
                type_definition_name(self.source, body.parent()?)
            }
            "identifier" => {
                let name = strip_backticks(text);
                let source = self.source;
                let found =
                    nearest_binding(node, |scope, child| binding(source, scope, child, name));
                self.type_of_found(name, found)
            }
            _ => None,
        }
    }

    fn type_of_found(&self, name: &str, found: Found) -> Option<String> {
        let source = self.source;
        self.declarations
            .type_of_found(name, found, |initializer| literal_type(source, initializer))
    }

    fn type_name(&self, type_node: Node) -> Option<String> {
        written_type_name(self.source, type_node)
    }
}

/// A written type as Scala type facts spell it: the base name of its last
/// segment, without type arguments.
fn written_type_name(source: &str, type_node: Node) -> Option<String> {
    let base = base_type_name_node(type_node)?;
    let name: String = strip_type_decorations(node_text(source, base), &SCALA_TYPE_NAME_RULES)
        .split_whitespace()
        .collect();
    (!name.is_empty()).then_some(name)
}

fn strip_backticks(name: &str) -> &str {
    name.strip_prefix('`')
        .and_then(|name| name.strip_suffix('`'))
        .unwrap_or(name)
}

/// What `scope` binds for `name` used inside `child`.
fn binding<'tree>(
    source: &str,
    scope: Node<'tree>,
    child: Node<'tree>,
    name: &str,
) -> Option<Found<'tree>> {
    let names = |node: Node| names_identifier(source, node, name);
    match scope.kind() {
        "block" | "indented_block" => {
            let mut cursor = scope.walk();
            let definitions: Vec<Node> = scope.named_children(&mut cursor).collect();
            definitions
                .into_iter()
                .filter(|definition| definition.id() != child.id())
                .find_map(|definition| definition_binding(source, definition, name))
        }
        "function_definition" | "given_definition" => {
            let mut cursor = scope.walk();
            let lists: Vec<Node> = scope
                .children_by_field_name("parameters", &mut cursor)
                .collect();
            lists.into_iter().find_map(|list| {
                let mut cursor = list.walk();
                let parameters: Vec<Node> = list.named_children(&mut cursor).collect();
                parameters.into_iter().find_map(|parameter| {
                    let node = parameter.child_by_field_name("name")?;
                    if strip_backticks(node_text(source, node)) != name {
                        return None;
                    }
                    let repeated = parameter
                        .child_by_field_name("type")
                        .is_some_and(|type_node| type_node.kind() == "repeated_parameter_type");
                    Some(if repeated {
                        Found::Unknown
                    } else {
                        Found::Declaration {
                            name: node,
                            initializer: None,
                        }
                    })
                })
            })
        }
        "lambda_expression" => {
            let parameters = scope.child_by_field_name("parameters")?;
            if parameters.id() == child.id() || !names(parameters) {
                return None;
            }
            let mut cursor = parameters.walk();
            let typed = (parameters.kind() == "bindings")
                .then(|| {
                    parameters.named_children(&mut cursor).find(|binding| {
                        binding
                            .child_by_field_name("name")
                            .is_some_and(|node| strip_backticks(node_text(source, node)) == name)
                    })
                })
                .flatten()
                .and_then(|binding| binding.child_by_field_name("type"))
                .and_then(|type_node| written_type_name(source, type_node));
            Some(typed.map_or(Found::Unknown, Found::Type))
        }
        "case_clause" => {
            let pattern = scope.child_by_field_name("pattern")?;
            (pattern.id() != child.id() && names(pattern)).then_some(Found::Unknown)
        }
        "for_expression" => {
            let mut cursor = scope.walk();
            let enumerators: Vec<Node> = scope
                .named_children(&mut cursor)
                .filter(|node| node.kind() == "enumerators")
                .collect();
            enumerators
                .into_iter()
                .any(|enumerators| {
                    let mut cursor = enumerators.walk();
                    let patterns: Vec<Node> = enumerators
                        .named_children(&mut cursor)
                        .filter_map(|enumerator| enumerator.named_child(0))
                        .collect();
                    patterns.into_iter().any(names)
                })
                .then_some(Found::Unknown)
        }
        "template_body" => Some(member(source, scope, name)),
        "compilation_unit" | "class_definition" | "object_definition" | "trait_definition" => {
            Some(Found::Unknown)
        }
        _ => None,
    }
}

/// What one statement of a block or template body says about `name`: a
/// `val` or `var` of that name and its initializer, or no known type for a
/// name a pattern, a `def`, an `object`, or a `given` binds.
fn definition_binding<'tree>(
    source: &str,
    definition: Node<'tree>,
    name: &str,
) -> Option<Found<'tree>> {
    match definition.kind() {
        "val_definition" | "var_definition" | "val_declaration" | "var_declaration" => {
            let pattern = definition
                .child_by_field_name("pattern")
                .or_else(|| definition.child_by_field_name("name"))?;
            if pattern.kind() == "identifier" {
                (strip_backticks(node_text(source, pattern)) == name).then(|| Found::Declaration {
                    name: pattern,
                    initializer: definition.child_by_field_name("value"),
                })
            } else {
                names_identifier(source, pattern, name).then_some(Found::Unknown)
            }
        }
        "function_definition"
        | "function_declaration"
        | "object_definition"
        | "given_definition"
        | "class_definition" => definition
            .child_by_field_name("name")
            .filter(|node| strip_backticks(node_text(source, *node)) == name)
            .map(|_| Found::Unknown),
        _ => None,
    }
}

/// A member named `name` of the template body, or a constructor parameter
/// of its class. Any other name may be inherited, so it has no known type.
fn member<'tree>(source: &str, body: Node<'tree>, name: &str) -> Found<'tree> {
    let mut cursor = body.walk();
    let members: Vec<Node> = body.named_children(&mut cursor).collect();
    if let Some(found) = members
        .into_iter()
        .find_map(|definition| definition_binding(source, definition, name))
    {
        return found;
    }
    let Some(owner) = body.parent() else {
        return Found::Unknown;
    };
    let mut cursor = owner.walk();
    let lists: Vec<Node> = owner
        .named_children(&mut cursor)
        .filter(|child| child.kind() == "class_parameters")
        .collect();
    lists
        .into_iter()
        .find_map(|list| {
            let mut cursor = list.walk();
            let parameters: Vec<Node> = list.named_children(&mut cursor).collect();
            parameters.into_iter().find_map(|parameter| {
                let node = parameter.child_by_field_name("name")?;
                if parameter.kind() != "class_parameter"
                    || strip_backticks(node_text(source, node)) != name
                {
                    return None;
                }
                let repeated = parameter
                    .child_by_field_name("type")
                    .is_some_and(|type_node| type_node.kind() == "repeated_parameter_type");
                Some(if repeated {
                    Found::Unknown
                } else {
                    Found::Declaration {
                        name: node,
                        initializer: None,
                    }
                })
            })
        })
        .unwrap_or(Found::Unknown)
}

/// The template body of the class, object, trait, or enum that `this` names
/// at `node`, or none inside an anonymous class.
fn this_body(node: Node<'_>) -> Option<Node<'_>> {
    let body = std::iter::successors(node.parent(), Node::parent)
        .find(|ancestor| ancestor.kind() == "template_body")?;
    TYPE_DEFINITIONS
        .contains(&body.parent()?.kind())
        .then_some(body)
}

fn type_definition_name(source: &str, definition: Node) -> Option<String> {
    TYPE_DEFINITIONS
        .contains(&definition.kind())
        .then(|| definition.child_by_field_name("name"))
        .flatten()
        .map(|name| strip_backticks(node_text(source, name)).to_string())
}

fn names_identifier(source: &str, root: Node, name: &str) -> bool {
    let mut stack = vec![root];
    while let Some(node) = stack.pop() {
        if node.kind() == "identifier" && strip_backticks(node_text(source, node)) == name {
            return true;
        }
        let mut cursor = node.walk();
        stack.extend(node.named_children(&mut cursor));
    }
    false
}

/// The type of a literal initializer of a `val` or `var` with no type fact.
fn literal_type(source: &str, node: Node) -> Option<String> {
    let text = node_text(source, node);
    Some(
        match node.kind() {
            "integer_literal" => suffixed_integer_type(text, "Long", "Int"),
            "floating_point_literal" => suffixed_float_type(text, "Float", "Double"),
            "character_literal" => "Char",
            "string" => "String",
            "boolean_literal" => "Boolean",
            _ => return None,
        }
        .to_string(),
    )
}
