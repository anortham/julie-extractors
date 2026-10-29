//! The static type of each argument of a Kotlin call, so a reader can tell
//! which overload the call binds: `run(1)` binds `run(a: Int)`, not
//! `run(a: String)`. A named argument also gets its name in `arg_names`.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use tree_sitter::{Node, Tree};

use crate::base::argument_types::{
    self, CallArguments, Declarations, Found, MemberNames, ScopeBindings, enclosing,
    nearest_binding, node_text, suffixed_float_type,
};
use crate::base::types::strip_type_decorations;
use crate::base::{BaseExtractor, Identifier, Symbol, TypeInfo};
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};

use super::helpers::strip_backticks;
use super::type_facts::{
    InitializerIndex, KOTLIN_TYPE_NAME_RULES, base_type_name, declared_type_child,
};

const TYPE_BODIES: [&str; 2] = ["class_body", "enum_class_body"];
const FUNCTION_KINDS: [&str; 6] = [
    "function_declaration",
    "anonymous_function",
    "secondary_constructor",
    "getter",
    "setter",
    "lambda_literal",
];

/// Adds `arg_types` to each Kotlin call identifier that has `arg_count`, and
/// `arg_names` when an argument is named. A call with a trailing lambda gets
/// neither, since the lambda fills the last parameter, not the next one.
pub(crate) fn enrich(
    tree: &Tree,
    source: &str,
    symbols: &[Symbol],
    types: &HashMap<String, TypeInfo>,
    identifiers: &mut [Identifier],
) {
    let base = BaseExtractor::new(
        "kotlin".to_string(),
        String::new(),
        source.to_string(),
        Path::new(""),
    );
    let mut reader = Reader {
        base: &base,
        source,
        declarations: Declarations::new(symbols, types),
        index: None,
        root: tree.root_node(),
        casts: MemberNames::new(),
        bindings: ScopeBindings::new(false),
    };
    argument_types::annotate_calls("kotlin", tree, identifiers, |node, _| {
        let call = enclosing(node, &["call_expression"])?;
        let mut cursor = call.walk();
        let list = call
            .named_children(&mut cursor)
            .find(|child| child.kind() == "value_arguments")?;
        let mut cursor = list.walk();
        let arguments: Vec<Node> = list
            .named_children(&mut cursor)
            .filter(|argument| argument.kind() == "value_argument")
            .collect();
        let mut types = Vec::new();
        let mut names = Vec::new();
        for argument in arguments {
            let mut cursor = argument.walk();
            let children: Vec<Node> = argument.children(&mut cursor).collect();
            let is_named = children.iter().any(|child| child.kind() == "=");
            let value = argument.named_children(&mut argument.walk()).last()?;
            let name = if is_named {
                Some(strip_backticks(node_text(source, argument.named_child(0)?)).to_string())
            } else {
                None
            };
            names.push(name);
            types.push(reader.type_of(value, 0));
        }
        Some(CallArguments {
            types,
            names: names.iter().any(Option::is_some).then_some(names),
        })
    });
}

struct Reader<'a, 'tree> {
    base: &'a BaseExtractor,
    source: &'a str,
    declarations: Declarations<'a>,
    index: Option<InitializerIndex>,
    root: Node<'tree>,
    /// The names whose type each member tests or casts.
    casts: MemberNames,
    bindings: ScopeBindings<'tree, Found<'tree>>,
}

impl<'tree> Reader<'_, 'tree> {
    fn type_of(&mut self, node: Node<'tree>, depth: u32) -> Option<String> {
        if !should_visit_tree_depth(depth) {
            return None;
        }
        let text = node_text(self.source, node);
        match node.kind() {
            "parenthesized_expression" => {
                self.type_of(node.named_child(0)?, child_tree_depth(depth)?)
            }
            "number_literal" => integer_type(text).map(str::to_string),
            "float_literal" => Some(suffixed_float_type(text, "Float", "Double").to_string()),
            "character_literal" => Some("Char".to_string()),
            "string_literal" | "multiline_string_literal" => Some("String".to_string()),
            "unary_expression" => {
                let operand = node.named_child(0)?;
                let operator = node.child(0)?;
                (matches!(operator.kind(), "-" | "+")
                    && matches!(operand.kind(), "number_literal" | "float_literal"))
                .then(|| self.type_of(operand, child_tree_depth(depth)?))
                .flatten()
            }
            "binary_expression" => {
                let operator = node.child_by_field_name("operator")?;
                let left = node.child_by_field_name("left")?;
                (operator.kind() == "+"
                    && self.type_of(left, child_tree_depth(depth)?).as_deref() == Some("String"))
                .then(|| "String".to_string())
            }
            "as_expression" => {
                let right = node.child_by_field_name("right")?;
                let name = base_type_name(self.base, right)?;
                Some(
                    strip_type_decorations(&name, &KOTLIN_TYPE_NAME_RULES)
                        .split_whitespace()
                        .collect(),
                )
                .filter(|name: &String| !name.is_empty())
            }
            "call_expression" => {
                let base = self.base;
                let root = self.root;
                self.index
                    .get_or_insert_with(|| InitializerIndex::build(base, root))
                    .constructed_type(base, node)
            }
            "navigation_expression" => {
                let mut cursor = node.walk();
                let parts: Vec<Node> = node.children(&mut cursor).collect();
                match parts.as_slice() {
                    [receiver, separator, member]
                        if separator.kind() == "::"
                            && receiver.kind() == "identifier"
                            && node_text(self.source, *member) == "class" =>
                    {
                        Some("kotlin.reflect.KClass".to_string())
                    }
                    [receiver, separator, member]
                        if separator.kind() == "."
                            && receiver.kind() == "this_expression"
                            && receiver.named_child_count() == 0
                            && member.kind() == "identifier" =>
                    {
                        let name = strip_backticks(node_text(self.source, *member)).to_string();
                        if self.member_casts(node, &name) || self.member_casts(node, "this") {
                            return None;
                        }
                        let found = self.member_of_this(node, &name)?;
                        self.type_of_found(&name, found)
                    }
                    _ => None,
                }
            }
            "this_expression" => self.this_type(node),
            "identifier" => match text {
                "true" | "false" => Some("Boolean".to_string()),
                "null" => None,
                _ => self.value_type(node),
            },
            _ => None,
        }
    }

    fn type_of_found(&self, name: &str, found: Found) -> Option<String> {
        let source = self.source;
        self.declarations
            .type_of_found(name, found, |initializer| literal_type(source, initializer))
    }

    /// The type of the local, parameter, or property that the identifier
    /// names here. A name whose type a check or cast in the member narrows
    /// (`x is Box`, `x as Box`, `when (x)`) has no known type.
    fn value_type(&mut self, node: Node<'tree>) -> Option<String> {
        let name = strip_backticks(self.source.get(node.byte_range())?).to_string();
        if self.member_casts(node, &name) {
            return None;
        }
        let base = self.base;
        let bindings = &self.bindings;
        let mut crossed = Crossed::default();
        let found = nearest_binding(node, |scope, child| {
            binding(base, bindings, scope, child, &name, &mut crossed)
        });
        self.type_of_found(&name, found)
    }

    /// The property `name` of the class that `this` names at `node`.
    fn member_of_this(&self, node: Node<'tree>, name: &str) -> Option<Found<'tree>> {
        let body = this_body(node)?;
        Some(member(self.source, &self.bindings, body, name, false))
    }

    fn this_type(&mut self, node: Node<'tree>) -> Option<String> {
        if self.member_casts(node, "this") {
            return None;
        }
        match node.named_child(0) {
            None => {
                let body = this_body(node)?;
                declared_type_name(self.source, body.parent()?)
            }
            Some(label) => {
                let label = strip_backticks(node_text(self.source, label));
                let mut ancestors = std::iter::successors(node.parent(), Node::parent);
                ancestors
                    .find_map(|ancestor| match ancestor.kind() {
                        "annotated_lambda" | "function_declaration"
                            if labels(self.source, ancestor) == Some(label) =>
                        {
                            Some(None)
                        }
                        "class_declaration" | "object_declaration"
                            if declared_type_name(self.source, ancestor).as_deref()
                                == Some(label) =>
                        {
                            Some(Some(label.to_string()))
                        }
                        _ => None,
                    })
                    .flatten()
            }
        }
    }

    fn member_casts(&mut self, node: Node<'tree>, name: &str) -> bool {
        let member = std::iter::successors(Some(node), Node::parent)
            .find(|ancestor| {
                ancestor.parent().is_none_or(|parent| {
                    TYPE_BODIES.contains(&parent.kind()) || parent.kind() == "source_file"
                })
            })
            .unwrap_or(node);
        let source = self.source;
        let add = |node: Option<Node<'tree>>, names: &mut HashSet<String>| match node {
            Some(node) if node.kind() == "this_expression" && node.named_child_count() == 0 => {
                names.insert("this".to_string());
            }
            Some(node) if node.kind() == "identifier" => {
                names.insert(strip_backticks(node_text(source, node)).to_string());
            }
            _ => {}
        };
        self.casts
            .holds(member, name, |current, names| match current.kind() {
                "is_expression" => add(current.child_by_field_name("left"), names),
                "as_expression" => {
                    let mut cursor = current.walk();
                    if current
                        .children(&mut cursor)
                        .any(|child| child.kind() == "as")
                    {
                        add(current.child_by_field_name("left"), names);
                    }
                }
                "when_subject" => {
                    let mut inner = vec![current];
                    while let Some(node) = inner.pop() {
                        add(Some(node), names);
                        let mut cursor = node.walk();
                        inner.extend(node.named_children(&mut cursor));
                    }
                }
                _ => {}
            })
    }
}

/// What the walk from a name passed on its way out: a lambda or an extension
/// function, whose receiver can supply a member of that name before the
/// class does, and a function, which cannot see a plain constructor
/// parameter of its class.
#[derive(Default)]
struct Crossed {
    receiver: bool,
    function: bool,
}

/// What `scope` binds for `name` used inside `child`.
fn binding<'tree>(
    base: &BaseExtractor,
    bindings: &ScopeBindings<'tree, Found<'tree>>,
    scope: Node<'tree>,
    child: Node<'tree>,
    name: &str,
    crossed: &mut Crossed,
) -> Option<Found<'tree>> {
    let source = base.content.as_str();
    let declaration = |node: Node<'tree>| Found::Declaration {
        name: node,
        initializer: None,
    };
    let local = || {
        bindings.last_binding(source, scope, Some(child.start_byte()), name, |statement| {
            (statement.kind() == "property_declaration")
                .then(|| property(source, statement, name))
                .flatten()
        })
    };
    let found = match scope.kind() {
        "block" => local(),
        "lambda_literal" => {
            let mut cursor = scope.walk();
            let parameters = scope
                .named_children(&mut cursor)
                .find(|node| node.kind() == "lambda_parameters");
            local().or_else(|| match parameters {
                Some(parameters) if parameters.id() != child.id() => {
                    variable_names(base, parameters, name)
                }
                None if name == "it" => Some(Found::Unknown),
                _ => None,
            })
        }
        "function_declaration" | "anonymous_function" | "secondary_constructor" => {
            function_parameter(source, scope, name)
        }
        "setter" => {
            let mut cursor = scope.walk();
            scope
                .named_children(&mut cursor)
                .find(|node| node.kind() == "identifier" || node.kind() == "parameter")
                .and_then(|node| {
                    let node = if node.kind() == "parameter" {
                        node.named_child(0)?
                    } else {
                        node
                    };
                    (strip_backticks(node_text(source, node)) == name).then(|| declaration(node))
                })
        }
        "for_statement" => {
            let mut cursor = scope.walk();
            let close = scope
                .children(&mut cursor)
                .find(|node| node.kind() == ")")?;
            (child.start_byte() >= close.end_byte())
                .then(|| variable_names(base, scope, name))
                .flatten()
        }
        "catch_block" => {
            let mut cursor = scope.walk();
            let caught = scope
                .named_children(&mut cursor)
                .find(|node| node.kind() == "identifier")?;
            (caught.id() != child.id() && strip_backticks(node_text(source, caught)) == name)
                .then(|| declaration(caught))
        }
        "when_expression" => {
            let mut cursor = scope.walk();
            let subject = scope
                .named_children(&mut cursor)
                .find(|node| node.kind() == "when_subject")?;
            if subject.id() == child.id() {
                return None;
            }
            names_variable(source, subject, name).then_some(Found::Unknown)
        }
        kind if TYPE_BODIES.contains(&kind) => {
            return Some(if crossed.receiver {
                Found::Unknown
            } else {
                member(source, bindings, scope, name, crossed.function)
            });
        }
        "source_file" | "class_declaration" | "object_literal" => return Some(Found::Unknown),
        _ => None,
    };
    if FUNCTION_KINDS.contains(&scope.kind()) {
        crossed.function = true;
        crossed.receiver |= scope.kind() == "lambda_literal" || has_receiver(scope);
    }
    found
}

/// What a property declaration says about `name`: its variable and
/// initializer, or no known type for a name in a destructuring declaration.
fn property<'tree>(source: &str, declaration: Node<'tree>, name: &str) -> Option<Found<'tree>> {
    let mut cursor = declaration.walk();
    let children: Vec<Node> = declaration.children(&mut cursor).collect();
    let initializer = children
        .iter()
        .position(|child| child.kind() == "=")
        .and_then(|index| children.get(index + 1).copied());
    children.iter().find_map(|child| match child.kind() {
        "variable_declaration" => {
            let node = child.named_child(0)?;
            (strip_backticks(node_text(source, node)) == name).then_some(Found::Declaration {
                name: node,
                initializer,
            })
        }
        "multi_variable_declaration" => {
            names_variable(source, *child, name).then_some(Found::Unknown)
        }
        _ => None,
    })
}

/// A `variable_declaration` named `name` among the children of `holder`
/// (lambda parameters or a `for` variable) and its written type, or no known
/// type for a name in a destructuring declaration there.
fn variable_names<'tree>(
    base: &BaseExtractor,
    holder: Node<'tree>,
    name: &str,
) -> Option<Found<'tree>> {
    let source = base.content.as_str();
    let mut cursor = holder.walk();
    let children: Vec<Node> = holder.named_children(&mut cursor).collect();
    children.into_iter().find_map(|child| match child.kind() {
        "variable_declaration" => {
            let node = child.named_child(0)?;
            if strip_backticks(node_text(source, node)) != name {
                return None;
            }
            let written = declared_type_child(child)
                .and_then(|type_node| base_type_name(base, type_node))
                .map(|type_name| {
                    strip_type_decorations(&type_name, &KOTLIN_TYPE_NAME_RULES)
                        .split_whitespace()
                        .collect::<String>()
                })
                .filter(|type_name| !type_name.is_empty());
            Some(match written {
                Some(type_name) => Found::Type(type_name),
                None => Found::Declaration {
                    name: node,
                    initializer: None,
                },
            })
        }
        "multi_variable_declaration" => {
            names_variable(source, child, name).then_some(Found::Unknown)
        }
        _ => None,
    })
}

fn names_variable(source: &str, holder: Node, name: &str) -> bool {
    let mut cursor = holder.walk();
    let children: Vec<Node> = holder.named_children(&mut cursor).collect();
    children.into_iter().any(|child| {
        child.kind() == "variable_declaration"
            && child
                .named_child(0)
                .is_some_and(|node| strip_backticks(node_text(source, node)) == name)
    })
}

/// A parameter named `name` of a function or constructor. A `vararg`
/// parameter holds an array, not a value of its written type.
fn function_parameter<'tree>(source: &str, scope: Node<'tree>, name: &str) -> Option<Found<'tree>> {
    let mut cursor = scope.walk();
    let parameters = scope
        .named_children(&mut cursor)
        .find(|node| node.kind() == "function_value_parameters")?;
    let mut cursor = parameters.walk();
    let children: Vec<Node> = parameters.named_children(&mut cursor).collect();
    children.iter().enumerate().find_map(|(index, parameter)| {
        if parameter.kind() != "parameter" {
            return None;
        }
        let node = parameter.named_child(0)?;
        if strip_backticks(node_text(source, node)) != name {
            return None;
        }
        let is_vararg = index.checked_sub(1).is_some_and(|previous| {
            children[previous].kind() == "parameter_modifiers"
                && node_text(source, children[previous]).contains("vararg")
        });
        Some(if is_vararg {
            Found::Unknown
        } else {
            Found::Declaration {
                name: node,
                initializer: None,
            }
        })
    })
}

/// True for an extension function, whose receiver type comes before its name.
fn has_receiver(function: Node) -> bool {
    let Some(name) = function.child_by_field_name("name") else {
        return false;
    };
    let mut cursor = function.walk();
    function.named_children(&mut cursor).any(|child| {
        child.end_byte() <= name.start_byte()
            && !matches!(child.kind(), "modifiers" | "type_parameters")
    })
}

/// A property named `name` of the class body, or a constructor parameter of
/// its class: a `val` or `var` parameter is a property, and a plain one is
/// visible only outside the class's functions. Any other name may be
/// inherited or come from a companion, so it has no known type.
fn member<'tree>(
    source: &str,
    bindings: &ScopeBindings<'tree, Found<'tree>>,
    body: Node<'tree>,
    name: &str,
    in_function: bool,
) -> Found<'tree> {
    let declared =
        bindings.first_binding_except(source, body, body, name, |member| match member.kind() {
            "property_declaration" => property(source, member, name),
            "enum_entry" => member
                .named_child(0)
                .is_some_and(|node| strip_backticks(node_text(source, node)) == name)
                .then_some(Found::Unknown),
            _ => None,
        });
    if let Some(found) = declared {
        return found;
    }
    let parameter = body
        .parent()
        .filter(|class| class.kind() == "class_declaration")
        .and_then(|class| {
            let mut cursor = class.walk();
            class
                .named_children(&mut cursor)
                .find(|child| child.kind() == "primary_constructor")
        })
        .and_then(|constructor| {
            let mut stack = vec![constructor];
            while let Some(node) = stack.pop() {
                if node.kind() == "class_parameter" {
                    let identifier = node.named_child(0);
                    let name_node = {
                        let mut cursor = node.walk();
                        node.named_children(&mut cursor)
                            .find(|child| child.kind() == "identifier")
                    };
                    if let Some(name_node) = name_node.or(identifier)
                        && strip_backticks(node_text(source, name_node)) == name
                    {
                        let mut cursor = node.walk();
                        let is_property = node
                            .children(&mut cursor)
                            .any(|child| matches!(child.kind(), "val" | "var"));
                        return Some((name_node, is_property));
                    }
                    continue;
                }
                let mut cursor = node.walk();
                stack.extend(node.named_children(&mut cursor));
            }
            None
        });
    match parameter {
        Some((node, is_property)) if is_property || !in_function => Found::Declaration {
            name: node,
            initializer: None,
        },
        _ => Found::Unknown,
    }
}

/// The body of the class or named object that `this` names at `node`: none
/// inside a lambda or an extension function, whose receiver `this` can name,
/// and none inside a companion or an anonymous object.
fn this_body(node: Node<'_>) -> Option<Node<'_>> {
    let mut current = node.parent();
    while let Some(ancestor) = current {
        match ancestor.kind() {
            "lambda_literal" => return None,
            "function_declaration" if has_receiver(ancestor) => return None,
            kind if TYPE_BODIES.contains(&kind) => {
                let owner = ancestor.parent()?;
                return matches!(owner.kind(), "class_declaration" | "object_declaration")
                    .then_some(ancestor);
            }
            _ => {}
        }
        current = ancestor.parent();
    }
    None
}

fn declared_type_name(source: &str, declaration: Node) -> Option<String> {
    matches!(
        declaration.kind(),
        "class_declaration" | "object_declaration"
    )
    .then(|| declaration.child_by_field_name("name"))
    .flatten()
    .map(|name| strip_backticks(node_text(source, name)).to_string())
}

/// The label of a labeled lambda (`lbl@{ … }`) or the name of a function,
/// which `this@name` can also name.
fn labels<'a>(source: &'a str, node: Node) -> Option<&'a str> {
    match node.kind() {
        "annotated_lambda" => {
            let mut cursor = node.walk();
            let label = node
                .named_children(&mut cursor)
                .find(|child| child.kind() == "label")?;
            Some(strip_backticks(
                node_text(source, label).trim_end_matches('@'),
            ))
        }
        _ => node
            .child_by_field_name("name")
            .map(|name| strip_backticks(node_text(source, name))),
    }
}

/// The type of a literal initializer of a property with no type fact.
fn literal_type(source: &str, node: Node) -> Option<String> {
    let text = node_text(source, node);
    match node.kind() {
        "number_literal" => integer_type(text).map(str::to_string),
        "float_literal" => Some(suffixed_float_type(text, "Float", "Double").to_string()),
        "character_literal" => Some("Char".to_string()),
        "string_literal" | "multiline_string_literal" => Some("String".to_string()),
        "identifier" if matches!(text, "true" | "false") => Some("Boolean".to_string()),
        _ => None,
    }
}

/// The type of a Kotlin integer literal: `L` gives `Long`, `u` gives `UInt`
/// or, past its range, `ULong`, and `uL` gives `ULong`. With no suffix the
/// value picks `Int`, else `Long`; a value past `Long` has no type.
fn integer_type(text: &str) -> Option<&'static str> {
    let lower = text.to_ascii_lowercase().replace('_', "");
    let (number, suffix) = if let Some(number) = lower.strip_suffix("ul") {
        (number, "ul")
    } else if let Some(number) = lower.strip_suffix('u') {
        (number, "u")
    } else if let Some(number) = lower.strip_suffix('l') {
        (number, "l")
    } else {
        (lower.as_str(), "")
    };
    let value = if let Some(digits) = number.strip_prefix("0x") {
        u64::from_str_radix(digits, 16)
    } else if let Some(digits) = number.strip_prefix("0b") {
        u64::from_str_radix(digits, 2)
    } else {
        number.parse::<u64>()
    }
    .ok()?;
    Some(match suffix {
        "ul" => "ULong",
        "u" if value <= u64::from(u32::MAX) => "UInt",
        "u" => "ULong",
        "l" if value <= i64::MAX as u64 => "Long",
        "" if value <= i32::MAX as u64 => "Int",
        "" if value <= i64::MAX as u64 => "Long",
        _ => return None,
    })
}
