//! Declared-type fact recording for Swift.

use super::signatures::return_type_node;
use crate::base::BaseExtractor;
use crate::base::types::{TypeNameRules, strip_type_decorations};
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};
use std::collections::{HashMap, HashSet};
use tree_sitter::Node;

pub(super) const SWIFT_TYPE_NAME_RULES: TypeNameRules = TypeNameRules {
    nullable_suffixes: &["?", "!"],
    reference_prefixes: &["inout"],
    generic_open: &['<'],
};

pub(super) fn collect_type_names(base: &BaseExtractor, root: Node) -> HashSet<String> {
    let mut names = HashSet::new();
    collect_type_names_into(base, root, 0, &mut names);
    names
}

fn collect_type_names_into(
    base: &BaseExtractor,
    node: Node,
    depth: u32,
    names: &mut HashSet<String>,
) {
    if !should_visit_tree_depth(depth) {
        return;
    }
    if node.kind() == "class_declaration"
        && let Some(name_node) = node.child_by_field_name("name")
    {
        insert_type_name(base, name_node, names);
    }
    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_type_names_into(base, child, child_depth, names);
    }
}

fn insert_type_name(base: &BaseExtractor, name_node: Node, names: &mut HashSet<String>) {
    let text = base.get_node_text(&name_node);
    let resolved = strip_type_decorations(&text, &SWIFT_TYPE_NAME_RULES);
    if !resolved.is_empty() {
        names.insert(resolved);
    }
}

/// Record a syntactically stated type for a symbol (`is_inferred=false`).
pub(super) fn record_declared_type(base: &mut BaseExtractor, symbol_id: &str, type_node: Node) {
    record_type_node(base, symbol_id, type_node, false);
}

pub(super) fn record_declared_type_text(
    base: &mut BaseExtractor,
    symbol_id: &str,
    type_node: Node,
    declared_text: &str,
) {
    let Some(base_name) = base_type_name(base, type_node) else {
        return;
    };
    base.record_declared_type_fact_with_declared(
        symbol_id,
        &base_name,
        declared_text,
        &SWIFT_TYPE_NAME_RULES,
        false,
    );
}

/// Record the type an untyped binding's initializer produces
/// (`is_inferred=true`): `Foo(...)` when `Foo` names a same-file type, or a
/// call to a same-file function or method with a declared return type.
/// `try`, `try!`, and `await` pass the type through, `try?` makes it optional,
/// and a postfix `!` removes one optional layer. Anything else records nothing.
pub(super) fn record_initializer_type(
    base: &mut BaseExtractor,
    symbol_id: &str,
    value: Node,
    same_file_type_names: &HashSet<String>,
    return_types: &ReturnTypeIndex,
) {
    let scope = InitializerScope {
        base,
        same_file_type_names,
        return_types,
    };
    let Some(TypeShape { name, declared }) = scope.shape_of(value, 0) else {
        return;
    };
    base.record_declared_type_fact_with_declared(
        symbol_id,
        &name,
        &declared,
        &SWIFT_TYPE_NAME_RULES,
        true,
    );
}

/// A return type reduced to its bindable base name and its written text.
#[derive(Debug, Clone, PartialEq, Eq)]
struct TypeShape {
    name: String,
    declared: String,
}

impl TypeShape {
    fn optional(self) -> Self {
        let wrapped = self
            .declared
            .strip_suffix(['?', '!'])
            .unwrap_or(&self.declared);
        Self {
            declared: format!("{wrapped}?"),
            ..self
        }
    }

    fn forced(self) -> Self {
        let declared = self
            .declared
            .strip_suffix(['?', '!'])
            .map(str::to_string)
            .unwrap_or(self.declared);
        Self { declared, ..self }
    }
}

/// Where a declaration or call sits: at file scope, in a same-file type or
/// an extension of one, or in a type context whose members are unknown
/// (a protocol, or an extension of a type declared elsewhere).
#[derive(Debug, Clone, PartialEq, Eq)]
enum TypeContext {
    File,
    Type(String),
    Unknown,
}

#[derive(Debug)]
enum Owner {
    Free,
    /// A function nested in a body; visible only inside this byte range.
    Local(std::ops::Range<usize>),
    Type(String),
}

#[derive(Debug)]
struct ReturnEntry {
    owner: Owner,
    is_static: bool,
    /// `None` when the function declares no usable return type.
    shape: Option<TypeShape>,
}

/// Same-file facts that call-initializer inference needs, built once per file.
/// Protocol members and members of extensions of types declared elsewhere
/// are left out: their `Self`, associated types, and generics are unknown.
#[derive(Debug, Default)]
pub(super) struct ReturnTypeIndex {
    functions: HashMap<String, Vec<ReturnEntry>>,
    /// Names bound as a variable, property, parameter, or enum case anywhere.
    value_names: HashSet<String>,
    /// `(type, name)` for properties and enum cases a same-file type declares.
    member_values: HashSet<(String, String)>,
    /// Class, struct, enum, and actor names declared in the file.
    declared_types: HashSet<String>,
    /// Same-file types with an inheritance clause on a declaration or extension.
    inheriting_types: HashSet<String>,
}

impl ReturnTypeIndex {
    pub(super) fn build(base: &BaseExtractor, root: Node) -> Self {
        let mut index = Self::default();
        let mut type_generics: HashMap<String, Vec<String>> = HashMap::new();
        let mut functions = Vec::new();
        let mut members = Vec::new();
        let mut stack = vec![root];
        while let Some(node) = stack.pop() {
            match node.kind() {
                "class_declaration" if !is_extension(base, node) => {
                    if let Some(name) = node.child_by_field_name("name") {
                        let name = base.get_node_text(&name);
                        type_generics
                            .entry(name.clone())
                            .or_default()
                            .extend(type_parameter_names(base, node));
                        index.declared_types.insert(name);
                    }
                }
                "function_declaration" => functions.push(node),
                "property_declaration" | "enum_entry" => members.push(node),
                "pattern" => {
                    index.value_names.extend(
                        node.child_by_field_name("bound_identifier")
                            .into_iter()
                            .chain(named_identifiers(node))
                            .map(|name| base.get_node_text(&name)),
                    );
                }
                "parameter" | "lambda_parameter" => {
                    index
                        .value_names
                        .extend(named_identifiers(node).map(|name| base.get_node_text(&name)));
                }
                _ => {}
            }
            stack.extend(node.named_children(&mut node.walk()));
        }
        index.inheriting_types = inheriting_types(base, root, &index.declared_types);
        for member in members {
            let TypeContext::Type(owner) = index.member_context(base, member) else {
                continue;
            };
            let names: Vec<String> = if member.kind() == "enum_entry" {
                let mut cursor = member.walk();
                member
                    .children_by_field_name("name", &mut cursor)
                    .map(|name| base.get_node_text(&name))
                    .collect()
            } else {
                let mut cursor = member.walk();
                member
                    .children_by_field_name("name", &mut cursor)
                    .flat_map(|pattern| {
                        pattern
                            .child_by_field_name("bound_identifier")
                            .into_iter()
                            .chain(named_identifiers(pattern))
                    })
                    .map(|name| base.get_node_text(&name))
                    .collect()
            };
            index.value_names.extend(names.iter().cloned());
            index
                .member_values
                .extend(names.into_iter().map(|name| (owner.clone(), name)));
        }
        for function in functions {
            if let Some((name, entry)) = index.return_entry(base, function, &type_generics) {
                index.functions.entry(name).or_default().push(entry);
            }
        }
        index
    }

    fn return_entry(
        &self,
        base: &BaseExtractor,
        function: Node,
        type_generics: &HashMap<String, Vec<String>>,
    ) -> Option<(String, ReturnEntry)> {
        let name = base.get_node_text(&function.child_by_field_name("name")?);
        let parent = function.parent()?;
        let owner = match parent.kind() {
            "source_file" => Owner::Free,
            "class_body" | "enum_class_body" => match self.member_context(base, function) {
                TypeContext::Type(owner) => Owner::Type(owner),
                _ => return None,
            },
            _ => Owner::Local(parent.byte_range()),
        };
        let mut generics = Vec::new();
        let mut current = Some(function);
        while let Some(node) = current {
            match node.kind() {
                "function_declaration" | "init_declaration" | "subscript_declaration" => {
                    generics.extend(type_parameter_names(base, node))
                }
                "class_declaration" => match self.type_context(base, node) {
                    TypeContext::Type(owner) => {
                        generics.extend(type_generics.get(&owner).into_iter().flatten().cloned())
                    }
                    _ => return None,
                },
                _ => {}
            }
            current = node.parent();
        }
        let self_type = match &owner {
            Owner::Type(owner) => Some(owner.as_str()),
            _ => None,
        };
        let shape = return_type_node(function).and_then(|type_node| {
            let name = base_type_name(base, type_node)?;
            let head = name.split('.').next().unwrap_or(&name);
            let name = match head {
                "Self" if name == "Self" => self_type?.to_string(),
                _ if generics.iter().any(|generic| generic == head) || head == "Self" => {
                    return None;
                }
                _ => name,
            };
            Some(TypeShape {
                name,
                declared: base.get_node_text(&type_node),
            })
        });
        let entry = ReturnEntry {
            owner,
            is_static: is_static(base, function),
            shape,
        };
        Some((name, entry))
    }

    /// The type context of a declaration held directly in a type body.
    fn member_context(&self, base: &BaseExtractor, member: Node) -> TypeContext {
        match member.parent().and_then(|body| body.parent()) {
            Some(owner) if owner.kind() == "class_declaration" => self.type_context(base, owner),
            _ => TypeContext::Unknown,
        }
    }

    fn type_context(&self, base: &BaseExtractor, declaration: Node) -> TypeContext {
        type_declaration_name(base, declaration)
            .filter(|name| self.declared_types.contains(name))
            .map_or(TypeContext::Unknown, TypeContext::Type)
    }

    /// The type context enclosing `node`, from the nearest type declaration.
    fn enclosing_context(&self, base: &BaseExtractor, node: Node) -> TypeContext {
        let mut current = node.parent();
        while let Some(parent) = current {
            match parent.kind() {
                "class_declaration" => return self.type_context(base, parent),
                _ => current = parent.parent(),
            }
        }
        TypeContext::File
    }

    /// The return type all candidates agree on, if there is at least one.
    fn unanimous<'a>(entries: impl IntoIterator<Item = &'a ReturnEntry>) -> Option<TypeShape> {
        let mut shapes = entries.into_iter().map(|entry| entry.shape.as_ref());
        let first = shapes.next()??;
        shapes
            .all(|shape| shape == Some(first))
            .then(|| first.clone())
    }

    fn member_call(&self, owner: &str, name: &str, static_only: bool) -> Option<TypeShape> {
        if self
            .member_values
            .contains(&(owner.to_string(), name.to_string()))
        {
            return None;
        }
        let members: Vec<&ReturnEntry> = self
            .functions
            .get(name)?
            .iter()
            .filter(|entry| matches!(&entry.owner, Owner::Type(o) if o == owner))
            .collect();
        if static_only && members.iter().any(|entry| !entry.is_static) {
            return None;
        }
        Self::unanimous(members)
    }

    /// An unqualified call: an in-scope local function shadows a member of
    /// the enclosing type, which shadows a free function. A type that
    /// inherits may hold unseen members, so it never falls back to free
    /// functions.
    fn unqualified_call(&self, name: &str, call: Node, context: &TypeContext) -> Option<TypeShape> {
        if self.value_names.contains(name) {
            return None;
        }
        let entries = self.functions.get(name)?;
        let locals: Vec<&ReturnEntry> = entries
            .iter()
            .filter(|entry| {
                matches!(&entry.owner, Owner::Local(scope) if scope.contains(&call.start_byte()))
            })
            .collect();
        if !locals.is_empty() {
            return Self::unanimous(locals);
        }
        let free = || entries.iter().filter(|e| matches!(e.owner, Owner::Free));
        match context {
            TypeContext::File => Self::unanimous(free()),
            TypeContext::Type(owner) => {
                let has_member = entries
                    .iter()
                    .any(|e| matches!(&e.owner, Owner::Type(o) if o == owner));
                if has_member {
                    self.member_call(owner, name, false)
                } else if self.inheriting_types.contains(owner) {
                    None
                } else {
                    Self::unanimous(free())
                }
            }
            TypeContext::Unknown => None,
        }
    }
}

struct InitializerScope<'a> {
    base: &'a BaseExtractor,
    same_file_type_names: &'a HashSet<String>,
    return_types: &'a ReturnTypeIndex,
}

impl InitializerScope<'_> {
    fn shape_of(&self, value: Node, depth: u32) -> Option<TypeShape> {
        if !should_visit_tree_depth(depth) {
            return None;
        }
        match value.kind() {
            "try_expression" => {
                let inner =
                    self.shape_of(value.child_by_field_name("expr")?, child_tree_depth(depth)?)?;
                let operator = value
                    .named_children(&mut value.walk())
                    .find(|child| child.kind() == "try_operator")?;
                if self.base.get_node_text(&operator).ends_with('?') {
                    Some(inner.optional())
                } else {
                    Some(inner)
                }
            }
            "await_expression" => {
                self.shape_of(value.child_by_field_name("expr")?, child_tree_depth(depth)?)
            }
            "postfix_expression" => {
                let operation = value.child_by_field_name("operation")?;
                if operation.kind() != "bang" {
                    return None;
                }
                Some(
                    self.shape_of(
                        value.child_by_field_name("target")?,
                        child_tree_depth(depth)?,
                    )?
                    .forced(),
                )
            }
            "call_expression" => self.call_shape(value),
            _ => None,
        }
    }

    fn call_shape(&self, call: Node) -> Option<TypeShape> {
        let callee = call.named_child(0)?;
        let index = self.return_types;
        match callee.kind() {
            "simple_identifier" => {
                let name = self.base.get_node_text(&callee);
                if self.same_file_type_names.contains(&name) {
                    return Some(TypeShape {
                        declared: name.clone(),
                        name,
                    });
                }
                let context = index.enclosing_context(self.base, call);
                index.unqualified_call(&name, call, &context)
            }
            "navigation_expression" => {
                let method = callee
                    .child_by_field_name("suffix")?
                    .child_by_field_name("suffix")?;
                if method.kind() != "simple_identifier" {
                    return None;
                }
                let method = self.base.get_node_text(&method);
                let target = callee.child_by_field_name("target")?;
                let context = || match index.enclosing_context(self.base, call) {
                    TypeContext::Type(owner) => Some(owner),
                    _ => None,
                };
                match target.kind() {
                    "self_expression" => index.member_call(&context()?, &method, false),
                    "simple_identifier" => {
                        let target = self.base.get_node_text(&target);
                        let owner = if target == "Self" {
                            context()?
                        } else if index.declared_types.contains(&target)
                            && !index.value_names.contains(&target)
                        {
                            target
                        } else {
                            return None;
                        };
                        index.member_call(&owner, &method, true)
                    }
                    _ => None,
                }
            }
            _ => None,
        }
    }
}

fn is_extension(base: &BaseExtractor, declaration: Node) -> bool {
    declaration
        .child_by_field_name("declaration_kind")
        .is_some_and(|kind| base.get_node_text(&kind) == "extension")
}

/// The name a class-like declaration declares or, for an extension, the
/// single-segment name it extends.
fn type_declaration_name(base: &BaseExtractor, declaration: Node) -> Option<String> {
    let name = declaration.child_by_field_name("name")?;
    match name.kind() {
        "type_identifier" => Some(base.get_node_text(&name)),
        "user_type" => {
            let mut cursor = name.walk();
            let segments: Vec<Node> = name.named_children(&mut cursor).collect();
            match segments.as_slice() {
                [segment] if segment.kind() == "type_identifier" => {
                    Some(base.get_node_text(segment))
                }
                _ => None,
            }
        }
        _ => None,
    }
}

fn inheriting_types(
    base: &BaseExtractor,
    root: Node,
    declared_types: &HashSet<String>,
) -> HashSet<String> {
    let mut inheriting = HashSet::new();
    let mut stack = vec![root];
    while let Some(node) = stack.pop() {
        if node.kind() == "class_declaration"
            && node
                .named_children(&mut node.walk())
                .any(|child| child.kind() == "inheritance_specifier")
            && let Some(name) = type_declaration_name(base, node)
            && declared_types.contains(&name)
        {
            inheriting.insert(name);
        }
        stack.extend(node.named_children(&mut node.walk()));
    }
    inheriting
}

fn named_identifiers<'a>(node: Node<'a>) -> impl Iterator<Item = Node<'a>> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor)
        .filter(|child| child.kind() == "simple_identifier")
        .collect::<Vec<_>>()
        .into_iter()
}

fn type_parameter_names(base: &BaseExtractor, declaration: Node) -> Vec<String> {
    let mut cursor = declaration.walk();
    declaration
        .named_children(&mut cursor)
        .filter(|child| child.kind() == "type_parameters")
        .flat_map(|parameters| {
            let mut cursor = parameters.walk();
            parameters
                .named_children(&mut cursor)
                .filter(|parameter| parameter.kind() == "type_parameter")
                .filter_map(|parameter| {
                    parameter
                        .named_children(&mut parameter.walk())
                        .find(|child| child.kind() == "type_identifier")
                })
                .collect::<Vec<_>>()
        })
        .map(|name| base.get_node_text(&name))
        .collect()
}

fn is_static(base: &BaseExtractor, function: Node) -> bool {
    function.children(&mut function.walk()).any(|child| {
        matches!(child.kind(), "static" | "class")
            || (child.kind() == "modifiers"
                && child.named_children(&mut child.walk()).any(|modifier| {
                    matches!(base.get_node_text(&modifier).as_str(), "static" | "class")
                }))
    })
}

fn record_type_node(base: &mut BaseExtractor, symbol_id: &str, type_node: Node, is_inferred: bool) {
    let Some(base_name) = base_type_name(base, type_node) else {
        return;
    };
    let declared = base.get_node_text(&type_node);
    base.record_declared_type_fact_with_declared(
        symbol_id,
        &base_name,
        &declared,
        &SWIFT_TYPE_NAME_RULES,
        is_inferred,
    );
}

/// The base type name a type node states, with namespace qualifiers kept and
/// generic arguments, optional wrappers, and `some`/`any` dropped. Shapes without a single
/// base name (arrays, dictionaries, tuples, function types, compositions)
/// yield `None`.
pub(super) fn base_type_name(base: &BaseExtractor, node: Node) -> Option<String> {
    let mut node = node;
    loop {
        match node.kind() {
            "type_identifier" | "simple_identifier" | "primitive_type" => {
                return Some(base.get_node_text(&node));
            }
            "optional_type" => {
                node = node.child_by_field_name("wrapped")?;
            }
            "opaque_type" | "existential_type" => {
                node = node.named_child(0)?;
            }
            "type_annotation" => {
                node = node
                    .child_by_field_name("name")
                    .or_else(|| named_type_field(node))?;
            }
            "user_type" => {
                let mut cursor = node.walk();
                let segments: Vec<String> = node
                    .named_children(&mut cursor)
                    .filter(|child| child.kind() == "type_identifier")
                    .map(|child| base.get_node_text(&child))
                    .collect();
                return (!segments.is_empty()).then(|| segments.join("."));
            }
            _ => return None,
        }
    }
}

/// Reduce legacy metadata type text (`propertyType`, `returnType`) to a base
/// type name by the same rules as [`base_type_name`], or `None` when the text
/// has no single base name.
pub(super) fn legacy_base_type_name(text: &str) -> Option<String> {
    let mut trimmed = text.trim();
    while let Some(rest) = trimmed
        .strip_suffix('?')
        .or_else(|| trimmed.strip_suffix('!'))
    {
        trimmed = rest.trim_end();
    }
    for prefix in ["inout", "some", "any"] {
        if let Some(rest) = trimmed.strip_prefix(prefix)
            && rest.starts_with(char::is_whitespace)
        {
            trimmed = rest.trim_start();
        }
    }
    if trimmed.is_empty()
        || trimmed.starts_with(['[', '('])
        || trimmed.contains("->")
        || trimmed.contains('&')
    {
        return None;
    }
    let resolved = strip_type_decorations(trimmed, &SWIFT_TYPE_NAME_RULES);
    let is_base_name = !resolved.is_empty()
        && resolved
            .chars()
            .all(|c| c.is_alphanumeric() || c == '_' || c == '.');
    is_base_name.then_some(resolved)
}

fn named_type_field(node: Node) -> Option<Node> {
    let mut cursor = node.walk();
    node.children_by_field_name("type", &mut cursor)
        .find(|child| child.is_named())
}

pub(super) fn property_type_node(node: Node) -> Option<Node> {
    node.children(&mut node.walk())
        .find(|child| child.kind() == "type_annotation")
        .and_then(|annotation| {
            annotation
                .child_by_field_name("name")
                .or_else(|| named_type_field(annotation))
        })
}

pub(super) fn property_value_node(node: Node) -> Option<Node> {
    let mut cursor = node.walk();
    node.children_by_field_name("value", &mut cursor)
        .find(|child| child.is_named())
}

pub(super) fn nearest_callable_ancestor(node: Node) -> bool {
    let mut current = node.parent();
    while let Some(parent) = current {
        match parent.kind() {
            "function_declaration"
            | "init_declaration"
            | "deinit_declaration"
            | "protocol_function_declaration" => return true,
            "class_declaration" | "protocol_declaration" => return false,
            _ => current = parent.parent(),
        }
    }
    false
}
