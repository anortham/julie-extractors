//! Declared-type fact recording for C++.

use super::declarators::structured_binding;
use super::helpers::is_template_parameter_name;
use super::identifiers::{
    enclosing_class_name, enclosing_type_name, scope_segment_name, this_receiver_type,
};
use super::name_bindings::{
    binds_locally, declaration_names, enclosing_namespaces, namespace_path,
};
use crate::base::BaseExtractor;
use crate::base::types::TypeNameRules;
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};
use std::collections::{HashMap, HashSet};
use tree_sitter::Node;

pub(super) const TYPE_NAME_RULES: TypeNameRules = TypeNameRules {
    nullable_suffixes: &[],
    reference_prefixes: &["const", "volatile", "struct", "class"],
    generic_open: &['<'],
};

/// Record a variable's written type, or for an `auto` variable the type its
/// initializer produces (`is_inferred=true`): a same-file class constructed by
/// `Foo()` or `new Foo()`, or the declared return type of a same-file callee.
pub(super) fn record_variable_fact(
    base: &mut BaseExtractor,
    symbol_id: &str,
    declaration: Node,
    declarator: Node,
    return_types: &ReturnTypeIndex,
) {
    let Some(type_node) = declaration.child_by_field_name("type") else {
        return;
    };
    if structured_binding(declarator).is_some() {
        return;
    }
    if is_auto_type(type_node) {
        let value = match declarator.kind() {
            "init_declarator" => declarator.child_by_field_name("value"),
            _ => None,
        };
        if let Some(shape) =
            value.and_then(|value| initializer_type(base, value, declaration, return_types))
        {
            let declared = if shape.declared.contains('&') && !is_decltype_auto(type_node) {
                &shape.name
            } else {
                &shape.declared
            };
            base.record_declared_type_fact_with_declared(
                symbol_id,
                &shape.name,
                declared,
                &TYPE_NAME_RULES,
                true,
            );
        }
        return;
    }
    record_stated_type(base, symbol_id, declaration, type_node, Some(declarator));
}

pub(super) fn record_parameter_fact(base: &mut BaseExtractor, symbol_id: &str, param_node: Node) {
    let Some(type_node) = param_node.child_by_field_name("type") else {
        return;
    };
    if is_auto_type(type_node) {
        return;
    }
    record_stated_type(
        base,
        symbol_id,
        param_node,
        type_node,
        param_node.child_by_field_name("declarator"),
    );
}

pub(super) fn record_field_fact(
    base: &mut BaseExtractor,
    symbol_id: &str,
    field_node: Node,
    declarator: Option<Node>,
) {
    let Some(type_node) = field_node.child_by_field_name("type") else {
        return;
    };
    if is_auto_type(type_node) {
        return;
    }
    record_stated_type(base, symbol_id, field_node, type_node, declarator);
}

/// Record a callable's stated return type: the `type` of the declaration that
/// owns its declarator, decorated by the pointer and reference declarators
/// around it, or its trailing return type when the stated type is `auto`.
pub(super) fn record_return_fact(
    base: &mut BaseExtractor,
    symbol_id: &str,
    function_declarator: Node,
) {
    let Some((_, shape)) = stated_return_type(base, function_declarator) else {
        return;
    };
    base.record_declared_type_fact_with_declared(
        symbol_id,
        &shape.name,
        &shape.declared,
        &TYPE_NAME_RULES,
        false,
    );
}

/// A callable's stated return type node, with its structural base name and
/// written text.
fn stated_return_type<'a>(
    base: &BaseExtractor,
    function_declarator: Node<'a>,
) -> Option<(Node<'a>, TypeShape)> {
    if function_declarator.kind() != "function_declarator" {
        return None;
    }
    let declarator = decorated_declarator(function_declarator);
    let owner = declarator.parent()?;
    let type_node = owner.child_by_field_name("type")?;
    if has_error_before(owner, declarator) {
        return None;
    }
    if !is_auto_type(type_node) {
        let shape = TypeShape {
            name: structural_base_name(base, type_node, 0)?,
            declared: declared_type_text(base, owner, type_node, Some(declarator)),
        };
        return Some((type_node, shape));
    }
    let mut cursor = function_declarator.walk();
    let descriptor = function_declarator
        .children(&mut cursor)
        .find(|child| child.kind() == "trailing_return_type")
        .and_then(|trailing| trailing.named_child(0))
        .filter(|descriptor| descriptor.kind() == "type_descriptor")?;
    let stated = descriptor.child_by_field_name("type")?;
    let shape = TypeShape {
        name: structural_base_name(base, stated, 0)?,
        declared: base.get_node_text(&descriptor),
    };
    Some((stated, shape))
}

/// Whether the parser gave up between a declaration's start and its
/// declarator, as it does for `MACRO static Type f()`: the `type` it reports
/// is then the macro.
fn has_error_before(owner: Node, declarator: Node) -> bool {
    owner
        .children(&mut owner.walk())
        .any(|child| child.is_error() && child.start_byte() < declarator.start_byte())
}

/// The outermost pointer or reference declarator around a function declarator.
fn decorated_declarator(function_declarator: Node) -> Node {
    let mut declarator = function_declarator;
    while let Some(parent) = declarator
        .parent()
        .filter(|parent| matches!(parent.kind(), "pointer_declarator" | "reference_declarator"))
    {
        declarator = parent;
    }
    declarator
}

fn record_stated_type(
    base: &mut BaseExtractor,
    symbol_id: &str,
    container: Node,
    type_node: Node,
    declarator: Option<Node>,
) {
    if declarator.is_some_and(|declarator| contains_function_declarator(declarator, 0)) {
        return;
    }
    record_type(base, symbol_id, container, type_node, declarator);
}

fn record_type(
    base: &mut BaseExtractor,
    symbol_id: &str,
    container: Node,
    type_node: Node,
    declarator: Option<Node>,
) {
    let Some(base_name) = structural_base_name(base, type_node, 0) else {
        return;
    };
    let declared = declared_type_text(base, container, type_node, declarator);
    base.record_declared_type_fact_with_declared(
        symbol_id,
        &base_name,
        &declared,
        &TYPE_NAME_RULES,
        false,
    );
}

fn is_auto_type(type_node: Node) -> bool {
    matches!(type_node.kind(), "placeholder_type_specifier" | "auto")
}

/// `decltype(auto)` keeps the initializer's references; plain `auto` drops them.
fn is_decltype_auto(type_node: Node) -> bool {
    type_node
        .children(&mut type_node.walk())
        .any(|child| child.kind() == "decltype")
}

fn structural_base_name(base: &BaseExtractor, node: Node, depth: u32) -> Option<String> {
    if !should_visit_tree_depth(depth) {
        return None;
    }
    let mut node = node;
    loop {
        match node.kind() {
            "type_identifier" | "identifier" | "primitive_type" | "namespace_identifier" => {
                return Some(base.get_node_text(&node));
            }
            "sized_type_specifier" => {
                return single_word_sized_type(node).map(|node| base.get_node_text(&node));
            }
            "template_type" => {
                node = node.child_by_field_name("name")?;
            }
            "qualified_identifier" => {
                let child_depth = child_tree_depth(depth)?;
                let scope = node.child_by_field_name("scope")?;
                let name = node.child_by_field_name("name")?;
                let scope_text = structural_base_name(base, scope, child_depth)
                    .unwrap_or_else(|| base.get_node_text(&scope));
                let name_text = structural_base_name(base, name, child_depth)?;
                return Some(format!("{scope_text}::{name_text}"));
            }
            "class_specifier" | "struct_specifier" | "union_specifier" | "enum_specifier" => {
                let name = node
                    .children(&mut node.walk())
                    .find(|child| child.kind() == "type_identifier")?;
                return Some(base.get_node_text(&name));
            }
            _ => return None,
        }
    }
}

fn declared_type_text(
    base: &BaseExtractor,
    container: Node,
    type_node: Node,
    declarator: Option<Node>,
) -> String {
    let mut start = type_node.start_byte();
    let mut end = type_node.end_byte();
    let mut cursor = container.walk();
    for child in container.children(&mut cursor) {
        if child.kind() == "type_qualifier" {
            start = start.min(child.start_byte());
            end = end.max(child.end_byte());
        }
    }
    let mut declared = base.content[start..end].to_string();
    if let Some(declarator) = declarator {
        declared.push_str(&decoration_suffix(base, declarator, 0));
    }
    declared
}

fn single_word_sized_type(node: Node) -> Option<Node> {
    let mut cursor = node.walk();
    let words = node
        .children(&mut cursor)
        .filter(|child| child.kind() != "type_qualifier")
        .count();
    (words == 1).then_some(node)
}

fn contains_function_declarator(node: Node, depth: u32) -> bool {
    if !should_visit_tree_depth(depth) {
        return false;
    }
    if node.kind() == "function_declarator" {
        return true;
    }
    let Some(child_depth) = child_tree_depth(depth) else {
        return false;
    };
    match node.kind() {
        "pointer_declarator"
        | "reference_declarator"
        | "array_declarator"
        | "parenthesized_declarator"
        | "init_declarator" => node
            .child_by_field_name("declarator")
            .or_else(|| node.named_child(0))
            .is_some_and(|inner| contains_function_declarator(inner, child_depth)),
        _ => false,
    }
}

fn decoration_suffix(base: &BaseExtractor, node: Node, depth: u32) -> String {
    if !should_visit_tree_depth(depth) {
        return String::new();
    }
    let Some(child_depth) = child_tree_depth(depth) else {
        return String::new();
    };
    match node.kind() {
        "pointer_declarator" => {
            let inner = node
                .child_by_field_name("declarator")
                .map(|inner| decoration_suffix(base, inner, child_depth))
                .unwrap_or_default();
            format!("*{inner}")
        }
        "reference_declarator" => {
            let kind = reference_kind(base, node);
            let inner = node
                .named_child(0)
                .map(|inner| decoration_suffix(base, inner, child_depth))
                .unwrap_or_default();
            format!("{kind}{inner}")
        }
        "init_declarator" => node
            .child_by_field_name("declarator")
            .map(|inner| decoration_suffix(base, inner, child_depth))
            .unwrap_or_default(),
        "parenthesized_declarator" | "array_declarator" => node
            .child_by_field_name("declarator")
            .or_else(|| node.named_child(0))
            .map(|inner| decoration_suffix(base, inner, child_depth))
            .unwrap_or_default(),
        _ => String::new(),
    }
}

fn reference_kind(base: &BaseExtractor, node: Node) -> &'static str {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        match base.get_node_text(&child).as_str() {
            "&&" => return "&&",
            "&" => return "&",
            _ => {}
        }
    }
    "&"
}

/// A type reduced to what a type fact records: the structural base name and
/// the written text.
#[derive(Debug, Clone, PartialEq, Eq)]
struct TypeShape {
    name: String,
    declared: String,
}

/// Declared return types of the file's callables by name, for `auto`
/// inference, with the names each class and namespace declares so that a
/// call hidden by a non-function name records nothing. Friend declarations
/// are left out: they are not members.
#[derive(Debug, Default)]
pub(super) struct ReturnTypeIndex {
    entries: HashMap<String, Vec<ReturnEntry>>,
    classes: HashMap<String, ClassScope>,
    /// Names declared at namespace scope that are not functions, by namespace path.
    namespace_names: HashMap<String, HashSet<String>>,
}

#[derive(Debug)]
struct ReturnEntry {
    /// The enclosing or qualifying class (or namespace, for an out-of-line
    /// `ns::f`); `None` for a free function.
    owner: Option<String>,
    /// The namespace path that declares a free function.
    namespace: String,
    /// `None` when the return type is absent, deduced, or a template parameter.
    shape: Option<TypeShape>,
}

/// Every same-file definition of one class name, merged.
#[derive(Debug)]
struct ClassScope {
    /// Whether unqualified lookup from its members stops at the class and its
    /// namespaces: every definition is top-level with no base.
    closed: bool,
    namespaces: HashSet<String>,
    /// Every name the class body declares, functions included.
    members: HashSet<String>,
}

impl ReturnTypeIndex {
    pub(super) fn build(base: &BaseExtractor, root: Node) -> Self {
        let mut index = Self::default();
        let mut stack = vec![root];
        while let Some(node) = stack.pop() {
            match node.kind() {
                "function_declarator" => {
                    if let Some((name, entry)) = return_entry(base, node) {
                        index.entries.entry(name).or_default().push(entry);
                    }
                }
                "class_specifier" | "struct_specifier" => index.add_class(base, node),
                _ => {}
            }
            if node.parent().is_some_and(|parent| {
                matches!(parent.kind(), "translation_unit" | "declaration_list")
            }) {
                let mut names = Vec::new();
                declaration_names(base, node, false, &mut names);
                if !names.is_empty() {
                    index
                        .namespace_names
                        .entry(namespace_path(base, node))
                        .or_default()
                        .extend(names);
                }
            }
            stack.extend(node.named_children(&mut node.walk()));
        }
        index
    }

    fn add_class(&mut self, base: &BaseExtractor, class: Node) {
        let Some(body) = class.child_by_field_name("body") else {
            return;
        };
        let Some(name) = class
            .child_by_field_name("name")
            .filter(|name| name.kind() == "type_identifier")
        else {
            return;
        };
        let has_base = class
            .children(&mut class.walk())
            .any(|child| child.kind() == "base_class_clause");
        let mut members = Vec::new();
        let mut cursor = body.walk();
        for member in body.named_children(&mut cursor) {
            declaration_names(base, member, true, &mut members);
        }
        let scope = self
            .classes
            .entry(base.get_node_text(&name))
            .or_insert_with(|| ClassScope {
                closed: true,
                namespaces: HashSet::new(),
                members: HashSet::new(),
            });
        scope.closed &= !has_base && !inside_class_body(class);
        scope.namespaces.insert(namespace_path(base, class));
        scope.members.extend(members);
    }

    /// The base type every same-named member of `owner` agrees on.
    fn lookup(&self, name: &str, owner: &str) -> Option<TypeShape> {
        agreed_shape(
            self.entries
                .get(name)?
                .iter()
                .filter(|entry| entry.owner.as_deref() == Some(owner)),
        )
    }

    /// The free function an unqualified call in `namespace` reaches: the
    /// innermost enclosing namespace that declares the name decides. A
    /// same-file candidate in any other namespace (reachable by a
    /// using-directive or argument-dependent lookup), or a non-function name
    /// in the deciding namespace, records nothing.
    fn free_lookup(&self, name: &str, namespace: &str) -> Option<TypeShape> {
        let free: Vec<&ReturnEntry> = self
            .entries
            .get(name)?
            .iter()
            .filter(|entry| entry.owner.is_none())
            .collect();
        let chain = enclosing_namespaces(namespace);
        if free
            .iter()
            .any(|entry| !chain.contains(&entry.namespace.as_str()))
        {
            return None;
        }
        for scope in chain {
            if self
                .namespace_names
                .get(scope)
                .is_some_and(|names| names.contains(name))
            {
                return None;
            }
            let mut here = free
                .iter()
                .copied()
                .filter(|entry| entry.namespace == scope)
                .peekable();
            if here.peek().is_some() {
                return agreed_shape(here);
            }
        }
        None
    }

    fn declares(&self, name: &str, owner: &str) -> bool {
        self.entries.get(name).is_some_and(|entries| {
            entries
                .iter()
                .any(|entry| entry.owner.as_deref() == Some(owner))
        })
    }

    /// The namespace an unqualified call inside a member of `class` falls
    /// back to when the class declares nothing named `name`: only for a
    /// closed class defined in one namespace.
    fn fallback_namespace(&self, class: &str, name: &str) -> Option<&str> {
        let scope = self.classes.get(class)?;
        if !scope.closed || scope.members.contains(name) || scope.namespaces.len() != 1 {
            return None;
        }
        scope.namespaces.iter().next().map(String::as_str)
    }
}

/// The base type every entry agrees on, with the written text when that
/// agrees too.
fn agreed_shape<'a>(mut entries: impl Iterator<Item = &'a ReturnEntry>) -> Option<TypeShape> {
    let mut agreed = entries.next()?.shape.clone()?;
    for entry in entries {
        let shape = entry.shape.as_ref()?;
        if shape.name != agreed.name {
            return None;
        }
        if shape.declared != agreed.declared {
            agreed.declared = agreed.name.clone();
        }
    }
    Some(agreed)
}

fn inside_class_body(node: Node) -> bool {
    let mut current = node.parent();
    while let Some(candidate) = current {
        if matches!(
            candidate.kind(),
            "class_specifier" | "struct_specifier" | "union_specifier"
        ) {
            return true;
        }
        current = candidate.parent();
    }
    false
}

fn return_entry(base: &BaseExtractor, function_declarator: Node) -> Option<(String, ReturnEntry)> {
    let holder = decorated_declarator(function_declarator).parent()?;
    let is_callable_holder = matches!(
        holder.kind(),
        "function_definition" | "declaration" | "field_declaration"
    );
    let is_friend = holder
        .parent()
        .is_some_and(|parent| parent.kind() == "friend_declaration");
    if !is_callable_holder || is_friend {
        return None;
    }
    let (name, scope) =
        callable_name(base, function_declarator.child_by_field_name("declarator")?)?;
    let owner = match scope {
        Some(scope) => Some(scope_segment_name(base, scope)?),
        None => enclosing_type_name(base, function_declarator),
    };
    let shape = stated_return_type(base, function_declarator)
        .filter(|(type_node, _)| !names_template_parameter(base, *type_node))
        .map(|(_, shape)| shape);
    let namespace = namespace_path(base, function_declarator);
    Some((
        name,
        ReturnEntry {
            owner,
            namespace,
            shape,
        },
    ))
}

/// A callable's plain name and, when written qualified, the scope segment
/// just before it.
fn callable_name<'a>(base: &BaseExtractor, node: Node<'a>) -> Option<(String, Option<Node<'a>>)> {
    let mut node = node;
    let mut scope = None;
    while node.kind() == "qualified_identifier" {
        scope = Some(node.child_by_field_name("scope")?);
        node = node.child_by_field_name("name")?;
    }
    let name = match node.kind() {
        "identifier" | "field_identifier" => node,
        "template_function" => node.child_by_field_name("name")?,
        _ => return None,
    };
    Some((base.get_node_text(&name), scope))
}

/// Whether a type's leftmost name is a template parameter in scope, so the
/// type depends on the instantiation.
fn names_template_parameter(base: &BaseExtractor, type_node: Node) -> bool {
    let mut node = type_node;
    loop {
        let next = match node.kind() {
            "qualified_identifier" => node.child_by_field_name("scope"),
            "template_type" => node.child_by_field_name("name"),
            "type_identifier" | "namespace_identifier" => {
                return is_template_parameter_name(base, &node);
            }
            _ => return false,
        };
        let Some(next) = next else {
            return false;
        };
        node = next;
    }
}

fn initializer_type(
    base: &BaseExtractor,
    value: Node,
    origin: Node,
    return_types: &ReturnTypeIndex,
) -> Option<TypeShape> {
    if let Some(name) = inferred_constructor_name(base, value, origin) {
        return Some(TypeShape {
            declared: name.clone(),
            name,
        });
    }
    if value.kind() != "call_expression" {
        return None;
    }
    let function = value.child_by_field_name("function")?;
    match function.kind() {
        "identifier" | "template_function" | "qualified_identifier" => {
            let (name, scope) = callable_name(base, function)?;
            match scope {
                Some(scope) if is_template_parameter_name(base, &scope) => None,
                Some(scope) => return_types.lookup(&name, &scope_segment_name(base, scope)?),
                None if binds_locally(base, value, &name) => None,
                None => match enclosing_class_name(base, value) {
                    Some(class) if return_types.declares(&name, &class) => {
                        return_types.lookup(&name, &class)
                    }
                    Some(class) => return_types
                        .fallback_namespace(&class, &name)
                        .and_then(|namespace| return_types.free_lookup(&name, namespace)),
                    None if inside_class_body(value) => None,
                    None => return_types.free_lookup(&name, &namespace_path(base, value)),
                },
            }
        }
        "field_expression" => {
            let class = this_receiver_type(base, function)?;
            let field = function.child_by_field_name("field")?;
            if field.kind() != "field_identifier" {
                return None;
            }
            return_types.lookup(&base.get_node_text(&field), &class)
        }
        _ => None,
    }
}

fn inferred_constructor_name(base: &BaseExtractor, value: Node, origin: Node) -> Option<String> {
    match value.kind() {
        "call_expression" => {
            let function = value.child_by_field_name("function")?;
            if function.kind() != "identifier" {
                return None;
            }
            let name = base.get_node_text(&function);
            same_file_defines_type(base, origin, &name).then_some(name)
        }
        "new_expression" => {
            let type_node = value.child_by_field_name("type")?;
            if type_node.kind() == "qualified_identifier" {
                return None;
            }
            let name = structural_base_name(base, type_node, 0)?;
            same_file_defines_type(base, origin, &name).then_some(name)
        }
        _ => None,
    }
}

fn same_file_defines_type(base: &BaseExtractor, node: Node, name: &str) -> bool {
    find_named_type(file_root(node), base, name, 0)
}

fn file_root(mut node: Node) -> Node {
    while let Some(parent) = node.parent() {
        node = parent;
    }
    node
}

fn find_named_type(node: Node, base: &BaseExtractor, name: &str, depth: u32) -> bool {
    if !should_visit_tree_depth(depth) {
        return false;
    }
    if matches!(
        node.kind(),
        "class_specifier" | "struct_specifier" | "union_specifier"
    ) {
        let found = node
            .children(&mut node.walk())
            .any(|child| child.kind() == "type_identifier" && base.get_node_text(&child) == name);
        if found {
            return true;
        }
    }
    let Some(child_depth) = child_tree_depth(depth) else {
        return false;
    };
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if find_named_type(child, base, name, child_depth) {
            return true;
        }
    }
    false
}
