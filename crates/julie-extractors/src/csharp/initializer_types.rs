//! Inferred types for `var` locals whose initializer calls a same-file method
//! or local function with a declared return type. C# and Razor share it.

use crate::base::BaseExtractor;
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};
use std::collections::HashMap;
use std::ops::Range;
use tree_sitter::Node;

/// A declaring type: its simple name and the start byte of its declaration.
/// The class a Razor file compiles to has an empty name and no position.
#[derive(Debug, Clone, PartialEq, Eq)]
struct TypeKey {
    name: String,
    start: usize,
}

impl TypeKey {
    fn file_class() -> Self {
        Self {
            name: String::new(),
            start: usize::MAX,
        }
    }
}

/// A return type reduced to what inference needs: the base name (`None` for
/// type parameters and shapes without one), the written text, and the type
/// arguments of a generic name.
#[derive(Debug, Clone, PartialEq, Eq)]
struct TypeShape {
    name: Option<String>,
    declared: String,
    args: Vec<TypeShape>,
}

impl TypeShape {
    fn awaited(self) -> Option<TypeShape> {
        match self.name.as_deref() {
            Some("Task" | "ValueTask") if self.args.len() == 1 => self.args.into_iter().next(),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct Arity {
    required: usize,
    total: usize,
    variadic: bool,
}

impl Arity {
    fn accepts(self, arguments: usize) -> bool {
        arguments >= self.required && (self.variadic || arguments <= self.total)
    }
}

#[derive(Debug)]
struct Callable {
    /// The innermost declaring type; `None` for a top-level local function.
    owner: Option<TypeKey>,
    /// The byte range a local function is visible in; `None` for a method.
    local_scope: Option<Range<usize>>,
    is_static: bool,
    arity: Arity,
    /// `None` for `void` and for type-parameter returns.
    shape: Option<TypeShape>,
}

/// Where a name is visible: a type (`None` for top-level code) and, for a
/// binding inside a member body, that member's start byte.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Scope {
    owner: Option<TypeKey>,
    member: Option<usize>,
}

/// Names that every class, struct, and record inherits from `object` or
/// `ValueType`, or that a record synthesizes. A call by one of these names can
/// bind to an inherited member that no same-file declaration shows.
const INHERITED_NAMES: &[&str] = &[
    "Equals",
    "ReferenceEquals",
    "GetHashCode",
    "GetType",
    "ToString",
    "MemberwiseClone",
    "Finalize",
    "PrintMembers",
    "Deconstruct",
];

const TYPE_KINDS: &[&str] = &[
    "class_declaration",
    "struct_declaration",
    "record_declaration",
    "interface_declaration",
    "enum_declaration",
    "extension_declaration",
];

const MEMBER_KINDS: &[&str] = &[
    "method_declaration",
    "constructor_declaration",
    "destructor_declaration",
    "operator_declaration",
    "conversion_operator_declaration",
    "property_declaration",
    "indexer_declaration",
    "event_declaration",
];

/// Declared return types of the file's methods and local functions, by name,
/// and the scopes of every other binding (local, parameter, field, property,
/// event, pattern or query variable) by name. Explicit interface
/// implementations are left out: no simple name or `this.` access reaches them.
#[derive(Debug, Default)]
pub(crate) struct ReturnTypeIndex {
    callables: HashMap<String, Vec<Callable>>,
    bindings: HashMap<String, Vec<Scope>>,
    file_class: bool,
}

impl ReturnTypeIndex {
    /// Index a C# file. Code outside every declared type is top-level code.
    pub(crate) fn for_csharp(base: &BaseExtractor, root: Node) -> Self {
        Self::build(base, root, false, &[])
    }

    /// Index a Razor file. Code outside every declared type belongs to the
    /// file class, whose `@typeparam` names are `type_parameters`.
    pub(crate) fn for_razor(base: &BaseExtractor, root: Node, type_parameters: &[String]) -> Self {
        Self::build(base, root, true, type_parameters)
    }

    fn build(base: &BaseExtractor, root: Node, file_class: bool, file_generics: &[String]) -> Self {
        let mut index = Self {
            callables: HashMap::new(),
            bindings: HashMap::new(),
            file_class,
        };
        let mut stack = vec![root];
        while let Some(node) = stack.pop() {
            if let Some((name, callable)) = index.callable(base, node, file_generics) {
                index.callables.entry(name).or_default().push(callable);
            }
            for (name, declaration) in bindings(base, node) {
                if let Some(scope) = index.scope(base, declaration) {
                    index.bindings.entry(name).or_default().push(scope);
                }
            }
            stack.extend(node.named_children(&mut node.walk()));
        }
        index
    }

    fn callable(
        &self,
        base: &BaseExtractor,
        node: Node,
        file_generics: &[String],
    ) -> Option<(String, Callable)> {
        let (returns, local_scope) = match node.kind() {
            "method_declaration" => {
                let mut cursor = node.walk();
                if node
                    .children(&mut cursor)
                    .any(|child| child.kind() == "explicit_interface_specifier")
                {
                    return None;
                }
                (node.child_by_field_name("returns")?, None)
            }
            "local_function_statement" => {
                (node.child_by_field_name("type")?, Some(local_scope(node)?))
            }
            _ => return None,
        };
        let owner = self
            .enclosing_types(base, node)?
            .into_iter()
            .next()
            .map(|(key, _)| key);
        if owner.is_none() && local_scope.is_none() {
            return None;
        }
        let mut generics = enclosing_type_parameters(base, node);
        generics.extend(file_generics.iter().cloned());
        let shape = (base.get_node_text(&returns).trim() != "void")
            .then(|| type_shape(base, returns, &generics, 0))
            .flatten();
        let callable = Callable {
            owner,
            local_scope,
            is_static: has_modifier(base, node, "static"),
            arity: arity(node),
            shape,
        };
        Some((
            base.get_node_text(&node.child_by_field_name("name")?),
            callable,
        ))
    }

    /// The declared text of the type a `var` initializer produces, or `None`
    /// when the initializer is not a resolvable same-file call. `await`
    /// removes one `Task<T>` / `ValueTask<T>` layer, also through
    /// `.ConfigureAwait(..)`; `!` and parentheses keep the type.
    pub(crate) fn initializer_type(&self, base: &BaseExtractor, value: Node) -> Option<String> {
        let shape = self.shape_of(base, value, 0)?;
        shape.name.is_some().then_some(shape.declared)
    }

    fn shape_of(&self, base: &BaseExtractor, value: Node, depth: u32) -> Option<TypeShape> {
        if !should_visit_tree_depth(depth) {
            return None;
        }
        let child_depth = child_tree_depth(depth)?;
        match value.kind() {
            "parenthesized_expression" => self.shape_of(base, value.named_child(0)?, child_depth),
            "postfix_unary_expression" => {
                let operator = value.child(value.child_count().checked_sub(1)? as u32)?;
                if operator.kind() != "!" {
                    return None;
                }
                self.shape_of(base, value.named_child(0)?, child_depth)
            }
            "await_expression" => {
                let operand = value.named_child(0)?;
                let task = configured_task(base, operand).unwrap_or(operand);
                self.shape_of(base, task, child_depth)?.awaited()
            }
            "invocation_expression" => self.call_shape(base, value),
            _ => None,
        }
    }

    fn call_shape(&self, base: &BaseExtractor, call: Node) -> Option<TypeShape> {
        let function = call.child_by_field_name("function")?;
        let arguments = call.child_by_field_name("arguments").map_or(0, |list| {
            let mut cursor = list.walk();
            list.named_children(&mut cursor)
                .filter(|argument| argument.kind() == "argument")
                .count()
        });
        let name = match function.kind() {
            "member_access_expression" => function.child_by_field_name("name")?,
            _ => function,
        };
        let name = simple_name(base, name)?;
        if INHERITED_NAMES.contains(&name.as_str()) {
            return None;
        }
        match function.kind() {
            "identifier" | "generic_name" => self.simple_name_call(base, &name, call, arguments),
            "member_access_expression" => {
                let receiver = function.child_by_field_name("expression")?;
                match receiver.kind() {
                    "this" => {
                        let (owner, _) = self.enclosing_types(base, call)?.into_iter().next()?;
                        agree(
                            self.members(&name, &owner)
                                .filter(|c| c.arity.accepts(arguments)),
                        )
                    }
                    "identifier" | "generic_name" => {
                        let type_name = simple_name(base, receiver)?;
                        if self.visible_binding(base, &type_name, call)? {
                            return None;
                        }
                        let candidates: Vec<&Callable> = self
                            .callables
                            .get(&name)
                            .into_iter()
                            .flatten()
                            .filter(|c| {
                                c.local_scope.is_none()
                                    && c.owner.as_ref().is_some_and(|o| o.name == type_name)
                                    && c.arity.accepts(arguments)
                            })
                            .collect();
                        if !candidates.iter().all(|c| c.is_static) {
                            return None;
                        }
                        agree(candidates.into_iter())
                    }
                    _ => None,
                }
            }
            _ => None,
        }
    }

    /// A call by simple name: a local function in scope, or a method of the
    /// innermost enclosing type that declares the name. A type with a base
    /// list or `partial` may inherit or share members from another file, so
    /// the search stops there. A same-named local, parameter, field,
    /// property, or event hides the methods, so the call records nothing.
    fn simple_name_call(
        &self,
        base: &BaseExtractor,
        name: &str,
        call: Node,
        arguments: usize,
    ) -> Option<TypeShape> {
        let types = self.enclosing_types(base, call)?;
        let owner = types.first().map(|(owner, _)| owner);
        if self.bound(name, owner, enclosing_member(call)) {
            return None;
        }
        let locals = self.callables.get(name)?.iter().filter(|c| {
            c.owner.as_ref() == owner
                && c.local_scope
                    .as_ref()
                    .is_some_and(|scope| scope.contains(&call.start_byte()))
        });
        let mut members = Vec::new();
        for (owner, open) in &types {
            if self.bound(name, Some(owner), None) {
                return None;
            }
            members = self.members(name, owner).collect();
            if !members.is_empty() || *open {
                break;
            }
        }
        agree(locals.chain(members).filter(|c| c.arity.accepts(arguments)))
    }

    /// Whether a non-method binding named `name` is visible at `node`, from
    /// its member body or from any enclosing type. `None` inside an extension
    /// block.
    fn visible_binding(&self, base: &BaseExtractor, name: &str, node: Node) -> Option<bool> {
        let types = self.enclosing_types(base, node)?;
        let innermost = types.first().map(|(owner, _)| owner);
        Some(
            self.bound(name, innermost, enclosing_member(node))
                || types
                    .iter()
                    .any(|(owner, _)| self.bound(name, Some(owner), None)),
        )
    }

    /// Whether a binding named `name` belongs to `owner` at type level or to
    /// `member`'s body.
    fn bound(&self, name: &str, owner: Option<&TypeKey>, member: Option<usize>) -> bool {
        self.bindings.get(name).is_some_and(|scopes| {
            scopes.iter().any(|scope| {
                scope.owner.as_ref() == owner && (scope.member.is_none() || scope.member == member)
            })
        })
    }

    fn scope(&self, base: &BaseExtractor, declaration: Node) -> Option<Scope> {
        let owner = self
            .enclosing_types(base, declaration)?
            .into_iter()
            .next()
            .map(|(key, _)| key);
        Some(Scope {
            owner,
            member: enclosing_member(declaration),
        })
    }

    fn members<'a>(&'a self, name: &str, owner: &'a TypeKey) -> impl Iterator<Item = &'a Callable> {
        self.callables
            .get(name)
            .into_iter()
            .flatten()
            .filter(move |c| c.local_scope.is_none() && c.owner.as_ref() == Some(owner))
    }

    /// The types around `node`, innermost first, each with whether it may
    /// have members declared elsewhere (a base list or `partial`). `None`
    /// inside an extension block, whose receiver is not an enclosing type.
    fn enclosing_types(&self, base: &BaseExtractor, node: Node) -> Option<Vec<(TypeKey, bool)>> {
        let mut types = Vec::new();
        let mut current = node.parent();
        while let Some(ancestor) = current {
            match ancestor.kind() {
                "class_declaration"
                | "struct_declaration"
                | "record_declaration"
                | "interface_declaration" => {
                    let name = base.get_node_text(&ancestor.child_by_field_name("name")?);
                    let mut cursor = ancestor.walk();
                    let open = ancestor
                        .children(&mut cursor)
                        .any(|child| child.kind() == "base_list")
                        || has_modifier(base, ancestor, "partial");
                    let key = TypeKey {
                        name,
                        start: ancestor.start_byte(),
                    };
                    types.push((key, open));
                }
                "extension_declaration" => return None,
                _ => {}
            }
            current = ancestor.parent();
        }
        if self.file_class {
            types.push((TypeKey::file_class(), true));
        }
        Some(types)
    }
}

/// The outermost member (method, constructor, property, ...) around `node`
/// inside its innermost type, by start byte. `None` at type level, in
/// top-level code, and in Razor markup.
fn enclosing_member(node: Node) -> Option<usize> {
    let mut member = None;
    let mut current = node.parent();
    while let Some(ancestor) = current {
        if TYPE_KINDS.contains(&ancestor.kind()) {
            break;
        }
        if MEMBER_KINDS.contains(&ancestor.kind()) {
            member = Some(ancestor.start_byte());
        }
        current = ancestor.parent();
    }
    member
}

/// The non-method names `node` binds, each with the node whose scope it
/// takes. A property or event name takes the scope of its declaration, so it
/// is visible to the whole type. Query and Razor `case` bindings take every
/// identifier, which can only hide more calls.
fn bindings<'a>(base: &BaseExtractor, node: Node<'a>) -> Vec<(String, Node<'a>)> {
    let named = |field: &str| {
        let mut cursor = node.walk();
        node.children_by_field_name(field, &mut cursor)
            .filter(|child| child.kind() == "identifier")
            .map(|child| (base.get_node_text(&child), node))
            .collect::<Vec<_>>()
    };
    match node.kind() {
        "implicit_parameter" => vec![(base.get_node_text(&node), node)],
        "parameter"
        | "variable_declarator"
        | "declaration_pattern"
        | "recursive_pattern"
        | "catch_declaration"
        | "declaration_expression"
        | "tuple_pattern"
        | "from_clause" => named("name"),
        "property_declaration" | "event_declaration" => node
            .parent()
            .map(|parent| {
                named("name")
                    .into_iter()
                    .map(|(name, _)| (name, parent))
                    .collect()
            })
            .unwrap_or_default(),
        "foreach_statement" | "razor_foreach" => named("left"),
        "join_clause" | "join_into_clause" | "let_clause" | "query_continuation" => {
            let mut cursor = node.walk();
            node.named_children(&mut cursor)
                .filter(|child| child.kind() == "identifier")
                .map(|child| (base.get_node_text(&child), node))
                .collect()
        }
        "razor_case_condition" => base
            .get_node_text(&node)
            .split(|c: char| !(c.is_alphanumeric() || c == '_'))
            .filter(|word| !word.is_empty())
            .map(|word| (word.to_string(), node))
            .collect(),
        _ => Vec::new(),
    }
}

/// The return type every candidate agrees on.
fn agree<'a>(mut candidates: impl Iterator<Item = &'a Callable>) -> Option<TypeShape> {
    let first = candidates.next()?.shape.as_ref()?;
    candidates
        .all(|c| c.shape.as_ref() == Some(first))
        .then(|| first.clone())
}

/// `task.ConfigureAwait(..)` awaits like `task`.
fn configured_task<'a>(base: &BaseExtractor, operand: Node<'a>) -> Option<Node<'a>> {
    if operand.kind() != "invocation_expression" {
        return None;
    }
    let function = operand.child_by_field_name("function")?;
    if function.kind() != "member_access_expression"
        || base.get_node_text(&function.child_by_field_name("name")?) != "ConfigureAwait"
    {
        return None;
    }
    function.child_by_field_name("expression")
}

/// The block a local function is visible in. A top-level local function is
/// visible to all top-level statements.
fn local_scope(function: Node) -> Option<Range<usize>> {
    let parent = function.parent()?;
    let scope = if parent.kind() == "global_statement" {
        parent.parent()?
    } else {
        parent
    };
    Some(scope.byte_range())
}

fn simple_name(base: &BaseExtractor, node: Node) -> Option<String> {
    match node.kind() {
        "identifier" => Some(base.get_node_text(&node)),
        "generic_name" => Some(base.get_node_text(&node.named_child(0)?)),
        _ => None,
    }
}

fn has_modifier(base: &BaseExtractor, node: Node, modifier: &str) -> bool {
    let mut cursor = node.walk();
    node.children(&mut cursor)
        .any(|child| child.kind() == "modifier" && base.get_node_text(&child) == modifier)
}

fn arity(callable: Node) -> Arity {
    let mut arity = Arity {
        required: 0,
        total: 0,
        variadic: false,
    };
    let Some(parameters) = callable.child_by_field_name("parameters") else {
        return arity;
    };
    let mut cursor = parameters.walk();
    for child in parameters.children(&mut cursor) {
        match child.kind() {
            "params" => arity.variadic = true,
            "parameter" => {
                let mut parameter_cursor = child.walk();
                let optional = child
                    .children(&mut parameter_cursor)
                    .any(|part| part.kind() == "=");
                arity.total += 1;
                if !optional {
                    arity.required += 1;
                }
            }
            _ => {}
        }
    }
    arity
}

/// Type parameter names in scope at `node`: its own and every enclosing
/// type's and callable's.
fn enclosing_type_parameters(base: &BaseExtractor, node: Node) -> Vec<String> {
    let mut names = Vec::new();
    let mut current = Some(node);
    while let Some(item) = current {
        let mut cursor = item.walk();
        for list in item
            .children(&mut cursor)
            .filter(|child| child.kind() == "type_parameter_list")
        {
            let mut list_cursor = list.walk();
            names.extend(
                list.named_children(&mut list_cursor)
                    .filter_map(|parameter| parameter.child_by_field_name("name"))
                    .map(|name| base.get_node_text(&name)),
            );
        }
        current = item.parent();
    }
    names
}

fn type_shape(
    base: &BaseExtractor,
    node: Node,
    generics: &[String],
    depth: u32,
) -> Option<TypeShape> {
    if !should_visit_tree_depth(depth) {
        return None;
    }
    let child_depth = child_tree_depth(depth)?;
    let declared = base.get_node_text(&node);
    let inner = |field: &str| {
        node.child_by_field_name(field)
            .and_then(|inner| type_shape(base, inner, generics, child_depth))
    };
    let (name, args) = match node.kind() {
        "predefined_type" => (Some(declared.clone()), Vec::new()),
        "identifier" => (
            Some(declared.clone()).filter(|name| !generics.contains(name)),
            Vec::new(),
        ),
        "generic_name" => {
            let mut cursor = node.walk();
            let args = node
                .named_children(&mut cursor)
                .filter(|child| child.kind() == "type_argument_list")
                .flat_map(|list| {
                    let mut list_cursor = list.walk();
                    list.named_children(&mut list_cursor).collect::<Vec<_>>()
                })
                .map(|argument| type_shape(base, argument, generics, child_depth))
                .collect::<Option<Vec<_>>>()?;
            (simple_name(base, node), args)
        }
        "qualified_name" | "alias_qualified_name" => {
            let inner = inner("name")?;
            (inner.name, inner.args)
        }
        "nullable_type" | "ref_type" | "scoped_type" => {
            let inner = inner("type")?;
            (inner.name, inner.args)
        }
        "array_type" => (inner("type")?.name, Vec::new()),
        _ => (None, Vec::new()),
    };
    Some(TypeShape {
        name,
        declared,
        args,
    })
}
