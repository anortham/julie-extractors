//! Inferred types for untyped `let` and `use` bindings whose initializer
//! calls a same-file function or member with a declared return type.

use super::identifiers::{enclosing_type_name, instance_receiver_type};
use super::parameters::member_return_type;
use super::types::{
    direct_child, direct_type_child, direct_type_child_after, first_identifier,
    structural_base_name, terminal_identifier,
};
use crate::base::BaseExtractor;
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};
use std::collections::{HashMap, HashSet};
use std::ops::Range;
use tree_sitter::Node;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CallableKind {
    Function,
    Value,
    InstanceMember,
    StaticMember,
}

#[derive(Debug)]
struct Callable<'t> {
    kind: CallableKind,
    /// The module that qualifies a module-level binding, or the type that
    /// owns a member. `None` for local and class-level bindings.
    container: Option<String>,
    /// Curried argument groups: `let f a (b, c)` has two.
    groups: usize,
    return_type: Option<Node<'t>>,
    /// Where an unqualified call can see a `let` binding: from the binding to
    /// the end of its enclosing module, type, or expression.
    scope: Range<usize>,
}

/// Declared return types of the file's `let` functions and members, by name,
/// plus every name a pattern binds anywhere in the file (parameters, lambda
/// and match variables, loop variables), since any of them may shadow a
/// function at the call site.
#[derive(Debug)]
pub(super) struct ReturnTypeIndex<'t> {
    callables: HashMap<String, Vec<Callable<'t>>>,
    shadows: HashSet<String>,
}

impl<'t> ReturnTypeIndex<'t> {
    pub(super) fn build(base: &BaseExtractor, root: Node<'t>) -> Self {
        let mut callables: HashMap<String, Vec<Callable<'t>>> = HashMap::new();
        let mut shadows = HashSet::new();
        let mut stack = vec![root];
        while let Some(node) = stack.pop() {
            let entry = match node.kind() {
                "function_or_value_defn" => binding_callable(base, node),
                "member_defn" => member_callable(base, node),
                "identifier" if is_pattern_binder(node) => {
                    shadows.insert(base.get_node_text(&node).trim().to_string());
                    None
                }
                _ => None,
            };
            if let Some((name, callable)) = entry {
                callables.entry(name).or_default().push(callable);
            }
            stack.extend(node.named_children(&mut node.walk()));
        }
        Self { callables, shadows }
    }

    /// The base name and written type a binding's initializer produces when
    /// it fully applies a same-file callable with a declared return type.
    /// `let!` and `use!` unwrap one `Async` layer inside `async { }`, and one
    /// `Task`, `ValueTask`, or `Async` layer inside `task { }` or
    /// `backgroundTask { }`; any other builder records nothing.
    pub(super) fn initializer_type(
        &self,
        base: &BaseExtractor,
        keyword: Node<'t>,
        value: Node<'t>,
    ) -> Option<(String, String)> {
        let (head, groups) = callee(base, value, 0, 0)?;
        let mut type_node = self.return_type(base, head, groups)?;
        if base.get_node_text(&keyword).ends_with('!') {
            type_node = computation_result(base, keyword, type_node)?;
        }
        let name = structural_base_name(base, type_node)?;
        if name == "_" || name.starts_with(['\'', '^']) {
            return None;
        }
        Some((name, base.get_node_text(&type_node).trim().to_string()))
    }

    fn return_type(&self, base: &BaseExtractor, head: Node<'t>, groups: usize) -> Option<Node<'t>> {
        let text = base.get_node_text(&head);
        let segments: Vec<&str> = text.split('.').map(str::trim).collect();
        match segments.as_slice() {
            [name] if !self.shadows.contains(*name) => {
                self.unanimous(base, name, groups, |callable| {
                    matches!(callable.kind, CallableKind::Function | CallableKind::Value)
                        && callable.scope.contains(&head.start_byte())
                })
            }
            [receiver, name] => {
                if let Some(owner) = terminal_identifier(head)
                    .and_then(|method| instance_receiver_type(base, method))
                {
                    return self.unanimous(base, name, groups, |callable| {
                        callable.kind == CallableKind::InstanceMember
                            && callable.container.as_deref() == Some(owner.as_str())
                    });
                }
                self.unanimous(base, name, groups, |callable| {
                    callable.kind != CallableKind::InstanceMember
                        && callable.container.as_deref() == Some(*receiver)
                })
            }
            _ => None,
        }
    }

    /// The return type every accepted same-named callable agrees on; each
    /// must take exactly `groups` argument groups.
    fn unanimous(
        &self,
        base: &BaseExtractor,
        name: &str,
        groups: usize,
        accepts: impl Fn(&Callable) -> bool,
    ) -> Option<Node<'t>> {
        let declared = |callable: &Callable<'t>| {
            callable
                .return_type
                .filter(|_| callable.groups == groups && callable.kind != CallableKind::Value)
                .map(|type_node| base.get_node_text(&type_node).trim().to_string())
        };
        let mut candidates = self.callables.get(name)?.iter().filter(|c| accepts(c));
        let first = candidates.next()?;
        let first_declared = declared(first)?;
        candidates
            .all(|callable| declared(callable).as_ref() == Some(&first_declared))
            .then_some(first.return_type)
            .flatten()
    }
}

fn binding_callable<'t>(base: &BaseExtractor, node: Node<'t>) -> Option<(String, Callable<'t>)> {
    let container = module_container(base, node);
    let scope = node.start_byte()..scope_end(node);
    if let Some(left) = direct_child(node, "function_declaration_left") {
        let name = base.get_node_text(&direct_child(left, "identifier")?);
        let groups = direct_child(left, "argument_patterns").map_or(0, |patterns| {
            patterns
                .named_children(&mut patterns.walk())
                .filter(|pattern| !pattern.is_extra())
                .count()
        });
        let callable = Callable {
            kind: CallableKind::Function,
            container,
            groups,
            return_type: direct_type_child_after(node, left),
            scope,
        };
        return Some((name.trim().to_string(), callable));
    }
    let name = first_identifier(direct_child(node, "value_declaration_left")?)?;
    let callable = Callable {
        kind: CallableKind::Value,
        container,
        groups: 0,
        return_type: None,
        scope,
    };
    Some((base.get_node_text(&name).trim().to_string(), callable))
}

fn member_callable<'t>(base: &BaseExtractor, node: Node<'t>) -> Option<(String, Callable<'t>)> {
    let definition = direct_child(node, "method_or_prop_defn")?;
    let name = definition.child_by_field_name("name")?;
    let kind = if direct_child(node, "static").is_some() {
        CallableKind::StaticMember
    } else if name.child_by_field_name("instance").is_some() {
        CallableKind::InstanceMember
    } else {
        return None;
    };
    let method = base.get_node_text(&terminal_identifier(name)?);
    let callable = Callable {
        kind,
        container: enclosing_type_name(base, node),
        groups: definition
            .children_by_field_name("args", &mut definition.walk())
            .count(),
        return_type: member_return_type(definition).or_else(|| direct_type_child(definition)),
        scope: 0..usize::MAX,
    };
    Some((method.trim().to_string(), callable))
}

/// Whether `identifier` is a name a pattern, loop, or `use` binds, as opposed
/// to a union case qualifier or a record field label.
fn is_pattern_binder(identifier: Node) -> bool {
    let mut parent = identifier.parent();
    while let Some(node) =
        parent.filter(|node| matches!(node.kind(), "long_identifier" | "long_identifier_or_op"))
    {
        parent = node.parent();
    }
    parent.is_some_and(|node| {
        let kind = node.kind();
        (kind.ends_with("_pattern") && kind != "field_pattern")
            || matches!(
                kind,
                "argument_patterns" | "for_expression" | "declaration_expression"
            )
    })
}

/// The end of the module, type, or `let ... in` expression that encloses a
/// binding. Module `let`s sit in their own `declaration_expression` with no
/// `in` continuation, and class `let`s in their own `type_extension_elements`,
/// so those wrappers are skipped.
fn scope_end(binding: Node) -> usize {
    let mut current = binding.parent();
    while let Some(node) = current {
        let wrapper = match node.kind() {
            "declaration_expression" => node.child_by_field_name("in").is_none(),
            "type_extension_elements" => true,
            _ => false,
        };
        if !wrapper {
            return node.end_byte();
        }
        current = node.parent();
    }
    usize::MAX
}

/// The module whose name qualifies a module-level binding (`M.load ()`).
fn module_container(base: &BaseExtractor, binding: Node) -> Option<String> {
    let mut current = binding.parent();
    while let Some(ancestor) = current.filter(|node| node.kind() == "declaration_expression") {
        current = ancestor.parent();
    }
    let module = current?;
    let name = match module.kind() {
        "module_defn" => direct_child(module, "identifier")?,
        "named_module" => direct_child(module, "long_identifier")?,
        _ => return None,
    };
    let text = base.get_node_text(&name);
    Some(text.rsplit('.').next()?.trim().to_string())
}

/// The callee of an application and how many curried arguments it receives.
/// `x |> f a` applies `f` to `a` and then `x`.
fn callee<'t>(
    base: &BaseExtractor,
    node: Node<'t>,
    applied: usize,
    depth: u32,
) -> Option<(Node<'t>, usize)> {
    if !should_visit_tree_depth(depth) {
        return None;
    }
    let child_depth = child_tree_depth(depth)?;
    match node.kind() {
        "long_identifier_or_op" => Some((node, applied)),
        "application_expression" => {
            let mut parts = node
                .named_children(&mut node.walk())
                .filter(|part| !part.is_extra())
                .collect::<Vec<_>>()
                .into_iter();
            let head = parts.next()?;
            callee(base, head, applied + parts.len(), child_depth)
        }
        "infix_expression" => {
            let operator = direct_child(node, "infix_op")?;
            if base.get_node_text(&operator).trim() != "|>" {
                return None;
            }
            let function = node.named_children(&mut node.walk()).last()?;
            callee(base, function, applied + 1, child_depth)
        }
        "paren_expression" | "typed_expression" => {
            callee(base, node.named_child(0)?, applied, child_depth)
        }
        _ => None,
    }
}

/// The bound value of a `let!`/`use!` whose right side has `type_node`.
fn computation_result<'t>(
    base: &BaseExtractor,
    keyword: Node<'t>,
    type_node: Node<'t>,
) -> Option<Node<'t>> {
    let mut current = keyword.parent();
    let expression = loop {
        let node = current?;
        if node.kind() == "ce_expression" {
            break node;
        }
        current = node.parent();
    };
    let builder = base.get_node_text(&expression.named_child(0)?);
    let wrappers: &[&str] = match builder.trim() {
        "async" => &["Async"],
        "task" | "backgroundTask" => &["Task", "ValueTask", "Async"],
        _ => return None,
    };
    let (wrapper, argument) = match type_node.kind() {
        "generic_type" => {
            let arguments = direct_child(type_node, "type_attributes")?;
            let attributes: Vec<Node> = arguments.named_children(&mut arguments.walk()).collect();
            let [only] = attributes[..] else {
                return None;
            };
            (
                direct_child(type_node, "long_identifier")?,
                only.named_child(0)?,
            )
        }
        "postfix_type" => (
            type_node.named_children(&mut type_node.walk()).last()?,
            type_node.named_child(0)?,
        ),
        _ => return None,
    };
    let wrapper = base.get_node_text(&wrapper);
    let wrapper = wrapper.rsplit('.').next()?.trim();
    wrappers.contains(&wrapper).then_some(argument)
}
