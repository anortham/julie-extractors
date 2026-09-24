//! Declared-type fact recording for C++.

use super::declarators::{declarator_target, structured_binding};
use super::helpers::is_template_parameter_name;
use super::identifiers::is_this_receiver;
use super::name_bindings::{
    binds_locally, declaration_names, enclosing_namespaces, namespace_path, using_declaration_name,
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
/// call hidden by a non-function name records nothing. Classes are keyed by
/// their full identity (`ns::Outer::Inner`), so a same-named class in another
/// namespace never answers. A friend function is not a member: it enters as
/// a free function that no namespace reaches, so its name records nothing.
#[derive(Debug, Default)]
pub(super) struct ReturnTypeIndex {
    entries: HashMap<String, Vec<ReturnEntry>>,
    classes: HashMap<String, ClassScope>,
    /// Class templates that the file specializes (explicitly or partially).
    specialized: HashSet<String>,
    /// Every namespace path and class identity the file declares, with each
    /// of their prefixes.
    scopes: HashSet<String>,
    /// Names declared at namespace scope that are not functions, by namespace path.
    namespace_names: HashMap<String, HashSet<String>>,
    /// Every name a `#define` in the file defines.
    macros: HashSet<String>,
}

/// The namespace of a friend function: never a real namespace path, so no
/// free call reaches it.
const FRIEND_NAMESPACE: &str = "?friend";

#[derive(Debug, Clone, PartialEq, Eq)]
enum Owner {
    Free,
    /// A member of the same-file class with this full identity.
    Class(String),
    /// A member of a class the file cannot identify: an unnamed class, a
    /// template specialization, or a scope defined elsewhere. Never matches.
    Unknown,
}

#[derive(Debug)]
struct ReturnEntry {
    owner: Owner,
    /// The namespace path that declares the callable.
    namespace: String,
    /// `None` when the return type is absent, deduced, or uses a template parameter.
    shape: Option<TypeShape>,
    /// The first name of each name the return type writes (`a` in `a::B<C>`,
    /// and `C`), which must mean the same entity at the call.
    type_names: Vec<String>,
    /// Where the return type's names are looked up.
    lookup: Lookup,
}

#[derive(Debug)]
enum Lookup {
    /// The owner class, then its namespaces: an in-class declaration or a
    /// trailing return type.
    Owner,
    Namespace(String),
    Unknown,
}

/// A scope a name lookup starts from.
enum Scope {
    Class(String),
    Namespace(String),
}

/// Where a name lookup ends.
#[derive(Debug, PartialEq, Eq)]
enum Found {
    /// The class or namespace that declares the name.
    Declared(String),
    /// The class with an unseen base or a second definition that stops the
    /// lookup undecided.
    Undecided(String),
    /// No same-file scope declares the name; the lookup ran through the
    /// namespaces around this one.
    NotInFile(String),
}

/// Every same-file definition of one class identity, merged.
#[derive(Debug, Default)]
struct ClassScope {
    definitions: usize,
    has_base: bool,
    /// The identity of the class whose body defines this one.
    outer: Option<String>,
    namespace: String,
    /// Every name the class body declares, functions included, and the names
    /// of its out-of-line member definitions.
    members: HashSet<String>,
    /// Names a using-declaration brings in from a base class.
    using_names: HashSet<String>,
}

/// The class whose scope an expression sees first.
enum Caller {
    Free,
    Class(String),
    Unknown,
}

/// A written `a::B<int>::` qualifier.
#[derive(Debug)]
struct WrittenScope {
    global: bool,
    segments: Vec<Segment>,
}

#[derive(Debug)]
struct Segment {
    name: String,
    templated: bool,
}

impl ReturnTypeIndex {
    pub(super) fn build(base: &BaseExtractor, root: Node) -> Self {
        let mut index = Self::default();
        let mut out_of_line = Vec::new();
        let mut stack = vec![root];
        while let Some(node) = stack.pop() {
            match node.kind() {
                "function_declarator" => match return_entry(base, node) {
                    Some((name, Some(scope), entry)) => out_of_line.push((name, scope, entry)),
                    Some((name, None, entry)) => index.entries.entry(name).or_default().push(entry),
                    None => {}
                },
                "class_specifier" | "struct_specifier" | "union_specifier" => {
                    index.add_class(base, node);
                }
                "preproc_def" | "preproc_function_def" => {
                    if let Some(name) = node.child_by_field_name("name") {
                        index.macros.insert(base.get_node_text(&name));
                    }
                }
                "namespace_definition" => {
                    if let Some(body) = node.child_by_field_name("body") {
                        index.add_scope(&namespace_path(base, body));
                    }
                }
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
        for (name, scope, mut entry) in out_of_line {
            let class = index
                .resolve_scope(&scope, &[], &entry.namespace)
                .or_else(|| {
                    let key = external_class_key(&scope, &entry.namespace)?;
                    index
                        .classes
                        .entry(key.clone())
                        .or_insert_with(|| ClassScope {
                            definitions: 1,
                            has_base: true,
                            namespace: entry.namespace.clone(),
                            ..ClassScope::default()
                        });
                    Some(key)
                });
            if let Some(class) = class
                && let Some(members) = index.classes.get_mut(&class)
            {
                members.members.insert(name.clone());
                entry.owner = Owner::Class(class);
            } else {
                entry.lookup = Lookup::Unknown;
            }
            index.entries.entry(name).or_default().push(entry);
        }
        index
    }

    /// The class an out-of-line definition written as `scope::name` inside
    /// `namespace` belongs to.
    fn out_of_line_class(&self, scope: &WrittenScope, namespace: &str) -> Option<String> {
        self.resolve_scope(scope, &[], namespace).or_else(|| {
            external_class_key(scope, namespace).filter(|key| self.classes.contains_key(key))
        })
    }

    fn add_scope(&mut self, path: &str) {
        let mut prefix = path;
        while !prefix.is_empty() {
            self.scopes.insert(prefix.to_string());
            prefix = prefix.rsplit_once("::").map_or("", |(outer, _)| outer);
        }
    }

    fn add_class(&mut self, base: &BaseExtractor, class: Node) {
        let Some(body) = class.child_by_field_name("body") else {
            return;
        };
        let Some(name) = class.child_by_field_name("name") else {
            return;
        };
        if name.kind() == "template_type" {
            if let Some(template) = name.child_by_field_name("name")
                && let Some(prefix) = class_prefix(base, class)
            {
                self.specialized
                    .insert(join_scope(&prefix, &base.get_node_text(&template)));
            }
            return;
        }
        let Some(key) = class_key(base, class) else {
            return;
        };
        self.add_scope(&key);
        let mut members = Vec::new();
        let mut using_names = Vec::new();
        let mut cursor = body.walk();
        for member in body.named_children(&mut cursor) {
            declaration_names(base, member, true, &mut members);
            if member.kind() == "using_declaration" {
                using_names.extend(using_declaration_name(base, member));
            }
        }
        let scope = self.classes.entry(key).or_default();
        scope.definitions += 1;
        scope.has_base |= class
            .children(&mut class.walk())
            .any(|child| child.kind() == "base_class_clause");
        scope.outer = enclosing_class(class).and_then(|outer| class_key(base, outer));
        scope.namespace = namespace_path(base, class);
        scope.members.extend(members);
        scope.using_names.extend(using_names);
    }

    /// The base type every same-named member of `class` agrees on. A class
    /// defined more than once, or a using-declaration that adds base-class
    /// overloads of the name, records nothing.
    fn member_lookup(&self, name: &str, class: &str) -> Option<Vec<&ReturnEntry>> {
        let scope = self.classes.get(class)?;
        if scope.definitions != 1 || scope.using_names.contains(name) {
            return None;
        }
        let members: Vec<&ReturnEntry> = self
            .entries
            .get(name)?
            .iter()
            .filter(|entry| matches!(&entry.owner, Owner::Class(owner) if owner == class))
            .collect();
        (!members.is_empty()).then_some(members)
    }

    /// The free function an unqualified call in `namespace` reaches: the
    /// innermost enclosing namespace that declares the name decides. A
    /// same-file candidate in any other namespace (reachable by a
    /// using-directive or argument-dependent lookup), or a non-function name
    /// in the deciding namespace, records nothing.
    fn free_lookup(&self, name: &str, namespace: &str) -> Option<Vec<&ReturnEntry>> {
        let free: Vec<&ReturnEntry> = self
            .entries
            .get(name)?
            .iter()
            .filter(|entry| entry.owner == Owner::Free)
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
            let here: Vec<&ReturnEntry> = free
                .iter()
                .copied()
                .filter(|entry| entry.namespace == scope)
                .collect();
            if !here.is_empty() {
                return Some(here);
            }
        }
        None
    }

    /// An unqualified call: a member of the calling class when the class
    /// declares the name, else the free function its namespace reaches, but
    /// only from a top-level class with no base, whose lookup cannot stop in
    /// another class.
    fn unqualified_lookup(
        &self,
        base: &BaseExtractor,
        call: Node,
        name: &str,
    ) -> Option<Vec<&ReturnEntry>> {
        match self.calling_class(base, call) {
            Caller::Free => self.free_lookup(name, &namespace_path(base, call)),
            Caller::Unknown => None,
            Caller::Class(class) => {
                let scope = self.classes.get(&class)?;
                if scope.definitions != 1 {
                    return None;
                }
                if scope.members.contains(name) {
                    return self.member_lookup(name, &class);
                }
                if scope.has_base || scope.outer.is_some() {
                    return None;
                }
                self.free_lookup(name, &scope.namespace)
            }
        }
    }

    /// A qualified call `a::B::name()`: a member of the same-file class the
    /// written qualifier names from the call's scope.
    fn qualified_lookup(
        &self,
        base: &BaseExtractor,
        call: Node,
        scope: &WrittenScope,
        name: &str,
    ) -> Option<Vec<&ReturnEntry>> {
        let class = match self.calling_class(base, call) {
            Caller::Free => self.resolve_scope(scope, &[], &namespace_path(base, call)),
            Caller::Unknown => None,
            Caller::Class(class) => {
                let chain = self.class_chain(&class);
                let namespace = chain
                    .last()
                    .and_then(|outermost| self.classes.get(*outermost))
                    .map(|outermost| outermost.namespace.clone())?;
                self.resolve_scope(scope, &chain, &namespace)
            }
        }?;
        self.member_lookup(name, &class)
    }

    fn this_lookup(
        &self,
        base: &BaseExtractor,
        call: Node,
        name: &str,
    ) -> Option<Vec<&ReturnEntry>> {
        match self.calling_class(base, call) {
            Caller::Class(class) => self.member_lookup(name, &class),
            Caller::Free | Caller::Unknown => None,
        }
    }

    /// The base type every candidate agrees on, when each name its return
    /// type writes means the same entity at `call` as at the callee. A name
    /// a macro, a local, or a scope between the two redefines records nothing.
    /// An out-of-line definition redeclares an in-class member, so the
    /// in-class declaration answers for it.
    fn agreed_at(
        &self,
        base: &BaseExtractor,
        call: Node,
        candidates: &[&ReturnEntry],
    ) -> Option<TypeShape> {
        let caller = match self.calling_class(base, call) {
            Caller::Free => Scope::Namespace(namespace_path(base, call)),
            Caller::Class(class) => Scope::Class(class),
            Caller::Unknown => return None,
        };
        for entry in candidates {
            let callee = match (&entry.lookup, &entry.owner) {
                (Lookup::Owner, Owner::Class(class)) => Scope::Class(class.clone()),
                (Lookup::Namespace(_), Owner::Class(_))
                    if candidates.iter().any(|other| {
                        matches!(other.lookup, Lookup::Owner) && other.owner == entry.owner
                    }) =>
                {
                    continue;
                }
                (Lookup::Namespace(namespace), _) => Scope::Namespace(namespace.clone()),
                _ => return None,
            };
            for name in &entry.type_names {
                if self.macros.contains(name) || binds_locally(base, call, name) {
                    return None;
                }
                let at_callee = self.find_name(&callee, name);
                let at_call = self.find_name(&caller, name);
                let same = match (&at_callee, &at_call) {
                    (Found::NotInFile(outer), Found::NotInFile(inner)) => {
                        enclosing_namespaces(inner).contains(&outer.as_str())
                    }
                    _ => at_callee == at_call,
                };
                if !same {
                    return None;
                }
            }
        }
        agreed_shape(candidates.iter().copied())
    }

    /// Where an unqualified name looked up from `scope` is declared: the
    /// classes around it innermost first (a class's own name is declared in
    /// its outer scope), then its namespaces outward.
    fn find_name(&self, scope: &Scope, name: &str) -> Found {
        let namespace = match scope {
            Scope::Namespace(namespace) => namespace.clone(),
            Scope::Class(class) => {
                let chain = self.class_chain(class);
                for class in &chain {
                    let Some(class_scope) = self
                        .classes
                        .get(*class)
                        .filter(|class_scope| class_scope.definitions == 1)
                    else {
                        return Found::Undecided(class.to_string());
                    };
                    let (outer, own_name) = class.rsplit_once("::").unwrap_or(("", class));
                    if own_name == name {
                        return Found::Declared(outer.to_string());
                    }
                    if class_scope.members.contains(name)
                        || self.scopes.contains(&join_scope(class, name))
                    {
                        return Found::Declared(class.to_string());
                    }
                    if class_scope.has_base {
                        return Found::Undecided(class.to_string());
                    }
                }
                chain
                    .last()
                    .and_then(|outermost| self.classes.get(*outermost))
                    .map(|outermost| outermost.namespace.clone())
                    .unwrap_or_default()
            }
        };
        enclosing_namespaces(&namespace)
            .into_iter()
            .find(|candidate| {
                self.scopes.contains(&join_scope(candidate, name))
                    || self
                        .namespace_names
                        .get(*candidate)
                        .is_some_and(|names| names.contains(name))
            })
            .map_or_else(
                || Found::NotInFile(namespace.clone()),
                |found| Found::Declared(found.to_string()),
            )
    }

    /// The class whose scope `node` sees first: the class whose body holds
    /// it, or the class an out-of-line member definition names.
    fn calling_class(&self, base: &BaseExtractor, node: Node) -> Caller {
        let mut current = node.parent();
        while let Some(scope) = current {
            match scope.kind() {
                "class_specifier" | "struct_specifier" | "union_specifier" => {
                    return class_key(base, scope).map_or(Caller::Unknown, Caller::Class);
                }
                "function_definition" => {
                    if let Some(written) = out_of_line_scope(base, scope) {
                        return written
                            .and_then(|written| {
                                self.out_of_line_class(&written, &namespace_path(base, scope))
                            })
                            .map_or(Caller::Unknown, Caller::Class);
                    }
                }
                _ => {}
            }
            current = scope.parent();
        }
        Caller::Free
    }

    /// A class and the classes whose bodies enclose it, innermost first.
    fn class_chain<'a>(&'a self, class: &'a str) -> Vec<&'a str> {
        let mut chain = vec![class];
        let mut current = class;
        while let Some(outer) = self
            .classes
            .get(current)
            .and_then(|scope| scope.outer.as_deref())
        {
            chain.push(outer);
            current = outer;
        }
        chain
    }

    /// The same-file class a written qualifier names, looked up from the
    /// classes in `chain` and then from `namespace` outward. The first scope
    /// that declares the qualifier's first name decides; a class with a base
    /// that does not declare it stops the lookup, since the base may.
    fn resolve_scope(
        &self,
        scope: &WrittenScope,
        chain: &[&str],
        namespace: &str,
    ) -> Option<String> {
        let first = &scope.segments.first()?.name;
        if scope.global {
            return self.class_at("", &scope.segments);
        }
        for class in chain {
            let class_scope = self.classes.get(*class)?;
            if class_scope.definitions != 1 {
                return None;
            }
            let (outer, own_name) = class.rsplit_once("::").unwrap_or(("", class));
            if own_name == first {
                return self.class_at(outer, &scope.segments);
            }
            if self.scopes.contains(&join_scope(class, first))
                || class_scope.members.contains(first)
            {
                return self.class_at(class, &scope.segments);
            }
            if class_scope.has_base {
                return None;
            }
        }
        enclosing_namespaces(namespace)
            .into_iter()
            .find(|candidate| {
                self.scopes.contains(&join_scope(candidate, first))
                    || self
                        .namespace_names
                        .get(*candidate)
                        .is_some_and(|names| names.contains(first))
            })
            .and_then(|found| self.class_at(found, &scope.segments))
    }

    /// The class `segments` names inside `prefix`, unless a templated segment
    /// names a class template the file specializes.
    fn class_at(&self, prefix: &str, segments: &[Segment]) -> Option<String> {
        let mut key = prefix.to_string();
        for segment in segments {
            key = join_scope(&key, &segment.name);
            if segment.templated && self.specialized.contains(&key) {
                return None;
            }
        }
        self.classes.contains_key(&key).then_some(key)
    }
}

/// The identity of a class or namespace defined in another file, as the
/// file's out-of-line definitions write it: the same written qualifier in the
/// same namespace names the same scope. It never equals a same-file class
/// identity. Such a scope counts as having a base, since its definition is
/// unseen. A templated qualifier gets none: it may name a specialization.
fn external_class_key(scope: &WrittenScope, namespace: &str) -> Option<String> {
    if scope.segments.is_empty() || scope.segments.iter().any(|segment| segment.templated) {
        return None;
    }
    let path: Vec<&str> = scope
        .segments
        .iter()
        .map(|segment| segment.name.as_str())
        .collect();
    let global = if scope.global { "::" } else { "" };
    Some(format!("{namespace}?{global}{}", path.join("::")))
}

fn join_scope(prefix: &str, name: &str) -> String {
    if prefix.is_empty() {
        name.to_string()
    } else {
        format!("{prefix}::{name}")
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

fn is_class_node(node: Node) -> bool {
    matches!(
        node.kind(),
        "class_specifier" | "struct_specifier" | "union_specifier"
    )
}

fn enclosing_class(node: Node) -> Option<Node> {
    let mut current = node.parent();
    while let Some(candidate) = current {
        if is_class_node(candidate) {
            return Some(candidate);
        }
        current = candidate.parent();
    }
    None
}

/// A class's full identity: its namespace path, the classes around it, and
/// its name. `None` when it or an enclosing class has no plain name (an
/// unnamed class or a template specialization).
fn class_key(base: &BaseExtractor, class: Node) -> Option<String> {
    let name = class
        .child_by_field_name("name")
        .filter(|name| name.kind() == "type_identifier")?;
    Some(join_scope(
        &class_prefix(base, class)?,
        &base.get_node_text(&name),
    ))
}

/// The identity of the scope that defines a class: its enclosing class, or
/// its namespace path.
fn class_prefix(base: &BaseExtractor, class: Node) -> Option<String> {
    match enclosing_class(class) {
        Some(outer) => class_key(base, outer),
        None => Some(namespace_path(base, class)),
    }
}

/// The written scope of an out-of-line member definition: `None` when the
/// definition's name is not qualified, `Some(None)` when the qualifier is not
/// a plain path.
fn out_of_line_scope(base: &BaseExtractor, definition: Node) -> Option<Option<WrittenScope>> {
    let target = declarator_target(definition.child_by_field_name("declarator")?)?;
    target.function?;
    let (_, global, scopes) = split_qualified(target.name)?;
    if scopes.is_empty() && !global {
        return None;
    }
    Some(written_scope(base, global, &scopes))
}

/// Where a callable's declarator places it, and where its return type's names
/// are looked up: a member of the class whose body declares it, or a free
/// function. A declaration in a block declares a free function even inside a
/// method, and its return type sees the block, which the index does not model,
/// so its lookup is unknown.
fn declaring_owner(base: &BaseExtractor, function_declarator: Node) -> (Owner, Lookup) {
    let mut current = function_declarator.parent();
    while let Some(scope) = current {
        if is_class_node(scope) {
            return class_key(base, scope).map_or((Owner::Unknown, Lookup::Unknown), |class| {
                (Owner::Class(class), Lookup::Owner)
            });
        }
        if scope.kind() == "compound_statement" {
            return (Owner::Free, Lookup::Unknown);
        }
        current = scope.parent();
    }
    (
        Owner::Free,
        Lookup::Namespace(namespace_path(base, function_declarator)),
    )
}

fn is_friend(holder: Node) -> bool {
    let mut current = holder.parent();
    while let Some(parent) = current.filter(|parent| parent.kind() == "template_declaration") {
        current = parent.parent();
    }
    current.is_some_and(|parent| parent.kind() == "friend_declaration")
}

/// A callable's name, its written qualifier when the declarator is
/// qualified, and its entry.
fn return_entry(
    base: &BaseExtractor,
    function_declarator: Node,
) -> Option<(String, Option<WrittenScope>, ReturnEntry)> {
    let holder = decorated_declarator(function_declarator).parent()?;
    let is_callable_holder = matches!(
        holder.kind(),
        "function_definition" | "declaration" | "field_declaration"
    );
    if !is_callable_holder {
        return None;
    }
    let (name, global, scopes) =
        split_qualified(function_declarator.child_by_field_name("declarator")?)?;
    let name = callee_name(base, name)?;
    let qualified = !scopes.is_empty() || global;
    let friend = is_friend(holder);
    if friend && qualified {
        return None;
    }
    let stated = stated_return_type(base, function_declarator)
        .filter(|(type_node, _)| !uses_template_parameter(base, *type_node, 0));
    let mut type_names = Vec::new();
    let mut trailing = false;
    if let Some((type_node, _)) = stated {
        first_names(base, type_node, true, &mut type_names, 0);
        trailing = type_node
            .parent()
            .is_some_and(|parent| parent.kind() == "type_descriptor");
    }
    let namespace = namespace_path(base, function_declarator);
    let mut entry = ReturnEntry {
        owner: Owner::Free,
        lookup: if trailing {
            Lookup::Owner
        } else {
            Lookup::Namespace(namespace.clone())
        },
        namespace,
        shape: stated.map(|(_, shape)| shape),
        type_names,
    };
    if friend {
        entry.namespace = FRIEND_NAMESPACE.to_string();
        entry.lookup = Lookup::Unknown;
        return Some((name, None, entry));
    }
    if !qualified {
        (entry.owner, entry.lookup) = declaring_owner(base, function_declarator);
        // A block cannot hold a definition: a macro such as
        // `NS_BEGIN namespace x {` misparsed a namespace-scope one into it.
        if holder.kind() == "function_definition"
            && entry.owner == Owner::Free
            && matches!(entry.lookup, Lookup::Unknown)
            && !entry
                .type_names
                .iter()
                .any(|name| binds_locally(base, function_declarator, name))
        {
            entry.lookup = Lookup::Namespace(entry.namespace.clone());
        }
        return Some((name, None, entry));
    }
    entry.owner = Owner::Unknown;
    let written = written_scope(base, global, &scopes);
    Some((name, written, entry))
}

/// A possibly qualified name split into its final name node, whether it
/// starts with `::`, and the scope nodes before it.
fn split_qualified(node: Node) -> Option<(Node, bool, Vec<Node>)> {
    let mut node = node;
    let mut global = false;
    let mut scopes = Vec::new();
    while node.kind() == "qualified_identifier" {
        match node.child_by_field_name("scope") {
            Some(scope) => scopes.push(scope),
            None if scopes.is_empty() => global = true,
            None => return None,
        }
        node = node.child_by_field_name("name")?;
    }
    Some((node, global, scopes))
}

fn callee_name(base: &BaseExtractor, node: Node) -> Option<String> {
    let name = match node.kind() {
        "identifier" | "field_identifier" => node,
        "template_function" => node.child_by_field_name("name")?,
        _ => return None,
    };
    Some(base.get_node_text(&name))
}

/// The first name of each name a type writes: `a` and `c` in `a::B<c::D>`.
fn first_names(
    base: &BaseExtractor,
    node: Node,
    leading: bool,
    names: &mut Vec<String>,
    depth: u32,
) {
    let Some(child_depth) = child_tree_depth(depth).filter(|_| should_visit_tree_depth(depth))
    else {
        return;
    };
    match node.kind() {
        "type_identifier" | "namespace_identifier" | "identifier" => {
            if leading {
                names.push(base.get_node_text(&node));
            }
        }
        "qualified_identifier" | "template_type" | "template_function" => {
            let (lead, rest) = match node.kind() {
                "qualified_identifier" => ("scope", "name"),
                _ => ("name", "arguments"),
            };
            if let Some(child) = node.child_by_field_name(lead) {
                first_names(base, child, leading, names, child_depth);
            }
            if let Some(child) = node.child_by_field_name(rest) {
                let rest_leads = node.kind() != "qualified_identifier";
                first_names(base, child, rest_leads, names, child_depth);
            }
        }
        _ => {
            let mut cursor = node.walk();
            for child in node.named_children(&mut cursor) {
                first_names(base, child, true, names, child_depth);
            }
        }
    }
}

/// A qualifier of plain namespace or class names, some with template
/// arguments. `None` for anything else (`decltype(x)::`, a dependent name).
fn written_scope(base: &BaseExtractor, global: bool, scopes: &[Node]) -> Option<WrittenScope> {
    let segments = scopes
        .iter()
        .map(|scope| match scope.kind() {
            "namespace_identifier" | "type_identifier" => Some(Segment {
                name: base.get_node_text(scope),
                templated: false,
            }),
            "template_type" => Some(Segment {
                name: base.get_node_text(&scope.child_by_field_name("name")?),
                templated: true,
            }),
            _ => None,
        })
        .collect::<Option<Vec<_>>>()?;
    Some(WrittenScope { global, segments })
}

/// Whether any name in a type, template arguments included, is a template
/// parameter in scope, so the type depends on the instantiation.
fn uses_template_parameter(base: &BaseExtractor, node: Node, depth: u32) -> bool {
    if !should_visit_tree_depth(depth) {
        return true;
    }
    if matches!(node.kind(), "type_identifier" | "namespace_identifier")
        && is_template_parameter_name(base, &node)
    {
        return true;
    }
    let Some(child_depth) = child_tree_depth(depth) else {
        return true;
    };
    let mut cursor = node.walk();
    node.named_children(&mut cursor)
        .any(|child| uses_template_parameter(base, child, child_depth))
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
            let (name, global, scopes) = split_qualified(function)?;
            let name = callee_name(base, name)?;
            if return_types.macros.contains(&name) {
                return None;
            }
            if scopes.is_empty() && !global {
                if binds_locally(base, value, &name) {
                    return None;
                }
                let candidates = return_types.unqualified_lookup(base, value, &name)?;
                return return_types.agreed_at(base, value, &candidates);
            }
            if scopes.first().is_some_and(|first| {
                is_template_parameter_name(base, first)
                    || binds_locally(base, value, &base.get_node_text(first))
            }) {
                return None;
            }
            let scope = written_scope(base, global, &scopes)?;
            if scope
                .segments
                .iter()
                .any(|segment| return_types.macros.contains(&segment.name))
            {
                return None;
            }
            let candidates = return_types.qualified_lookup(base, value, &scope, &name)?;
            return_types.agreed_at(base, value, &candidates)
        }
        "field_expression" => {
            if !is_this_receiver(function) {
                return None;
            }
            let field = function.child_by_field_name("field")?;
            if field.kind() != "field_identifier" {
                return None;
            }
            let name = base.get_node_text(&field);
            if return_types.macros.contains(&name) {
                return None;
            }
            let candidates = return_types.this_lookup(base, function, &name)?;
            return_types.agreed_at(base, function, &candidates)
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
