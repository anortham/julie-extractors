use super::helpers::{associated_item_owner, extract_impl_target_names, has_async_keyword};
use crate::base::BaseExtractor;
use crate::base::types::TypeNameRules;
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};
use std::collections::{HashMap, HashSet};
use tree_sitter::Node;

pub(super) const RUST_TYPE_NAME_RULES: TypeNameRules = TypeNameRules {
    nullable_suffixes: &[],
    reference_prefixes: &["&", "*const", "*mut", "*", "mut", "dyn", "impl"],
    generic_open: &['<'],
};

/// Record a syntactically stated type for a symbol (`is_inferred=false`).
pub(super) fn record_declared_type(base: &mut BaseExtractor, symbol_id: &str, type_node: Node) {
    record_type_node(base, symbol_id, type_node, false);
}

/// Record the type a `let` initializer produces (`is_inferred=true`): a
/// struct expression, a call to a same-file function or method with a
/// declared return type, or `Type::new(..)` / `Self::new(..)` when the file
/// declares no such `new`. `?`, `unwrap`, `expect`, and the `unwrap_or*`
/// methods unwrap one `Result`/`Option` layer; `map_err` keeps it, and
/// `ok_or*` and `*context` turn it into a `Result`. An `async fn` call needs
/// `.await` before its output type applies. Any other method ends the chain
/// with no fact.
pub(super) fn record_initializer_type(
    base: &mut BaseExtractor,
    symbol_id: &str,
    value: Node,
    return_types: &ReturnTypeIndex,
    impl_type: Option<&str>,
) {
    let scope = InitializerScope {
        base,
        return_types,
        impl_type,
    };
    let Some(TypeShape {
        name: Some(name),
        declared,
        ..
    }) = scope.shape_of(value, 0)
    else {
        return;
    };
    base.record_declared_type_fact_with_declared(
        symbol_id,
        &name,
        &declared,
        &RUST_TYPE_NAME_RULES,
        true,
    );
}

/// A type reduced to what initializer inference needs: the bindable base
/// name (`None` for generic parameters and shapes without one), the written
/// text, and the type arguments of a generic type. A future holds the
/// output of an `async fn` as its only argument.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct TypeShape {
    name: Option<String>,
    declared: String,
    args: Vec<TypeShape>,
    future: bool,
}

impl TypeShape {
    fn is_wrapper(&self) -> bool {
        matches!(self.name.as_deref(), Some("Result" | "Option")) && !self.args.is_empty()
    }

    fn unwrapped(self) -> Option<TypeShape> {
        if self.is_wrapper() {
            self.args.into_iter().next()
        } else {
            None
        }
    }

    fn awaited(self) -> Option<TypeShape> {
        if self.future {
            self.args.into_iter().next()
        } else {
            None
        }
    }

    fn into_result(self) -> Option<TypeShape> {
        self.is_wrapper().then(|| TypeShape {
            name: Some("Result".to_string()),
            declared: "Result".to_string(),
            args: self.args,
            future: false,
        })
    }

    fn future_of(output: TypeShape, declared: String) -> TypeShape {
        TypeShape {
            name: None,
            declared,
            args: vec![output],
            future: true,
        }
    }
}

/// Declared return types of the file's functions and impl methods, by name.
/// Trait methods are left out: their `Self` is not a concrete type. Names
/// that a binding, import, or value item may take over are kept per
/// outermost function and per file, so a bare call through one of them
/// resolves to nothing.
#[derive(Debug, Default)]
pub(super) struct ReturnTypeIndex {
    entries: HashMap<String, Vec<ReturnEntry>>,
    function_names: HashMap<usize, HashSet<String>>,
    file_names: HashSet<String>,
}

#[derive(Debug)]
struct ReturnEntry {
    /// The implemented type of the enclosing impl block; `None` for a free function.
    owner: Option<String>,
    /// `None` when the function declares no return type.
    shape: Option<TypeShape>,
}

impl ReturnTypeIndex {
    pub(super) fn build(base: &BaseExtractor, root: Node) -> Self {
        let mut index = Self::default();
        let mut stack = vec![(root, None, false)];
        while let Some((node, outer_function, in_use)) = stack.pop() {
            let outer_function = match node.kind() {
                "function_item" => {
                    if let Some((name, entry)) = return_entry(base, node) {
                        index.entries.entry(name).or_default().push(entry);
                    }
                    outer_function.or(Some(node.start_byte()))
                }
                _ => outer_function,
            };
            let in_use = in_use || node.kind() == "use_declaration";
            if node.kind() == "identifier" {
                index.note_binding_name(base, node, outer_function, in_use);
            } else if matches!(node.kind(), "static_item" | "const_item")
                && let Some(name) = node.child_by_field_name("name")
            {
                index.file_names.insert(base.get_node_text(&name));
            }
            stack.extend(
                node.named_children(&mut node.walk())
                    .map(|child| (child, outer_function, in_use)),
            );
        }
        index
    }

    fn note_binding_name(
        &mut self,
        base: &BaseExtractor,
        identifier: Node,
        outer_function: Option<usize>,
        in_use: bool,
    ) {
        if in_use {
            self.file_names.insert(base.get_node_text(&identifier));
            return;
        }
        let Some(outer_function) = outer_function else {
            return;
        };
        let names_a_callee_or_item = identifier.parent().is_some_and(|parent| {
            let field = |name| parent.child_by_field_name(name) == Some(identifier);
            match parent.kind() {
                "call_expression" | "generic_function" => field("function"),
                "scoped_identifier" => field("name"),
                "function_item" => field("name"),
                "macro_invocation" => field("macro"),
                _ => false,
            }
        });
        if !names_a_callee_or_item {
            self.function_names
                .entry(outer_function)
                .or_default()
                .insert(base.get_node_text(&identifier));
        }
    }

    /// True when `name` at `call` may bind to something other than a
    /// same-file function: a local, parameter, pattern, import, or value item.
    fn may_rebind(&self, call: Node, name: &str) -> bool {
        if self.file_names.contains(name) {
            return true;
        }
        let mut outer_function = None;
        let mut node = call;
        while let Some(parent) = node.parent() {
            if parent.kind() == "function_item" {
                outer_function = Some(parent.start_byte());
            }
            node = parent;
        }
        outer_function
            .and_then(|start| self.function_names.get(&start))
            .is_some_and(|names| names.contains(name))
    }

    /// The return type every same-named function with this owner agrees on.
    fn lookup(&self, name: &str, owner: Option<&str>) -> Option<TypeShape> {
        let mut shapes = self
            .entries
            .get(name)?
            .iter()
            .filter(|entry| entry.owner.as_deref() == owner)
            .map(|entry| entry.shape.as_ref());
        let first = shapes.next()??;
        shapes
            .all(|shape| shape == Some(first))
            .then(|| first.clone())
    }

    fn declares(&self, name: &str, owner: &str) -> bool {
        self.entries.get(name).is_some_and(|entries| {
            entries
                .iter()
                .any(|entry| entry.owner.as_deref() == Some(owner))
        })
    }
}

fn return_entry(base: &BaseExtractor, function: Node) -> Option<(String, ReturnEntry)> {
    let name = base.get_node_text(&function.child_by_field_name("name")?);
    let mut generics = type_parameter_names(base, function);
    let owner = match associated_item_owner(function) {
        None => None,
        Some(owner) if owner.kind() == "impl_item" => {
            generics.extend(type_parameter_names(base, owner));
            Some(extract_impl_target_names(base, owner).type_name?)
        }
        Some(_) => return None,
    };
    let shape = function
        .child_by_field_name("return_type")
        .and_then(|return_type| type_shape(base, return_type, &generics, owner.as_deref(), 0))
        .map(|output| {
            if has_async_keyword(base, function) {
                let declared = format!("impl Future<Output = {}>", output.declared);
                TypeShape::future_of(output, declared)
            } else {
                output
            }
        });
    Some((name, ReturnEntry { owner, shape }))
}

fn type_parameter_names(base: &BaseExtractor, item: Node) -> Vec<String> {
    let Some(parameters) = item.child_by_field_name("type_parameters") else {
        return Vec::new();
    };
    parameters
        .named_children(&mut parameters.walk())
        .filter(|parameter| parameter.kind() == "type_parameter")
        .filter_map(|parameter| parameter.child_by_field_name("name"))
        .map(|name| base.get_node_text(&name))
        .collect()
}

fn type_shape(
    base: &BaseExtractor,
    node: Node,
    generics: &[String],
    self_type: Option<&str>,
    depth: u32,
) -> Option<TypeShape> {
    if !should_visit_tree_depth(depth) {
        return None;
    }
    let name = base_type_name_node(node)
        .filter(|name| !has_opaque_path_root(base, *name, generics))
        .map(|name| base.get_node_text(&name))
        .and_then(|name| match name.as_str() {
            "Self" => self_type.map(str::to_string),
            _ if generics.contains(&name) => None,
            _ => Some(name),
        });
    let arguments = node
        .child_by_field_name("type_arguments")
        .filter(|_| node.kind() == "generic_type");
    let args = match (arguments, child_tree_depth(depth)) {
        (Some(arguments), Some(child_depth)) => arguments
            .named_children(&mut arguments.walk())
            .filter(|argument| argument.kind() != "lifetime")
            .map(|argument| type_shape(base, argument, generics, self_type, child_depth))
            .collect::<Option<_>>()?,
        _ => Vec::new(),
    };
    Some(TypeShape {
        name,
        declared: base.get_node_text(&node),
        args,
        future: false,
    })
}

/// True when a path-qualified type name hangs off a type parameter, `Self`,
/// or a qualified `<T as Trait>` root, so its concrete type is unknown.
fn has_opaque_path_root(base: &BaseExtractor, name: Node, generics: &[String]) -> bool {
    let Some(parent) = name.parent().filter(|parent| {
        matches!(
            parent.kind(),
            "scoped_type_identifier" | "scoped_identifier"
        )
    }) else {
        return false;
    };
    let mut path = parent.child_by_field_name("path");
    while let Some(segment) = path {
        path = match segment.kind() {
            "scoped_type_identifier" | "scoped_identifier" => segment.child_by_field_name("path"),
            "generic_type" => segment.child_by_field_name("type"),
            "bracketed_type" => return true,
            "identifier" | "type_identifier" => {
                let root = base.get_node_text(&segment);
                return root == "Self" || generics.contains(&root);
            }
            _ => return false,
        };
    }
    false
}

struct InitializerScope<'a> {
    base: &'a BaseExtractor,
    return_types: &'a ReturnTypeIndex,
    impl_type: Option<&'a str>,
}

impl InitializerScope<'_> {
    fn shape_of(&self, value: Node, depth: u32) -> Option<TypeShape> {
        if !should_visit_tree_depth(depth) {
            return None;
        }
        match value.kind() {
            "try_expression" => self
                .shape_of(value.named_child(0)?, child_tree_depth(depth)?)?
                .unwrapped(),
            "await_expression" => self
                .shape_of(value.named_child(0)?, child_tree_depth(depth)?)?
                .awaited(),
            "struct_expression" => Some(self.named_type(value.child_by_field_name("name")?)),
            "call_expression" => {
                let function = value.child_by_field_name("function")?;
                let function = if function.kind() == "generic_function" {
                    function.child_by_field_name("function")?
                } else {
                    function
                };
                match function.kind() {
                    "identifier" => {
                        let name = self.base.get_node_text(&function);
                        if self.return_types.may_rebind(function, &name) {
                            None
                        } else {
                            self.return_types.lookup(&name, None)
                        }
                    }
                    "scoped_identifier" => self.associated_call(function),
                    "field_expression" => self.method_call(function, child_tree_depth(depth)?),
                    _ => None,
                }
            }
            _ => None,
        }
    }

    fn associated_call(&self, function: Node) -> Option<TypeShape> {
        let path = function.child_by_field_name("path")?;
        let owner = self.named_type(path);
        let owner_name = owner.name.as_deref()?;
        if matches!(path.kind(), "identifier" | "type_identifier")
            && names_enclosing_type_parameter(self.base, function, owner_name)
        {
            return None;
        }
        let name = self
            .base
            .get_node_text(&function.child_by_field_name("name")?);
        if self.return_types.declares(&name, owner_name) {
            self.return_types.lookup(&name, Some(owner_name))
        } else {
            (name == "new").then_some(owner)
        }
    }

    fn method_call(&self, function: Node, receiver_depth: u32) -> Option<TypeShape> {
        let receiver = function.child_by_field_name("value")?;
        let method = self
            .base
            .get_node_text(&function.child_by_field_name("field")?);
        match method.as_str() {
            "unwrap" | "expect" | "unwrap_or" | "unwrap_or_else" | "unwrap_or_default" => {
                self.shape_of(receiver, receiver_depth)?.unwrapped()
            }
            "map_err" => self
                .shape_of(receiver, receiver_depth)
                .filter(TypeShape::is_wrapper),
            "ok_or" | "ok_or_else" | "context" | "with_context" => {
                self.shape_of(receiver, receiver_depth)?.into_result()
            }
            _ if receiver.kind() == "self" => {
                self.return_types.lookup(&method, Some(self.impl_type?))
            }
            _ => None,
        }
    }

    /// A type named by a path or struct-expression name; `Self` is the impl type.
    fn named_type(&self, node: Node) -> TypeShape {
        let name = base_type_name_node(node)
            .map(|name| self.base.get_node_text(&name))
            .and_then(|name| match name.as_str() {
                "Self" => self.impl_type.map(str::to_string),
                _ => Some(name),
            });
        TypeShape {
            name,
            declared: self.base.get_node_text(&node),
            args: Vec::new(),
            future: false,
        }
    }
}

/// True when `name` is a type parameter of a function, impl, or trait that
/// encloses `node`.
fn names_enclosing_type_parameter(base: &BaseExtractor, node: Node, name: &str) -> bool {
    let mut current = node;
    while let Some(parent) = current.parent() {
        if matches!(parent.kind(), "function_item" | "impl_item" | "trait_item")
            && type_parameter_names(base, parent)
                .iter()
                .any(|parameter| parameter == name)
        {
            return true;
        }
        current = parent;
    }
    false
}

/// Record the impl target type for a `self` parameter (`is_inferred=false`).
pub(super) fn record_impl_self_type(
    base: &mut BaseExtractor,
    symbol_id: &str,
    impl_type_name: &str,
) {
    base.record_declared_type_fact(symbol_id, impl_type_name, &RUST_TYPE_NAME_RULES, false);
}

fn record_type_node(base: &mut BaseExtractor, symbol_id: &str, type_node: Node, is_inferred: bool) {
    let Some(name_node) = base_type_name_node(type_node) else {
        return;
    };
    let base_name = base.get_node_text(&name_node);
    let declared = base.get_node_text(&type_node);
    base.record_declared_type_fact_with_declared(
        symbol_id,
        &base_name,
        &declared,
        &RUST_TYPE_NAME_RULES,
        is_inferred,
    );
}

/// Structurally reduce a type-position node to the single node naming its base
/// type: the final path segment, with generics, turbofish, reference, pointer,
/// `dyn`, and `impl` wrappers dropped. Shapes without one base name (tuples,
/// arrays, function types) yield nothing.
pub(super) fn base_type_name_node(node: Node) -> Option<Node> {
    let mut node = node;
    loop {
        match node.kind() {
            "type_identifier" | "identifier" | "primitive_type" => return Some(node),
            "scoped_type_identifier" | "scoped_identifier" => {
                return node.child_by_field_name("name");
            }
            "generic_type" | "generic_type_with_turbofish" | "reference_type" | "pointer_type" => {
                node = node.child_by_field_name("type")?;
            }
            "dynamic_type" | "abstract_type" => {
                node = node.child_by_field_name("trait")?;
            }
            _ => return None,
        }
    }
}

/// Record a function's declared return type (`is_inferred=false`). `Self`
/// resolves to the implemented type inside an impl block; the declared text
/// stays in `metadata.declared`.
pub(super) fn record_return_type(
    base: &mut BaseExtractor,
    symbol_id: &str,
    function: Node,
    impl_type_name: Option<&str>,
) {
    let Some(return_type) = function.child_by_field_name("return_type") else {
        return;
    };
    let Some(name_node) = base_type_name_node(return_type) else {
        return;
    };
    let base_name = base.get_node_text(&name_node);
    let base_name = match impl_type_name {
        Some(impl_type_name) if base_name == "Self" => impl_type_name.to_string(),
        _ => base_name,
    };
    let declared = base.get_node_text(&return_type);
    base.record_declared_type_fact_with_declared(
        symbol_id,
        &base_name,
        &declared,
        &RUST_TYPE_NAME_RULES,
        false,
    );
}
