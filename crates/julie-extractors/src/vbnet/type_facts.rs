use crate::base::BaseExtractor;
use crate::base::types::TypeNameRules;
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};
use std::collections::HashMap;
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
/// class, structure, or module. `Await` removes one `Task(Of T)` /
/// `ValueTask(Of T)` layer and `ConfigureAwait` keeps it. `is_shadowed` says
/// whether a local, parameter, or the enclosing member takes over a name.
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
        is_shadowed,
    };
    let Some(TypeShape {
        name: Some(name),
        declared,
        is_array,
        ..
    }) = scope.shape_of(initializer, 0)
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
}

#[derive(Debug, Default)]
struct TypeMembers {
    is_module: bool,
    inherits: bool,
    /// Return shape of each same-named member; `None` for a `Sub`, a
    /// `Function` without `As`, or a property, event, or field.
    members: HashMap<String, Vec<Option<TypeShape>>>,
}

impl TypeMembers {
    fn lookup(&self, name: &str) -> Option<Option<TypeShape>> {
        self.members
            .get(name)
            .map(|shapes| unanimous(shapes.iter()))
    }
}

fn unanimous<'a>(mut shapes: impl Iterator<Item = &'a Option<TypeShape>>) -> Option<TypeShape> {
    let first = shapes.next()?.as_ref()?;
    shapes
        .all(|shape| shape.as_ref() == Some(first))
        .then(|| first.clone())
}

fn is_type_block(node: Node) -> bool {
    matches!(
        node.kind(),
        "class_block" | "structure_block" | "module_block"
    )
}

impl ReturnTypeIndex {
    pub(super) fn build(base: &BaseExtractor, root: Node) -> Self {
        let mut index = Self::default();
        index.collect(base, root, &[], 0);
        index
    }

    fn collect(&mut self, base: &BaseExtractor, node: Node, generics: &[String], depth: u32) {
        if !should_visit_tree_depth(depth) {
            return;
        }
        let scoped;
        let generics = if is_type_block(node) {
            scoped = [generics, &type_parameter_names(base, node)].concat();
            self.add_type(base, node, &scoped);
            &scoped
        } else {
            generics
        };
        let Some(child_depth) = child_tree_depth(depth) else {
            return;
        };
        for child in node.named_children(&mut node.walk()) {
            self.collect(base, child, generics, child_depth);
        }
    }

    fn add_type(&mut self, base: &BaseExtractor, block: Node, generics: &[String]) {
        let mut entry = TypeMembers {
            is_module: block.kind() == "module_block",
            inherits: block.child_by_field_name("inherits").is_some(),
            members: HashMap::new(),
        };
        let mut add = |name: Node, shape: Option<TypeShape>| {
            entry
                .members
                .entry(name_key(&base.get_node_text(&name)))
                .or_default()
                .push(shape);
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
                    add(name, shape);
                }
                "property_declaration" | "event_declaration" => {
                    if let Some(name) = member.child_by_field_name("name") {
                        add(name, None);
                    }
                }
                "field_declaration" => {
                    for declarator in member.named_children(&mut member.walk()) {
                        if declarator.kind() == "variable_declarator"
                            && let Some(name) = declarator.child_by_field_name("name")
                        {
                            add(name, None);
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

struct InitializerScope<'a> {
    base: &'a BaseExtractor,
    return_types: &'a ReturnTypeIndex,
    enclosing: Vec<usize>,
    is_shadowed: &'a dyn Fn(&str) -> bool,
}

impl InitializerScope<'_> {
    fn shape_of(&self, value: Node, depth: u32) -> Option<TypeShape> {
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
                self.shape_of(operand, child_depth)?.awaited()
            }
            "invocation" => {
                let target = value.child_by_field_name("target")?;
                match target.kind() {
                    "identifier" => self.unqualified_call(&self.key(target)),
                    "member_access" => self.member_call(target, child_depth),
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

    fn member_call(&self, target: Node, object_depth: u32) -> Option<TypeShape> {
        let object = target.child_by_field_name("object")?;
        let member = self.key(target.child_by_field_name("member")?);
        match object.kind() {
            "me_expression" if self.key(object) != "mybase" => self.me_call(&member),
            "identifier" => self.shared_call(&self.key(object), &member),
            _ if member == "configureawait" => self
                .shape_of(object, object_depth)
                .filter(TypeShape::is_task),
            _ => None,
        }
    }

    fn me_call(&self, member: &str) -> Option<TypeShape> {
        let own = self.return_types.types.get(self.enclosing.first()?)?;
        if own.is_module {
            return None;
        }
        own.lookup(member)?
    }

    /// `T.F()` where `T` names a same-file type. A local, parameter, or
    /// member of the enclosing types with that name would take the qualifier.
    fn shared_call(&self, qualifier: &str, member: &str) -> Option<TypeShape> {
        if (self.is_shadowed)(qualifier) || self.enclosing_declares(qualifier) {
            return None;
        }
        let types = self.return_types.by_name.get(qualifier)?;
        unanimous(
            types
                .iter()
                .filter_map(|id| self.return_types.types.get(id)?.members.get(member))
                .flatten(),
        )
    }

    /// An unqualified call resolves through the innermost enclosing type that
    /// declares the name; a type with an `Inherits` clause stops the search
    /// because its base may declare it. Module functions come last.
    fn unqualified_call(&self, name: &str) -> Option<TypeShape> {
        if (self.is_shadowed)(name) {
            return None;
        }
        for id in &self.enclosing {
            let entry = self.return_types.types.get(id)?;
            if let Some(shape) = entry.lookup(name) {
                return shape;
            }
            if entry.inherits {
                return None;
            }
        }
        unanimous(
            self.return_types
                .modules
                .iter()
                .filter_map(|id| self.return_types.types.get(id)?.members.get(name))
                .flatten(),
        )
    }

    fn enclosing_declares(&self, name: &str) -> bool {
        self.enclosing.iter().any(|id| {
            self.return_types
                .types
                .get(id)
                .is_some_and(|entry| entry.members.contains_key(name))
        })
    }
}
