use crate::base::BaseExtractor;
use crate::base::types::TypeNameRules;
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};
use std::collections::HashMap;
use tree_sitter::Node;

pub(super) const TYPE_NAME_RULES: TypeNameRules = TypeNameRules {
    nullable_suffixes: &[],
    reference_prefixes: &["*const", "*", "?", "[]const", "[]"],
    generic_open: &['('],
};

pub(super) fn record_declared_type(base: &mut BaseExtractor, symbol_id: &str, type_node: Node) {
    record_type_node(base, symbol_id, type_node, false);
}

/// Record the type a local initializer produces (`is_inferred=true`): a
/// same-file container literal (`Store{ .. }`), or a call to a same-file
/// function with a declared return type (`load()`, `Store.open()`,
/// `Self.open()`, `self.next()`). `try` and a `catch` with a noreturn fallback
/// remove one error-union layer; `.?` and an `orelse` with a noreturn fallback
/// remove one optional layer. `Type.init(..)` on a same-file container that
/// declares no `init` records the container.
pub(super) fn record_initializer_type(
    base: &mut BaseExtractor,
    symbol_id: &str,
    value: Node,
    return_types: &ReturnTypeIndex,
) {
    if value.kind() == "struct_initializer" {
        if let Some(type_node) = struct_initializer_type(value) {
            record_inferred_same_file_container(base, symbol_id, type_node);
        }
        return;
    }
    let scope = InitializerScope { base, return_types };
    let Some(shape) = scope.shape_of(value, 0) else {
        return;
    };
    base.record_declared_type_fact_with_declared(
        symbol_id,
        &shape.name,
        &shape.declared,
        &TYPE_NAME_RULES,
        true,
    );
}

/// A declared type reduced to what initializer inference needs: the resolved
/// base name, the written text, and the wrapper layer the language unwraps.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct TypeShape {
    name: String,
    declared: String,
    layer: Layer,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Layer {
    ErrorUnion(Box<TypeShape>),
    Optional(Box<TypeShape>),
    Plain,
}

impl TypeShape {
    fn without_error_union(self) -> Option<TypeShape> {
        match self.layer {
            Layer::ErrorUnion(ok) => Some(*ok),
            _ => None,
        }
    }

    fn without_optional(self) -> Option<TypeShape> {
        match self.layer {
            Layer::Optional(child) => Some(*child),
            _ => None,
        }
    }
}

/// Declared return types of the file's functions by name, plus every other
/// container-level declaration name (recorded without a type, so it blocks
/// inference for a same-named call).
#[derive(Debug, Default)]
pub(super) struct ReturnTypeIndex {
    declarations: HashMap<String, Vec<ReturnEntry>>,
}

#[derive(Debug)]
struct ReturnEntry {
    /// Node id of the declaring container (the file root at top level).
    owner: usize,
    /// `None` for a non-function, a generic or valueless return, or a
    /// function of an anonymous container or of a container declared inside a
    /// function with `comptime`/`anytype` parameters or an inline loop (its
    /// types may alias values that change per instantiation).
    shape: Option<TypeShape>,
}

impl ReturnTypeIndex {
    pub(super) fn build(base: &BaseExtractor, root: Node) -> Self {
        let mut index = Self::default();
        let mut stack = vec![root];
        while let Some(node) = stack.pop() {
            if node.parent().is_some_and(is_container_or_root)
                && let Some((name, entry)) = return_entry(base, node)
            {
                index.declarations.entry(name).or_default().push(entry);
            }
            stack.extend(node.named_children(&mut node.walk()));
        }
        index
    }

    /// The shape every candidate agrees on; `None` when there is no candidate.
    fn unanimous<'a>(mut candidates: impl Iterator<Item = &'a ReturnEntry>) -> Option<TypeShape> {
        let first = candidates.next()?.shape.as_ref()?;
        candidates
            .all(|entry| entry.shape.as_ref() == Some(first))
            .then(|| first.clone())
    }

    fn entries(&self, name: &str) -> impl Iterator<Item = &ReturnEntry> {
        self.declarations.get(name).into_iter().flatten()
    }

    fn lookup_in_scope(&self, name: &str, scope: &[usize]) -> Option<TypeShape> {
        Self::unanimous(
            self.entries(name)
                .filter(|entry| scope.contains(&entry.owner)),
        )
    }

    fn lookup_member(&self, name: &str, owner: usize) -> Option<TypeShape> {
        Self::unanimous(self.member_entries(name, owner))
    }

    fn declares_member(&self, name: &str, owner: usize) -> bool {
        self.member_entries(name, owner).next().is_some()
    }

    fn member_entries(&self, name: &str, owner: usize) -> impl Iterator<Item = &ReturnEntry> {
        self.entries(name).filter(move |entry| entry.owner == owner)
    }
}

fn is_container(node: Node) -> bool {
    matches!(
        node.kind(),
        "struct_declaration" | "union_declaration" | "enum_declaration" | "opaque_declaration"
    )
}

fn is_container_or_root(node: Node) -> bool {
    is_container(node) || node.parent().is_none()
}

fn declaration_name(base: &BaseExtractor, declaration: Node) -> Option<String> {
    let name = declaration
        .named_children(&mut declaration.walk())
        .find(|child| child.kind() == "identifier")?;
    Some(base.get_node_text(&name))
}

fn return_entry(base: &BaseExtractor, declaration: Node) -> Option<(String, ReturnEntry)> {
    let (name, shape) = match declaration.kind() {
        "function_declaration" => {
            let name = base.get_node_text(&declaration.child_by_field_name("name")?);
            let shape = declaration
                .child_by_field_name("type")
                .and_then(|return_type| type_shape(base, return_type, 0));
            (name, shape)
        }
        "variable_declaration" => (declaration_name(base, declaration)?, None),
        _ => return None,
    };
    let owner = nearest_container(declaration);
    let generic_owner =
        container_type_name(base, owner).is_none() || is_inside_comptime_variation(base, owner);
    let entry = ReturnEntry {
        owner: owner.id(),
        shape: shape.filter(|_| !generic_owner),
    };
    Some((name, entry))
}

/// Whether the types visible at `node` can differ between instantiations: an
/// enclosing function has a `comptime`, `type`, or `anytype` parameter, or an
/// enclosing `inline for`/`inline while` rebinds its captures on each pass.
fn is_inside_comptime_variation(base: &BaseExtractor, node: Node) -> bool {
    let mut current = node.parent();
    while let Some(scope) = current {
        let varies = match scope.kind() {
            "function_declaration" => scope
                .named_children(&mut scope.walk())
                .filter(|child| child.kind() == "parameters")
                .any(|parameters| {
                    parameters
                        .named_children(&mut parameters.walk())
                        .any(|parameter| is_comptime_parameter(base, parameter))
                }),
            "for_statement" | "for_expression" | "while_statement" | "while_expression" => {
                has_keyword(scope, "inline")
            }
            _ => false,
        };
        if varies {
            return true;
        }
        current = scope.parent();
    }
    false
}

fn is_comptime_parameter(base: &BaseExtractor, parameter: Node) -> bool {
    has_keyword(parameter, "comptime")
        || parameter
            .child_by_field_name("type")
            .is_some_and(|type_node| {
                matches!(base.get_node_text(&type_node).as_str(), "type" | "anytype")
            })
}

fn type_shape(base: &BaseExtractor, node: Node, depth: u32) -> Option<TypeShape> {
    if !should_visit_tree_depth(depth) {
        return None;
    }
    let inner = match node.kind() {
        "error_union_type" => node.child_by_field_name("ok"),
        "nullable_type" => inner_type_child(node),
        _ => None,
    };
    let declared = base.get_node_text(&node);
    if let Some(inner) = inner {
        let inner = Box::new(type_shape(base, inner, child_tree_depth(depth)?)?);
        let name = inner.name.clone();
        let layer = if node.kind() == "error_union_type" {
            Layer::ErrorUnion(inner)
        } else {
            Layer::Optional(inner)
        };
        return Some(TypeShape {
            name,
            declared,
            layer,
        });
    }
    let name = match this_type_name(base, node) {
        Some(this_type) => this_type,
        None if names_fixed_type(base, node) => base.get_node_text(&base_type_name_node(node)?),
        None => return None,
    };
    if matches!(name.as_str(), "void" | "noreturn" | "type" | "anytype") {
        return None;
    }
    Some(TypeShape {
        name,
        declared,
        layer: Layer::Plain,
    })
}

/// Whether a type expression names one type for every instantiation: its
/// leading name resolves to a container-level declaration, or to a
/// function-local container. A parameter, a loop capture, a function-local
/// alias, or an unresolved name may stand for a different type each time.
fn names_fixed_type(base: &BaseExtractor, type_node: Node) -> bool {
    let Some(leading) = leading_type_identifier(type_node, 0) else {
        return true;
    };
    nearest_declaration(base, leading).is_some_and(|declaration| {
        declaration.parent().is_some_and(is_container_or_root)
            || declaration
                .named_children(&mut declaration.walk())
                .any(is_container)
    })
}

/// The first identifier of a type expression (`std` in `std.ArrayList(u8)`,
/// `T` in `?*T`); `None` for primitives and builtins.
fn leading_type_identifier(node: Node, depth: u32) -> Option<Node> {
    let child_depth = child_tree_depth(depth).filter(|_| should_visit_tree_depth(depth))?;
    let next = match node.kind() {
        "identifier" => return Some(node),
        "pointer_type" | "nullable_type" | "slice_type" => inner_type_child(node)?,
        "error_union_type" => node.child_by_field_name("ok")?,
        "parenthesized_expression" => node.named_child(0)?,
        "field_expression" => node.child_by_field_name("object")?,
        "call_expression" => node.child_by_field_name("function")?,
        _ => return None,
    };
    leading_type_identifier(next, child_depth)
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
        let child_depth = child_tree_depth(depth)?;
        match value.kind() {
            "parenthesized_expression" => self.shape_of(value.named_child(0)?, child_depth),
            "try_expression" => self
                .shape_of(value.named_child(0)?, child_depth)?
                .without_error_union(),
            "null_coercion_expression" => self
                .shape_of(value.named_child(0)?, child_depth)?
                .without_optional(),
            "catch_expression"
                if value
                    .children(&mut value.walk())
                    .last()
                    .is_some_and(|fallback| is_noreturn(self.base, fallback)) =>
            {
                self.shape_of(value.named_child(0)?, child_depth)?
                    .without_error_union()
            }
            "binary_expression"
                if value
                    .child_by_field_name("operator")
                    .is_some_and(|operator| operator.kind() == "orelse")
                    && is_noreturn(self.base, value.child_by_field_name("right")?) =>
            {
                self.shape_of(value.child_by_field_name("left")?, child_depth)?
                    .without_optional()
            }
            "call_expression" => self.call_shape(value),
            _ => None,
        }
    }

    fn call_shape(&self, call: Node) -> Option<TypeShape> {
        let function = call.child_by_field_name("function")?;
        match function.kind() {
            "identifier" => {
                let scope = enclosing_containers(call);
                self.return_types
                    .lookup_in_scope(&self.base.get_node_text(&function), &scope)
            }
            "field_expression" => {
                let member = self
                    .base
                    .get_node_text(&function.child_by_field_name("member")?);
                if let Some(receiver) = self_receiver_container(self.base, call) {
                    return self.return_types.lookup_member(&member, receiver.id());
                }
                self.type_member_call(function.child_by_field_name("object")?, &member)
            }
            _ => None,
        }
    }

    fn type_member_call(&self, object: Node, member: &str) -> Option<TypeShape> {
        let owner = type_container(self.base, object, 0)?;
        if self.return_types.declares_member(member, owner.id()) {
            return self.return_types.lookup_member(member, owner.id());
        }
        if member != "init" || declares_mixin(owner) {
            return None;
        }
        Some(TypeShape {
            declared: self.base.get_node_text(&object),
            name: container_type_name(self.base, owner)?,
            layer: Layer::Plain,
        })
    }
}

/// A container with `usingnamespace` may take `init` from the mixin, whose
/// return type this file does not know.
fn declares_mixin(container: Node) -> bool {
    container
        .named_children(&mut container.walk())
        .any(|child| child.kind() == "using_namespace_declaration")
}

/// A fallback that never yields a value, so `catch`/`orelse` keeps the
/// unwrapped type.
fn is_noreturn(base: &BaseExtractor, fallback: Node) -> bool {
    match fallback.kind() {
        "unreachable"
        | "return_expression"
        | "break_expression"
        | "continue_expression"
        | "block" => true,
        "builtin_function" => builtin_identifier(base, fallback)
            .is_some_and(|name| matches!(name.as_str(), "@panic" | "@trap" | "@compileError")),
        _ => false,
    }
}

/// Node ids of every container enclosing `node`, innermost first, ending with
/// the file root.
fn enclosing_containers(node: Node) -> Vec<usize> {
    let mut ids = Vec::new();
    let mut current = node.parent();
    while let Some(parent) = current {
        if is_container_or_root(parent) {
            ids.push(parent.id());
        }
        current = parent.parent();
    }
    ids
}

pub(super) fn nearest_symbol_ancestor_is_callable(node: Node) -> bool {
    let mut current = node.parent();
    while let Some(parent) = current {
        match parent.kind() {
            "function_declaration" | "test_declaration" => return true,
            "struct_declaration" | "union_declaration" | "enum_declaration"
            | "opaque_declaration" => return false,
            _ => current = parent.parent(),
        }
    }
    false
}

/// The same-file container named by the type of the first parameter when the
/// call's receiver is that parameter (`self.next()`).
pub(super) fn self_receiver_type(base: &BaseExtractor, node: Node) -> Option<String> {
    container_type_name(base, self_receiver_container(base, node)?)
}

fn self_receiver_container<'t>(base: &BaseExtractor, node: Node<'t>) -> Option<Node<'t>> {
    let function = node.child_by_field_name("function")?;
    if function.kind() != "field_expression" {
        return None;
    }
    let object = function.child_by_field_name("object")?;
    if object.kind() != "identifier" {
        return None;
    }
    let receiver_name = base.get_node_text(&object);
    let func_decl = enclosing_function(node)?;
    let first_param = first_parameter(func_decl)?;
    let param_name = first_param.child_by_field_name("name")?;
    if base.get_node_text(&param_name) != receiver_name {
        return None;
    }
    let container = type_container(base, first_param.child_by_field_name("type")?, 0)?;
    (container == nearest_container(func_decl) || super::helpers::is_inside_struct(func_decl))
        .then_some(container)
}

fn record_inferred_same_file_container(base: &mut BaseExtractor, symbol_id: &str, type_node: Node) {
    if type_container(base, type_node, 0).is_some() {
        record_type_node(base, symbol_id, type_node, true);
    }
}

fn record_type_node(base: &mut BaseExtractor, symbol_id: &str, type_node: Node, is_inferred: bool) {
    let base_name = match this_type_name(base, type_node) {
        Some(this_type) => this_type,
        None => {
            let Some(name_node) = base_type_name_node(type_node) else {
                return;
            };
            base.get_node_text(&name_node)
        }
    };
    let declared = base.get_node_text(&type_node);
    base.record_declared_type_fact_with_declared(
        symbol_id,
        &base_name,
        &declared,
        &TYPE_NAME_RULES,
        is_inferred,
    );
}

/// Reduce a type-position node to the node naming its base type: the last
/// segment of a qualified name (`std.mem.Allocator` -> `Allocator`), the
/// constructor of a generic application (`std.ArrayList(u8)` -> `ArrayList`),
/// with pointer, optional, slice, and error-union wrappers dropped.
pub(super) fn base_type_name_node(node: Node) -> Option<Node> {
    let mut node = node;
    loop {
        match node.kind() {
            "identifier" | "builtin_type" => return Some(node),
            "pointer_type" | "nullable_type" | "slice_type" => {
                node = inner_type_child(node)?;
            }
            "error_union_type" => {
                node = node.child_by_field_name("ok")?;
            }
            "parenthesized_expression" => {
                node = node.named_child(0)?;
            }
            "field_expression" => return node.child_by_field_name("member"),
            "call_expression" => {
                let function = node.child_by_field_name("function")?;
                return match function.kind() {
                    "identifier" => Some(function),
                    "field_expression" => function.child_by_field_name("member"),
                    _ => None,
                };
            }
            _ => return None,
        }
    }
}

fn inner_type_child(node: Node) -> Option<Node> {
    let sentinel = node.child_by_field_name("sentinel").map(|child| child.id());
    let mut cursor = node.walk();
    node.named_children(&mut cursor).find(|child| {
        Some(child.id()) != sentinel
            && matches!(
                child.kind(),
                "identifier"
                    | "builtin_type"
                    | "pointer_type"
                    | "nullable_type"
                    | "call_expression"
                    | "parenthesized_expression"
                    | "builtin_function"
                    | "field_expression"
                    | "array_type"
                    | "slice_type"
            )
    })
}

fn struct_initializer_type(node: Node) -> Option<Node> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor)
        .find(|child| child.kind() != "initializer_list")
}

fn is_this_type(base: &BaseExtractor, node: Node) -> bool {
    let mut node = node;
    loop {
        match node.kind() {
            "pointer_type" | "nullable_type" => {
                let Some(inner) = inner_type_child(node) else {
                    return false;
                };
                node = inner;
            }
            "parenthesized_expression" => {
                let Some(inner) = node.named_child(0) else {
                    return false;
                };
                node = inner;
            }
            "builtin_function" => {
                return builtin_identifier(base, node)
                    .map(|name| name == "@This")
                    .unwrap_or(false);
            }
            _ => return false,
        }
    }
}

fn builtin_identifier(base: &BaseExtractor, node: Node) -> Option<String> {
    let mut cursor = node.walk();
    let ident = node
        .named_children(&mut cursor)
        .find(|child| child.kind() == "builtin_identifier")?;
    Some(base.get_node_text(&ident))
}

fn enclosing_function(node: Node) -> Option<Node> {
    let mut current = node.parent();
    while let Some(parent) = current {
        if parent.kind() == "function_declaration" {
            return Some(parent);
        }
        current = parent.parent();
    }
    None
}

fn first_parameter(func_decl: Node) -> Option<Node> {
    let mut cursor = func_decl.walk();
    let params = func_decl
        .named_children(&mut cursor)
        .find(|child| child.kind() == "parameters")?;
    let mut param_cursor = params.walk();
    params
        .named_children(&mut param_cursor)
        .find(|child| child.kind() == "parameter")
}

/// The container a `@This()` type, or an in-scope alias of it
/// (`const Self = @This();`), names at `type_node`. A file is itself a struct,
/// so at file scope the name is the file stem (`Tokenizer.zig` -> `Tokenizer`).
fn this_type_name(base: &BaseExtractor, type_node: Node) -> Option<String> {
    if is_this_type(base, type_node) {
        return container_type_name(base, nearest_container(type_node));
    }
    let declaration = nearest_declaration(base, bare_type_name_node(type_node)?)?;
    let is_alias = declaration.kind() == "variable_declaration"
        && initializer_node(declaration).is_some_and(|value| is_this_type(base, value));
    is_alias.then(|| container_type_name(base, nearest_container(declaration)))?
}

/// The base type name of a type-position node when it is a bare identifier,
/// not the member of a qualified name (`other.Store`).
fn bare_type_name_node(type_node: Node) -> Option<Node> {
    base_type_name_node(type_node).filter(|name| {
        name.kind() == "identifier"
            && name
                .parent()
                .is_none_or(|parent| parent.kind() != "field_expression")
    })
}

/// The same-file container a type expression names: `@This()`, or a name
/// (`Store`, `Outer.Inner`) whose first segment resolves to its nearest
/// declaration in scope. Each declaration on the path must be a container or
/// an alias of `@This()` (the container that declares the alias). Any other
/// declaration, such as an import or an alias of another type, names nothing
/// this file can resolve.
fn type_container<'t>(base: &BaseExtractor, type_node: Node<'t>, depth: u32) -> Option<Node<'t>> {
    let child_depth = child_tree_depth(depth).filter(|_| should_visit_tree_depth(depth))?;
    let declaration = match type_node.kind() {
        "pointer_type" | "nullable_type" => {
            return type_container(base, inner_type_child(type_node)?, child_depth);
        }
        "parenthesized_expression" => {
            return type_container(base, type_node.named_child(0)?, child_depth);
        }
        "builtin_function" => {
            return is_this_type(base, type_node).then(|| nearest_container(type_node));
        }
        "identifier" => nearest_declaration(base, type_node)?,
        "field_expression" => {
            let owner =
                type_container(base, type_node.child_by_field_name("object")?, child_depth)?;
            let member = base.get_node_text(&type_node.child_by_field_name("member")?);
            owner
                .named_children(&mut owner.walk())
                .filter(|child| child.kind() == "variable_declaration")
                .find(|declaration| {
                    declaration_name(base, *declaration).as_deref() == Some(&member)
                })?
        }
        _ => return None,
    };
    if declaration.kind() != "variable_declaration" {
        return None;
    }
    if let Some(container) = declaration
        .named_children(&mut declaration.walk())
        .find(|child| is_container(*child))
    {
        return Some(container);
    }
    initializer_node(declaration)
        .filter(|value| is_this_type(base, *value))
        .map(|_| nearest_container(declaration))
}

/// The declaration (`variable_declaration`, `function_declaration`, or
/// function `parameter`) that the identifier `name` refers to: the nearest one
/// in an enclosing block, function, or container, searched outward from `name`.
fn nearest_declaration<'t>(base: &BaseExtractor, name: Node<'t>) -> Option<Node<'t>> {
    let text = base.get_node_text(&name);
    let mut current = name.parent();
    while let Some(scope) = current {
        let found = match scope.kind() {
            "function_declaration" => scope
                .named_children(&mut scope.walk())
                .find(|child| child.kind() == "parameters")
                .and_then(|parameters| {
                    parameters
                        .named_children(&mut parameters.walk())
                        .find(|parameter| {
                            parameter
                                .child_by_field_name("name")
                                .is_some_and(|name| base.get_node_text(&name) == text)
                        })
                }),
            _ if scope.kind() == "block" || is_container_or_root(scope) => scope
                .named_children(&mut scope.walk())
                .filter(|child| {
                    matches!(
                        child.kind(),
                        "variable_declaration" | "function_declaration"
                    )
                })
                .find(|declaration| declaration_name(base, *declaration).as_deref() == Some(&text)),
            _ => None,
        };
        if found.is_some() {
            return found;
        }
        current = scope.parent();
    }
    None
}

fn nearest_container(node: Node) -> Node {
    let mut current = node;
    while let Some(parent) = current.parent() {
        if matches!(
            parent.kind(),
            "struct_declaration" | "union_declaration" | "enum_declaration" | "opaque_declaration"
        ) {
            return parent;
        }
        current = parent;
    }
    current
}

fn container_type_name(base: &BaseExtractor, container: Node) -> Option<String> {
    if container.parent().is_none() {
        return Some(file_struct_name(&base.file_path));
    }
    let declaration = container
        .parent()
        .filter(|parent| parent.kind() == "variable_declaration")?;
    let name = declaration
        .children(&mut declaration.walk())
        .find(|child| child.kind() == "identifier")?;
    Some(base.get_node_text(&name))
}

/// The struct a Zig file declares: its file stem.
pub(super) fn file_struct_name(file_path: &str) -> String {
    let file_name = file_path.rsplit(['/', '\\']).next().unwrap_or(file_path);
    file_name
        .strip_suffix(".zig")
        .unwrap_or(file_name)
        .to_string()
}

pub(super) fn initializer_node(node: Node) -> Option<Node> {
    let mut cursor = node.walk();
    let children: Vec<Node> = node.children(&mut cursor).collect();
    let eq = children.iter().position(|child| child.kind() == "=")?;
    children[eq + 1..]
        .iter()
        .copied()
        .find(|child| child.is_named())
}

pub(super) fn has_keyword(node: Node, keyword: &str) -> bool {
    let mut cursor = node.walk();
    node.children(&mut cursor)
        .any(|child| child.kind() == keyword)
}
