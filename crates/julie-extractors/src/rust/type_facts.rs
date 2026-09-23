use super::helpers::{associated_item_owner, extract_impl_target_names};
use crate::base::BaseExtractor;
use crate::base::types::TypeNameRules;
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};
use std::collections::HashMap;
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
/// methods unwrap one `Result`/`Option` layer; `map_err`, `ok_or*`, and
/// `*context` keep it. Any other method ends the chain with no fact.
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
/// text, and the type arguments of a generic type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct TypeShape {
    name: Option<String>,
    declared: String,
    args: Vec<TypeShape>,
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
}

/// Declared return types of the file's functions and impl methods, by name.
/// Trait methods are left out: their `Self` is not a concrete type.
#[derive(Debug, Default)]
pub(super) struct ReturnTypeIndex(HashMap<String, Vec<ReturnEntry>>);

#[derive(Debug)]
struct ReturnEntry {
    /// The implemented type of the enclosing impl block; `None` for a free function.
    owner: Option<String>,
    /// `None` when the function declares no return type.
    shape: Option<TypeShape>,
}

impl ReturnTypeIndex {
    pub(super) fn build(base: &BaseExtractor, root: Node) -> Self {
        let mut entries: HashMap<String, Vec<ReturnEntry>> = HashMap::new();
        let mut stack = vec![root];
        while let Some(node) = stack.pop() {
            if node.kind() == "function_item"
                && let Some((name, entry)) = return_entry(base, node)
            {
                entries.entry(name).or_default().push(entry);
            }
            stack.extend(node.named_children(&mut node.walk()));
        }
        Self(entries)
    }

    /// The return type every same-named function with this owner agrees on.
    fn lookup(&self, name: &str, owner: Option<&str>) -> Option<TypeShape> {
        let mut shapes = self
            .0
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
        self.0.get(name).is_some_and(|entries| {
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
        .and_then(|return_type| type_shape(base, return_type, &generics, owner.as_deref(), 0));
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
    })
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
            "struct_expression" => Some(self.named_type(value.child_by_field_name("name")?)),
            "call_expression" => {
                let function = value.child_by_field_name("function")?;
                let function = if function.kind() == "generic_function" {
                    function.child_by_field_name("function")?
                } else {
                    function
                };
                match function.kind() {
                    "identifier" => self
                        .return_types
                        .lookup(&self.base.get_node_text(&function), None),
                    "scoped_identifier" => self.associated_call(function),
                    "field_expression" => self.method_call(function, child_tree_depth(depth)?),
                    _ => None,
                }
            }
            _ => None,
        }
    }

    fn associated_call(&self, function: Node) -> Option<TypeShape> {
        let owner = self.named_type(function.child_by_field_name("path")?);
        let owner_name = owner.name.as_deref()?;
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
            "map_err" | "ok_or" | "ok_or_else" | "context" | "with_context" => self
                .shape_of(receiver, receiver_depth)
                .filter(TypeShape::is_wrapper),
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
        }
    }
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
