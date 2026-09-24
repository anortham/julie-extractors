//! Declared-type fact recording for Kotlin.

use super::helpers;
use crate::base::types::{TypeNameRules, strip_type_decorations};
use crate::base::{BaseExtractor, Symbol};
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};
use std::collections::{HashMap, HashSet};
use tree_sitter::Node;

pub(super) const KOTLIN_TYPE_NAME_RULES: TypeNameRules = TypeNameRules {
    nullable_suffixes: &["?"],
    reference_prefixes: &[],
    generic_open: &['<'],
};

const DECLARED_TYPE_METADATA_KEYS: [&str; 3] = ["returnType", "propertyType", "dataType"];

/// Base type names for symbols whose metadata carries declared type text.
/// Shapes with no single base name (function types, backticked names with
/// spaces) record nothing.
pub(super) fn metadata_base_types(symbols: &[Symbol]) -> HashMap<String, String> {
    symbols
        .iter()
        .filter_map(|symbol| {
            let declared = declared_type_metadata(symbol)?;
            Some((symbol.id.clone(), base_type_name_from_text(declared)?))
        })
        .collect()
}

fn declared_type_metadata(symbol: &Symbol) -> Option<&str> {
    let metadata = symbol.metadata.as_ref()?;
    DECLARED_TYPE_METADATA_KEYS
        .iter()
        .find_map(|key| metadata.get(*key).and_then(serde_json::Value::as_str))
}

fn base_type_name_from_text(declared: &str) -> Option<String> {
    let stripped = strip_type_decorations(declared, &KOTLIN_TYPE_NAME_RULES);
    let segments: Vec<&str> = stripped.split('.').map(helpers::strip_backticks).collect();
    let is_qualified_name =
        !stripped.is_empty() && segments.iter().all(|segment| is_type_name_segment(segment));
    is_qualified_name.then(|| segments.join("."))
}

fn is_type_name_segment(segment: &str) -> bool {
    let mut chars = segment.chars();
    chars
        .next()
        .is_some_and(|first| first.is_alphabetic() || first == '_')
        && chars.all(|c| c.is_alphanumeric() || c == '_')
}

pub(super) fn record_declared_type(base: &mut BaseExtractor, symbol_id: &str, type_node: Node) {
    record_type_node(base, symbol_id, type_node, false);
}

/// Record a property's written type, else the type its initializer produces
/// (`is_inferred=true`) as [`InitializerIndex::shape_of`] reads it.
pub(super) fn record_property_facts(
    base: &mut BaseExtractor,
    symbol_id: &str,
    node: Node,
    index: &InitializerIndex,
) {
    if let Some(type_node) = property_type_node(node) {
        record_declared_type(base, symbol_id, type_node);
        return;
    }
    let Some(shape) = property_initializer_node(base, node)
        .and_then(|initializer| index.shape_of(base, initializer, 0))
    else {
        return;
    };
    base.record_declared_type_fact_with_declared(
        symbol_id,
        &shape.name,
        &shape.declared,
        &KOTLIN_TYPE_NAME_RULES,
        true,
    );
}

pub(super) fn declared_type_child(node: Node) -> Option<Node> {
    let mut cursor = node.walk();
    node.children(&mut cursor).find(|child| {
        matches!(
            child.kind(),
            "user_type" | "type" | "nullable_type" | "type_reference" | "function_type"
        )
    })
}

/// A type reduced to what initializer inference needs: its base name and its
/// written text.
#[derive(Debug, Clone, PartialEq, Eq)]
struct TypeShape {
    name: String,
    declared: String,
}

#[derive(Debug)]
struct ReturnEntry {
    /// Node id of the source file, class body, or block that declares the function.
    container: usize,
    /// A local function is reachable only after its declaration.
    local: bool,
    start_byte: usize,
    required_args: usize,
    /// `None` when a `vararg` parameter takes any number of arguments.
    max_args: Option<usize>,
    /// `None` for no declared return type, a type parameter, or a shape with
    /// no single base name.
    shape: Option<TypeShape>,
}

impl ReturnEntry {
    fn accepts(&self, arg_count: usize) -> bool {
        arg_count >= self.required_args && self.max_args.is_none_or(|max| arg_count <= max)
    }
}

/// An object or companion object that `TypeName.call()` reaches.
#[derive(Debug)]
struct StaticOwner {
    /// Node id of the node that declares the type; the name is visible below it.
    declared_in: usize,
    body: usize,
}

/// Same-file facts that initializer inference reads, built once per file:
/// class and object names, function return types, the objects and companions
/// each type name reaches, and the names that explicit imports bring in.
#[derive(Debug, Default)]
pub(super) struct InitializerIndex {
    type_names: HashSet<String>,
    functions: HashMap<String, Vec<ReturnEntry>>,
    static_owners: HashMap<String, Vec<StaticOwner>>,
    imported: HashSet<String>,
}

impl InitializerIndex {
    pub(super) fn build(base: &BaseExtractor, root: Node) -> Self {
        let mut index = Self::default();
        let mut stack = vec![root];
        while let Some(node) = stack.pop() {
            match node.kind() {
                "class_declaration" | "object_declaration" => index.add_type(base, node),
                "function_declaration" => {
                    if let Some((name, entry)) = return_entry(base, node) {
                        index.functions.entry(name).or_default().push(entry);
                    }
                }
                "import" => index.imported.extend(imported_name(base, node)),
                _ => {}
            }
            stack.extend(node.named_children(&mut node.walk()));
        }
        index
    }

    fn add_type(&mut self, base: &BaseExtractor, node: Node) {
        let Some((name, _)) = helpers::declared_name(base, &node) else {
            return;
        };
        let owners = if node.kind() == "object_declaration" {
            vec![node]
        } else {
            companion_objects(node)
        };
        if let Some(declared_in) = node.parent() {
            let owners = owners
                .into_iter()
                .filter_map(body_id)
                .map(|body| StaticOwner {
                    declared_in: declared_in.id(),
                    body,
                });
            self.static_owners
                .entry(name.clone())
                .or_default()
                .extend(owners);
        }
        self.type_names.insert(name);
    }

    /// The type an initializer produces: a same-file class constructor, or a
    /// call with a declared return type to a same-file function reached by a
    /// bare name, `this.`, or an object or companion through its type name.
    /// Parentheses pass the type through and `!!` drops nullability. Every
    /// same-named candidate the call reaches must agree, and at least one must
    /// accept the argument count.
    fn shape_of(&self, base: &BaseExtractor, value: Node, depth: u32) -> Option<TypeShape> {
        if !should_visit_tree_depth(depth) {
            return None;
        }
        match value.kind() {
            "parenthesized_expression" => {
                self.shape_of(base, value.named_child(0)?, child_tree_depth(depth)?)
            }
            "unary_expression"
                if value
                    .children(&mut value.walk())
                    .last()
                    .is_some_and(|operator| operator.kind() == "!!") =>
            {
                let shape = self.shape_of(base, value.named_child(0)?, child_tree_depth(depth)?)?;
                let declared = shape.declared.strip_suffix('?').unwrap_or(&shape.declared);
                Some(TypeShape {
                    declared: declared.to_string(),
                    name: shape.name,
                })
            }
            "call_expression" => self.call_shape(base, value),
            _ => None,
        }
    }

    fn call_shape(&self, base: &BaseExtractor, call: Node) -> Option<TypeShape> {
        let (callee, arg_count) = callee_and_arg_count(call)?;
        match callee.kind() {
            "identifier" => {
                let name = identifier_text(base, callee);
                if self.type_names.contains(&name) {
                    return Some(TypeShape {
                        declared: name.clone(),
                        name,
                    });
                }
                if self.imported.contains(&name) {
                    return None;
                }
                let containers = bare_call_containers(base, call)?;
                self.agreed(&name, arg_count, |entry| {
                    containers.contains(&entry.container)
                        && (!entry.local || entry.start_byte < call.start_byte())
                })
            }
            "navigation_expression" => {
                let parts: Vec<Node> = callee.named_children(&mut callee.walk()).collect();
                let [receiver, member] = parts.as_slice() else {
                    return None;
                };
                if member.kind() != "identifier" {
                    return None;
                }
                let name = identifier_text(base, *member);
                match receiver.kind() {
                    "this_expression" if receiver.named_child_count() == 0 => {
                        let body = this_body(base, call)?;
                        self.agreed(&name, arg_count, |entry| entry.container == body)
                    }
                    "identifier" => self.static_call(base, *receiver, call, &name, arg_count),
                    _ => None,
                }
            }
            _ => None,
        }
    }

    fn static_call(
        &self,
        base: &BaseExtractor,
        receiver: Node,
        call: Node,
        name: &str,
        arg_count: usize,
    ) -> Option<TypeShape> {
        let type_name = identifier_text(base, receiver);
        if self.imported.contains(&type_name) {
            return None;
        }
        let ancestors: HashSet<usize> = std::iter::successors(call.parent(), Node::parent)
            .map(|node| node.id())
            .collect();
        let bodies: Vec<usize> = self
            .static_owners
            .get(&type_name)?
            .iter()
            .filter(|owner| ancestors.contains(&owner.declared_in))
            .map(|owner| owner.body)
            .collect();
        self.agreed(name, arg_count, |entry| bodies.contains(&entry.container))
    }

    fn agreed(
        &self,
        name: &str,
        arg_count: usize,
        reaches: impl Fn(&ReturnEntry) -> bool,
    ) -> Option<TypeShape> {
        let candidates: Vec<&ReturnEntry> = self
            .functions
            .get(name)?
            .iter()
            .filter(|entry| reaches(entry))
            .collect();
        if !candidates.iter().any(|entry| entry.accepts(arg_count)) {
            return None;
        }
        let first = candidates.first()?.shape.as_ref()?;
        candidates
            .iter()
            .all(|entry| entry.shape.as_ref() == Some(first))
            .then(|| first.clone())
    }
}

const TYPE_SCOPE_KINDS: [&str; 5] = [
    "class_declaration",
    "object_declaration",
    "companion_object",
    "object_literal",
    "enum_entry",
];

/// The containers whose functions a bare call reaches: its enclosing blocks,
/// class bodies and their companions, and the file, stopping after the first
/// type with a supertype, whose inherited members hide outer functions.
/// `None` inside a lambda or an extension, whose implicit receiver may
/// declare a function of the same name.
fn bare_call_containers(base: &BaseExtractor, call: Node) -> Option<HashSet<usize>> {
    let mut containers = HashSet::new();
    for node in std::iter::successors(call.parent(), Node::parent) {
        if changes_implicit_receiver(base, node) {
            return None;
        }
        containers.insert(node.id());
        if node.kind() == "class_declaration" {
            containers.extend(companion_objects(node).into_iter().filter_map(body_id));
        }
        if TYPE_SCOPE_KINDS.contains(&node.kind()) && has_supertype(node) {
            break;
        }
    }
    Some(containers)
}

/// The class body `this` names at `call`; `None` inside a lambda or an
/// extension, where `this` may name another receiver.
fn this_body(base: &BaseExtractor, call: Node) -> Option<usize> {
    for node in std::iter::successors(call.parent(), Node::parent) {
        if changes_implicit_receiver(base, node) {
            return None;
        }
        if TYPE_SCOPE_KINDS.contains(&node.kind()) {
            return body_id(node);
        }
    }
    None
}

fn changes_implicit_receiver(base: &BaseExtractor, node: Node) -> bool {
    match node.kind() {
        "lambda_literal" | "anonymous_function" => true,
        "function_declaration" | "property_declaration" => {
            helpers::extract_receiver_type(base, &node).is_some()
        }
        _ => false,
    }
}

fn has_supertype(node: Node) -> bool {
    node.kind() == "enum_entry"
        || node
            .children(&mut node.walk())
            .any(|child| child.kind() == "delegation_specifiers")
}

fn companion_objects(class: Node) -> Vec<Node> {
    class
        .children(&mut class.walk())
        .filter(|child| child.kind() == "class_body")
        .flat_map(|body| {
            body.children(&mut body.walk())
                .filter(|member| member.kind() == "companion_object")
                .collect::<Vec<_>>()
        })
        .collect()
}

fn body_id(node: Node) -> Option<usize> {
    node.children(&mut node.walk())
        .find(|child| matches!(child.kind(), "class_body" | "enum_class_body"))
        .map(|body| body.id())
}

fn identifier_text(base: &BaseExtractor, node: Node) -> String {
    helpers::strip_backticks(&base.get_node_text(&node)).to_string()
}

/// The callee of a call and its argument count. A trailing lambda counts as
/// an argument; `f(a) { }` parses as a call that wraps the call `f(a)`.
fn callee_and_arg_count(call: Node) -> Option<(Node, usize)> {
    let (callee, count, has_parentheses) = call_parts(call)?;
    if callee.kind() != "call_expression" {
        return Some((callee, count));
    }
    if has_parentheses {
        return None;
    }
    let (inner, inner_count, inner_parentheses) = call_parts(callee)?;
    (inner_parentheses && inner.kind() != "call_expression").then_some((inner, inner_count + count))
}

fn call_parts(call: Node) -> Option<(Node, usize, bool)> {
    let mut cursor = call.walk();
    let mut children = call.named_children(&mut cursor);
    let callee = children.next()?;
    let mut count = 0;
    let mut has_parentheses = false;
    for child in children {
        match child.kind() {
            "value_arguments" => {
                has_parentheses = true;
                count += child
                    .named_children(&mut child.walk())
                    .filter(|argument| argument.kind() == "value_argument")
                    .count();
            }
            "annotated_lambda" => count += 1,
            _ => {}
        }
    }
    Some((callee, count, has_parentheses))
}

fn return_entry(base: &BaseExtractor, function: Node) -> Option<(String, ReturnEntry)> {
    let (name, _) = helpers::declared_name(base, &function)?;
    let container = function.parent()?;
    let generics = visible_type_parameters(base, function);
    let (required_args, max_args) = parameter_arity(base, function);
    let shape = helpers::return_type_node(function).and_then(|return_type| {
        let name = base_type_name(base, return_type)?;
        (!generics.contains(&name)).then(|| TypeShape {
            name,
            declared: base.get_node_text(&return_type),
        })
    });
    let entry = ReturnEntry {
        container: container.id(),
        local: !matches!(
            container.kind(),
            "source_file" | "class_body" | "enum_class_body"
        ),
        start_byte: function.start_byte(),
        required_args,
        max_args,
        shape,
    };
    Some((name, entry))
}

/// Type parameter names of the function and of every enclosing function and class.
fn visible_type_parameters(base: &BaseExtractor, function: Node) -> Vec<String> {
    std::iter::successors(Some(function), Node::parent)
        .filter(|node| matches!(node.kind(), "function_declaration" | "class_declaration"))
        .filter_map(|node| {
            node.children(&mut node.walk())
                .find(|child| child.kind() == "type_parameters")
        })
        .flat_map(|parameters| {
            parameters
                .named_children(&mut parameters.walk())
                .filter(|parameter| parameter.kind() == "type_parameter")
                .filter_map(|parameter| helpers::declared_name(base, &parameter))
                .map(|(name, _)| name)
                .collect::<Vec<_>>()
        })
        .collect()
}

/// The required and maximum argument counts; a parameter with a default is
/// optional and a `vararg` parameter lifts the maximum.
fn parameter_arity(base: &BaseExtractor, function: Node) -> (usize, Option<usize>) {
    let Some(parameters) = function
        .children(&mut function.walk())
        .find(|child| child.kind() == "function_value_parameters")
    else {
        return (0, Some(0));
    };
    let children: Vec<Node> = parameters.children(&mut parameters.walk()).collect();
    let mut required = 0;
    let mut total = 0;
    let mut has_vararg = false;
    let mut vararg_pending = false;
    for (position, child) in children.iter().enumerate() {
        match child.kind() {
            "parameter_modifiers" => {
                vararg_pending = base
                    .get_node_text(child)
                    .split_whitespace()
                    .any(|modifier| modifier == "vararg");
            }
            "parameter" => {
                total += 1;
                let has_default = children
                    .get(position + 1)
                    .is_some_and(|next| next.kind() == "=");
                if vararg_pending {
                    has_vararg = true;
                } else if !has_default {
                    required += 1;
                }
                vararg_pending = false;
            }
            _ => {}
        }
    }
    (required, (!has_vararg).then_some(total))
}

/// The simple name or alias an explicit, non-star import brings in.
fn imported_name(base: &BaseExtractor, import: Node) -> Option<String> {
    let children: Vec<Node> = import.children(&mut import.walk()).collect();
    if children.iter().any(|child| child.kind() == "*") {
        return None;
    }
    let name = match children.iter().find(|child| child.kind() == "identifier") {
        Some(alias) => *alias,
        None => {
            let path = children
                .iter()
                .find(|child| child.kind() == "qualified_identifier")?;
            path.named_children(&mut path.walk()).last()?
        }
    };
    Some(identifier_text(base, name))
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
        &KOTLIN_TYPE_NAME_RULES,
        is_inferred,
    );
}

fn base_type_name(base: &BaseExtractor, node: Node) -> Option<String> {
    let core = unwrap_type_wrappers(node)?;
    match core.kind() {
        "user_type" => user_type_base_name(base, core),
        "identifier" | "simple_identifier" => {
            Some(helpers::strip_backticks(&base.get_node_text(&core)).to_string())
        }
        _ => None,
    }
}

fn unwrap_type_wrappers(node: Node) -> Option<Node> {
    let mut current = node;
    for _ in 0..8 {
        match current.kind() {
            "type" | "type_reference" | "nullable_type" | "parenthesized_type"
            | "non_nullable_type" => {
                let mut cursor = current.walk();
                current = current.named_children(&mut cursor).next()?;
            }
            _ => return Some(current),
        }
    }
    Some(current)
}

fn user_type_base_name(base: &BaseExtractor, node: Node) -> Option<String> {
    let children: Vec<Node> = {
        let mut cursor = node.walk();
        node.children(&mut cursor).collect()
    };
    let identifiers: Vec<String> = children
        .iter()
        .filter(|child| child.kind() == "identifier" || child.kind() == "simple_identifier")
        .map(|child| helpers::strip_backticks(&base.get_node_text(child)).to_string())
        .collect();
    if identifiers.is_empty() {
        None
    } else {
        Some(identifiers.join("."))
    }
}

fn property_type_node(node: Node) -> Option<Node> {
    let var_decl = {
        let mut cursor = node.walk();
        node.children(&mut cursor)
            .find(|child| child.kind() == "variable_declaration")
    };
    if let Some(var_decl) = var_decl
        && let Some(type_node) = declared_type_child(var_decl)
    {
        return Some(type_node);
    }
    declared_type_child(node)
}

fn property_initializer_node<'a>(base: &BaseExtractor, node: Node<'a>) -> Option<Node<'a>> {
    let children: Vec<Node<'a>> = {
        let mut cursor = node.walk();
        node.children(&mut cursor).collect()
    };
    let assignment_index = children
        .iter()
        .position(|child| base.get_node_text(child) == "=")?;
    children.get(assignment_index + 1).copied()
}
