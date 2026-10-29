//! The static type of each argument of a Java method call or `new`
//! expression, so a reader can tell which overload the call binds: `run(1)`
//! binds `run(int)`, not `run(String)`.

use std::collections::HashMap;

use tree_sitter::{Node, Tree};

use crate::base::argument_types::{
    self, CallArguments, Declarations, Found, before, enclosing, nearest_binding, node_text,
    suffixed_float_type, suffixed_integer_type,
};
use crate::base::types::strip_type_decorations;
use crate::base::{Identifier, Symbol, TypeInfo};
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};

use super::type_facts::{JAVA_TYPE_NAME_RULES, names_single_base_type};

const CALL_KINDS: [&str; 2] = ["method_invocation", "object_creation_expression"];
const TYPE_BODIES: [&str; 5] = [
    "class_body",
    "interface_body",
    "enum_body",
    "enum_body_declarations",
    "annotation_type_body",
];
const PATTERN_KINDS: [&str; 3] = ["type_pattern", "record_pattern_component", "record_pattern"];

/// Adds `arg_types` to each Java call identifier that has `arg_count`: one
/// entry per argument, the type its expression states, or `null`.
pub(crate) fn enrich(
    tree: &Tree,
    source: &str,
    symbols: &[Symbol],
    types: &HashMap<String, TypeInfo>,
    identifiers: &mut [Identifier],
) {
    let mut reader = Reader {
        source,
        declarations: Declarations::new(symbols, types),
        patterns: HashMap::new(),
    };
    argument_types::annotate_calls("java", tree, identifiers, |node| {
        let call = enclosing(node, &CALL_KINDS)?;
        let list = call.child_by_field_name("arguments")?;
        let mut cursor = list.walk();
        let arguments: Vec<Node> = list
            .named_children(&mut cursor)
            .filter(|argument| !argument.is_extra())
            .collect();
        Some(CallArguments {
            types: arguments
                .into_iter()
                .map(|argument| reader.type_of(argument, 0))
                .collect(),
            names: None,
        })
    });
}

struct Reader<'a> {
    source: &'a str,
    declarations: Declarations<'a>,
    /// Whether a member declares a pattern variable of a name, by the
    /// member's node id and the name.
    patterns: HashMap<(usize, String), bool>,
}

impl Reader<'_> {
    fn type_of(&mut self, node: Node, depth: u32) -> Option<String> {
        if !should_visit_tree_depth(depth) {
            return None;
        }
        let text = node_text(self.source, node);
        match node.kind() {
            "parenthesized_expression" => {
                self.type_of(node.named_child(0)?, child_tree_depth(depth)?)
            }
            "decimal_integer_literal"
            | "hex_integer_literal"
            | "octal_integer_literal"
            | "binary_integer_literal" => Some(suffixed_integer_type(text, "long", "int").into()),
            "decimal_floating_point_literal" | "hex_floating_point_literal" => {
                Some(suffixed_float_type(text, "float", "double").into())
            }
            "character_literal" => Some("char".into()),
            "string_literal" => Some("String".into()),
            "true" | "false" => Some("boolean".into()),
            "class_literal" => Some("java.lang.Class".into()),
            "unary_expression" => {
                let operand = node.child_by_field_name("operand")?;
                let operator = node.child_by_field_name("operator")?;
                (matches!(operator.kind(), "-" | "+") && is_number_literal(operand.kind()))
                    .then(|| self.type_of(operand, child_tree_depth(depth)?))
                    .flatten()
            }
            "binary_expression" => self
                .is_string_concatenation(node, child_tree_depth(depth)?)
                .then(|| "String".into()),
            "object_creation_expression" => {
                let mut cursor = node.walk();
                if node
                    .named_children(&mut cursor)
                    .any(|child| child.kind() == "class_body")
                {
                    return None;
                }
                self.type_name(node.child_by_field_name("type")?)
            }
            "cast_expression" => self.type_name(node.child_by_field_name("type")?),
            "this" => enclosing_type_name(self.source, node),
            "field_access" => {
                let object = node.child_by_field_name("object")?;
                let field = node.child_by_field_name("field")?;
                match (object.kind(), field.kind()) {
                    ("this", "identifier") => {
                        let body = enclosing(node, &TYPE_BODIES)?;
                        let name = self.source.get(field.byte_range())?;
                        let found = member(self.source, body, name);
                        self.type_of_found(name, found)
                    }
                    ("identifier", "this") => {
                        let name = self.source.get(object.byte_range())?;
                        std::iter::successors(node.parent(), Node::parent)
                            .filter_map(|ancestor| declared_type_name(self.source, ancestor))
                            .find(|declared| *declared == name)
                            .map(str::to_string)
                    }
                    _ => None,
                }
            }
            "identifier" => self.value_type(node),
            _ => None,
        }
    }

    /// True for a `+` whose left spine of `+` operands holds a `String`
    /// operand: Java joins strings once either operand is a string.
    fn is_string_concatenation(&mut self, node: Node, depth: u32) -> bool {
        let mut current = node;
        loop {
            if current.kind() != "binary_expression" {
                return self.type_of(current, depth).as_deref() == Some("String");
            }
            if current.child_by_field_name("operator").map(|op| op.kind()) != Some("+") {
                return false;
            }
            let (Some(left), Some(right)) = (
                current.child_by_field_name("left"),
                current.child_by_field_name("right"),
            ) else {
                return false;
            };
            if self.type_of(right, depth).as_deref() == Some("String") {
                return true;
            }
            current = left;
        }
    }

    fn type_name(&self, type_node: Node) -> Option<String> {
        if !names_single_base_type(type_node) {
            return None;
        }
        let stripped =
            strip_type_decorations(node_text(self.source, type_node), &JAVA_TYPE_NAME_RULES);
        let name: String = stripped.split_whitespace().collect();
        (!name.is_empty() && name != "var").then_some(name)
    }

    /// The type of the local, parameter, or field that the identifier names
    /// here, or `None` when a pattern variable of that name is declared in
    /// the member, since a pattern's scope follows the flow of the code.
    fn value_type(&mut self, node: Node) -> Option<String> {
        let name = self.source.get(node.byte_range())?.to_string();
        if self.member_has_pattern(node, &name) {
            return None;
        }
        let source = self.source;
        let found = nearest_binding(node, |scope, child| binding(source, scope, child, &name));
        self.type_of_found(&name, found)
    }

    fn type_of_found(&self, name: &str, found: Found) -> Option<String> {
        self.declarations.type_of_found(name, found, |initializer| {
            literal_type(self.source, initializer)
        })
    }

    fn member_has_pattern(&mut self, node: Node, name: &str) -> bool {
        let member = std::iter::successors(Some(node), Node::parent)
            .find(|ancestor| {
                ancestor
                    .parent()
                    .is_none_or(|parent| TYPE_BODIES.contains(&parent.kind()))
            })
            .unwrap_or(node);
        let key = (member.id(), name.to_string());
        if let Some(found) = self.patterns.get(&key) {
            return *found;
        }
        let mut stack = vec![member];
        let mut found = false;
        while let Some(current) = stack.pop() {
            let binder = match current.kind() {
                "instanceof_expression" => current.child_by_field_name("name"),
                kind if PATTERN_KINDS.contains(&kind) => {
                    let mut cursor = current.walk();
                    current
                        .named_children(&mut cursor)
                        .find(|child| child.kind() == "identifier")
                }
                _ => None,
            };
            if binder.is_some_and(|binder| node_text(self.source, binder) == name) {
                found = true;
                break;
            }
            let mut cursor = current.walk();
            stack.extend(current.named_children(&mut cursor));
        }
        self.patterns.insert(key, found);
        found
    }
}

fn is_number_literal(kind: &str) -> bool {
    kind.ends_with("_integer_literal") || kind.ends_with("_floating_point_literal")
}

/// The type of a literal initializer, for a `var` whose type fact is absent.
fn literal_type(source: &str, node: Node) -> Option<String> {
    let text = node_text(source, node);
    Some(
        match node.kind() {
            kind if kind.ends_with("_integer_literal") => {
                suffixed_integer_type(text, "long", "int")
            }
            kind if kind.ends_with("_floating_point_literal") => {
                suffixed_float_type(text, "float", "double")
            }
            "character_literal" => "char",
            "string_literal" => "String",
            "true" | "false" => "boolean",
            _ => return None,
        }
        .to_string(),
    )
}

/// What `scope` binds for `name` used inside `child`.
fn binding<'tree>(
    source: &str,
    scope: Node<'tree>,
    child: Node<'tree>,
    name: &str,
) -> Option<Found<'tree>> {
    let named = |node: Option<Node<'tree>>| node.filter(|node| node_text(source, *node) == name);
    match scope.kind() {
        "block" | "constructor_body" | "switch_block_statement_group" => {
            local_declaration(source, before(scope, child), name)
        }
        "switch_block" => local_declaration(
            source,
            before(scope, child)
                .into_iter()
                .filter(|group| group.kind() == "switch_block_statement_group")
                .flat_map(|group| {
                    let mut cursor = group.walk();
                    group.named_children(&mut cursor).collect::<Vec<_>>()
                })
                .collect(),
            name,
        ),
        "for_statement" => {
            let init = scope.child_by_field_name("init")?;
            (init.id() != child.id())
                .then(|| local_declaration(source, vec![init], name))
                .flatten()
        }
        "enhanced_for_statement" => {
            let body = scope.child_by_field_name("body")?;
            (body.id() == child.id())
                .then(|| named(scope.child_by_field_name("name")))
                .flatten()
                .map(|node| Found::Declaration {
                    name: node,
                    initializer: None,
                })
        }
        "catch_clause" => {
            let mut cursor = scope.walk();
            let parameter = scope
                .named_children(&mut cursor)
                .find(|node| node.kind() == "catch_formal_parameter")?;
            (parameter.id() != child.id())
                .then(|| named(parameter.child_by_field_name("name")))
                .flatten()
                .map(|node| Found::Declaration {
                    name: node,
                    initializer: None,
                })
        }
        "resource_specification" => resource(source, before(scope, child), name),
        "try_with_resources_statement" => {
            let resources = scope.child_by_field_name("resources")?;
            if resources.id() == child.id() {
                return None;
            }
            let mut cursor = resources.walk();
            resource(
                source,
                resources.named_children(&mut cursor).collect(),
                name,
            )
        }
        "lambda_expression" => {
            let parameters = scope.child_by_field_name("parameters")?;
            if parameters.id() == child.id() {
                return None;
            }
            let names: Vec<Node> = match parameters.kind() {
                "identifier" => vec![parameters],
                "inferred_parameters" => {
                    let mut cursor = parameters.walk();
                    parameters.named_children(&mut cursor).collect()
                }
                _ => parameter_names(parameters),
            };
            names
                .into_iter()
                .find(|node| node_text(source, *node) == name)
                .map(|node| Found::Declaration {
                    name: node,
                    initializer: None,
                })
        }
        "method_declaration" | "constructor_declaration" => {
            let parameters = scope.child_by_field_name("parameters")?;
            parameter_names(parameters)
                .into_iter()
                .find(|node| node_text(source, *node) == name)
                .map(|node| Found::Declaration {
                    name: node,
                    initializer: None,
                })
        }
        kind if TYPE_BODIES.contains(&kind) => Some(member(source, scope, name)),
        "program" => Some(Found::Unknown),
        _ => None,
    }
}

fn parameter_names(parameters: Node) -> Vec<Node> {
    let mut cursor = parameters.walk();
    parameters
        .named_children(&mut cursor)
        .filter_map(|parameter| match parameter.kind() {
            "formal_parameter" => parameter.child_by_field_name("name"),
            "spread_parameter" => {
                let mut cursor = parameter.walk();
                parameter
                    .named_children(&mut cursor)
                    .find(|child| child.kind() == "variable_declarator")
                    .and_then(|declarator| declarator.child_by_field_name("name"))
            }
            _ => None,
        })
        .collect()
}

/// The last declarator named `name` among the `local_variable_declaration`s.
fn local_declaration<'tree>(
    source: &str,
    statements: Vec<Node<'tree>>,
    name: &str,
) -> Option<Found<'tree>> {
    statements
        .into_iter()
        .filter(|statement| statement.kind() == "local_variable_declaration")
        .flat_map(|statement| {
            let mut cursor = statement.walk();
            statement
                .children_by_field_name("declarator", &mut cursor)
                .collect::<Vec<_>>()
        })
        .rfind(|declarator| {
            declarator
                .child_by_field_name("name")
                .is_some_and(|node| node_text(source, node) == name)
        })
        .and_then(|declarator| {
            Some(Found::Declaration {
                name: declarator.child_by_field_name("name")?,
                initializer: declarator.child_by_field_name("value"),
            })
        })
}

fn resource<'tree>(source: &str, resources: Vec<Node<'tree>>, name: &str) -> Option<Found<'tree>> {
    resources
        .into_iter()
        .filter(|resource| resource.kind() == "resource")
        .filter_map(|resource| {
            let node = resource.child_by_field_name("name")?;
            (node_text(source, node) == name).then(|| Found::Declaration {
                name: node,
                initializer: resource.child_by_field_name("value"),
            })
        })
        .next_back()
}

/// A field, constant, record component, or enum constant named `name` that
/// the type body declares. Any other name may be inherited, so it has no
/// known type.
fn member<'tree>(source: &str, body: Node<'tree>, name: &str) -> Found<'tree> {
    let (body, enum_body) = match body.kind() {
        "enum_body_declarations" => (body, body.parent()),
        "enum_body" => {
            let mut cursor = body.walk();
            let declarations = body
                .named_children(&mut cursor)
                .find(|child| child.kind() == "enum_body_declarations");
            (declarations.unwrap_or(body), Some(body))
        }
        _ => (body, None),
    };
    if let Some(enum_body) = enum_body {
        let mut cursor = enum_body.walk();
        let constant = enum_body.named_children(&mut cursor).any(|child| {
            child.kind() == "enum_constant"
                && child
                    .child_by_field_name("name")
                    .is_some_and(|node| node_text(source, node) == name)
        });
        if constant {
            return enum_body
                .parent()
                .and_then(|declaration| declared_type_name(source, declaration))
                .map_or(Found::Unknown, |declared| Found::Type(declared.to_string()));
        }
    }
    let mut cursor = body.walk();
    let fields: Vec<Node> = body
        .named_children(&mut cursor)
        .filter(|child| matches!(child.kind(), "field_declaration" | "constant_declaration"))
        .collect();
    if let Some(found) = local_declaration_in_fields(source, fields, name) {
        return found;
    }
    let record_component = body
        .parent()
        .filter(|declaration| declaration.kind() == "record_declaration")
        .and_then(|declaration| declaration.child_by_field_name("parameters"))
        .and_then(|parameters| {
            parameter_names(parameters)
                .into_iter()
                .find(|node| node_text(source, *node) == name)
        });
    match record_component {
        Some(node) => Found::Declaration {
            name: node,
            initializer: None,
        },
        None => Found::Unknown,
    }
}

fn local_declaration_in_fields<'tree>(
    source: &str,
    fields: Vec<Node<'tree>>,
    name: &str,
) -> Option<Found<'tree>> {
    fields
        .into_iter()
        .flat_map(|field| {
            let mut cursor = field.walk();
            field
                .children_by_field_name("declarator", &mut cursor)
                .collect::<Vec<_>>()
        })
        .find_map(|declarator| {
            let node = declarator.child_by_field_name("name")?;
            (node_text(source, node) == name).then(|| Found::Declaration {
                name: node,
                initializer: declarator.child_by_field_name("value"),
            })
        })
}

/// The name of the type that a type declaration node declares.
fn declared_type_name<'a>(source: &'a str, node: Node) -> Option<&'a str> {
    matches!(
        node.kind(),
        "class_declaration"
            | "interface_declaration"
            | "enum_declaration"
            | "record_declaration"
            | "annotation_type_declaration"
    )
    .then(|| node.child_by_field_name("name"))
    .flatten()
    .map(|name| node_text(source, name))
}

/// The type `this` names: the nearest type declaration, or none inside an
/// anonymous class.
fn enclosing_type_name(source: &str, node: Node) -> Option<String> {
    let body = enclosing(node, &TYPE_BODIES)?;
    let declaration = match body.kind() {
        "enum_body_declarations" => body.parent()?.parent()?,
        _ => body.parent()?,
    };
    declared_type_name(source, declaration).map(str::to_string)
}
