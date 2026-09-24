use crate::base::BaseExtractor;
use crate::base::types::TypeNameRules;
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};
use std::collections::{HashMap, HashSet};
use tree_sitter::Node;

pub(super) const VBNET_TYPE_NAME_RULES: TypeNameRules = TypeNameRules {
    nullable_suffixes: &["?"],
    reference_prefixes: &[],
    generic_open: &['('],
};

/// Array base names such as `Worker()` are reduced structurally, so the
/// `(` generic opener must not cut the array suffix off again.
const VBNET_ARRAY_TYPE_NAME_RULES: TypeNameRules = TypeNameRules {
    nullable_suffixes: &[],
    reference_prefixes: &[],
    generic_open: &[],
};

struct ReducedType {
    base_name: String,
    is_array: bool,
}

pub(super) fn record_declared_type(
    base: &mut BaseExtractor,
    symbol_id: &str,
    type_node: Node,
    declarator_rank: Option<Node>,
) {
    record_type_node(base, symbol_id, type_node, declarator_rank, false);
}

pub(super) fn record_constructor_fact(base: &mut BaseExtractor, symbol_id: &str, class_name: &str) {
    base.record_declared_type_fact(symbol_id, class_name, &VBNET_TYPE_NAME_RULES, true);
}

pub(super) fn declared_type_node(node: Node) -> Option<Node> {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "as_clause" {
            return child.child_by_field_name("type");
        }
    }
    node.child_by_field_name("value")
        .filter(|value| value.kind() == "new_expression")
        .and_then(|value| value.child_by_field_name("type"))
}

pub(super) fn declarator_rank_node(node: Node) -> Option<Node> {
    named_child_of_kind(node, "array_rank_specifier")
}

pub(super) fn constructor_type_node(initializer: Node) -> Option<Node> {
    match initializer.kind() {
        "new_expression" => initializer.child_by_field_name("type"),
        "element_access" => {
            let object = initializer.child_by_field_name("object")?;
            if object.kind() == "new_expression" {
                object.child_by_field_name("type")
            } else {
                None
            }
        }
        _ => None,
    }
}

pub(super) fn simple_unqualified_name(base: &BaseExtractor, type_node: Node) -> Option<String> {
    if !is_simple_constructed_type(type_node) {
        return None;
    }
    let name_node = base_type_name_node(type_node)?;
    Some(base.get_node_text(&name_node))
}

fn is_simple_constructed_type(node: Node) -> bool {
    match node.kind() {
        "identifier" | "primitive_type" => true,
        "namespace_name" => single_identifier(node).is_some(),
        "array_type" => node
            .child_by_field_name("element")
            .is_some_and(is_simple_constructed_type),
        "nullable_type" => node.named_child(0).is_some_and(is_simple_constructed_type),
        _ => false,
    }
}

fn record_type_node(
    base: &mut BaseExtractor,
    symbol_id: &str,
    type_node: Node,
    declarator_rank: Option<Node>,
    is_inferred: bool,
) {
    let type_node = without_constructor_argument_list(base, type_node);
    let Some(reduced) = reduce_type(base, type_node) else {
        return;
    };
    let rank_suffix = declarator_rank
        .map(|rank| rank_suffix(base, rank))
        .unwrap_or_default();
    let base_name = format!("{}{}", reduced.base_name, rank_suffix);
    let declared = format!("{}{}", base.get_node_text(&type_node), rank_suffix);
    let rules = if reduced.is_array || !rank_suffix.is_empty() {
        &VBNET_ARRAY_TYPE_NAME_RULES
    } else {
        &VBNET_TYPE_NAME_RULES
    };
    base.record_declared_type_fact_with_declared(
        symbol_id,
        &base_name,
        &declared,
        rules,
        is_inferred,
    );
}

fn reduce_type(base: &BaseExtractor, node: Node) -> Option<ReducedType> {
    if node.kind() == "array_type" {
        let element = reduce_type(base, node.child_by_field_name("element")?)?;
        let rank = node.child_by_field_name("rank")?;
        return Some(ReducedType {
            base_name: format!("{}{}", element.base_name, rank_suffix(base, rank)),
            is_array: true,
        });
    }
    let name_node = base_type_name_node(node)?;
    Some(ReducedType {
        base_name: base.get_node_text(&name_node),
        is_array: false,
    })
}

/// `New Foo()` parses `Foo()` as an array type, but the empty parentheses are
/// the constructor argument list, so the element type is the declared type.
fn without_constructor_argument_list<'a>(base: &BaseExtractor, type_node: Node<'a>) -> Node<'a> {
    let in_new_expression = type_node
        .parent()
        .is_some_and(|parent| parent.kind() == "new_expression");
    if !in_new_expression || type_node.kind() != "array_type" {
        return type_node;
    }
    match (
        type_node.child_by_field_name("element"),
        type_node.child_by_field_name("rank"),
    ) {
        (Some(element), Some(rank)) if rank_suffix(base, rank) == "()" => element,
        _ => type_node,
    }
}

fn rank_suffix(base: &BaseExtractor, rank: Node) -> String {
    let sizes = rank.named_child_count();
    let commas = if sizes > 0 {
        sizes - 1
    } else {
        base.get_node_text(&rank).matches(',').count()
    };
    format!("({})", ",".repeat(commas))
}

fn base_type_name_node(node: Node) -> Option<Node> {
    let mut node = node;
    loop {
        match node.kind() {
            "identifier" | "primitive_type" | "namespace_name" => return Some(node),
            "generic_type" => {
                node = named_child_of_kind(node, "namespace_name")?;
            }
            "nullable_type" => {
                node = node.named_child(0)?;
            }
            "array_type" => {
                node = node.child_by_field_name("element")?;
            }
            "new_expression" => {
                node = node.child_by_field_name("type")?;
            }
            _ => return None,
        }
    }
}

fn single_identifier(node: Node) -> Option<Node> {
    let mut cursor = node.walk();
    let mut identifiers = node
        .children(&mut cursor)
        .filter(|child| child.kind() == "identifier");
    let first = identifiers.next()?;
    if identifiers.next().is_some() {
        return None;
    }
    Some(first)
}

fn named_child_of_kind<'a>(node: Node<'a>, kind: &str) -> Option<Node<'a>> {
    let mut cursor = node.walk();
    node.children(&mut cursor)
        .find(|child| child.kind() == kind)
}

/// The ` As T` / ` As New T(...)` signature suffix for a declared type node.
pub(super) fn as_clause_suffix(base: &BaseExtractor, type_node: Node) -> String {
    let keyword = if type_node
        .parent()
        .is_some_and(|parent| parent.kind() == "new_expression")
    {
        " As New "
    } else {
        " As "
    };
    format!("{keyword}{}", base.get_node_text(&type_node))
}

/// Record the declared return type of a call initializer (`is_inferred=true`):
/// an unqualified call, `Me.F()` / `MyClass.F()`, or `T.F()` on a same-file
/// class, structure, or module that is in scope. `Await` removes one
/// `Task(Of T)` / `ValueTask(Of T)` layer, also through an awaited
/// `ConfigureAwait(...)`. `is_shadowed` says whether a local, parameter, or
/// the enclosing member takes over a name.
pub(super) fn record_call_initializer_type(
    base: &mut BaseExtractor,
    symbol_id: &str,
    initializer: Node,
    return_types: &ReturnTypeIndex,
    is_shadowed: &dyn Fn(&str) -> bool,
) {
    let scope = InitializerScope {
        base,
        return_types,
        enclosing: enclosing_type_ids(initializer),
        namespace: enclosing_namespace(base, initializer),
        bound_names: enclosing_bound_names(base, initializer),
        is_shadowed,
    };
    let Some(TypeShape {
        name: Some(name),
        declared,
        is_array,
        ..
    }) = scope.shape_of(initializer, false, 0)
    else {
        return;
    };
    let rules = if is_array {
        &VBNET_ARRAY_TYPE_NAME_RULES
    } else {
        &VBNET_TYPE_NAME_RULES
    };
    base.record_declared_type_fact_with_declared(symbol_id, &name, &declared, rules, true);
}

/// The case-insensitive lookup key of a VB name; `[Name]` escapes are dropped.
pub(super) fn name_key(text: &str) -> String {
    text.trim_start_matches('[')
        .trim_end_matches(']')
        .to_lowercase()
}

/// Members every class and structure inherits from `Object`. An unqualified
/// call to one of them binds to `Me` before any module function.
const OBJECT_MEMBERS: &[&str] = &[
    "tostring",
    "equals",
    "gethashcode",
    "gettype",
    "referenceequals",
    "memberwiseclone",
    "finalize",
];

/// A return type reduced to what initializer inference needs: the bindable
/// base name (`None` for type parameters), the written text, and the type
/// arguments of a generic type.
#[derive(Debug, Clone, PartialEq, Eq)]
struct TypeShape {
    name: Option<String>,
    declared: String,
    is_array: bool,
    args: Vec<TypeShape>,
}

impl TypeShape {
    fn is_task(&self) -> bool {
        let Some(name) = self.name.as_deref() else {
            return false;
        };
        let simple = name.rsplit('.').next().unwrap_or(name);
        (simple.eq_ignore_ascii_case("Task") || simple.eq_ignore_ascii_case("ValueTask"))
            && self.args.len() == 1
    }

    fn awaited(self) -> Option<TypeShape> {
        if self.is_task() {
            self.args.into_iter().next()
        } else {
            None
        }
    }
}

/// Declared return types of the members of every class, structure, and
/// module in the file, keyed by type block. Interfaces are left out: an
/// interface member is never reached through `Me`, an unqualified call, or a
/// shared call.
#[derive(Debug, Default)]
pub(super) struct ReturnTypeIndex {
    types: HashMap<usize, TypeMembers>,
    by_name: HashMap<String, Vec<usize>>,
    modules: Vec<usize>,
    /// Namespaces named by `Imports` clauses without an alias.
    imports: Vec<Vec<String>>,
    /// Namespace segments and `Imports` alias names. VB can bind a
    /// qualifier to one of them before a same-file type.
    namespace_or_alias_names: HashSet<String>,
}

#[derive(Debug, Default)]
struct TypeMembers {
    is_module: bool,
    /// A `Partial` type, or one with more than one part in the file. Another
    /// part, maybe in another file, can add any overload, so no call to its
    /// members records a fact.
    is_split: bool,
    is_partial: bool,
    inherits: bool,
    /// Namespace path, outermost first, of a type not nested in another type.
    namespace: Vec<String>,
    /// The type block that declares this nested type.
    parent_type: Option<usize>,
    members: HashMap<String, Vec<Candidate>>,
}

/// One same-named member: its return shape (`None` for a `Sub`, a
/// `Function` without `As`, or a property, event, or field), how many
/// arguments it accepts, and whether it keeps base overloads visible.
#[derive(Debug)]
struct Candidate {
    shape: Option<TypeShape>,
    arity: Arity,
    keeps_base_overloads: bool,
}

#[derive(Debug, Clone, Copy)]
struct Arity {
    min: usize,
    /// `None` for a `ParamArray` parameter or a non-method member.
    max: Option<usize>,
}

impl Arity {
    const ANY: Arity = Arity { min: 0, max: None };

    fn accepts(self, count: usize) -> bool {
        count >= self.min && self.max.is_none_or(|max| count <= max)
    }
}

impl TypeMembers {
    /// An `Inherits` clause or another part can declare members this block
    /// does not show.
    fn is_open(&self) -> bool {
        self.inherits || self.is_split
    }

    /// `None` when the type does not declare `name`. `Some(None)` when it
    /// does but the call has no single known type: the candidates disagree,
    /// none takes `argument_count` arguments (VB then indexes the result of
    /// a parameterless function), another part can add an overload, or
    /// `Overloads` keeps base overloads that this file cannot see.
    fn lookup(&self, name: &str, argument_count: usize) -> Option<Option<TypeShape>> {
        let candidates = self.members.get(name)?;
        let base_overloads = self.inherits
            && candidates
                .iter()
                .any(|candidate| candidate.keeps_base_overloads);
        let callable = candidates
            .iter()
            .any(|candidate| candidate.arity.accepts(argument_count));
        if self.is_split || base_overloads || !callable {
            return Some(None);
        }
        Some(unanimous(
            candidates.iter().map(|candidate| candidate.shape.clone()),
        ))
    }
}

fn unanimous(mut shapes: impl Iterator<Item = Option<TypeShape>>) -> Option<TypeShape> {
    let first = shapes.next()??;
    shapes
        .all(|shape| shape.as_ref() == Some(&first))
        .then_some(first)
}

fn is_type_block(node: Node) -> bool {
    matches!(
        node.kind(),
        "class_block" | "structure_block" | "module_block"
    )
}

/// Where the index walk is: the type parameters in scope, the namespace
/// path, the enclosing type block, and its qualified name.
#[derive(Clone, Default)]
struct IndexScope {
    generics: Vec<String>,
    namespace: Vec<String>,
    parent_type: Option<usize>,
    qualified: String,
}

impl ReturnTypeIndex {
    pub(super) fn build(base: &BaseExtractor, root: Node) -> Self {
        let mut index = Self::default();
        let mut parts: HashMap<String, Vec<usize>> = HashMap::new();
        index.collect(base, root, &IndexScope::default(), &mut parts, 0);
        for ids in parts.values() {
            let shared = ids.len() > 1
                || ids
                    .iter()
                    .any(|id| index.types.get(id).is_some_and(|entry| entry.is_partial));
            for id in ids {
                if let Some(entry) = index.types.get_mut(id) {
                    entry.is_split = shared;
                }
            }
        }
        index
    }

    fn collect(
        &mut self,
        base: &BaseExtractor,
        node: Node,
        scope: &IndexScope,
        parts: &mut HashMap<String, Vec<usize>>,
        depth: u32,
    ) {
        if !should_visit_tree_depth(depth) {
            return;
        }
        let inner;
        let scope = match node.kind() {
            "namespace_block" => {
                if let Some(name) = node.child_by_field_name("name") {
                    self.namespace_or_alias_names.extend(
                        name.named_children(&mut name.walk())
                            .map(|segment| name_key(&base.get_node_text(&segment))),
                    );
                }
                inner = IndexScope {
                    namespace: nested_namespace(base, &scope.namespace, node),
                    ..scope.clone()
                };
                &inner
            }
            "imports_statement" => {
                self.imports.extend(unaliased_imports(base, node));
                let mut cursor = node.walk();
                self.namespace_or_alias_names.extend(
                    node.children_by_field_name("alias", &mut cursor)
                        .map(|alias| name_key(&base.get_node_text(&alias))),
                );
                return;
            }
            _ if is_type_block(node) => {
                let name = node
                    .child_by_field_name("name")
                    .map(|name| name_key(&base.get_node_text(&name)))
                    .unwrap_or_default();
                let qualified = if scope.parent_type.is_some() {
                    format!("{}.{name}", scope.qualified)
                } else {
                    format!("{}.{name}", scope.namespace.join("."))
                };
                inner = IndexScope {
                    generics: [scope.generics.as_slice(), &type_parameter_names(base, node)]
                        .concat(),
                    namespace: scope.namespace.clone(),
                    parent_type: Some(node.id()),
                    qualified: qualified.clone(),
                };
                self.add_type(base, node, &inner.generics, scope);
                parts.entry(qualified).or_default().push(node.id());
                &inner
            }
            _ => scope,
        };
        let Some(child_depth) = child_tree_depth(depth) else {
            return;
        };
        for child in node.named_children(&mut node.walk()) {
            self.collect(base, child, scope, parts, child_depth);
        }
    }

    fn add_type(
        &mut self,
        base: &BaseExtractor,
        block: Node,
        generics: &[String],
        at: &IndexScope,
    ) {
        let mut entry = TypeMembers {
            is_module: block.kind() == "module_block",
            is_split: false,
            is_partial: has_modifier(base, block, &["partial"]),
            inherits: block.child_by_field_name("inherits").is_some(),
            namespace: at.namespace.clone(),
            parent_type: at.parent_type,
            members: HashMap::new(),
        };
        let mut add = |name: Node, candidate: Candidate| {
            entry
                .members
                .entry(name_key(&base.get_node_text(&name)))
                .or_default()
                .push(candidate);
        };
        let field = |shape| Candidate {
            shape,
            arity: Arity::ANY,
            keeps_base_overloads: false,
        };
        for member in block.named_children(&mut block.walk()) {
            match member.kind() {
                "method_declaration" | "abstract_method_declaration" | "declare_statement" => {
                    let Some(name) = member.child_by_field_name("name") else {
                        continue;
                    };
                    let generics = [generics, &type_parameter_names(base, member)].concat();
                    let shape = member
                        .child_by_field_name("return_type")
                        .and_then(|return_type| type_shape(base, return_type, &generics, 0));
                    add(
                        name,
                        Candidate {
                            shape,
                            arity: parameter_arity(base, member),
                            keeps_base_overloads: has_modifier(
                                base,
                                member,
                                &["overloads", "overrides"],
                            ),
                        },
                    );
                }
                "property_declaration" | "event_declaration" => {
                    if let Some(name) = member.child_by_field_name("name") {
                        add(name, field(None));
                    }
                }
                "const_declaration" => {
                    let mut cursor = member.walk();
                    let names: Vec<Node> =
                        member.children_by_field_name("name", &mut cursor).collect();
                    for name in names {
                        add(name, field(None));
                    }
                }
                "field_declaration" => {
                    for declarator in member.named_children(&mut member.walk()) {
                        if declarator.kind() == "variable_declarator"
                            && let Some(name) = declarator.child_by_field_name("name")
                        {
                            add(name, field(None));
                        }
                    }
                }
                _ => {}
            }
        }
        let id = block.id();
        if let Some(name) = block.child_by_field_name("name") {
            self.by_name
                .entry(name_key(&base.get_node_text(&name)))
                .or_default()
                .push(id);
        }
        if entry.is_module {
            self.modules.push(id);
        }
        self.types.insert(id, entry);
    }
}

fn has_modifier(base: &BaseExtractor, node: Node, keywords: &[&str]) -> bool {
    let Some(modifiers) = node.child_by_field_name("modifiers") else {
        return false;
    };
    modifiers
        .named_children(&mut modifiers.walk())
        .any(|modifier| keywords.contains(&name_key(&base.get_node_text(&modifier)).as_str()))
}

/// How many arguments a method or `Declare` accepts: `Optional` parameters
/// may be left out and a `ParamArray` takes any number.
fn parameter_arity(base: &BaseExtractor, member: Node) -> Arity {
    let Some(parameters) = member.child_by_field_name("parameters") else {
        return Arity {
            min: 0,
            max: Some(0),
        };
    };
    let mut arity = Arity {
        min: 0,
        max: Some(0),
    };
    for parameter in parameters.named_children(&mut parameters.walk()) {
        if parameter.kind() != "parameter" {
            continue;
        }
        let prefix = parameter
            .child_by_field_name("name")
            .and_then(|name| base.content.get(parameter.start_byte()..name.start_byte()))
            .unwrap_or_default()
            .to_lowercase();
        let words: Vec<&str> = prefix.split_whitespace().collect();
        if words.contains(&"paramarray") {
            arity.max = None;
            continue;
        }
        arity.max = arity.max.map(|max| max + 1);
        if !words.contains(&"optional") && parameter.child_by_field_name("default_value").is_none()
        {
            arity.min += 1;
        }
    }
    arity
}

/// The namespace path inside `block`. `Namespace Global.X` starts again
/// from the root namespace.
fn nested_namespace(base: &BaseExtractor, outer: &[String], block: Node) -> Vec<String> {
    let segments: Vec<String> = block
        .child_by_field_name("name")
        .map(|name| {
            name.named_children(&mut name.walk())
                .map(|segment| name_key(&base.get_node_text(&segment)))
                .collect()
        })
        .unwrap_or_default();
    match segments.split_first() {
        Some((first, rest)) if first == "global" => rest.to_vec(),
        _ => [outer, segments.as_slice()].concat(),
    }
}

/// Namespaces named by an `Imports` clause. An alias (`Imports X = A.B`) does
/// not bring the namespace's members into scope, so aliased clauses are left
/// out.
fn unaliased_imports(base: &BaseExtractor, statement: Node) -> Vec<Vec<String>> {
    let mut cursor = statement.walk();
    statement
        .children_by_field_name("namespace", &mut cursor)
        .filter(|namespace| {
            namespace
                .prev_named_sibling()
                .is_none_or(|previous| previous.kind() != "identifier")
        })
        .map(|namespace| {
            namespace
                .named_children(&mut namespace.walk())
                .map(|segment| name_key(&base.get_node_text(&segment)))
                .collect()
        })
        .collect()
}

fn type_parameter_names(base: &BaseExtractor, item: Node) -> Vec<String> {
    let Some(parameters) = named_child_of_kind(item, "type_parameters") else {
        return Vec::new();
    };
    parameters
        .named_children(&mut parameters.walk())
        .filter_map(|parameter| parameter.child_by_field_name("name"))
        .map(|name| name_key(&base.get_node_text(&name)))
        .collect()
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
    match node.kind() {
        "array_type" => {
            let element = type_shape(
                base,
                node.child_by_field_name("element")?,
                generics,
                child_depth,
            )?;
            let rank = rank_suffix(base, node.child_by_field_name("rank")?);
            Some(TypeShape {
                name: element.name.map(|name| format!("{name}{rank}")),
                declared,
                is_array: true,
                args: Vec::new(),
            })
        }
        "nullable_type" => {
            let inner = type_shape(base, node.named_child(0)?, generics, child_depth)?;
            Some(TypeShape { declared, ..inner })
        }
        "generic_type" => {
            let name = base.get_node_text(&named_child_of_kind(node, "namespace_name")?);
            let arguments = named_child_of_kind(node, "type_argument_list")?;
            let args = arguments
                .named_children(&mut arguments.walk())
                .map(|argument| type_shape(base, argument, generics, child_depth))
                .collect::<Option<_>>()?;
            Some(TypeShape {
                name: Some(name),
                declared,
                is_array: false,
                args,
            })
        }
        "identifier" | "namespace_name" | "primitive_type" => Some(TypeShape {
            name: (!generics.contains(&name_key(&declared))).then(|| declared.clone()),
            declared,
            is_array: false,
            args: Vec::new(),
        }),
        _ => Some(TypeShape {
            name: None,
            declared,
            is_array: false,
            args: Vec::new(),
        }),
    }
}

/// Type block ids around `node`, innermost first.
fn enclosing_type_ids(node: Node) -> Vec<usize> {
    let mut ids = Vec::new();
    let mut current = node.parent();
    while let Some(ancestor) = current {
        if is_type_block(ancestor) {
            ids.push(ancestor.id());
        }
        current = ancestor.parent();
    }
    ids
}

fn enclosing_namespace(base: &BaseExtractor, node: Node) -> Vec<String> {
    let mut blocks = Vec::new();
    let mut current = node.parent();
    while let Some(ancestor) = current {
        if ancestor.kind() == "namespace_block" {
            blocks.push(ancestor);
        }
        current = ancestor.parent();
    }
    blocks.into_iter().rev().fold(Vec::new(), |outer, block| {
        nested_namespace(base, &outer, block)
    })
}

/// Names bound around `node` that have no symbol and take over a called
/// name: lambda parameters, and the implicit `Value` parameter of a `Set`
/// accessor.
fn enclosing_bound_names(base: &BaseExtractor, node: Node) -> Vec<String> {
    let mut names = Vec::new();
    let mut current = node.parent();
    while let Some(ancestor) = current {
        if is_type_block(ancestor) {
            break;
        }
        if ancestor.kind() == "set_accessor" {
            names.push("value".to_string());
        }
        if ancestor.kind() == "lambda_expression" {
            names.extend(
                ancestor
                    .named_children(&mut ancestor.walk())
                    .filter(|child| child.kind() == "lambda_parameter")
                    .filter_map(|parameter| parameter.child_by_field_name("name"))
                    .map(|name| name_key(&base.get_node_text(&name))),
            );
        }
        current = ancestor.parent();
    }
    names
}

/// The number of arguments a call passes; `Load(0, , 2)` passes three. The
/// grammar keeps no node for the separator, so the commas are counted in the
/// source text between the argument (and comment) nodes.
fn argument_count(base: &BaseExtractor, invocation: Node) -> Option<usize> {
    let arguments = invocation.child_by_field_name("arguments")?;
    if arguments.has_error() {
        return None;
    }
    let mut commas = 0;
    let mut gap_start = arguments.start_byte();
    for child in arguments.named_children(&mut arguments.walk()) {
        commas += base
            .content
            .get(gap_start..child.start_byte())?
            .matches(',')
            .count();
        gap_start = child.end_byte();
    }
    commas += base
        .content
        .get(gap_start..arguments.end_byte())?
        .matches(',')
        .count();
    let has_argument = arguments
        .named_children(&mut arguments.walk())
        .any(|child| child.kind() == "argument");
    Some(if commas == 0 && !has_argument {
        0
    } else {
        commas + 1
    })
}

struct InitializerScope<'a> {
    base: &'a BaseExtractor,
    return_types: &'a ReturnTypeIndex,
    enclosing: Vec<usize>,
    namespace: Vec<String>,
    bound_names: Vec<String>,
    is_shadowed: &'a dyn Fn(&str) -> bool,
}

impl InitializerScope<'_> {
    /// `in_await` is true only for the direct operand of `Await`, the one
    /// place where `ConfigureAwait(...)` keeps the awaited type.
    fn shape_of(&self, value: Node, in_await: bool, depth: u32) -> Option<TypeShape> {
        if !should_visit_tree_depth(depth) {
            return None;
        }
        let child_depth = child_tree_depth(depth)?;
        match value.kind() {
            "unary_expression" => {
                let operand = value.child_by_field_name("operand")?;
                if !self.is_await(value, operand) {
                    return None;
                }
                self.shape_of(operand, true, child_depth)?.awaited()
            }
            "parenthesized_expression" => {
                let inner = value
                    .named_children(&mut value.walk())
                    .find(|child| child.kind() != "comment")?;
                self.shape_of(inner, in_await, child_depth)
            }
            "invocation" => {
                let target = value.child_by_field_name("target")?;
                let argument_count = argument_count(self.base, value)?;
                match target.kind() {
                    "identifier" => self.unqualified_call(&self.key(target), argument_count),
                    "member_access" => {
                        self.member_call(target, argument_count, in_await, child_depth)
                    }
                    _ => None,
                }
            }
            _ => None,
        }
    }

    /// The grammar keeps no node for the unary operator keyword, so the
    /// source text before the operand names it.
    fn is_await(&self, unary: Node, operand: Node) -> bool {
        self.base
            .content
            .get(unary.start_byte()..operand.start_byte())
            .is_some_and(|operator| operator.trim().eq_ignore_ascii_case("Await"))
    }

    fn key(&self, node: Node) -> String {
        name_key(&self.base.get_node_text(&node))
    }

    fn shadowed(&self, name: &str) -> bool {
        (self.is_shadowed)(name) || self.bound_names.iter().any(|local| local == name)
    }

    fn member_call(
        &self,
        target: Node,
        argument_count: usize,
        in_await: bool,
        object_depth: u32,
    ) -> Option<TypeShape> {
        let object = target.child_by_field_name("object")?;
        let member = self.key(target.child_by_field_name("member")?);
        match object.kind() {
            "me_expression" if self.key(object) != "mybase" => {
                self.me_call(&member, argument_count)
            }
            "identifier" => self.shared_call(&self.key(object), &member, argument_count),
            _ if in_await && member == "configureawait" => self
                .shape_of(object, false, object_depth)
                .filter(TypeShape::is_task),
            _ => None,
        }
    }

    fn me_call(&self, member: &str, argument_count: usize) -> Option<TypeShape> {
        let own = self.return_types.types.get(self.enclosing.first()?)?;
        if own.is_module {
            return None;
        }
        own.lookup(member, argument_count)?
    }

    /// `T.F()` where `T` names a same-file type in scope. A local, a
    /// parameter, or a member of an enclosing type or of a module in scope
    /// with that name would take the qualifier. VB looks in each enclosing
    /// type, inherited members included, before it looks in namespaces, so an
    /// open enclosing type stops the search unless it declares `T` as a
    /// nested type. Every type in scope with that name must declare `F`. A
    /// namespace or an `Imports` alias with the name `T` anywhere in the file
    /// can take the qualifier first, so it stops the search.
    fn shared_call(
        &self,
        qualifier: &str,
        member: &str,
        argument_count: usize,
    ) -> Option<TypeShape> {
        if self.shadowed(qualifier)
            || self.module_declares(qualifier)
            || self
                .return_types
                .namespace_or_alias_names
                .contains(qualifier)
        {
            return None;
        }
        let types = self.return_types.by_name.get(qualifier)?;
        for id in &self.enclosing {
            let entry = self.return_types.types.get(id)?;
            if entry.members.contains_key(qualifier) {
                return None;
            }
            let declares_nested = types.iter().any(|nested| {
                self.return_types
                    .types
                    .get(nested)
                    .is_some_and(|nested| nested.parent_type == Some(*id))
            });
            if declares_nested {
                break;
            }
            if entry.is_open() {
                return None;
            }
        }
        unanimous(
            types
                .iter()
                .filter_map(|id| self.return_types.types.get(id))
                .filter(|entry| self.is_in_scope(entry))
                .map(|entry| entry.lookup(member, argument_count).flatten()),
        )
    }

    /// An unqualified call resolves through the innermost enclosing type that
    /// declares the name. An open type (`Inherits` or `Partial`) stops the
    /// search, because its base or other part can declare the name. Module
    /// functions in scope come last, after the members of `Object`.
    fn unqualified_call(&self, name: &str, argument_count: usize) -> Option<TypeShape> {
        if self.shadowed(name) {
            return None;
        }
        for id in &self.enclosing {
            let entry = self.return_types.types.get(id)?;
            if let Some(shape) = entry.lookup(name, argument_count) {
                return shape;
            }
            if entry.is_open() {
                return None;
            }
        }
        if OBJECT_MEMBERS.contains(&name) {
            return None;
        }
        unanimous(
            self.return_types
                .modules
                .iter()
                .filter_map(|id| self.return_types.types.get(id))
                .filter(|entry| self.is_in_scope(entry))
                .filter_map(|entry| entry.lookup(name, argument_count)),
        )
    }

    /// A nested type is in scope inside its declaring type. Any other type
    /// is in scope from its own or an inner namespace, or through `Imports`.
    fn is_in_scope(&self, entry: &TypeMembers) -> bool {
        match entry.parent_type {
            Some(parent) => self.enclosing.contains(&parent),
            None => {
                self.namespace.starts_with(&entry.namespace)
                    || self.return_types.imports.contains(&entry.namespace)
            }
        }
    }

    /// Whether a module in scope has a member with this name. VB promotes
    /// module members to the namespace, so the member can take a qualifier.
    fn module_declares(&self, name: &str) -> bool {
        self.return_types
            .modules
            .iter()
            .filter_map(|id| self.return_types.types.get(id))
            .any(|entry| self.is_in_scope(entry) && entry.members.contains_key(name))
    }
}
