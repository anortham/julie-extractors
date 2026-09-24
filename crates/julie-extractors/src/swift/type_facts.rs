//! Declared-type fact recording for Swift.

use super::signatures::return_type_node;
use crate::base::BaseExtractor;
use crate::base::types::{TypeNameRules, strip_type_decorations};
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};
use std::collections::{HashMap, HashSet};
use tree_sitter::Node;

pub(super) const SWIFT_TYPE_NAME_RULES: TypeNameRules = TypeNameRules {
    nullable_suffixes: &["?", "!"],
    reference_prefixes: &["inout"],
    generic_open: &['<'],
};

pub(super) fn collect_type_names(base: &BaseExtractor, root: Node) -> HashSet<String> {
    let mut names = HashSet::new();
    collect_type_names_into(base, root, 0, &mut names);
    names
}

fn collect_type_names_into(
    base: &BaseExtractor,
    node: Node,
    depth: u32,
    names: &mut HashSet<String>,
) {
    if !should_visit_tree_depth(depth) {
        return;
    }
    if node.kind() == "class_declaration"
        && let Some(name_node) = node.child_by_field_name("name")
    {
        insert_type_name(base, name_node, names);
    }
    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_type_names_into(base, child, child_depth, names);
    }
}

fn insert_type_name(base: &BaseExtractor, name_node: Node, names: &mut HashSet<String>) {
    let text = base.get_node_text(&name_node);
    let resolved = strip_type_decorations(&text, &SWIFT_TYPE_NAME_RULES);
    if !resolved.is_empty() {
        names.insert(resolved);
    }
}

/// Record a syntactically stated type for a symbol (`is_inferred=false`).
pub(super) fn record_declared_type(base: &mut BaseExtractor, symbol_id: &str, type_node: Node) {
    record_type_node(base, symbol_id, type_node, false);
}

pub(super) fn record_declared_type_text(
    base: &mut BaseExtractor,
    symbol_id: &str,
    type_node: Node,
    declared_text: &str,
) {
    let Some(base_name) = base_type_name(base, type_node) else {
        return;
    };
    base.record_declared_type_fact_with_declared(
        symbol_id,
        &base_name,
        declared_text,
        &SWIFT_TYPE_NAME_RULES,
        false,
    );
}

/// Record the type an untyped binding's initializer produces
/// (`is_inferred=true`): `Foo(...)` when `Foo` names a same-file type, or a
/// call to a same-file function or method with a declared return type.
/// `try`, `try!`, and `await` pass the type through, `try?` makes it optional,
/// and a postfix `!` removes one optional layer. Anything else records nothing.
pub(super) fn record_initializer_type(
    base: &mut BaseExtractor,
    symbol_id: &str,
    value: Node,
    same_file_type_names: &HashSet<String>,
    return_types: &ReturnTypeIndex,
) {
    let scope = InitializerScope {
        base,
        same_file_type_names,
        return_types,
    };
    let Some(TypeShape { name, declared }) = scope.shape_of(value, 0) else {
        return;
    };
    base.record_declared_type_fact_with_declared(
        symbol_id,
        &name,
        &declared,
        &SWIFT_TYPE_NAME_RULES,
        true,
    );
}

/// A return type reduced to its bindable base name and its written text.
#[derive(Debug, Clone, PartialEq, Eq)]
struct TypeShape {
    name: String,
    declared: String,
}

impl TypeShape {
    fn optional(self) -> Self {
        let wrapped = self
            .declared
            .strip_suffix(['?', '!'])
            .unwrap_or(&self.declared);
        Self {
            declared: format!("{wrapped}?"),
            ..self
        }
    }

    fn forced(self) -> Self {
        let declared = self
            .declared
            .strip_suffix(['?', '!'])
            .map(str::to_string)
            .unwrap_or(self.declared);
        Self { declared, ..self }
    }
}

/// Where a declaration or call sits: at file scope, in a same-file type or
/// an extension of one, or in a type context whose members are unknown
/// (a protocol, or an extension of a type declared elsewhere).
#[derive(Debug, Clone, PartialEq, Eq)]
enum TypeContext {
    File,
    Type(String),
    Unknown,
}

#[derive(Debug)]
enum Owner {
    Free,
    /// A function nested in a body; visible only inside this byte range.
    Local(std::ops::Range<usize>),
    Type(String),
}

/// The declaration a bare type name refers to from some point in the file.
#[derive(Debug, Clone, PartialEq, Eq)]
enum NameScope {
    /// A top-level declaration, or a name the file does not declare.
    File,
    /// A nested type, typealias, or generic parameter of a same-file type.
    Type(String),
    /// A generic parameter of a function, or a type declared in a local body.
    Local(std::ops::Range<usize>),
    /// A scope whose declarations are unknown: a protocol, an inheriting
    /// type, or an extension of a type declared elsewhere.
    Unknown,
}

/// Where a same-file type is declared, which decides where its bare name
/// is visible.
#[derive(Debug, PartialEq, Eq)]
enum TypeParent {
    TopLevel,
    Type(String),
    Local,
}

#[derive(Debug)]
struct Parameter {
    /// The argument label a call must write; `None` for `_`.
    label: Option<String>,
    has_default: bool,
    variadic: bool,
    function_typed: bool,
}

/// One call argument: a label (`None` when unlabeled), or the unlabeled
/// trailing closure, which Swift matches by type instead of by label.
#[derive(Debug)]
enum Argument {
    Label(Option<String>),
    TrailingClosure,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Fit {
    No,
    Maybe,
    Yes,
}

#[derive(Debug)]
struct ReturnEntry {
    owner: Owner,
    is_static: bool,
    parameters: Vec<Parameter>,
    /// `None` when the function declares no usable return type. The scope
    /// is what the head of the return type names at the callee.
    shape: Option<(TypeShape, NameScope)>,
}

impl ReturnEntry {
    /// Whether a call with `arguments` can bind to this function by labels
    /// and argument count. An unlabeled trailing closure that does not land
    /// on a required function-typed parameter gives `Maybe`: Swift's
    /// forward-scan rule also looks at parameter types.
    fn fit(&self, arguments: &[Argument]) -> Fit {
        let mut arguments = arguments.iter().peekable();
        for parameter in &self.parameters {
            match arguments.peek() {
                Some(Argument::TrailingClosure) => {
                    if !parameter.function_typed || parameter.has_default || parameter.variadic {
                        return Fit::Maybe;
                    }
                    arguments.next();
                }
                Some(Argument::Label(label)) if *label == parameter.label => {
                    arguments.next();
                    while parameter.variadic
                        && matches!(arguments.peek(), Some(Argument::Label(None)))
                    {
                        arguments.next();
                    }
                }
                _ if parameter.has_default || parameter.variadic => {}
                _ => return Fit::No,
            }
        }
        if arguments.next().is_some() {
            Fit::No
        } else {
            Fit::Yes
        }
    }
}

/// Same-file facts that call-initializer inference needs, built once per file.
/// Protocol members and members of extensions of types declared elsewhere
/// are left out: their `Self`, associated types, and generics are unknown.
#[derive(Debug, Default)]
pub(super) struct ReturnTypeIndex {
    functions: HashMap<String, Vec<ReturnEntry>>,
    /// Names bound as a variable, property, parameter, or enum case anywhere.
    value_names: HashSet<String>,
    /// `(type, name)` for properties and enum cases a same-file type declares.
    member_values: HashSet<(String, String)>,
    /// Class, struct, enum, and actor names declared exactly once in the file.
    /// A name declared twice (two nested `Node` types) is left out, because
    /// the index keys owners by simple name.
    declared_types: HashSet<String>,
    /// Same-file types with an inheritance clause on a declaration or extension.
    inheriting_types: HashSet<String>,
    type_parents: HashMap<String, TypeParent>,
    /// Nested type, typealias, and generic parameter names of each same-file
    /// type, from its declaration and its same-file extensions.
    type_members: HashMap<String, HashSet<String>>,
}

impl ReturnTypeIndex {
    pub(super) fn build(base: &BaseExtractor, root: Node) -> Self {
        let mut index = Self::default();
        let mut type_generics: HashMap<String, Vec<String>> = HashMap::new();
        let mut declaration_counts: HashMap<String, usize> = HashMap::new();
        let mut functions = Vec::new();
        let mut members = Vec::new();
        let mut typealiases = Vec::new();
        let mut type_declarations = Vec::new();
        let mut stack = vec![root];
        while let Some(node) = stack.pop() {
            index.value_names.extend(
                node.children_by_field_name("bound_identifier", &mut node.walk())
                    .map(|name| base.get_node_text(&name)),
            );
            if node.kind() == "class_declaration" {
                type_declarations.push(node);
            }
            match node.kind() {
                "class_declaration" if !is_extension(base, node) => {
                    if let Some(name) = node.child_by_field_name("name") {
                        let name = base.get_node_text(&name);
                        type_generics
                            .entry(name.clone())
                            .or_default()
                            .extend(type_parameter_names(base, node));
                        index
                            .type_parents
                            .insert(name.clone(), type_parent(base, node));
                        *declaration_counts.entry(name).or_default() += 1;
                    }
                }
                "function_declaration" => functions.push(node),
                "typealias_declaration" => typealiases.push(node),
                "property_declaration" | "enum_entry" => members.push(node),
                "pattern" | "parameter" | "lambda_parameter" => {
                    index
                        .value_names
                        .extend(named_identifiers(node).map(|name| base.get_node_text(&name)));
                }
                "capture_list_item" => {
                    if let Some(name) = node
                        .child_by_field_name("name")
                        .filter(|name| name.kind() == "simple_identifier")
                    {
                        index.value_names.insert(base.get_node_text(&name));
                    }
                }
                _ => {}
            }
            stack.extend(node.named_children(&mut node.walk()));
        }
        index.declared_types = declaration_counts
            .into_iter()
            .filter(|(_, count)| *count == 1)
            .map(|(name, _)| name)
            .collect();
        index.inheriting_types = inheriting_types(base, root, &index.declared_types);
        index.add_generic_typealiases(base, &typealiases, &mut type_generics);
        for declaration in type_declarations {
            let TypeContext::Type(owner) = index.type_context(base, declaration) else {
                continue;
            };
            let members = index.type_members.entry(owner.clone()).or_default();
            members.extend(type_generics.get(&owner).into_iter().flatten().cloned());
            if let Some(body) = declaration.child_by_field_name("body") {
                members.extend(declared_type_names(base, body));
            }
        }
        for member in members {
            let TypeContext::Type(owner) = index.member_context(base, member) else {
                continue;
            };
            let names: Vec<String> = if member.kind() == "enum_entry" {
                let mut cursor = member.walk();
                member
                    .children_by_field_name("name", &mut cursor)
                    .map(|name| base.get_node_text(&name))
                    .collect()
            } else {
                let mut cursor = member.walk();
                member
                    .children_by_field_name("name", &mut cursor)
                    .flat_map(|pattern| {
                        pattern
                            .child_by_field_name("bound_identifier")
                            .into_iter()
                            .chain(named_identifiers(pattern))
                    })
                    .map(|name| base.get_node_text(&name))
                    .collect()
            };
            index.value_names.extend(names.iter().cloned());
            index
                .member_values
                .extend(names.into_iter().map(|name| (owner.clone(), name)));
        }
        for function in functions {
            if let Some((name, entry)) = index.return_entry(base, function, &type_generics) {
                index.functions.entry(name).or_default().push(entry);
            }
        }
        index
    }

    /// Treat a member typealias whose right-hand side names a generic
    /// parameter as a generic itself, repeated so aliases of aliases count.
    fn add_generic_typealiases(
        &self,
        base: &BaseExtractor,
        typealiases: &[Node],
        type_generics: &mut HashMap<String, Vec<String>>,
    ) {
        let mut changed = true;
        while changed {
            changed = false;
            for alias in typealiases {
                let TypeContext::Type(owner) = self.member_context(base, *alias) else {
                    continue;
                };
                let Some(name_node) = alias.child_by_field_name("name") else {
                    continue;
                };
                let name = base.get_node_text(&name_node);
                let generics = self.enclosing_generics(base, *alias, type_generics);
                if generics.contains(&name) {
                    continue;
                }
                let names_generic = type_identifiers(*alias).into_iter().any(|identifier| {
                    identifier.id() != name_node.id()
                        && generics.contains(&base.get_node_text(&identifier))
                });
                if names_generic {
                    type_generics.entry(owner).or_default().push(name);
                    changed = true;
                }
            }
        }
    }

    /// The generic parameters of every same-file type that encloses `node`.
    fn enclosing_generics(
        &self,
        base: &BaseExtractor,
        node: Node,
        type_generics: &HashMap<String, Vec<String>>,
    ) -> Vec<String> {
        let mut generics = Vec::new();
        let mut current = node.parent();
        while let Some(parent) = current {
            if parent.kind() == "class_declaration"
                && let TypeContext::Type(owner) = self.type_context(base, parent)
            {
                generics.extend(type_generics.get(&owner).into_iter().flatten().cloned());
            }
            current = parent.parent();
        }
        generics
    }

    fn return_entry(
        &self,
        base: &BaseExtractor,
        function: Node,
        type_generics: &HashMap<String, Vec<String>>,
    ) -> Option<(String, ReturnEntry)> {
        let name = base.get_node_text(&function.child_by_field_name("name")?);
        let parent = function.parent()?;
        let owner = match parent.kind() {
            "source_file" => Owner::Free,
            "class_body" | "enum_class_body" => match self.member_context(base, function) {
                TypeContext::Type(owner) => Owner::Type(owner),
                _ => return None,
            },
            _ => Owner::Local(parent.byte_range()),
        };
        let mut generics = Vec::new();
        let mut current = Some(function);
        while let Some(node) = current {
            match node.kind() {
                "function_declaration" | "init_declaration" | "subscript_declaration" => {
                    generics.extend(type_parameter_names(base, node))
                }
                "class_declaration" => match self.type_context(base, node) {
                    TypeContext::Type(owner) => {
                        generics.extend(type_generics.get(&owner).into_iter().flatten().cloned())
                    }
                    _ => return None,
                },
                _ => {}
            }
            current = node.parent();
        }
        let self_type = match &owner {
            Owner::Type(owner) => Some(owner.as_str()),
            _ => None,
        };
        let shape = return_type_node(function).and_then(|type_node| {
            let (core, optional_layers) = unwrap_optional_spellings(base, type_node);
            let name = base_type_name(base, core)?;
            let head = name.split('.').next().unwrap_or(&name);
            let name = match head {
                "Self" if name == "Self" => self_type?.to_string(),
                _ if generics.iter().any(|generic| generic == head) || head == "Self" => {
                    return None;
                }
                _ => name,
            };
            let scope = self.name_scope(base, name.split('.').next()?, function);
            let implicitly_unwrapped = type_node
                .next_sibling()
                .is_some_and(|next| next.kind() == "!");
            let optional_layers = optional_layers + usize::from(implicitly_unwrapped);
            let declared = if optional_layers == 0 {
                base.get_node_text(&type_node)
            } else {
                format!(
                    "{}{}",
                    base.get_node_text(&core),
                    "?".repeat(optional_layers)
                )
            };
            Some((TypeShape { name, declared }, scope))
        });
        let entry = ReturnEntry {
            owner,
            is_static: is_static(base, function),
            parameters: parameters(base, function),
            shape,
        };
        Some((name, entry))
    }

    /// The type context of a declaration held directly in a type body.
    fn member_context(&self, base: &BaseExtractor, member: Node) -> TypeContext {
        match member.parent().and_then(|body| body.parent()) {
            Some(owner) if owner.kind() == "class_declaration" => self.type_context(base, owner),
            _ => TypeContext::Unknown,
        }
    }

    /// An extension can only name a top-level type, so an extension whose
    /// name matches a nested same-file type extends a type from elsewhere.
    fn type_context(&self, base: &BaseExtractor, declaration: Node) -> TypeContext {
        type_declaration_name(base, declaration)
            .filter(|name| self.declared_types.contains(name))
            .filter(|name| {
                !is_extension(base, declaration)
                    || self.type_parents.get(name) == Some(&TypeParent::TopLevel)
            })
            .map_or(TypeContext::Unknown, TypeContext::Type)
    }

    /// Whether the bare type name at `call` refers to the same-file type of
    /// that name and not to a generic parameter, typealias, or other type
    /// that shadows it. A local type is never visible this way.
    fn type_visible(&self, base: &BaseExtractor, name: &str, call: Node) -> bool {
        let declared_in = match self.type_parents.get(name) {
            Some(TypeParent::TopLevel) => NameScope::File,
            Some(TypeParent::Type(parent)) => NameScope::Type(parent.clone()),
            _ => return false,
        };
        self.name_scope(base, name, call) == declared_in
    }

    /// The declaration a bare type name refers to at `from`: the nearest
    /// enclosing generic parameter list, local body, or same-file type that
    /// declares it, else file scope. A scope with unknown declarations on
    /// the way gives `Unknown`.
    fn name_scope(&self, base: &BaseExtractor, name: &str, from: Node) -> NameScope {
        let mut current = Some(from);
        while let Some(node) = current {
            match node.kind() {
                "function_declaration" | "init_declaration" | "subscript_declaration"
                    if type_parameter_names(base, node).iter().any(|g| g == name) =>
                {
                    return NameScope::Local(node.byte_range());
                }
                "statements" if declared_type_names(base, node).any(|n| n == name) => {
                    return NameScope::Local(node.byte_range());
                }
                "class_declaration" => {
                    let TypeContext::Type(owner) = self.type_context(base, node) else {
                        return NameScope::Unknown;
                    };
                    if self
                        .type_members
                        .get(&owner)
                        .is_some_and(|members| members.contains(name))
                    {
                        return NameScope::Type(owner);
                    }
                    if self.inheriting_types.contains(&owner) {
                        return NameScope::Unknown;
                    }
                }
                "protocol_declaration" => return NameScope::Unknown,
                _ => {}
            }
            current = node.parent();
        }
        NameScope::File
    }

    /// The type context enclosing `node`, from the nearest type declaration.
    fn enclosing_context(&self, base: &BaseExtractor, node: Node) -> TypeContext {
        let mut current = node.parent();
        while let Some(parent) = current {
            match parent.kind() {
                "class_declaration" => return self.type_context(base, parent),
                _ => current = parent.parent(),
            }
        }
        TypeContext::File
    }

    /// The return type that every candidate the call may bind to agrees
    /// on, when at least one candidate surely fits the call's labels.
    fn resolve<'a>(
        entries: impl IntoIterator<Item = &'a ReturnEntry>,
        arguments: &[Argument],
    ) -> Option<(TypeShape, NameScope)> {
        let mut any_sure_fit = false;
        let mut shapes = Vec::new();
        for entry in entries {
            match entry.fit(arguments) {
                Fit::No => continue,
                Fit::Maybe => {}
                Fit::Yes => any_sure_fit = true,
            }
            shapes.push(entry.shape.as_ref());
        }
        let first = (*shapes.first()?)?;
        (any_sure_fit && shapes.iter().all(|shape| *shape == Some(first))).then(|| first.clone())
    }

    /// A member call on a same-file type. A type with an inheritance clause
    /// records nothing: a base class or a protocol extension can add a
    /// same-named overload with other labels that the call picks instead.
    fn member_call(
        &self,
        owner: &str,
        name: &str,
        static_only: bool,
        arguments: &[Argument],
    ) -> Option<(TypeShape, NameScope)> {
        if self.inheriting_types.contains(owner)
            || self
                .member_values
                .contains(&(owner.to_string(), name.to_string()))
        {
            return None;
        }
        let members: Vec<&ReturnEntry> = self
            .functions
            .get(name)?
            .iter()
            .filter(|entry| matches!(&entry.owner, Owner::Type(o) if o == owner))
            .collect();
        if static_only && members.iter().any(|entry| !entry.is_static) {
            return None;
        }
        Self::resolve(members, arguments)
    }

    /// An unqualified call: lookup walks out from the call, and the first
    /// scope that declares the name wins. A local function counts in the
    /// body that holds it, a member in its type, and a free function at file
    /// scope. Lookup stops at a type context that is unknown or inherits,
    /// because it may hold unseen members.
    fn unqualified_call(
        &self,
        base: &BaseExtractor,
        name: &str,
        call: Node,
        arguments: &[Argument],
    ) -> Option<(TypeShape, NameScope)> {
        if self.value_names.contains(name) {
            return None;
        }
        let entries = self.functions.get(name)?;
        let mut static_only = false;
        let mut current = call.parent();
        while let Some(node) = current {
            let locals: Vec<&ReturnEntry> = entries
                .iter()
                .filter(|entry| matches!(&entry.owner, Owner::Local(scope) if *scope == node.byte_range()))
                .collect();
            if !locals.is_empty() {
                return Self::resolve(locals, arguments);
            }
            if node.kind() == "class_declaration" {
                let TypeContext::Type(owner) = self.type_context(base, node) else {
                    return None;
                };
                if entries
                    .iter()
                    .any(|e| matches!(&e.owner, Owner::Type(o) if *o == owner))
                {
                    return self.member_call(&owner, name, static_only, arguments);
                }
                if self.inheriting_types.contains(&owner) {
                    return None;
                }
                static_only = true;
            }
            current = node.parent();
        }
        Self::resolve(
            entries.iter().filter(|e| matches!(e.owner, Owner::Free)),
            arguments,
        )
    }
}

struct InitializerScope<'a> {
    base: &'a BaseExtractor,
    same_file_type_names: &'a HashSet<String>,
    return_types: &'a ReturnTypeIndex,
}

impl InitializerScope<'_> {
    fn shape_of(&self, value: Node, depth: u32) -> Option<TypeShape> {
        if !should_visit_tree_depth(depth) {
            return None;
        }
        match value.kind() {
            "try_expression" => {
                let inner =
                    self.shape_of(value.child_by_field_name("expr")?, child_tree_depth(depth)?)?;
                let operator = value
                    .named_children(&mut value.walk())
                    .find(|child| child.kind() == "try_operator")?;
                if self.base.get_node_text(&operator).ends_with('?') {
                    Some(inner.optional())
                } else {
                    Some(inner)
                }
            }
            "await_expression" => {
                self.shape_of(value.child_by_field_name("expr")?, child_tree_depth(depth)?)
            }
            "postfix_expression" => {
                let operation = value.child_by_field_name("operation")?;
                if operation.kind() != "bang" {
                    return None;
                }
                Some(
                    self.shape_of(
                        value.child_by_field_name("target")?,
                        child_tree_depth(depth)?,
                    )?
                    .forced(),
                )
            }
            "call_expression" => self.call_shape(value),
            _ => None,
        }
    }

    /// A call's return type, kept only when the head of its name means the
    /// same declaration at the call as at the callee.
    fn call_shape(&self, call: Node) -> Option<TypeShape> {
        let callee = forced_try_callee(self.base, call, call.named_child(0)?);
        if callee.kind() == "simple_identifier" {
            let name = self.base.get_node_text(&callee);
            if self.same_file_type_names.contains(&name) {
                call_arguments(self.base, call)?;
                return Some(TypeShape {
                    declared: name.clone(),
                    name,
                });
            }
        }
        let (shape, scope) = self.callee_shape(call, callee)?;
        let head = shape.name.split('.').next()?;
        (scope != NameScope::Unknown
            && self.return_types.name_scope(self.base, head, call) == scope)
            .then_some(shape)
    }

    fn callee_shape(&self, call: Node, callee: Node) -> Option<(TypeShape, NameScope)> {
        let arguments = call_arguments(self.base, call)?;
        let index = self.return_types;
        match callee.kind() {
            "simple_identifier" => {
                let name = self.base.get_node_text(&callee);
                index.unqualified_call(self.base, &name, call, &arguments)
            }
            "navigation_expression" => {
                let method = callee
                    .child_by_field_name("suffix")?
                    .child_by_field_name("suffix")?;
                if method.kind() != "simple_identifier" {
                    return None;
                }
                let method = self.base.get_node_text(&method);
                let target = callee.child_by_field_name("target")?;
                let context = || match index.enclosing_context(self.base, call) {
                    TypeContext::Type(owner) => Some(owner),
                    _ => None,
                };
                match target.kind() {
                    "self_expression" => index.member_call(&context()?, &method, false, &arguments),
                    "simple_identifier" => {
                        let target = self.base.get_node_text(&target);
                        let owner = if target == "Self" {
                            context()?
                        } else if index.declared_types.contains(&target)
                            && !index.value_names.contains(&target)
                            && index.type_visible(self.base, &target, call)
                        {
                            target
                        } else {
                            return None;
                        };
                        index.member_call(&owner, &method, true, &arguments)
                    }
                    _ => None,
                }
            }
            _ => None,
        }
    }
}

/// The labels of a call's arguments, trailing closures included. `None`
/// for a subscript (`Foo[0]` parses as a call whose arguments open with `[`)
/// or an argument shape this reader does not know.
/// The callee of `try! f()` when the scanner split `try!` into `try` and a
/// prefix `!` bound to the callee.
///
/// tree-sitter-swift 0.7.3 builds its `try!` suppression mask with
/// `1UL << FAKE_TRY_BANG`, and `FAKE_TRY_BANG` is token 32. Where `long` is
/// 32 bits (Windows), the shift is undefined, `!` becomes an operator, and
/// `try! f()` parses as `try (!f)()`. A `!` that starts at the very end of the
/// `try` keyword can only be the `try!` spelling, so the prefix is dropped.
fn forced_try_callee<'tree>(
    base: &BaseExtractor,
    call: Node<'tree>,
    callee: Node<'tree>,
) -> Node<'tree> {
    let split_try_bang = callee.kind() == "prefix_expression"
        && callee
            .child_by_field_name("operation")
            .filter(|operation| operation.kind() == "bang")
            .zip(
                call.parent()
                    .filter(|parent| parent.kind() == "try_expression")
                    .and_then(|parent| {
                        parent
                            .named_children(&mut parent.walk())
                            .find(|child| child.kind() == "try_operator")
                    }),
            )
            .is_some_and(|(bang, operator)| {
                base.get_node_text(&operator) == "try" && operator.end_byte() == bang.start_byte()
            });
    if split_try_bang {
        callee.child_by_field_name("target").unwrap_or(callee)
    } else {
        callee
    }
}

fn call_arguments(base: &BaseExtractor, call: Node) -> Option<Vec<Argument>> {
    let suffix = call
        .named_children(&mut call.walk())
        .find(|child| child.kind() == "call_suffix")?;
    let mut arguments = Vec::new();
    let mut closure_label = None;
    for part in suffix.named_children(&mut suffix.walk()) {
        match part.kind() {
            "value_arguments" => {
                if part.child(0)?.kind() == "[" {
                    return None;
                }
                arguments.extend(
                    part.named_children(&mut part.walk())
                        .filter(|argument| argument.kind() == "value_argument")
                        .map(|argument| {
                            Argument::Label(
                                argument
                                    .child_by_field_name("name")
                                    .map(|label| base.get_node_text(&label)),
                            )
                        }),
                );
            }
            "simple_identifier" => closure_label = Some(base.get_node_text(&part)),
            "lambda_literal" => arguments.push(match closure_label.take() {
                Some(label) => Argument::Label(Some(label)),
                None => Argument::TrailingClosure,
            }),
            "comment" | "multiline_comment" => {}
            _ => return None,
        }
    }
    Some(arguments)
}

fn parameters(base: &BaseExtractor, function: Node) -> Vec<Parameter> {
    let mut parameters: Vec<Parameter> = Vec::new();
    let mut cursor = function.walk();
    for (index, child) in function.children(&mut cursor).enumerate() {
        if child.kind() == "parameter" {
            parameters.push(parameter(base, child));
        } else if function.field_name_for_child(index as u32) == Some("default_value")
            && let Some(last) = parameters.last_mut()
        {
            last.has_default = true;
        }
    }
    parameters
}

fn parameter(base: &BaseExtractor, node: Node) -> Parameter {
    let label = node
        .child_by_field_name("external_name")
        .or_else(|| node.child_by_field_name("name"))
        .map(|label| base.get_node_text(&label))
        .filter(|label| label != "_");
    let mut cursor = node.walk();
    let function_typed = node
        .children_by_field_name("name", &mut cursor)
        .any(|child| child.kind() == "function_type");
    Parameter {
        label,
        has_default: false,
        variadic: node
            .children(&mut node.walk())
            .any(|child| child.kind() == "..."),
        function_typed,
    }
}

/// Where a non-extension type declaration sits: at file scope, directly in
/// a type body, or in a function or other local body.
fn type_parent(base: &BaseExtractor, declaration: Node) -> TypeParent {
    match declaration.parent() {
        Some(parent) if parent.kind() == "source_file" => TypeParent::TopLevel,
        Some(body) if matches!(body.kind(), "class_body" | "enum_class_body") => body
            .parent()
            .and_then(|owner| type_declaration_name(base, owner))
            .map_or(TypeParent::Local, TypeParent::Type),
        _ => TypeParent::Local,
    }
}

/// Strip `T?`, `Optional<T>`, and `ImplicitlyUnwrappedOptional<T>` (also
/// `Swift.`-qualified) from a type node, and count the layers removed.
fn unwrap_optional_spellings<'a>(base: &BaseExtractor, node: Node<'a>) -> (Node<'a>, usize) {
    let mut node = node;
    let mut layers = 0;
    loop {
        let wrapped = match node.kind() {
            "optional_type" => node.child_by_field_name("wrapped"),
            "user_type" => optional_payload(base, node),
            _ => None,
        };
        let Some(wrapped) = wrapped else {
            return (node, layers);
        };
        node = wrapped;
        layers += 1;
    }
}

fn optional_payload<'a>(base: &BaseExtractor, user_type: Node<'a>) -> Option<Node<'a>> {
    let mut cursor = user_type.walk();
    let children: Vec<Node> = user_type.named_children(&mut cursor).collect();
    let (arguments, segments) = children.split_last()?;
    let segments: Vec<String> = segments
        .iter()
        .map(|segment| (segment.kind() == "type_identifier").then(|| base.get_node_text(segment)))
        .collect::<Option<_>>()?;
    let is_optional = matches!(
        segments
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>()
            .as_slice(),
        ["Optional" | "ImplicitlyUnwrappedOptional"]
            | ["Swift", "Optional" | "ImplicitlyUnwrappedOptional"]
    );
    if !is_optional || arguments.kind() != "type_arguments" {
        return None;
    }
    let mut cursor = arguments.walk();
    let payload: Vec<Node> = arguments.named_children(&mut cursor).collect();
    match payload.as_slice() {
        [payload] => Some(*payload),
        _ => None,
    }
}

/// Names of the types, typealiases, protocols, and associated types declared
/// directly in `body`.
fn declared_type_names<'a>(
    base: &'a BaseExtractor,
    body: Node<'a>,
) -> impl Iterator<Item = String> + 'a {
    let mut cursor = body.walk();
    body.named_children(&mut cursor)
        .filter(|child| match child.kind() {
            "class_declaration" => !is_extension(base, *child),
            "typealias_declaration" | "protocol_declaration" | "associatedtype_declaration" => true,
            _ => false,
        })
        .filter_map(|child| child.child_by_field_name("name"))
        .map(|name| base.get_node_text(&name))
        .collect::<Vec<_>>()
        .into_iter()
}

fn type_identifiers(node: Node) -> Vec<Node> {
    let mut found = Vec::new();
    let mut stack = vec![node];
    while let Some(current) = stack.pop() {
        if current.kind() == "type_identifier" {
            found.push(current);
        }
        stack.extend(current.named_children(&mut current.walk()));
    }
    found
}

fn is_extension(base: &BaseExtractor, declaration: Node) -> bool {
    declaration
        .child_by_field_name("declaration_kind")
        .is_some_and(|kind| base.get_node_text(&kind) == "extension")
}

/// The name a class-like declaration declares or, for an extension, the
/// single-segment name it extends.
fn type_declaration_name(base: &BaseExtractor, declaration: Node) -> Option<String> {
    let name = declaration.child_by_field_name("name")?;
    match name.kind() {
        "type_identifier" => Some(base.get_node_text(&name)),
        "user_type" => {
            let mut cursor = name.walk();
            let segments: Vec<Node> = name.named_children(&mut cursor).collect();
            match segments.as_slice() {
                [segment] if segment.kind() == "type_identifier" => {
                    Some(base.get_node_text(segment))
                }
                _ => None,
            }
        }
        _ => None,
    }
}

fn inheriting_types(
    base: &BaseExtractor,
    root: Node,
    declared_types: &HashSet<String>,
) -> HashSet<String> {
    let mut inheriting = HashSet::new();
    let mut stack = vec![root];
    while let Some(node) = stack.pop() {
        if node.kind() == "class_declaration"
            && node
                .named_children(&mut node.walk())
                .any(|child| child.kind() == "inheritance_specifier")
            && let Some(name) = type_declaration_name(base, node)
            && declared_types.contains(&name)
        {
            inheriting.insert(name);
        }
        stack.extend(node.named_children(&mut node.walk()));
    }
    inheriting
}

fn named_identifiers<'a>(node: Node<'a>) -> impl Iterator<Item = Node<'a>> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor)
        .filter(|child| child.kind() == "simple_identifier")
        .collect::<Vec<_>>()
        .into_iter()
}

fn type_parameter_names(base: &BaseExtractor, declaration: Node) -> Vec<String> {
    let mut cursor = declaration.walk();
    declaration
        .named_children(&mut cursor)
        .filter(|child| child.kind() == "type_parameters")
        .flat_map(|parameters| {
            let mut cursor = parameters.walk();
            parameters
                .named_children(&mut cursor)
                .filter(|parameter| parameter.kind() == "type_parameter")
                .filter_map(|parameter| {
                    parameter
                        .named_children(&mut parameter.walk())
                        .find(|child| child.kind() == "type_identifier")
                })
                .collect::<Vec<_>>()
        })
        .map(|name| base.get_node_text(&name))
        .collect()
}

fn is_static(base: &BaseExtractor, function: Node) -> bool {
    function.children(&mut function.walk()).any(|child| {
        matches!(child.kind(), "static" | "class")
            || (child.kind() == "modifiers"
                && child.named_children(&mut child.walk()).any(|modifier| {
                    matches!(base.get_node_text(&modifier).as_str(), "static" | "class")
                }))
    })
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
        &SWIFT_TYPE_NAME_RULES,
        is_inferred,
    );
}

/// The base type name a type node states, with namespace qualifiers kept and
/// generic arguments, optional wrappers, and `some`/`any` dropped. Shapes without a single
/// base name (arrays, dictionaries, tuples, function types, compositions)
/// yield `None`.
pub(super) fn base_type_name(base: &BaseExtractor, node: Node) -> Option<String> {
    let mut node = node;
    loop {
        match node.kind() {
            "type_identifier" | "simple_identifier" | "primitive_type" => {
                return Some(base.get_node_text(&node));
            }
            "optional_type" => {
                node = node.child_by_field_name("wrapped")?;
            }
            "opaque_type" | "existential_type" => {
                node = node.named_child(0)?;
            }
            "type_annotation" => {
                node = node
                    .child_by_field_name("name")
                    .or_else(|| named_type_field(node))?;
            }
            "user_type" => {
                let mut cursor = node.walk();
                let segments: Vec<String> = node
                    .named_children(&mut cursor)
                    .filter(|child| child.kind() == "type_identifier")
                    .map(|child| base.get_node_text(&child))
                    .collect();
                return (!segments.is_empty()).then(|| segments.join("."));
            }
            _ => return None,
        }
    }
}

/// Reduce legacy metadata type text (`propertyType`, `returnType`) to a base
/// type name by the same rules as [`base_type_name`], or `None` when the text
/// has no single base name.
pub(super) fn legacy_base_type_name(text: &str) -> Option<String> {
    let mut trimmed = text.trim();
    while let Some(rest) = trimmed
        .strip_suffix('?')
        .or_else(|| trimmed.strip_suffix('!'))
    {
        trimmed = rest.trim_end();
    }
    for prefix in ["inout", "some", "any"] {
        if let Some(rest) = trimmed.strip_prefix(prefix)
            && rest.starts_with(char::is_whitespace)
        {
            trimmed = rest.trim_start();
        }
    }
    if trimmed.is_empty()
        || trimmed.starts_with(['[', '('])
        || trimmed.contains("->")
        || trimmed.contains('&')
    {
        return None;
    }
    let resolved = strip_type_decorations(trimmed, &SWIFT_TYPE_NAME_RULES);
    let is_base_name = !resolved.is_empty()
        && resolved
            .chars()
            .all(|c| c.is_alphanumeric() || c == '_' || c == '.');
    is_base_name.then_some(resolved)
}

fn named_type_field(node: Node) -> Option<Node> {
    let mut cursor = node.walk();
    node.children_by_field_name("type", &mut cursor)
        .find(|child| child.is_named())
}

pub(super) fn property_type_node(node: Node) -> Option<Node> {
    node.children(&mut node.walk())
        .find(|child| child.kind() == "type_annotation")
        .and_then(|annotation| {
            annotation
                .child_by_field_name("name")
                .or_else(|| named_type_field(annotation))
        })
}

pub(super) fn property_value_node(node: Node) -> Option<Node> {
    let mut cursor = node.walk();
    node.children_by_field_name("value", &mut cursor)
        .find(|child| child.is_named())
}

pub(super) fn nearest_callable_ancestor(node: Node) -> bool {
    let mut current = node.parent();
    while let Some(parent) = current {
        match parent.kind() {
            "function_declaration"
            | "init_declaration"
            | "deinit_declaration"
            | "protocol_function_declaration" => return true,
            "class_declaration" | "protocol_declaration" => return false,
            _ => current = parent.parent(),
        }
    }
    false
}
