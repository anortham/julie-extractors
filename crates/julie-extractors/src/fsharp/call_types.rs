//! Inferred types for untyped `let` and `use` bindings whose initializer
//! calls a same-file function or member with a declared return type.

use super::identifiers::{enclosing_type_body, instance_receiver_type};
use super::parameters::member_return_type;
use super::types::{
    direct_child, direct_type_child, direct_type_child_after, first_identifier,
    structural_base_name, terminal_identifier,
};
use crate::base::BaseExtractor;
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};
use std::collections::HashMap;
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
    /// The module node that holds a module-level binding, or the type body
    /// that declares a member. `None` for local and class-level bindings.
    owner: Option<Node<'t>>,
    /// Curried argument groups: `let f a (b, c)` has two.
    groups: usize,
    return_type: Option<Node<'t>>,
    /// Where an unqualified call can see a `let` binding: from the end of the
    /// binding (its start for `let rec`) to the end of its enclosing module,
    /// type, or expression.
    scope: Range<usize>,
    /// Where a call qualified by the owner's name can see the callable: from
    /// the binding or the owning type definition to the end of the module or
    /// namespace that holds the owner.
    qualified: Range<usize>,
}

/// Members every .NET type inherits from `obj`; a same-file member with one
/// of these names may lose overload resolution to the inherited one.
const OBJECT_MEMBERS: &[&str] = &[
    "Equals",
    "Finalize",
    "GetHashCode",
    "GetType",
    "MemberwiseClone",
    "ReferenceEquals",
    "ToString",
];

const TYPE_BODIES: &[&str] = &[
    "anon_type_defn",
    "delegate_type_defn",
    "enum_type_defn",
    "interface_type_defn",
    "record_type_defn",
    "type_abbrev_defn",
    "union_type_defn",
];

/// Declared return types of the file's `let` functions and members, by name,
/// plus what can hide them at a call site: where a pattern binds each name
/// (parameters, lambda and match variables, loop variables), the same-file
/// modules and types that can serve as a qualifier, and the `open`
/// declarations and `[<AutoOpen>]` modules that bring unknown names in.
#[derive(Debug)]
pub(super) struct ReturnTypeIndex<'t> {
    callables: HashMap<String, Vec<Callable<'t>>>,
    shadows: HashMap<String, Vec<usize>>,
    /// Module and type definition nodes by the name a qualified call uses.
    declarations: HashMap<String, Vec<Node<'t>>>,
    /// From each `open` or `[<AutoOpen>]` module to the end of the module,
    /// namespace, or file that holds it.
    opens: Vec<Range<usize>>,
}

impl<'t> ReturnTypeIndex<'t> {
    pub(super) fn build(base: &BaseExtractor, root: Node<'t>) -> Self {
        let mut callables: HashMap<String, Vec<Callable<'t>>> = HashMap::new();
        let mut shadows: HashMap<String, Vec<usize>> = HashMap::new();
        let mut declarations: HashMap<String, Vec<Node<'t>>> = HashMap::new();
        let mut opens = Vec::new();
        let mut stack = vec![root];
        while let Some(node) = stack.pop() {
            if node.kind() == "import_decl" || is_auto_open(base, node) {
                opens.push(node.start_byte()..container_end(node));
            }
            if let Some(name) = declared_name(base, node) {
                declarations.entry(name).or_default().push(node);
            }
            let entry = match node.kind() {
                "function_or_value_defn" => binding_callable(base, node),
                "member_defn" => member_callable(base, node),
                "identifier" if is_pattern_binder(node) => {
                    shadows
                        .entry(base.get_node_text(&node).trim().to_string())
                        .or_default()
                        .push(node.start_byte());
                    None
                }
                _ => None,
            };
            if let Some((name, callable)) = entry {
                callables.entry(name).or_default().push(callable);
            }
            stack.extend(node.named_children(&mut node.walk()));
        }
        Self {
            callables,
            shadows,
            declarations,
            opens,
        }
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
        if is_flexible(base, type_node) {
            return None;
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
        let call = head.start_byte();
        match segments.as_slice() {
            [name] if !self.shadows.contains_key(*name) => {
                self.unanimous(base, name, groups, |callable| {
                    matches!(callable.kind, CallableKind::Function | CallableKind::Value)
                        && callable.scope.contains(&call)
                        && !self.opened_between(callable.scope.start, call)
                })
            }
            [_, name] if OBJECT_MEMBERS.contains(name) => None,
            [receiver, name] => {
                if terminal_identifier(head)
                    .and_then(|method| instance_receiver_type(base, method))
                    .is_some()
                {
                    let member = ancestor(head, "member_defn")?;
                    if self.binds_within(receiver, member.byte_range()) {
                        return None;
                    }
                    let body = enclosing_type_body(head)?;
                    if body.kind() == "type_extension" || inherits(body) {
                        return None;
                    }
                    return self.unanimous(base, name, groups, |callable| {
                        callable.kind == CallableKind::InstanceMember
                            && callable.owner == Some(body)
                    });
                }
                if self.shadows.contains_key(*receiver) {
                    return None;
                }
                let declaration = self.nearest_declaration(receiver, call)?;
                if inherits(declaration) || self.opened_between(declaration.start_byte(), call) {
                    return None;
                }
                self.unanimous(base, name, groups, |callable| {
                    callable.kind != CallableKind::InstanceMember
                        && callable.owner == Some(declaration)
                        && callable.qualified.contains(&call)
                })
            }
            _ => None,
        }
    }

    /// The same-file module or type that `qualifier` names at `call`: the
    /// visible declaration that starts last, since a nearer one hides the
    /// outer ones. `None` when a module and a type with that name are both
    /// visible, or when no same-file declaration is.
    fn nearest_declaration(&self, qualifier: &str, call: usize) -> Option<Node<'t>> {
        let visible: Vec<Node<'t>> = self
            .declarations
            .get(qualifier)?
            .iter()
            .copied()
            .filter(|declaration| declaration_range(*declaration).contains(&call))
            .collect();
        let nearest = *visible.iter().max_by_key(|node| node.start_byte())?;
        visible
            .iter()
            .all(|node| is_module(*node) == is_module(nearest))
            .then_some(nearest)
    }

    /// Whether an `open` or `[<AutoOpen>]` module after `definition` is in
    /// scope at `call`; the names it brings in may hide the definition.
    fn opened_between(&self, definition: usize, call: usize) -> bool {
        self.opens
            .iter()
            .any(|open| definition < open.start && open.contains(&call))
    }

    /// Whether a pattern inside `range` rebinds `name`, which hides a member's
    /// self identifier from that point on.
    fn binds_within(&self, name: &str, range: Range<usize>) -> bool {
        self.shadows
            .get(name)
            .is_some_and(|starts| starts.iter().any(|start| range.contains(start)))
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
    let visible_from = if direct_child(node, "rec").is_some() {
        node.start_byte()
    } else {
        node.end_byte()
    };
    let scope = visible_from..scope_end(node);
    let (owner, qualified) = match module_container(node) {
        Some(module) => (Some(module), visible_from..container_end(module)),
        None => (None, 0..0),
    };
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
            owner,
            groups,
            return_type: direct_type_child_after(node, left),
            scope,
            qualified,
        };
        return Some((name.trim().to_string(), callable));
    }
    let name = first_identifier(direct_child(node, "value_declaration_left")?)?;
    let callable = Callable {
        kind: CallableKind::Value,
        owner,
        groups: 0,
        return_type: None,
        scope,
        qualified,
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
    if ancestor(node, "interface_implementation").is_some() {
        return None;
    }
    let method = base.get_node_text(&terminal_identifier(name)?);
    let callable = Callable {
        kind,
        owner: enclosing_type_body(node),
        groups: definition
            .children_by_field_name("args", &mut definition.walk())
            .count(),
        return_type: member_return_type(definition).or_else(|| direct_type_child(definition)),
        scope: 0..0,
        qualified: ancestor(node, "type_definition").map_or(0..0, |type_definition| {
            type_definition.start_byte()..container_end(type_definition)
        }),
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
fn module_container(binding: Node) -> Option<Node> {
    let mut current = binding.parent();
    while let Some(ancestor) = current.filter(|node| node.kind() == "declaration_expression") {
        current = ancestor.parent();
    }
    current.filter(|module| is_module(*module))
}

/// The name a qualified call uses for a module, module abbreviation, or type
/// definition node.
fn declared_name(base: &BaseExtractor, node: Node) -> Option<String> {
    let name = match node.kind() {
        "module_defn" => direct_child(node, "identifier")?,
        "named_module" => direct_child(node, "long_identifier")?,
        kind if TYPE_BODIES.contains(&kind) => {
            direct_child(node, "type_name")?.child_by_field_name("type_name")?
        }
        _ => return None,
    };
    let text = base.get_node_text(&name);
    Some(text.rsplit('.').next()?.trim().to_string())
}

/// Where a qualified call can name a module or type: from its definition (the
/// whole `type ... and ...` group for a type) to the end of the module,
/// namespace, or file that holds it.
fn declaration_range(declaration: Node) -> Range<usize> {
    let definition = if is_module(declaration) {
        declaration
    } else {
        declaration.parent().unwrap_or(declaration)
    };
    definition.start_byte()..container_end(definition)
}

fn is_module(node: Node) -> bool {
    matches!(node.kind(), "module_defn" | "named_module")
}

fn is_auto_open(base: &BaseExtractor, node: Node) -> bool {
    node.kind() == "module_defn"
        && direct_child(node, "attributes").is_some_and(|attributes| {
            attributes
                .named_children(&mut attributes.walk())
                .any(|attribute| {
                    let text = base.get_node_text(&attribute);
                    let name = text.split('(').next().unwrap_or_default();
                    matches!(
                        name.rsplit('.').next().map(str::trim),
                        Some("AutoOpen" | "AutoOpenAttribute")
                    )
                })
        })
}

/// Whether a type body has an `inherit` clause; members of the base type,
/// possibly in another file, then take part in overload resolution.
fn inherits(body: Node) -> bool {
    direct_child(body, "class_inherits_decl").is_some()
}

/// A flexible type (`#seq<int>`) is a hidden type parameter the caller fixes.
fn is_flexible(base: &BaseExtractor, type_node: Node) -> bool {
    type_node.kind() == "flexible_type"
        || base
            .get_node_text(&type_node)
            .trim_start_matches(['(', ' '])
            .starts_with('#')
}

/// The end of the module, namespace, or file that holds a module or type
/// definition; its name is not visible past that point without an `open`.
fn container_end(definition: Node) -> usize {
    definition
        .parent()
        .map_or(definition.end_byte(), |parent| parent.end_byte())
}

fn ancestor<'t>(node: Node<'t>, kind: &str) -> Option<Node<'t>> {
    let mut current = node.parent();
    while let Some(candidate) = current {
        if candidate.kind() == kind {
            return Some(candidate);
        }
        current = candidate.parent();
    }
    None
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
