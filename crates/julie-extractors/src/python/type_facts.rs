/// Declared-type fact recording for receiver-typed call resolution.
/// Records the one plainly named type an annotation binds: a bare identifier,
/// a plain dotted name, or a subscript whose base is one of those. Wrappers
/// that do not change the receiver type (`Optional[X]`, `X | None`,
/// `Annotated[X, ...]`, `ClassVar[X]`, `Final[X]`, `Mapped[X]`) and string
/// forward references (`"X"`) unwrap to `X`. Other unions and inline
/// callables record nothing.
use super::{PythonExtractor, helpers, signatures};
use crate::base::BaseExtractor;
use crate::base::types::{TypeNameRules, strip_type_decorations};
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};
use std::collections::{HashMap, HashSet};
use tree_sitter::Node;

pub(super) const PYTHON_TYPE_NAME_RULES: TypeNameRules = TypeNameRules {
    nullable_suffixes: &[],
    reference_prefixes: &[],
    generic_open: &['['],
};

const TRANSPARENT_WRAPPERS: &[&str] = &["Optional", "Annotated", "ClassVar", "Final", "Mapped"];

/// Record a syntactically stated annotation for a symbol (`is_inferred=false`).
pub(super) fn record_annotation_fact(base: &mut BaseExtractor, symbol_id: &str, type_node: Node) {
    if let Some(named) = plainly_named_annotation(base, type_node) {
        let declared = base.get_node_text(&type_node);
        base.record_declared_type_fact_with_declared(
            symbol_id,
            &named,
            &declared,
            &PYTHON_TYPE_NAME_RULES,
            false,
        );
    }
}

/// Record the type an unannotated assignment's value produces
/// (`is_inferred=true`): `Foo()` for a same-file class `Foo`, or a call to a
/// same-file function (`load()`), a method through `self`/`cls`, a method
/// through a same-file class name (`Foo.create()`), or a method on such a
/// call's result, when every same-named candidate declares the same return
/// type. An `async def` needs `await`; a plain `def` must not be awaited.
/// Type-variable returns, `Self` outside a class, and callees behind any
/// decorator other than [`TRANSPARENT_DECORATORS`] record nothing.
pub(super) fn record_initializer_fact(
    extractor: &mut PythonExtractor,
    symbol_id: &str,
    value: Node,
) {
    if let Some(inferred) = value_type(extractor, value, 0) {
        extractor.base.record_declared_type_fact_with_declared(
            symbol_id,
            &inferred.name,
            &inferred.declared,
            &PYTHON_TYPE_NAME_RULES,
            true,
        );
    }
}

/// Decorators that return the function they wrap, keeping its return type.
const TRANSPARENT_DECORATORS: &[&str] = &[
    "staticmethod",
    "classmethod",
    "abstractmethod",
    "overload",
    "override",
    "final",
    "cache",
    "lru_cache",
];

const TYPE_VARIABLE_FACTORIES: &[&str] = &["TypeVar", "ParamSpec", "TypeVarTuple"];

/// The type a value produces: its base name and its written return type.
#[derive(Debug, Clone, PartialEq, Eq)]
struct InferredType {
    name: String,
    declared: String,
}

/// What a call to a same-file function produces: `returns` is `None` when
/// the function has no usable declared return type.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Callee {
    is_async: bool,
    returns: Option<InferredType>,
}

impl Callee {
    fn result(&self, awaited: bool) -> Option<InferredType> {
        (self.is_async == awaited)
            .then(|| self.returns.clone())
            .flatten()
    }
}

/// Declared return types of the file's functions, by name and owning class
/// (`None` for module-level and nested functions).
#[derive(Debug, Default)]
pub(crate) struct ReturnTypeIndex(HashMap<(String, Option<String>), Vec<Callee>>);

impl ReturnTypeIndex {
    pub(super) fn build(base: &BaseExtractor, root: Node) -> Self {
        let mut functions = Vec::new();
        let mut type_variables = HashSet::new();
        let mut stack = vec![root];
        while let Some(node) = stack.pop() {
            match node.kind() {
                "function_definition" => functions.push(node),
                "assignment" => {
                    if let Some(name) = type_variable_name(base, node) {
                        type_variables.insert(name);
                    }
                }
                _ => {}
            }
            stack.extend(node.named_children(&mut node.walk()));
        }
        let mut entries: HashMap<_, Vec<Callee>> = HashMap::new();
        for function in functions {
            let Some(name) = function.child_by_field_name("name") else {
                continue;
            };
            let owner = owning_class(function);
            let owner_name = owner
                .and_then(|class| class.child_by_field_name("name"))
                .map(|name| base.get_node_text(&name));
            let callee = Callee {
                is_async: signatures::has_async_keyword(&function),
                returns: has_only_transparent_decorators(base, function)
                    .then(|| {
                        declared_return(
                            base,
                            function,
                            owner,
                            owner_name.as_deref(),
                            &type_variables,
                        )
                    })
                    .flatten(),
            };
            entries
                .entry((base.get_node_text(&name), owner_name))
                .or_default()
                .push(callee);
        }
        Self(entries)
    }

    /// The callee every same-named function with this owner agrees on.
    fn lookup(&self, name: &str, owner: Option<&str>) -> Option<&Callee> {
        let mut callees = self
            .0
            .get(&(name.to_string(), owner.map(str::to_string)))?
            .iter();
        let first = callees.next()?;
        callees.all(|callee| callee == first).then_some(first)
    }
}

fn type_variable_name(base: &BaseExtractor, assignment: Node) -> Option<String> {
    let left = assignment
        .child_by_field_name("left")
        .filter(|left| left.kind() == "identifier")?;
    let factory = assignment
        .child_by_field_name("right")
        .filter(|right| right.kind() == "call")?
        .child_by_field_name("function")?;
    let factory = base.get_node_text(&factory);
    let factory = factory.rsplit('.').next().unwrap_or(&factory);
    TYPE_VARIABLE_FACTORIES
        .contains(&factory)
        .then(|| base.get_node_text(&left))
}

/// The class whose body defines `function` directly; `None` for module-level
/// and nested functions.
fn owning_class(function: Node) -> Option<Node> {
    let mut current = function.parent()?;
    loop {
        match current.kind() {
            "decorated_definition" | "block" => current = current.parent()?,
            "class_definition" => return Some(current),
            _ => return None,
        }
    }
}

fn declared_return(
    base: &BaseExtractor,
    function: Node,
    owner: Option<Node>,
    owner_name: Option<&str>,
    type_variables: &HashSet<String>,
) -> Option<InferredType> {
    let return_type = function.child_by_field_name("return_type")?;
    let named = plainly_named_annotation(base, return_type)?;
    let name = strip_type_decorations(&named, &PYTHON_TYPE_NAME_RULES);
    let name = match name.rsplit('.').next() {
        Some("Self") => owner_name?.to_string(),
        _ if type_variables.contains(&name) => return None,
        _ if type_parameter_names(base, function).contains(&name) => return None,
        _ if owner.is_some_and(|class| class_type_parameter_names(base, class).contains(&name)) => {
            return None;
        }
        _ => name,
    };
    Some(InferredType {
        name,
        declared: base.get_node_text(&return_type),
    })
}

fn has_only_transparent_decorators(base: &BaseExtractor, function: Node) -> bool {
    let Some(decorated) = function
        .parent()
        .filter(|parent| parent.kind() == "decorated_definition")
    else {
        return true;
    };
    decorated
        .named_children(&mut decorated.walk())
        .filter(|child| child.kind() == "decorator")
        .all(|decorator| {
            let Some(expression) = decorator.named_child(0) else {
                return false;
            };
            let callee = if expression.kind() == "call" {
                expression.child_by_field_name("function")
            } else {
                Some(expression)
            };
            callee.is_some_and(|callee| {
                let text = base.get_node_text(&callee);
                TRANSPARENT_DECORATORS.contains(&text.rsplit('.').next().unwrap_or(&text))
            })
        })
}

/// PEP 695 type parameter names declared on a function or class.
fn type_parameter_names(base: &BaseExtractor, definition: Node) -> Vec<String> {
    let Some(parameters) = definition.child_by_field_name("type_parameters") else {
        return Vec::new();
    };
    parameters
        .named_children(&mut parameters.walk())
        .filter_map(|parameter| {
            let text = base.get_node_text(&parameter);
            let name = text
                .trim_start_matches('*')
                .split(|c: char| !(c.is_alphanumeric() || c == '_'))
                .next()?;
            (!name.is_empty()).then(|| name.to_string())
        })
        .collect()
}

/// A class's PEP 695 type parameters plus every name used as a type
/// argument of its bases (`Generic[T]`, `Base[T]`), which may be an imported
/// type variable.
fn class_type_parameter_names(base: &BaseExtractor, class: Node) -> Vec<String> {
    let mut names = type_parameter_names(base, class);
    let Some(superclasses) = class.child_by_field_name("superclasses") else {
        return names;
    };
    let mut stack: Vec<(Node, bool)> = vec![(superclasses, false)];
    while let Some((node, in_arguments)) = stack.pop() {
        if in_arguments && node.kind() == "identifier" {
            names.push(base.get_node_text(&node));
        }
        let mut cursor = node.walk();
        for (index, child) in node.children(&mut cursor).enumerate() {
            let is_argument = node.kind() == "subscript"
                && node.field_name_for_child(index as u32) == Some("subscript");
            if child.is_named() {
                stack.push((child, in_arguments || is_argument));
            }
        }
    }
    names
}

fn value_type(extractor: &PythonExtractor, value: Node, depth: u32) -> Option<InferredType> {
    if !should_visit_tree_depth(depth) {
        return None;
    }
    let child_depth = child_tree_depth(depth)?;
    match value.kind() {
        "parenthesized_expression" => value_type(extractor, value.named_child(0)?, child_depth),
        "await" => call_result(extractor, value.named_child(0)?, true, child_depth),
        "call" => call_result(extractor, value, false, child_depth),
        _ => None,
    }
}

fn call_result(
    extractor: &PythonExtractor,
    call: Node,
    awaited: bool,
    depth: u32,
) -> Option<InferredType> {
    if call.kind() != "call" {
        return None;
    }
    let base = &extractor.base;
    let function = call.child_by_field_name("function")?;
    let (name, owner) = match function.kind() {
        "identifier" => {
            let name = base.get_node_text(&function);
            if extractor.same_file_class_names.contains(&name) {
                return (!awaited).then(|| InferredType {
                    name: name.clone(),
                    declared: name,
                });
            }
            (name, None)
        }
        "attribute" => {
            let object = function.child_by_field_name("object")?;
            let owner = if object.kind() == "identifier" {
                let receiver = base.get_node_text(&object);
                match receiver.as_str() {
                    "self" | "cls" => helpers::enclosing_class_name(base, &function)?,
                    _ if extractor.same_file_class_names.contains(&receiver) => receiver,
                    _ => return None,
                }
            } else {
                value_type(extractor, object, depth)?.name
            };
            let method = base.get_node_text(&function.child_by_field_name("attribute")?);
            (method, Some(owner))
        }
        _ => return None,
    };
    extractor
        .return_types
        .lookup(&name, owner.as_deref())?
        .result(awaited)
}

fn plainly_named_annotation(base: &BaseExtractor, node: Node) -> Option<String> {
    plainly_named_annotation_at(base, node, 0)
}

fn plainly_named_annotation_at(base: &BaseExtractor, node: Node, depth: u32) -> Option<String> {
    let child_depth = crate::tree_traversal::child_tree_depth(depth)?;
    match node.kind() {
        "type" => plainly_named_annotation_at(base, node.named_child(0)?, child_depth),
        "identifier" | "none" => Some(base.get_node_text(&node)),
        "attribute" | "member_type" => is_plain_name(node).then(|| base.get_node_text(&node)),
        "string" => forward_reference(base, node),
        "generic_type" => {
            let mut cursor = node.walk();
            let head = node.named_children(&mut cursor).next()?;
            let arguments = type_arguments(node);
            named_or_unwrapped(base, node, head, &arguments, child_depth)
        }
        "subscript" => {
            let value = node.child_by_field_name("value")?;
            if !is_plain_name(value) {
                return None;
            }
            let mut cursor = node.walk();
            let arguments: Vec<Node> = node
                .children_by_field_name("subscript", &mut cursor)
                .collect();
            named_or_unwrapped(base, node, value, &arguments, child_depth)
        }
        "binary_operator" | "union_type" => {
            let mut members = Vec::new();
            collect_union_members(node, &mut members);
            let mut non_none = members.into_iter().filter(|member| !is_none(*member));
            let only = non_none.next()?;
            non_none
                .next()
                .is_none()
                .then(|| plainly_named_annotation_at(base, only, child_depth))?
        }
        _ => None,
    }
}

fn type_arguments(generic: Node) -> Vec<Node> {
    let mut cursor = generic.walk();
    generic
        .named_children(&mut cursor)
        .find(|child| child.kind() == "type_parameter")
        .map(|parameters| {
            let mut inner = parameters.walk();
            parameters.named_children(&mut inner).collect()
        })
        .unwrap_or_default()
}

/// The inner name of a wrapper (`Optional`, `Union`, ...) or the whole
/// subscripted text for any other head; a wrapper that does not reduce to one
/// plain name records nothing.
fn named_or_unwrapped(
    base: &BaseExtractor,
    node: Node,
    head: Node,
    arguments: &[Node],
    depth: u32,
) -> Option<String> {
    let head_text = base.get_node_text(&head);
    let wrapper = head_text.rsplit('.').next().unwrap_or(&head_text);
    if wrapper == "Union" {
        let mut non_none = arguments.iter().filter(|argument| !is_none(**argument));
        let only = non_none.next()?;
        return non_none
            .next()
            .is_none()
            .then(|| plainly_named_annotation_at(base, *only, depth))?;
    }
    if !TRANSPARENT_WRAPPERS.contains(&wrapper) {
        return Some(base.get_node_text(&node));
    }
    plainly_named_annotation_at(base, *arguments.first()?, depth)
}

fn collect_union_members<'a>(node: Node<'a>, members: &mut Vec<Node<'a>>) {
    collect_union_members_at(node, members, 0);
}

fn collect_union_members_at<'a>(node: Node<'a>, members: &mut Vec<Node<'a>>, depth: u32) {
    let Some(child_depth) = crate::tree_traversal::child_tree_depth(depth) else {
        members.push(node);
        return;
    };
    let is_pipe = node
        .child_by_field_name("operator")
        .is_none_or(|operator| operator.kind() == "|");
    if matches!(node.kind(), "binary_operator" | "union_type") && is_pipe {
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            collect_union_members_at(child, members, child_depth);
        }
    } else if node.kind() == "type" && node.named_child_count() == 1 {
        collect_union_members_at(node.named_child(0).unwrap_or(node), members, child_depth);
    } else {
        members.push(node);
    }
}

fn is_none(node: Node) -> bool {
    match node.kind() {
        "none" => true,
        "type" => node.named_child(0).is_some_and(is_none),
        _ => false,
    }
}

fn forward_reference(base: &BaseExtractor, node: Node) -> Option<String> {
    let mut cursor = node.walk();
    let content = node
        .named_children(&mut cursor)
        .find(|child| child.kind() == "string_content")?;
    let text = base.get_node_text(&content);
    let text = text.trim();
    let is_dotted_name = !text.is_empty()
        && text.split('.').all(|segment| {
            segment
                .chars()
                .next()
                .is_some_and(|c| c.is_alphabetic() || c == '_')
                && segment.chars().all(|c| c.is_alphanumeric() || c == '_')
        });
    is_dotted_name.then(|| text.to_string())
}

fn is_plain_name(node: Node) -> bool {
    match node.kind() {
        "type" | "member_type" => node.named_child(0).is_some_and(is_plain_name),
        "identifier" => true,
        "attribute" => node
            .child_by_field_name("object")
            .is_some_and(is_plain_name),
        _ => false,
    }
}
