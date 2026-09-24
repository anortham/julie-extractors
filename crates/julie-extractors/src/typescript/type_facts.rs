//! Declared-type fact recording for TypeScript.

use crate::base::BaseExtractor;
use crate::base::types::TypeNameRules;
use crate::javascript::type_facts::record_new_expression_fact;
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};
use std::collections::HashMap;
use std::ops::Range;
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
/// and class methods. Every value binding of a name is an entry with the
/// byte range where it is visible; bindings that are not typed callables
/// (parameters, imports, namespaces, plain variables) have no shape, so a call
/// that can reach one never agrees on a type.
#[derive(Debug, Default)]
pub(super) struct ReturnTypeIndex {
    values: HashMap<String, Vec<ValueEntry>>,
    /// Class members by the class node's start byte and the member name.
    members: HashMap<(usize, String), Vec<MemberEntry>>,
    /// Every namespace or module block by its qualified name path.
    namespaces: Vec<(Vec<String>, Range<usize>)>,
}

#[derive(Debug)]
struct ValueEntry {
    /// The byte ranges where the binding is visible. A namespace export has
    /// one range for each block of its namespace and of nested namespaces.
    scopes: Vec<Range<usize>>,
    /// The class node's start byte when the binding names a class.
    class: Option<usize>,
    /// `None` when the binding is not a function or declares no return type.
    shape: Option<TypeShape>,
}

#[derive(Debug)]
struct MemberEntry {
    is_static: bool,
    shape: Option<TypeShape>,
}

impl ReturnTypeIndex {
    pub(super) fn build(base: &BaseExtractor, root: Node) -> Self {
        let mut index = Self::default();
        let mut stack = vec![root];
        while let Some(node) = stack.pop() {
            if matches!(node.kind(), "internal_module" | "module") {
                index
                    .namespaces
                    .push((namespace_path(base, node), node.byte_range()));
            }
            stack.extend(node.named_children(&mut node.walk()));
        }
        stack.push(root);
        while let Some(node) = stack.pop() {
            index.add_node(base, node);
            stack.extend(node.named_children(&mut node.walk()));
        }
        index
    }

    fn add_node(&mut self, base: &BaseExtractor, node: Node) {
        match node.kind() {
            "function_declaration" | "generator_function_declaration" | "function_signature" => {
                let shape = return_shape(base, node, None);
                self.add_name(base, node, block_scope(node), None, shape);
            }
            "function_expression" | "generator_function" => {
                let shape = return_shape(base, node, None);
                self.add_name(base, node, node.byte_range(), None, shape);
            }
            "class_declaration" | "abstract_class_declaration" => {
                self.add_name(base, node, block_scope(node), Some(node.start_byte()), None);
            }
            "class" => {
                self.add_name(base, node, node.byte_range(), Some(node.start_byte()), None);
            }
            "variable_declarator" => self.add_declarator(base, node),
            "method_definition"
            | "method_signature"
            | "abstract_method_signature"
            | "public_field_definition" => self.add_class_member(base, node),
            "required_parameter" | "optional_parameter" => {
                let owner = node
                    .parent()
                    .filter(|parent| parent.kind() == "formal_parameters")
                    .and_then(|parameters| parameters.parent())
                    .unwrap_or(node);
                self.shadow_field(base, node, "pattern", owner.byte_range());
            }
            "arrow_function" | "catch_clause" => {
                self.shadow_field(base, node, "parameter", node.byte_range());
            }
            "for_in_statement" => {
                let scope = match node.child_by_field_name("kind").map(|kind| kind.kind()) {
                    Some("let" | "const") => node.byte_range(),
                    _ => function_scope(node),
                };
                self.shadow_field(base, node, "left", scope);
            }
            "import_specifier" => {
                let field = if node.child_by_field_name("alias").is_some() {
                    "alias"
                } else {
                    "name"
                };
                self.shadow_field(base, node, field, block_scope(node));
            }
            "import_clause" | "namespace_import" | "import_require_clause" | "import_alias" => {
                if let Some(name) = node
                    .named_children(&mut node.walk())
                    .find(|child| child.kind() == "identifier")
                {
                    self.push_value(base, name, block_scope(node), None, None);
                }
            }
            "internal_module" | "module" => {
                let mut name = node.child_by_field_name("name");
                while let Some(nested) = name.filter(|n| n.kind() == "nested_identifier") {
                    name = nested.named_child(0);
                }
                if let Some(name) = name.filter(|name| name.kind() == "identifier") {
                    self.push_value(base, name, block_scope(node), None, None);
                }
            }
            "enum_declaration" => self.add_name(base, node, block_scope(node), None, None),
            _ => {}
        }
    }

    fn add_declarator(&mut self, base: &BaseExtractor, declarator: Node) {
        let Some(name) = declarator.child_by_field_name("name") else {
            return;
        };
        let scope = if declarator
            .parent()
            .is_some_and(|parent| parent.kind() == "variable_declaration")
        {
            function_scope(declarator)
        } else {
            block_scope(declarator)
        };
        if name.kind() != "identifier" {
            self.shadow_pattern(base, name, scope);
            return;
        }
        let class = declarator
            .child_by_field_name("value")
            .filter(|value| value.kind() == "class")
            .map(|class| class.start_byte());
        let shape = callable_value(declarator)
            .filter(|_| declarator.child_by_field_name("type").is_none())
            .and_then(|callable| return_shape(base, callable, None));
        self.push_value(base, name, scope, class, shape);
    }

    fn add_class_member(&mut self, base: &BaseExtractor, member: Node) {
        let Some(class) = member
            .parent()
            .filter(|parent| parent.kind() == "class_body")
            .and_then(|body| body.parent())
        else {
            return;
        };
        let Some(name) = member.child_by_field_name("name") else {
            return;
        };
        let callable = if has_child_kind(member, "get") || has_child_kind(member, "set") {
            None
        } else if member.kind() == "public_field_definition" {
            callable_value(member).filter(|_| member.child_by_field_name("type").is_none())
        } else {
            Some(member)
        };
        let owner = class_name(base, class);
        let shape = callable.and_then(|callable| return_shape(base, callable, owner.as_deref()));
        self.members
            .entry((class.start_byte(), base.get_node_text(&name)))
            .or_default()
            .push(MemberEntry {
                is_static: has_child_kind(member, "static"),
                shape,
            });
    }

    fn add_name(
        &mut self,
        base: &BaseExtractor,
        node: Node,
        scope: Range<usize>,
        class: Option<usize>,
        shape: Option<TypeShape>,
    ) {
        if let Some(name) = node.child_by_field_name("name") {
            self.push_value(base, name, scope, class, shape);
        }
    }

    fn shadow_field(&mut self, base: &BaseExtractor, node: Node, field: &str, scope: Range<usize>) {
        if let Some(pattern) = node.child_by_field_name(field) {
            self.shadow_pattern(base, pattern, scope);
        }
    }

    fn shadow_pattern(&mut self, base: &BaseExtractor, pattern: Node, scope: Range<usize>) {
        let mut stack = vec![pattern];
        while let Some(node) = stack.pop() {
            if matches!(
                node.kind(),
                "identifier" | "shorthand_property_identifier_pattern"
            ) {
                self.push_value(base, node, scope.clone(), None, None);
            }
            stack.extend(node.named_children(&mut node.walk()));
        }
    }

    fn push_value(
        &mut self,
        base: &BaseExtractor,
        name: Node,
        scope: Range<usize>,
        class: Option<usize>,
        shape: Option<TypeShape>,
    ) {
        let scopes = match exporting_namespace(name) {
            Some(namespace) => {
                let path = namespace_path(base, namespace);
                self.namespaces
                    .iter()
                    .filter(|(block_path, _)| block_path.starts_with(&path))
                    .map(|(_, range)| range.clone())
                    .collect()
            }
            None => vec![scope],
        };
        self.values
            .entry(base.get_node_text(&name))
            .or_default()
            .push(ValueEntry {
                scopes,
                class,
                shape,
            });
    }

    fn visible_values<'a>(
        &'a self,
        name: &str,
        at: usize,
    ) -> impl Iterator<Item = &'a ValueEntry> + 'a {
        self.values
            .get(name)
            .into_iter()
            .flatten()
            .filter(move |entry| entry.scopes.iter().any(|scope| scope.contains(&at)))
    }

    /// The return type every binding of `name` visible at `at` agrees on.
    fn function_shape(&self, name: &str, at: usize) -> Option<TypeShape> {
        agreed(
            self.visible_values(name, at)
                .map(|entry| entry.shape.as_ref()),
        )
    }

    /// The class `name` names at `at`, when it is the only binding visible
    /// there: a namespace, enum, or second class of that name blocks it.
    fn class_at(&self, name: &str, at: usize) -> Option<usize> {
        let mut visible = self.visible_values(name, at);
        let only = visible.next()?;
        visible.next().is_none().then_some(only.class)?
    }

    /// The return type every same-named member of one class agrees on.
    fn member_shape(&self, class: usize, name: &str, is_static: bool) -> Option<TypeShape> {
        agreed(
            self.members
                .get(&(class, name.to_string()))?
                .iter()
                .filter(|entry| entry.is_static == is_static)
                .map(|entry| entry.shape.as_ref()),
        )
    }
}

fn agreed<'a>(mut shapes: impl Iterator<Item = Option<&'a TypeShape>>) -> Option<TypeShape> {
    let first = shapes.next()??;
    shapes
        .all(|shape| shape == Some(first))
        .then(|| first.clone())
}

/// Where a `let`, `const`, class, function, or import binding is visible: the
/// nearest enclosing block.
fn block_scope(node: Node) -> Range<usize> {
    enclosing_scope(
        node,
        &[
            "program",
            "statement_block",
            "switch_body",
            "for_statement",
            "for_in_statement",
        ],
    )
}

/// Where a `var` binding is visible: the nearest enclosing function,
/// namespace, static block, or the file.
fn function_scope(node: Node) -> Range<usize> {
    enclosing_scope(
        node,
        &[
            "program",
            "function_declaration",
            "generator_function_declaration",
            "function_expression",
            "generator_function",
            "arrow_function",
            "method_definition",
            "class_static_block",
            "internal_module",
            "module",
        ],
    )
}

fn enclosing_scope(node: Node, kinds: &[&str]) -> Range<usize> {
    let mut current = node.parent();
    while let Some(scope) = current {
        if kinds.contains(&scope.kind()) {
            return scope.byte_range();
        }
        current = scope.parent();
    }
    node.byte_range()
}

/// The namespace or module whose members include the binding `name`
/// declares: an `export` in a namespace body, or any declaration in an
/// ambient (`declare`) namespace, where members are exported implicitly.
fn exporting_namespace(name: Node) -> Option<Node> {
    let mut declaration = name;
    while let Some(parent) = declaration.parent() {
        if !matches!(
            parent.kind(),
            "nested_identifier"
                | "object_pattern"
                | "array_pattern"
                | "pair_pattern"
                | "object_assignment_pattern"
                | "assignment_pattern"
                | "rest_pattern"
                | "variable_declarator"
                | "lexical_declaration"
                | "variable_declaration"
                | "function_declaration"
                | "generator_function_declaration"
                | "function_signature"
                | "class_declaration"
                | "abstract_class_declaration"
                | "enum_declaration"
                | "internal_module"
                | "module"
                | "import_alias"
                | "ambient_declaration"
        ) {
            break;
        }
        declaration = parent;
    }
    let exported = declaration
        .parent()
        .filter(|parent| parent.kind() == "export_statement");
    let body = exported
        .unwrap_or(declaration)
        .parent()
        .filter(|parent| parent.kind() == "statement_block")?;
    let namespace = body
        .parent()
        .filter(|parent| matches!(parent.kind(), "internal_module" | "module"))?;
    let ambient = std::iter::successors(namespace.parent(), Node::parent)
        .any(|ancestor| ancestor.kind() == "ambient_declaration");
    (exported.is_some() || ambient).then_some(namespace)
}

/// The qualified name of a namespace block: `namespace A.B` nested in
/// `namespace X` is `["X", "A", "B"]`.
fn namespace_path(base: &BaseExtractor, namespace: Node) -> Vec<String> {
    let mut path = Vec::new();
    let mut current = Some(namespace);
    while let Some(node) = current {
        if matches!(node.kind(), "internal_module" | "module")
            && let Some(name) = node.child_by_field_name("name")
        {
            let text = base.get_node_text(&name);
            let segments = text.split('.').map(|segment| segment.trim().to_string());
            path.splice(0..0, segments);
        }
        current = node.parent();
    }
    path
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
                declared: element
                    .declared
                    .filter(|_| !element.nullable)
                    .map(|element| format!("{element}[]")),
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
            "satisfies_expression" => {
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
        let at = call.start_byte();
        match function.kind() {
            "identifier" => self
                .return_types
                .function_shape(&self.base.get_node_text(&function), at),
            "member_expression" => {
                let object = function.child_by_field_name("object")?;
                let method = self
                    .base
                    .get_node_text(&function.child_by_field_name("property")?);
                match object.kind() {
                    "this" => {
                        let (class, is_static) = this_class(object)?;
                        self.return_types.member_shape(class, &method, is_static)
                    }
                    "identifier" => {
                        let class = self
                            .return_types
                            .class_at(&self.base.get_node_text(&object), at)?;
                        self.return_types.member_shape(class, &method, true)
                    }
                    _ => None,
                }
            }
            _ => None,
        }
    }
}

/// The start byte of the class `this` names at `node`, and whether `this` is
/// the class itself (a static context). A non-arrow function or an object
/// literal rebinds `this`, so it yields `None`.
fn this_class(node: Node) -> Option<(usize, bool)> {
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
            "class_body" => return Some((scope.parent()?.start_byte(), is_static)),
            _ => {}
        }
        current = scope.parent();
    }
    None
}
