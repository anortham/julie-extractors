use crate::base::BaseExtractor;
use crate::base::types::TypeNameRules;
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};
use std::collections::{HashMap, HashSet};
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
            record_inferred_same_file_container(base, symbol_id, type_node, value);
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
/// inference for a same-named call), and the names of same-file containers.
#[derive(Debug, Default)]
pub(super) struct ReturnTypeIndex {
    declarations: HashMap<String, Vec<ReturnEntry>>,
    containers: HashSet<String>,
}

#[derive(Debug)]
struct ReturnEntry {
    /// Node id of the declaring container (the file root at top level).
    owner: usize,
    /// The declaring container's name; `None` for an anonymous container.
    owner_name: Option<String>,
    /// `None` for a non-function, a generic or valueless return, or a
    /// function of an anonymous container (its types may alias `comptime`
    /// parameters).
    shape: Option<TypeShape>,
}

impl ReturnTypeIndex {
    pub(super) fn build(base: &BaseExtractor, root: Node) -> Self {
        let mut index = Self::default();
        let mut stack = vec![root];
        while let Some(node) = stack.pop() {
            if node.kind() == "variable_declaration"
                && let Some(name) = declaration_name(base, node)
                && node.named_children(&mut node.walk()).any(is_container)
            {
                index.containers.insert(name);
            }
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

    fn lookup_member(&self, name: &str, owner: &str) -> Option<TypeShape> {
        Self::unanimous(self.member_entries(name, owner))
    }

    fn declares_member(&self, name: &str, owner: &str) -> bool {
        self.member_entries(name, owner).next().is_some()
    }

    fn member_entries<'a>(
        &'a self,
        name: &str,
        owner: &'a str,
    ) -> impl Iterator<Item = &'a ReturnEntry> {
        self.entries(name)
            .filter(move |entry| entry.owner_name.as_deref() == Some(owner))
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
            let generics = comptime_type_parameters(base, declaration);
            let shape = declaration
                .child_by_field_name("type")
                .and_then(|return_type| type_shape(base, return_type, &generics, 0));
            (name, shape)
        }
        "variable_declaration" => (declaration_name(base, declaration)?, None),
        _ => return None,
    };
    let owner = nearest_container(declaration);
    let owner_name = container_type_name(base, owner);
    let entry = ReturnEntry {
        owner: owner.id(),
        shape: shape.filter(|_| owner_name.is_some()),
        owner_name,
    };
    Some((name, entry))
}

/// The `comptime T: type` parameter names of a function and of every function
/// that encloses it.
fn comptime_type_parameters(base: &BaseExtractor, function: Node) -> Vec<String> {
    let mut names = Vec::new();
    let mut current = Some(function);
    while let Some(node) = current {
        if node.kind() == "function_declaration"
            && let Some(parameters) = node
                .named_children(&mut node.walk())
                .find(|child| child.kind() == "parameters")
        {
            names.extend(
                parameters
                    .named_children(&mut parameters.walk())
                    .filter(|parameter| {
                        parameter
                            .child_by_field_name("type")
                            .is_some_and(|type_node| base.get_node_text(&type_node) == "type")
                    })
                    .filter_map(|parameter| parameter.child_by_field_name("name"))
                    .map(|name| base.get_node_text(&name)),
            );
        }
        current = node.parent();
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
    let inner = match node.kind() {
        "error_union_type" => node.child_by_field_name("ok"),
        "nullable_type" => inner_type_child(node),
        _ => None,
    };
    let declared = base.get_node_text(&node);
    if let Some(inner) = inner {
        let inner = Box::new(type_shape(base, inner, generics, child_tree_depth(depth)?)?);
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
    let name_node = base_type_name_node(node)?;
    let name = match this_type_name(base, node) {
        Some(this_type) => this_type,
        None => base.get_node_text(&name_node),
    };
    if generics.contains(&base.get_node_text(&name_node))
        || matches!(name.as_str(), "void" | "noreturn" | "type" | "anytype")
    {
        return None;
    }
    Some(TypeShape {
        name,
        declared,
        layer: Layer::Plain,
    })
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
                    .is_some_and(is_noreturn) =>
            {
                self.shape_of(value.named_child(0)?, child_depth)?
                    .without_error_union()
            }
            "binary_expression"
                if value
                    .child_by_field_name("operator")
                    .is_some_and(|operator| operator.kind() == "orelse")
                    && is_noreturn(value.child_by_field_name("right")?) =>
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
                if let Some(receiver) = self_receiver_type(self.base, call) {
                    return self.return_types.lookup_member(&member, &receiver);
                }
                self.type_member_call(function.child_by_field_name("object")?, &member)
            }
            _ => None,
        }
    }

    fn type_member_call(&self, object: Node, member: &str) -> Option<TypeShape> {
        if !matches!(object.kind(), "identifier" | "builtin_function") {
            return None;
        }
        let owner = match this_type_name(self.base, object) {
            Some(this_type) => this_type,
            None if object.kind() == "identifier" => {
                let name = self.base.get_node_text(&object);
                self.return_types
                    .containers
                    .contains(&name)
                    .then_some(name)?
            }
            None => return None,
        };
        if self.return_types.declares_member(member, &owner) {
            return self.return_types.lookup_member(member, &owner);
        }
        (member == "init" && self.return_types.containers.contains(&owner)).then(|| TypeShape {
            declared: self.base.get_node_text(&object),
            name: owner,
            layer: Layer::Plain,
        })
    }
}

/// A fallback that never yields a value, so `catch`/`orelse` keeps the
/// unwrapped type.
fn is_noreturn(fallback: Node) -> bool {
    matches!(
        fallback.kind(),
        "unreachable" | "return_expression" | "break_expression" | "continue_expression" | "block"
    )
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

pub(super) fn self_receiver_type(base: &BaseExtractor, node: Node) -> Option<String> {
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
    let type_node = first_param.child_by_field_name("type")?;
    if let Some(this_type) = this_type_name(base, type_node) {
        return Some(this_type);
    }
    if !super::helpers::is_inside_struct(func_decl) {
        return None;
    }
    let name_node = base_type_name_node(type_node)?;
    let name = base.get_node_text(&name_node);
    same_file_container(base, file_root(node), &name).then_some(name)
}

fn record_inferred_same_file_container(
    base: &mut BaseExtractor,
    symbol_id: &str,
    type_node: Node,
    from: Node,
) {
    let Some(name_node) = base_type_name_node(type_node) else {
        return;
    };
    let name = base.get_node_text(&name_node);
    if !same_file_container(base, file_root(from), &name) {
        return;
    }
    record_type_node(base, symbol_id, type_node, true);
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

/// The container a `@This()` type, or a same-container alias of it
/// (`const Self = @This();`), names at `type_node`. A file is itself a struct,
/// so at file scope the name is the file stem (`Tokenizer.zig` -> `Tokenizer`).
fn this_type_name(base: &BaseExtractor, type_node: Node) -> Option<String> {
    let container = nearest_container(type_node);
    if is_this_type(base, type_node) {
        return container_type_name(base, container);
    }
    let name_node = base_type_name_node(type_node).filter(|name| name.kind() == "identifier")?;
    let alias = base.get_node_text(&name_node);
    let is_alias = container
        .children(&mut container.walk())
        .filter(|child| child.kind() == "variable_declaration")
        .any(|declaration| {
            declaration
                .children(&mut declaration.walk())
                .find(|child| child.kind() == "identifier")
                .is_some_and(|name| base.get_node_text(&name) == alias)
                && initializer_node(declaration).is_some_and(|value| is_this_type(base, value))
        });
    if is_alias {
        container_type_name(base, container)
    } else {
        None
    }
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

fn file_root(mut node: Node) -> Node {
    while let Some(parent) = node.parent() {
        node = parent;
    }
    node
}

fn same_file_container(base: &BaseExtractor, root: Node, name: &str) -> bool {
    find_container_declaration(root, name, base, 0)
}

fn find_container_declaration(node: Node, name: &str, base: &BaseExtractor, depth: u32) -> bool {
    if !should_visit_tree_depth(depth) {
        return false;
    }
    if node.kind() == "variable_declaration" {
        let mut cursor = node.walk();
        let ident = node
            .named_children(&mut cursor)
            .find(|child| child.kind() == "identifier");
        if let Some(ident) = ident
            && base.get_node_text(&ident) == name
        {
            let mut kind_cursor = node.walk();
            if node.named_children(&mut kind_cursor).any(|child| {
                matches!(
                    child.kind(),
                    "struct_declaration"
                        | "union_declaration"
                        | "enum_declaration"
                        | "opaque_declaration"
                )
            }) {
                return true;
            }
        }
    }
    let Some(child_depth) = child_tree_depth(depth) else {
        return false;
    };
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if find_container_declaration(child, name, base, child_depth) {
            return true;
        }
    }
    false
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
