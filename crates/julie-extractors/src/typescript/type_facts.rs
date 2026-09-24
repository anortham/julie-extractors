//! Declared-type fact recording for TypeScript.

use crate::base::BaseExtractor;
use crate::base::types::TypeNameRules;
use crate::javascript::type_facts::record_new_expression_fact;
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};
use std::collections::HashMap;
use tree_sitter::Node;

pub(super) const TYPE_NAME_RULES: TypeNameRules = TypeNameRules {
    nullable_suffixes: &[],
    reference_prefixes: &[],
    generic_open: &['<'],
};

/// Record facts for a `variable_declarator` that names a plain identifier:
/// the annotation when present, then a plain `new Foo()` initializer, then,
/// for an unannotated declarator, a same-file call initializer.
/// Destructuring declarators record nothing.
pub(super) fn record_variable_type_facts(
    base: &mut BaseExtractor,
    symbol_id: &str,
    declarator_node: Node,
    return_types: &ReturnTypeIndex,
) {
    let names_identifier = declarator_node
        .child_by_field_name("name")
        .is_some_and(|name| name.kind() == "identifier");
    if !names_identifier {
        return;
    }
    record_annotation_fact(base, symbol_id, declarator_node);
    let Some(value_node) = declarator_node.child_by_field_name("value") else {
        return;
    };
    record_new_expression_fact(base, symbol_id, value_node, &TYPE_NAME_RULES);
    if declarator_node.child_by_field_name("type").is_none() {
        record_call_initializer_fact(base, symbol_id, value_node, return_types);
    }
}

/// Record a declared fact from a node's `type` field when the annotation names
/// a single type plainly. Unions, intersections, object/mapped/conditional
/// types, function types, and literal types record nothing.
pub(super) fn record_annotation_fact(
    base: &mut BaseExtractor,
    symbol_id: &str,
    annotated_node: Node,
) {
    let Some(type_node) = annotation_type_node(annotated_node) else {
        return;
    };
    let Some(declared) = declared_type_text(base, type_node) else {
        return;
    };
    base.record_declared_type_fact(symbol_id, &declared, &TYPE_NAME_RULES, false);
}

/// Record the declared type of a binding destructured from an annotated
/// parameter: `({ page }: { page: Page })` gives `page` the type `Page`.
pub(super) fn record_destructured_binding_fact(
    base: &mut BaseExtractor,
    symbol_id: &str,
    binding_node: Node,
) {
    let (key, pattern) = match binding_node.parent() {
        Some(parent) if parent.kind() == "object_pattern" => {
            (base.get_node_text(&binding_node), parent)
        }
        Some(parent) if parent.kind() == "pair_pattern" => {
            let Some(key) = parent.child_by_field_name("key") else {
                return;
            };
            let Some(pattern) = parent.parent().filter(|p| p.kind() == "object_pattern") else {
                return;
            };
            (base.get_node_text(&key), pattern)
        }
        Some(parent) if parent.kind() == "object_assignment_pattern" => {
            let Some(pattern) = parent.parent().filter(|p| p.kind() == "object_pattern") else {
                return;
            };
            (base.get_node_text(&binding_node), pattern)
        }
        _ => return,
    };
    let Some(parameter) = pattern.parent().filter(|parameter| {
        matches!(
            parameter.kind(),
            "required_parameter" | "optional_parameter"
        )
    }) else {
        return;
    };
    let Some(object_type) = annotation_type_node(parameter).filter(|t| t.kind() == "object_type")
    else {
        return;
    };
    let mut cursor = object_type.walk();
    let member = object_type.named_children(&mut cursor).find(|member| {
        member.kind() == "property_signature"
            && member
                .child_by_field_name("name")
                .is_some_and(|name| base.get_node_text(&name) == key)
    });
    if let Some(member) = member {
        record_annotation_fact(base, symbol_id, member);
    }
}

/// Record a callable's declared return type from its `return_type`
/// annotation: `getUser(): User` records `User`, and `Promise<User[]>`
/// records `Promise` with the full text as `declared`.
pub(super) fn record_return_type_fact(
    base: &mut BaseExtractor,
    symbol_id: &str,
    callable_node: Node,
) {
    let Some(type_node) = field_type_node(callable_node, "return_type")
        .filter(|type_node| !matches!(base.get_node_text(type_node).as_str(), "void" | "never"))
    else {
        return;
    };
    let Some(declared) = declared_type_text(base, type_node) else {
        return;
    };
    base.record_declared_type_fact(symbol_id, &declared, &TYPE_NAME_RULES, false);
}

fn annotation_type_node(annotated_node: Node<'_>) -> Option<Node<'_>> {
    field_type_node(annotated_node, "type")
}

fn field_type_node<'t>(annotated_node: Node<'t>, field: &str) -> Option<Node<'t>> {
    let annotation = annotated_node.child_by_field_name(field)?;
    let mut cursor = annotation.walk();
    annotation.named_children(&mut cursor).last()
}

fn declared_type_text(base: &BaseExtractor, type_node: Node) -> Option<String> {
    if is_plain_named_type(type_node) {
        return Some(base.get_node_text(&type_node));
    }
    if type_node.kind() == "array_type" {
        let mut cursor = type_node.walk();
        let element = type_node.named_children(&mut cursor).next()?;
        if is_plain_named_type(element) {
            return Some(base.get_node_text(&type_node));
        }
    }
    None
}

fn is_plain_named_type(type_node: Node) -> bool {
    match type_node.kind() {
        "type_identifier" | "nested_type_identifier" | "generic_type" => true,
        "predefined_type" => !is_unique_symbol_operator(type_node),
        _ => false,
    }
}

fn is_unique_symbol_operator(predefined_type_node: Node) -> bool {
    predefined_type_node
        .child(0)
        .is_some_and(|child| child.kind() == "unique symbol")
}

/// Record the type a same-file call initializer produces (`is_inferred=true`):
/// `load()`, `this.load()`, or `Type.create()` with a declared return type.
/// `await` removes one `Promise`/`PromiseLike` layer and `!` removes `null`
/// and `undefined`. A nullable, generic, or unsupported result records nothing.
fn record_call_initializer_fact(
    base: &mut BaseExtractor,
    symbol_id: &str,
    value: Node,
    return_types: &ReturnTypeIndex,
) {
    let scope = InitializerScope { base, return_types };
    let Some(TypeShape {
        declared: Some(declared),
        nullable: false,
        ..
    }) = scope.shape_of(value, 0)
    else {
        return;
    };
    base.record_declared_type_fact(symbol_id, &declared, &TYPE_NAME_RULES, true);
}

/// A return type reduced to what initializer inference needs.
#[derive(Debug, Clone, PartialEq, Eq)]
struct TypeShape {
    /// The recordable type text; `None` for type parameters and unsupported types.
    declared: Option<String>,
    /// The base name of a generic type: `Promise` in `Promise<User>`.
    generic_name: Option<String>,
    args: Vec<TypeShape>,
    /// A union added `null` or `undefined` to the type.
    nullable: bool,
}

impl TypeShape {
    fn opaque() -> Self {
        Self {
            declared: None,
            generic_name: None,
            args: Vec::new(),
            nullable: false,
        }
    }

    fn is_promise(&self) -> bool {
        matches!(
            self.generic_name.as_deref(),
            Some("Promise" | "PromiseLike")
        ) && self.args.len() == 1
    }

    fn awaited(self) -> Option<TypeShape> {
        if !self.is_promise() {
            return None;
        }
        let nullable = self.nullable;
        let mut inner = self.args.into_iter().next()?;
        if inner.is_promise() {
            return None;
        }
        inner.nullable |= nullable;
        Some(inner)
    }

    fn non_null(self) -> TypeShape {
        TypeShape {
            nullable: false,
            ..self
        }
    }
}

/// Declared return types of the file's functions, function-valued consts,
/// and class methods, by name. Other value bindings of a name (parameters,
/// imports, plain variables) are entries with no shape, so a call through a
/// shadowing binding never agrees on a type.
#[derive(Debug, Default)]
pub(super) struct ReturnTypeIndex(HashMap<String, Vec<ReturnEntry>>);

#[derive(Debug)]
struct ReturnEntry {
    /// The class of a method; `None` for functions and other value bindings.
    owner: Option<String>,
    is_static: bool,
    /// `None` when the binding is not a function or declares no return type.
    shape: Option<TypeShape>,
}

impl ReturnTypeIndex {
    pub(super) fn build(base: &BaseExtractor, root: Node) -> Self {
        let mut index = Self::default();
        let mut stack = vec![root];
        while let Some(node) = stack.pop() {
            index.add_node(base, node);
            stack.extend(node.named_children(&mut node.walk()));
        }
        index
    }

    fn add_node(&mut self, base: &BaseExtractor, node: Node) {
        match node.kind() {
            "function_declaration" | "generator_function_declaration" | "function_signature" => {
                if let Some(name) = node.child_by_field_name("name") {
                    let shape = return_shape(base, node, None);
                    self.push(base.get_node_text(&name), None, false, shape);
                }
            }
            "variable_declarator" => {
                let Some(name) = node.child_by_field_name("name") else {
                    return;
                };
                if name.kind() != "identifier" {
                    self.shadow_pattern(base, name);
                    return;
                }
                let shape = callable_value(node)
                    .filter(|_| node.child_by_field_name("type").is_none())
                    .and_then(|callable| return_shape(base, callable, None));
                self.push(base.get_node_text(&name), None, false, shape);
            }
            "method_definition"
            | "method_signature"
            | "abstract_method_signature"
            | "public_field_definition" => self.add_class_member(base, node),
            "required_parameter" | "optional_parameter" => {
                self.shadow_field(base, node, "pattern");
            }
            "arrow_function" | "catch_clause" => self.shadow_field(base, node, "parameter"),
            "for_in_statement" => self.shadow_field(base, node, "left"),
            "import_specifier" => {
                let field = if node.child_by_field_name("alias").is_some() {
                    "alias"
                } else {
                    "name"
                };
                self.shadow_field(base, node, field);
            }
            "import_clause" | "namespace_import" | "import_require_clause" => {
                for child in node.named_children(&mut node.walk()) {
                    if child.kind() == "identifier" {
                        self.shadow_pattern(base, child);
                    }
                }
            }
            _ => {}
        }
    }

    fn add_class_member(&mut self, base: &BaseExtractor, member: Node) {
        let Some(owner) = member
            .parent()
            .filter(|parent| parent.kind() == "class_body")
            .and_then(|body| body.parent())
            .and_then(|class| class_name(base, class))
        else {
            return;
        };
        let Some(name) = member.child_by_field_name("name") else {
            return;
        };
        if has_child_kind(member, "get") || has_child_kind(member, "set") {
            return;
        }
        let callable = if member.kind() == "public_field_definition" {
            match callable_value(member).filter(|_| member.child_by_field_name("type").is_none()) {
                Some(callable) => callable,
                None => return,
            }
        } else {
            member
        };
        let shape = return_shape(base, callable, Some(&owner));
        self.push(
            base.get_node_text(&name),
            Some(owner),
            has_child_kind(member, "static"),
            shape,
        );
    }

    fn shadow_field(&mut self, base: &BaseExtractor, node: Node, field: &str) {
        if let Some(pattern) = node.child_by_field_name(field) {
            self.shadow_pattern(base, pattern);
        }
    }

    fn shadow_pattern(&mut self, base: &BaseExtractor, pattern: Node) {
        let mut stack = vec![pattern];
        while let Some(node) = stack.pop() {
            if matches!(
                node.kind(),
                "identifier" | "shorthand_property_identifier_pattern"
            ) {
                self.push(base.get_node_text(&node), None, false, None);
            }
            stack.extend(node.named_children(&mut node.walk()));
        }
    }

    fn push(
        &mut self,
        name: String,
        owner: Option<String>,
        is_static: bool,
        shape: Option<TypeShape>,
    ) {
        self.0.entry(name).or_default().push(ReturnEntry {
            owner,
            is_static,
            shape,
        });
    }

    /// The return type every same-named callable with this owner agrees on.
    fn lookup(&self, name: &str, owner: Option<&str>, is_static: bool) -> Option<TypeShape> {
        let mut shapes = self
            .0
            .get(name)?
            .iter()
            .filter(|entry| entry.owner.as_deref() == owner && entry.is_static == is_static)
            .map(|entry| entry.shape.as_ref());
        let first = shapes.next()??;
        shapes
            .all(|shape| shape == Some(first))
            .then(|| first.clone())
    }

    fn binds_value(&self, name: &str) -> bool {
        self.0
            .get(name)
            .is_some_and(|entries| entries.iter().any(|entry| entry.owner.is_none()))
    }
}

fn has_child_kind(node: Node, kind: &str) -> bool {
    node.children(&mut node.walk())
        .any(|child| child.kind() == kind)
}

/// The function a declarator or class field binds, through parentheses only:
/// `as` would replace the function's type with the asserted one.
fn callable_value(node: Node) -> Option<Node> {
    let mut value = node.child_by_field_name("value")?;
    while value.kind() == "parenthesized_expression" && value.named_child_count() == 1 {
        value = value.named_child(0)?;
    }
    matches!(
        value.kind(),
        "arrow_function" | "function_expression" | "generator_function"
    )
    .then_some(value)
}

fn class_name(base: &BaseExtractor, class: Node) -> Option<String> {
    if !matches!(
        class.kind(),
        "class_declaration" | "abstract_class_declaration" | "class"
    ) {
        return None;
    }
    let name = class.child_by_field_name("name").or_else(|| {
        class
            .parent()
            .filter(|parent| parent.kind() == "variable_declarator")
            .and_then(|declarator| declarator.child_by_field_name("name"))
            .filter(|name| name.kind() == "identifier")
    })?;
    Some(base.get_node_text(&name))
}

fn return_shape(base: &BaseExtractor, callable: Node, owner: Option<&str>) -> Option<TypeShape> {
    let annotation = callable
        .child_by_field_name("return_type")
        .filter(|annotation| annotation.kind() == "type_annotation")?;
    let type_node = annotation.named_children(&mut annotation.walk()).last()?;
    let generics = enclosing_type_parameters(base, callable);
    Some(type_shape(base, type_node, &generics, owner, 0))
}

/// Type parameter names in scope at `node`: its own and every enclosing
/// declaration's.
fn enclosing_type_parameters(base: &BaseExtractor, node: Node) -> Vec<String> {
    let mut names = Vec::new();
    let mut current = Some(node);
    while let Some(scope) = current {
        if let Some(parameters) = scope.child_by_field_name("type_parameters") {
            names.extend(
                parameters
                    .named_children(&mut parameters.walk())
                    .filter(|parameter| parameter.kind() == "type_parameter")
                    .filter_map(|parameter| parameter.child_by_field_name("name"))
                    .map(|name| base.get_node_text(&name)),
            );
        }
        current = scope.parent();
    }
    names
}

fn type_shape(
    base: &BaseExtractor,
    node: Node,
    generics: &[String],
    this_type: Option<&str>,
    depth: u32,
) -> TypeShape {
    if !should_visit_tree_depth(depth) {
        return TypeShape::opaque();
    }
    let Some(child_depth) = child_tree_depth(depth) else {
        return TypeShape::opaque();
    };
    let text = base.get_node_text(&node);
    match node.kind() {
        "parenthesized_type" => node.named_child(0).map_or_else(TypeShape::opaque, |inner| {
            type_shape(base, inner, generics, this_type, child_depth)
        }),
        "union_type" => nullable_union_shape(base, node, generics, this_type, child_depth),
        "this_type" => this_type.map_or_else(TypeShape::opaque, |owner| TypeShape {
            declared: Some(owner.to_string()),
            ..TypeShape::opaque()
        }),
        "type_identifier" if generics.contains(&text) => TypeShape::opaque(),
        "generic_type" => TypeShape {
            declared: Some(text),
            generic_name: node
                .child_by_field_name("name")
                .map(|name| base.get_node_text(&name)),
            args: node
                .child_by_field_name("type_arguments")
                .map(|arguments| {
                    arguments
                        .named_children(&mut arguments.walk())
                        .map(|argument| {
                            type_shape(base, argument, generics, this_type, child_depth)
                        })
                        .collect()
                })
                .unwrap_or_default(),
            nullable: false,
        },
        "array_type" => {
            let element = node
                .named_child(0)
                .map_or_else(TypeShape::opaque, |element| {
                    type_shape(base, element, generics, this_type, child_depth)
                });
            TypeShape {
                declared: (element.declared.is_some() && !element.nullable).then_some(text),
                ..TypeShape::opaque()
            }
        }
        _ if is_plain_named_type(node) && !matches!(text.as_str(), "void" | "never") => TypeShape {
            declared: Some(text),
            ..TypeShape::opaque()
        },
        _ => TypeShape::opaque(),
    }
}

/// `T | null`, `T | undefined`, and `null | T | undefined` are `T` marked
/// nullable. Any other union is opaque.
fn nullable_union_shape(
    base: &BaseExtractor,
    union: Node,
    generics: &[String],
    this_type: Option<&str>,
    depth: u32,
) -> TypeShape {
    let mut members = Vec::new();
    let mut stack = vec![union];
    while let Some(node) = stack.pop() {
        for member in node.named_children(&mut node.walk()) {
            if member.kind() == "union_type" {
                stack.push(member);
            } else {
                members.push(member);
            }
        }
    }
    let (nulls, rest): (Vec<Node>, Vec<Node>) = members
        .into_iter()
        .partition(|member| matches!(base.get_node_text(member).as_str(), "null" | "undefined"));
    match rest.as_slice() {
        [single] if !nulls.is_empty() => TypeShape {
            nullable: true,
            ..type_shape(base, *single, generics, this_type, depth)
        },
        _ => TypeShape::opaque(),
    }
}

struct InitializerScope<'a> {
    base: &'a BaseExtractor,
    return_types: &'a ReturnTypeIndex,
}

impl InitializerScope<'_> {
    fn shape_of(&self, value: Node, depth: u32) -> Option<TypeShape> {
        if !should_visit_tree_depth(depth) {
            return None;
        }
        match value.kind() {
            "parenthesized_expression" if value.named_child_count() == 1 => {
                self.shape_of(value.named_child(0)?, child_tree_depth(depth)?)
            }
            "await_expression" => self
                .shape_of(value.named_child(0)?, child_tree_depth(depth)?)?
                .awaited(),
            "non_null_expression" => self
                .shape_of(value.named_child(0)?, child_tree_depth(depth)?)
                .map(TypeShape::non_null),
            "call_expression" => self.call_shape(value),
            _ => None,
        }
    }

    fn call_shape(&self, call: Node) -> Option<TypeShape> {
        let function = call.child_by_field_name("function")?;
        if has_child_kind(call, "?.") || has_child_kind(function, "optional_chain") {
            return None;
        }
        match function.kind() {
            "identifier" => {
                self.return_types
                    .lookup(&self.base.get_node_text(&function), None, false)
            }
            "member_expression" => {
                let object = function.child_by_field_name("object")?;
                let method = self
                    .base
                    .get_node_text(&function.child_by_field_name("property")?);
                match object.kind() {
                    "this" => {
                        let (owner, is_static) = this_class(self.base, object)?;
                        self.return_types.lookup(&method, Some(&owner), is_static)
                    }
                    "identifier" => {
                        let class = self.base.get_node_text(&object);
                        if self.return_types.binds_value(&class) {
                            return None;
                        }
                        self.return_types.lookup(&method, Some(&class), true)
                    }
                    _ => None,
                }
            }
            _ => None,
        }
    }
}

/// The class `this` names at `node`, and whether that is the class itself
/// (a static context). A non-arrow function or an object literal rebinds
/// `this`, so it yields `None`.
fn this_class(base: &BaseExtractor, node: Node) -> Option<(String, bool)> {
    let mut is_static = false;
    let mut current = node.parent();
    while let Some(scope) = current {
        match scope.kind() {
            "function_declaration"
            | "function_expression"
            | "generator_function"
            | "generator_function_declaration"
            | "object" => return None,
            "method_definition" | "public_field_definition" => {
                is_static = has_child_kind(scope, "static");
            }
            "class_static_block" => is_static = true,
            "class_body" => return Some((class_name(base, scope.parent()?)?, is_static)),
            _ => {}
        }
        current = scope.parent();
    }
    None
}
