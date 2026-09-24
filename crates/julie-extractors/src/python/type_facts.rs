/// Declared-type fact recording for receiver-typed call resolution.
/// Records the one plainly named type an annotation binds: a bare identifier,
/// a plain dotted name, or a subscript whose base is one of those. Wrappers
/// that do not change the receiver type (`Optional[X]`, `X | None`,
/// `Annotated[X, ...]`, `ClassVar[X]`, `Final[X]`, `Mapped[X]`) and string
/// forward references (`"X"`) unwrap to `X`. Other unions and inline
/// callables record nothing.
use super::{PythonExtractor, signatures};
use crate::base::BaseExtractor;
use crate::base::types::{TypeNameRules, strip_type_decorations};
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};
use std::collections::hash_map::Entry;
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

/// Decorators that return the function they wrap, keeping its return type,
/// when they are builtins or come from [`DECORATOR_MODULES`].
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

const DECORATOR_MODULES: &[&str] = &[
    "builtins",
    "functools",
    "typing",
    "typing_extensions",
    "abc",
];

const TYPE_VARIABLE_FACTORIES: &[&str] = &["TypeVar", "ParamSpec", "TypeVarTuple"];

/// Returns whose runtime value is not the named type: `TypeGuard[X]` and
/// `TypeIs[X]` return a `bool`, `Literal["a"]` a value of the literal's type.
const NON_TYPE_RETURNS: &[&str] = &["TypeGuard", "TypeIs", "Literal"];

/// The type a value produces: its base name, its written return type, and
/// the same-file class definition (by node id) the name refers to, if known.
#[derive(Debug, Clone, PartialEq, Eq)]
struct InferredType {
    name: String,
    declared: String,
    class: Option<usize>,
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

/// Where a function name is bound: the module, a class body (a method, by
/// class node id), or the function whose body defines it (by node id).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum Scope {
    Module,
    Class(usize),
    Function(usize),
}

/// How one scope binds a name: only by `def`s, only by `class`es (their node
/// ids), only by one import (its qualified name, `functools.cache`), only as
/// a parameter, or by anything else (an assignment, loop or `with` target,
/// match capture, ...).
#[derive(Debug, Clone, PartialEq, Eq)]
enum Binding {
    Functions,
    Classes(Vec<usize>),
    Import(String),
    Parameter,
    Other,
}

impl Binding {
    fn merge(&mut self, other: Binding) {
        match (&mut *self, other) {
            (Binding::Classes(nodes), Binding::Classes(more)) => nodes.extend(more),
            (existing, other) if *existing == other => {}
            (existing, _) => *existing = Binding::Other,
        }
    }
}

/// What a bare name at a call site refers to. `Class` holds the class node
/// when exactly one definition binds the name.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Resolved {
    Functions(Scope),
    Class(Option<usize>),
    Shadowed,
}

/// Declared return types of the file's functions by name and defining scope,
/// plus every scope's name bindings for resolving bare names at call sites.
#[derive(Debug, Default)]
pub(crate) struct ReturnTypeIndex {
    returns: HashMap<(String, Scope), Vec<Callee>>,
    bindings: HashMap<(usize, String), Binding>,
    type_variables: HashSet<String>,
}

impl ReturnTypeIndex {
    pub(super) fn build(base: &BaseExtractor, root: Node, class_names: &HashSet<String>) -> Self {
        let mut functions = Vec::new();
        let mut assignments = Vec::new();
        let mut index = Self::default();
        let mut stack = vec![(root, root.id())];
        while let Some((node, scope)) = stack.pop() {
            match node.kind() {
                "function_definition" => functions.push(node),
                "assignment" => assignments.push(node),
                _ => {}
            }
            record_bindings(base, node, scope, &mut index.bindings);
            let inner = if introduces_scope(node) {
                node.id()
            } else {
                scope
            };
            stack.extend(
                node.named_children(&mut node.walk())
                    .map(|child| (child, inner)),
            );
        }
        index.type_variables = assignments
            .into_iter()
            .filter_map(|assignment| index.type_variable_name(base, assignment))
            .collect();
        for function in functions {
            let Some(name) = function.child_by_field_name("name") else {
                continue;
            };
            let (scope, owner) = match defining_scope(function) {
                DefiningScope::Module => (Scope::Module, None),
                DefiningScope::Function(outer) => (Scope::Function(outer.id()), None),
                DefiningScope::Class(class) => {
                    let Some(class_name) = class.child_by_field_name("name") else {
                        continue;
                    };
                    (
                        Scope::Class(class.id()),
                        Some((base.get_node_text(&class_name), class.id())),
                    )
                }
            };
            let callee = Callee {
                is_async: signatures::has_async_keyword(&function),
                returns: index
                    .has_only_transparent_decorators(base, function)
                    .then(|| index.declared_return(base, function, owner, class_names))
                    .flatten(),
            };
            index
                .returns
                .entry((base.get_node_text(&name), scope))
                .or_default()
                .push(callee);
        }
        index
    }

    /// The callee every same-named function in this scope agrees on.
    fn lookup(&self, name: &str, scope: Scope) -> Option<&Callee> {
        let mut callees = self.returns.get(&(name.to_string(), scope))?.iter();
        let first = callees.next()?;
        callees.all(|callee| callee == first).then_some(first)
    }

    /// The scope that binds a bare name used at `node`, by Python's scope
    /// rules: the innermost enclosing function, lambda, or comprehension that
    /// binds it, a class body only for code directly inside it, then the
    /// module. `None` when no scope binds the name.
    fn binding<'t>(&self, name: &str, node: Node<'t>) -> Option<(Node<'t>, &Binding)> {
        let mut class_body_visible = true;
        let mut current = node;
        while let Some(parent) = current.parent() {
            current = parent;
            if !introduces_scope(current) && current.kind() != "module" {
                continue;
            }
            let is_class = current.kind() == "class_definition";
            if let Some(binding) = self.bindings.get(&(current.id(), name.to_string()))
                && (class_body_visible || !is_class)
            {
                return Some((current, binding));
            }
            class_body_visible = false;
        }
        None
    }

    fn resolve(&self, name: &str, node: Node) -> Option<Resolved> {
        let (scope, binding) = self.binding(name, node)?;
        Some(match (scope.kind(), binding) {
            (_, Binding::Classes(nodes)) => Resolved::Class(match nodes.as_slice() {
                [only] => Some(*only),
                _ => None,
            }),
            ("function_definition", Binding::Functions) => {
                Resolved::Functions(Scope::Function(scope.id()))
            }
            ("module", Binding::Functions) => Resolved::Functions(Scope::Module),
            _ => Resolved::Shadowed,
        })
    }

    /// The qualified name of an identifier or dotted name through the
    /// file's imports (`ft.cache` after `import functools as ft` is
    /// `functools.cache`). A name the file does not bind keeps its spelling,
    /// since it is a builtin or comes from an unseen import. `None` when the
    /// file binds the root name some other way.
    fn qualified_name(&self, base: &BaseExtractor, node: Node) -> Option<String> {
        let mut root = node;
        while root.kind() == "attribute" {
            root = root.child_by_field_name("object")?;
        }
        if root.kind() != "identifier" {
            return None;
        }
        let text = base.get_node_text(&node);
        let root_text = base.get_node_text(&root);
        match self.binding(&root_text, node) {
            None => Some(text),
            Some((_, Binding::Import(qualified))) => {
                Some(format!("{qualified}{}", &text[root_text.len()..]))
            }
            Some(_) => None,
        }
    }

    fn type_variable_name(&self, base: &BaseExtractor, assignment: Node) -> Option<String> {
        let left = assignment
            .child_by_field_name("left")
            .filter(|left| left.kind() == "identifier")?;
        let factory = assignment
            .child_by_field_name("right")
            .filter(|right| right.kind() == "call")?
            .child_by_field_name("function")?;
        let is_factory =
            |name: &str| TYPE_VARIABLE_FACTORIES.contains(&name.rsplit('.').next().unwrap_or(name));
        (is_factory(&base.get_node_text(&factory))
            || self
                .qualified_name(base, factory)
                .is_some_and(|name| is_factory(&name)))
        .then(|| base.get_node_text(&left))
    }

    fn has_only_transparent_decorators(&self, base: &BaseExtractor, function: Node) -> bool {
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
                callee
                    .and_then(|callee| self.qualified_name(base, callee))
                    .is_some_and(|name| match name.rsplit_once('.') {
                        None => TRANSPARENT_DECORATORS.contains(&name.as_str()),
                        Some((module, name)) => {
                            DECORATOR_MODULES.contains(&module)
                                && TRANSPARENT_DECORATORS.contains(&name)
                        }
                    })
            })
    }

    fn declared_return(
        &self,
        base: &BaseExtractor,
        function: Node,
        owner: Option<(String, usize)>,
        class_names: &HashSet<String>,
    ) -> Option<InferredType> {
        let return_type = function.child_by_field_name("return_type")?;
        let named = plainly_named_annotation(base, return_type)?;
        let name = strip_type_decorations(&named, &PYTHON_TYPE_NAME_RULES);
        let last = name.rsplit('.').next().unwrap_or(&name);
        let (name, class) = match last {
            "Self" => {
                let (owner_name, owner_id) = owner?;
                (owner_name, Some(owner_id))
            }
            _ if NON_TYPE_RETURNS.contains(&last)
                || self.type_variables.contains(&name)
                || is_generic_parameter(base, function, &name, class_names) =>
            {
                return None;
            }
            _ => {
                let class = match self.resolve(&name, function) {
                    Some(Resolved::Class(class)) => class,
                    _ => None,
                };
                (name, class)
            }
        };
        Some(InferredType {
            name,
            declared: base.get_node_text(&return_type),
            class,
        })
    }
}

fn introduces_scope(node: Node) -> bool {
    is_comprehension(node)
        || matches!(
            node.kind(),
            "function_definition" | "class_definition" | "lambda"
        )
}

fn is_comprehension(node: Node) -> bool {
    matches!(
        node.kind(),
        "list_comprehension"
            | "set_comprehension"
            | "dictionary_comprehension"
            | "generator_expression"
    )
}

/// The scope a walrus binds in: the nearest enclosing scope that is not a
/// comprehension (PEP 572).
fn walrus_scope(node: Node) -> usize {
    let mut current = node;
    while let Some(parent) = current.parent() {
        current = parent;
        if introduces_scope(current) && !is_comprehension(current) {
            break;
        }
    }
    current.id()
}

fn insert_binding(
    bindings: &mut HashMap<(usize, String), Binding>,
    scope: usize,
    name: String,
    binding: Binding,
) {
    match bindings.entry((scope, name)) {
        Entry::Occupied(mut existing) => existing.get_mut().merge(binding),
        Entry::Vacant(slot) => {
            slot.insert(binding);
        }
    }
}

/// Record the names `node` binds in `scope`.
fn record_bindings(
    base: &BaseExtractor,
    node: Node,
    scope: usize,
    bindings: &mut HashMap<(usize, String), Binding>,
) {
    let mut scope = scope;
    let mut targets = Vec::new();
    let binding =
        match node.kind() {
            "function_definition" => {
                targets.extend(node.child_by_field_name("name"));
                Binding::Functions
            }
            "class_definition" => {
                targets.extend(node.child_by_field_name("name"));
                Binding::Classes(vec![node.id()])
            }
            "assignment"
            | "augmented_assignment"
            | "for_statement"
            | "for_in_clause"
            | "type_alias_statement" => {
                targets.extend(node.child_by_field_name("left"));
                Binding::Other
            }
            "named_expression" => {
                scope = walrus_scope(node);
                targets.extend(node.child_by_field_name("name"));
                Binding::Other
            }
            "as_pattern" | "except_clause" => {
                targets.extend(node.child_by_field_name("alias").or_else(|| {
                    node.named_children(&mut node.walk())
                        .last()
                        .filter(|child| child.kind() == "identifier")
                }));
                Binding::Other
            }
            "import_statement" | "import_from_statement" => {
                record_import_bindings(base, node, scope, bindings);
                return;
            }
            "parameters" | "lambda_parameters" => {
                targets.push(node);
                Binding::Parameter
            }
            "global_statement" | "nonlocal_statement" | "splat_pattern" => {
                targets.push(node);
                Binding::Other
            }
            "case_pattern" | "keyword_pattern" | "union_pattern" => {
                targets.extend(node.named_children(&mut node.walk()).filter(|child| {
                    child.kind() == "dotted_name" && child.named_child_count() == 1
                }));
                Binding::Other
            }
            _ => return,
        };
    let mut stack = targets;
    while let Some(target) = stack.pop() {
        match target.kind() {
            "identifier" => {
                insert_binding(
                    bindings,
                    scope,
                    base.get_node_text(&target),
                    binding.clone(),
                );
            }
            "attribute" | "subscript" | "type" => {}
            "default_parameter" | "typed_default_parameter" => {
                stack.extend(target.child_by_field_name("name"));
            }
            "dotted_name" => stack.extend(target.named_child(0)),
            _ => stack.extend(target.named_children(&mut target.walk())),
        }
    }
}

/// Bind each imported name to its qualified name: `import a.b` binds `a` to
/// `a`, `import a.b as c` binds `c` to `a.b`, and `from m import x as y`
/// binds `y` to `m.x`.
fn record_import_bindings(
    base: &BaseExtractor,
    node: Node,
    scope: usize,
    bindings: &mut HashMap<(usize, String), Binding>,
) {
    let module = node
        .child_by_field_name("module_name")
        .map(|module| base.get_node_text(&module));
    for name in node.children_by_field_name("name", &mut node.walk()) {
        let (path, alias) = if name.kind() == "aliased_import" {
            let (Some(path), Some(alias)) = (
                name.child_by_field_name("name"),
                name.child_by_field_name("alias"),
            ) else {
                continue;
            };
            (base.get_node_text(&path), Some(base.get_node_text(&alias)))
        } else {
            (base.get_node_text(&name), None)
        };
        let (bound, qualified) = match (&module, alias) {
            (Some(module), alias) => (
                alias.unwrap_or_else(|| path.clone()),
                format!("{module}.{path}"),
            ),
            (None, Some(alias)) => (alias, path),
            (None, None) => {
                let root = path.split('.').next().unwrap_or(&path).to_string();
                (root.clone(), root)
            }
        };
        insert_binding(bindings, scope, bound, Binding::Import(qualified));
    }
}

enum DefiningScope<'a> {
    Module,
    Class(Node<'a>),
    Function(Node<'a>),
}

/// The scope that binds `function`'s name: the nearest enclosing class or
/// function, through any compound statement (`if`, `try`, `with`, ...).
fn defining_scope(function: Node) -> DefiningScope {
    let mut current = function;
    while let Some(parent) = current.parent() {
        match parent.kind() {
            "class_definition" => return DefiningScope::Class(parent),
            "function_definition" => return DefiningScope::Function(parent),
            _ => current = parent,
        }
    }
    DefiningScope::Module
}

/// Builtin classes and `typing` names; a type variable cannot have these
/// names.
const NOT_TYPE_VARIABLES: &[&str] = &[
    "bool",
    "bytearray",
    "bytes",
    "complex",
    "dict",
    "float",
    "frozenset",
    "int",
    "list",
    "memoryview",
    "object",
    "range",
    "set",
    "str",
    "tuple",
    "type",
    "Any",
    "AbstractSet",
    "AsyncGenerator",
    "AsyncIterable",
    "AsyncIterator",
    "Awaitable",
    "BinaryIO",
    "Callable",
    "ChainMap",
    "Collection",
    "Container",
    "Coroutine",
    "Counter",
    "DefaultDict",
    "Deque",
    "Dict",
    "FrozenSet",
    "Generator",
    "Hashable",
    "IO",
    "Iterable",
    "Iterator",
    "List",
    "LiteralString",
    "Mapping",
    "Match",
    "MutableMapping",
    "MutableSequence",
    "MutableSet",
    "Never",
    "NoReturn",
    "OrderedDict",
    "Pattern",
    "Sequence",
    "Set",
    "Sized",
    "TextIO",
    "Tuple",
    "Type",
];

/// Whether `name` may be a type variable of `function`: a PEP 695 type
/// parameter or class-base type argument of it or of an enclosing
/// definition, or a name that is not a same-file or builtin class and that
/// the parameter annotations of it or of an enclosing function use as a type
/// argument (`def first(items: list[T]) -> T`), or use in any position when
/// the name is spelled like a type variable (`def same(x: T) -> T`).
fn is_generic_parameter(
    base: &BaseExtractor,
    function: Node,
    name: &str,
    class_names: &HashSet<String>,
) -> bool {
    let may_be_type_variable =
        !name.contains('.') && !class_names.contains(name) && !NOT_TYPE_VARIABLES.contains(&name);
    let mut current = Some(function);
    while let Some(definition) = current {
        let bound = match definition.kind() {
            "function_definition" => {
                type_parameter_names(base, definition)
                    .iter()
                    .any(|p| p == name)
                    || (may_be_type_variable
                        && parameter_annotation_use(base, definition, name).is_some_and(
                            |as_type_argument| as_type_argument || is_type_variable_spelling(name),
                        ))
            }
            "class_definition" => class_type_parameter_names(base, definition)
                .iter()
                .any(|p| p == name),
            _ => false,
        };
        if bound {
            return true;
        }
        current = definition.parent();
    }
    false
}

/// How `function`'s parameter annotations use `name`: `Some(true)` as a type
/// argument of a generic (`list[T]`, `Callable[[T], R]`), `Some(false)` only
/// as a whole annotation or inside `Optional`/`Union`/`|`, `None` not at all.
fn parameter_annotation_use(base: &BaseExtractor, function: Node, name: &str) -> Option<bool> {
    let parameters = function.child_by_field_name("parameters")?;
    let mut stack: Vec<(Node, bool)> = parameters
        .named_children(&mut parameters.walk())
        .filter_map(|parameter| parameter.child_by_field_name("type"))
        .map(|annotation| (annotation, false))
        .collect();
    let mut found = None;
    while let Some((node, as_type_argument)) = stack.pop() {
        if node.kind() == "identifier" && base.get_node_text(&node) == name {
            found = Some(found.unwrap_or(false) || as_type_argument);
        }
        if node.kind() == "string" {
            let text = base.get_node_text(&node);
            if text
                .split(|c: char| !(c.is_alphanumeric() || c == '_'))
                .any(|word| word == name)
            {
                found = Some(found.unwrap_or(false) || as_type_argument || text.contains('['));
            }
            continue;
        }
        let head = match node.kind() {
            "generic_type" => node.named_child(0),
            "subscript" => node.child_by_field_name("value"),
            _ => None,
        };
        let opens_type_arguments = head.is_some_and(|head| {
            let text = base.get_node_text(&head);
            let wrapper = text.rsplit('.').next().unwrap_or(&text);
            wrapper != "Union" && !TRANSPARENT_WRAPPERS.contains(&wrapper)
        });
        for child in node.named_children(&mut node.walk()) {
            let is_head = head.is_some_and(|head| head.id() == child.id());
            stack.push((
                child,
                as_type_argument || (opens_type_arguments && !is_head),
            ));
        }
    }
    found
}

/// Conventional type-variable names: `T`, `KT`, `T1`, `AnyStr`, and names
/// ending in `T` after a lowercase letter (`ModelT`), with any leading `_`
/// and a `_co`/`_contra` variance suffix.
fn is_type_variable_spelling(name: &str) -> bool {
    let name = name.trim_start_matches('_');
    let name = name
        .strip_suffix("_co")
        .or_else(|| name.strip_suffix("_contra"))
        .unwrap_or(name);
    let short_capitals = (1..=3).contains(&name.len())
        && name.starts_with(|c: char| c.is_ascii_uppercase())
        && name
            .chars()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit());
    let lowercase_then_t = name
        .strip_suffix('T')
        .and_then(|rest| rest.chars().last())
        .is_some_and(|c| c.is_ascii_lowercase());
    short_capitals || lowercase_then_t || name == "AnyStr"
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
    let index = &extractor.return_types;
    let (name, scope) = match function.kind() {
        "identifier" => {
            let name = base.get_node_text(&function);
            let class = match index.resolve(&name, function) {
                Some(Resolved::Functions(scope)) => {
                    return index.lookup(&name, scope)?.result(awaited);
                }
                Some(Resolved::Class(class)) => class,
                None if extractor.same_file_class_names.contains(&name) => None,
                _ => return None,
            };
            return (!awaited).then(|| InferredType {
                name: name.clone(),
                declared: name,
                class,
            });
        }
        "attribute" => {
            let object = function.child_by_field_name("object")?;
            let owner = if object.kind() == "identifier" {
                let receiver = base.get_node_text(&object);
                match receiver.as_str() {
                    "self" | "cls" => receiver_class(base, index, &receiver, object)?,
                    _ => match index.resolve(&receiver, object) {
                        Some(Resolved::Class(Some(class))) => class,
                        _ => return None,
                    },
                }
            } else {
                value_type(extractor, object, depth)?.class?
            };
            let method = base.get_node_text(&function.child_by_field_name("attribute")?);
            if index.bindings.get(&(owner, method.clone())) != Some(&Binding::Functions) {
                return None;
            }
            (method, Scope::Class(owner))
        }
        _ => return None,
    };
    index.lookup(&name, scope)?.result(awaited)
}

/// The class of the method whose receiver `self`/`cls` is at `receiver`:
/// the name is the method's first plain parameter, the method is not a
/// `@staticmethod`, and nothing else in the method rebinds the name.
fn receiver_class(
    base: &BaseExtractor,
    index: &ReturnTypeIndex,
    name: &str,
    receiver: Node,
) -> Option<usize> {
    let (method, binding) = index.binding(name, receiver)?;
    if method.kind() != "function_definition" || *binding != Binding::Parameter {
        return None;
    }
    let first = method
        .child_by_field_name("parameters")?
        .named_child(0)
        .filter(|first| first.kind() == "identifier")?;
    if base.get_node_text(&first) != name || is_static_method(base, method) {
        return None;
    }
    match defining_scope(method) {
        DefiningScope::Class(class) => Some(class.id()),
        _ => None,
    }
}

fn is_static_method(base: &BaseExtractor, function: Node) -> bool {
    function
        .parent()
        .filter(|parent| parent.kind() == "decorated_definition")
        .is_some_and(|decorated| {
            decorated
                .named_children(&mut decorated.walk())
                .filter(|child| child.kind() == "decorator")
                .any(|decorator| {
                    let text = base.get_node_text(&decorator);
                    text.rsplit(|c: char| !(c.is_alphanumeric() || c == '_'))
                        .any(|word| word == "staticmethod")
                })
        })
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
