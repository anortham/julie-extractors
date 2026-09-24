use crate::base::types::{TypeNameRules, strip_type_decorations};
use crate::base::{BaseExtractor, Symbol};
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};
use std::collections::{HashMap, HashSet};
use tree_sitter::Node;

pub(super) const SCALA_TYPE_NAME_RULES: TypeNameRules = TypeNameRules {
    nullable_suffixes: &[],
    reference_prefixes: &[],
    generic_open: &['['],
};

const DECLARED_TYPE_METADATA_KEYS: [&str; 2] = ["returnType", "propertyType"];

/// Base type names for symbols whose metadata carries declared type text.
/// Shapes with no single base name (tuples, function types, compound types)
/// record nothing.
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

/// The base type name of declared type text: `List[Int]` gives `List`.
pub(super) fn base_type_name_from_text(declared: &str) -> Option<String> {
    let name = strip_type_decorations(declared, &SCALA_TYPE_NAME_RULES);
    let is_qualified_name = !name.is_empty() && name.split('.').all(is_type_name_segment);
    is_qualified_name.then_some(name)
}

fn is_type_name_segment(segment: &str) -> bool {
    let mut chars = segment.chars();
    chars
        .next()
        .is_some_and(|first| first.is_alphabetic() || first == '_' || first == '$')
        && chars.all(|c| c.is_alphanumeric() || c == '_' || c == '$')
}

pub(super) fn record_declared_type(base: &mut BaseExtractor, symbol_id: &str, type_node: Node) {
    record_type_node(base, symbol_id, type_node, false);
}

/// Record the type a `val`/`var` initializer produces (`is_inferred=true`):
/// `new T(..)`, a same-file class constructor call, or a call to or reference
/// of a same-file `def` with a declared return type. `.get` unwraps one
/// `Option`/`Some`/`Try`/`Success` layer; any other trailing method records
/// nothing.
pub(super) fn record_initializer_type(
    base: &mut BaseExtractor,
    symbol_id: &str,
    value: Node,
    index: &ReturnTypeIndex,
) {
    if value.kind() == "instance_expression" {
        if let Some(type_node) = instance_type_node(value) {
            record_type_node(base, symbol_id, type_node, true);
        }
        return;
    }
    let scope = InitializerScope { base, index };
    let Some(TypeShape {
        name: Some(name),
        declared,
        ..
    }) = scope.shape_of(value, 0)
    else {
        return;
    };
    base.record_declared_type_fact_with_declared(
        symbol_id,
        &name,
        &declared,
        &SCALA_TYPE_NAME_RULES,
        true,
    );
}

/// `Any`/`AnyRef` members every class, object, and trait inherits.
const UNIVERSAL_MEMBERS: [&str; 15] = [
    "toString",
    "hashCode",
    "equals",
    "getClass",
    "clone",
    "finalize",
    "notify",
    "notifyAll",
    "wait",
    "eq",
    "ne",
    "isInstanceOf",
    "asInstanceOf",
    "synchronized",
    "##",
];

/// The Scala types whose parameterless `.get` returns their one type
/// argument, with the package that declares each.
const GET_UNWRAPS: [(&str, &str); 4] = [
    ("Option", "scala"),
    ("Some", "scala"),
    ("Try", "scala.util"),
    ("Success", "scala.util"),
];

/// Wildcard imports that cannot bring a type named like a `GET_UNWRAPS` entry.
const SAFE_WILDCARD_PACKAGES: [&str; 8] = [
    "scala",
    "scala.util",
    "scala.collection",
    "scala.concurrent",
    "scala.jdk",
    "scala.annotation",
    "scala.math",
    "java",
];

/// Synthetic companion members of a case class and of an enum.
const CASE_COMPANION_MEMBERS: [&str; 5] = ["apply", "unapply", "fromProduct", "tupled", "curried"];
const ENUM_COMPANION_MEMBERS: [&str; 3] = ["values", "valueOf", "fromOrdinal"];

/// A type reduced to what initializer inference needs: the bindable base
/// name (`None` for type parameters, abstract type members, and shapes
/// without one), the written name path without type arguments, the written
/// text, and the type arguments.
#[derive(Debug, Clone, PartialEq, Eq)]
struct TypeShape {
    name: Option<String>,
    path: String,
    declared: String,
    args: Vec<TypeShape>,
}

impl TypeShape {
    fn of_class(name: &str) -> Self {
        Self {
            name: Some(name.to_string()),
            path: name.to_string(),
            declared: name.to_string(),
            args: Vec::new(),
        }
    }

    /// The wrapped type `.get` returns when this is the Scala `Option`,
    /// `Some`, `Try`, or `Success`.
    fn got(self, index: &ReturnTypeIndex) -> Option<TypeShape> {
        let name = self.name.as_deref()?;
        let path = self.path.strip_prefix("_root_.").unwrap_or(&self.path);
        let unwraps = if path == name {
            index.bare_wrappers.contains(name)
        } else {
            GET_UNWRAPS
                .iter()
                .any(|(wrapper, package)| path == format!("{package}.{wrapper}"))
        };
        unwraps.then(|| self.args.into_iter().next()).flatten()
    }
}

#[derive(Debug)]
struct DefEntry {
    /// Parameter lists a caller must write; `implicit`/`using` lists excluded.
    explicit_lists: usize,
    total_lists: usize,
    /// `None` when the def declares no return type.
    shape: Option<TypeShape>,
}

impl DefEntry {
    fn accepts(&self, applied_lists: usize) -> bool {
        applied_lists == self.explicit_lists || applied_lists == self.total_lists
    }
}

/// The file's defs keyed by the node that declares them, plus its class
/// and type names, built once per file before the symbol walk.
#[derive(Debug, Default)]
pub(super) struct ReturnTypeIndex {
    defs: HashMap<(usize, String), Vec<DefEntry>>,
    classes: HashSet<String>,
    case_classes: HashSet<String>,
    /// Classes, traits, enums, and type aliases.
    types: HashSet<String>,
    /// Type members declared with no definition.
    abstract_types: HashSet<String>,
    imports: Vec<Imported>,
    /// `GET_UNWRAPS` names that mean the Scala type when written unqualified.
    bare_wrappers: HashSet<String>,
}

/// One name or wildcard an `import` brings into scope.
#[derive(Debug)]
enum Imported {
    Name { local: String, full: String },
    Wildcard(String),
}

impl ReturnTypeIndex {
    pub(super) fn build(base: &BaseExtractor, root: Node) -> Self {
        let mut index = Self::default();
        let mut defs = Vec::new();
        let mut stack = vec![(root, root.id())];
        while let Some((node, scope)) = stack.pop() {
            if matches!(node.kind(), "function_definition" | "function_declaration") {
                defs.push((node, scope));
            } else {
                index.add(base, node);
            }
            stack.extend(
                node.named_children(&mut node.walk())
                    .map(|child| (child, node.id())),
            );
        }
        for (def, scope) in defs {
            if let Some(name) = def.child_by_field_name("name") {
                let entry = index.def_entry(base, def);
                index
                    .defs
                    .entry((scope, base.get_node_text(&name)))
                    .or_default()
                    .push(entry);
            }
        }
        index.bare_wrappers = index.bare_wrappers();
        index
    }

    /// A wrapper name means the Scala type unless the file declares a type
    /// with that name or an import can bring another one. `Try` and
    /// `Success` also need an import from `scala.util`.
    fn bare_wrappers(&self) -> HashSet<String> {
        let foreign_wildcard = self.imports.iter().any(|import| {
            matches!(import, Imported::Wildcard(path) if !SAFE_WILDCARD_PACKAGES
                .iter()
                .any(|safe| path == safe || path.starts_with(&format!("{safe}."))))
        });
        if foreign_wildcard {
            return HashSet::new();
        }
        GET_UNWRAPS
            .iter()
            .filter(|(wrapper, package)| {
                let full = format!("{package}.{wrapper}");
                let named = |import: &&Imported| {
                    matches!(import, Imported::Name { local, .. } if local == wrapper)
                };
                let foreign = self.imports.iter().filter(named).any(
                    |import| !matches!(import, Imported::Name { full: from, .. } if *from == full),
                );
                let imported = self.imports.iter().any(|import| match import {
                    Imported::Name { local, full: from } => local == wrapper && *from == full,
                    Imported::Wildcard(path) => path == package,
                });
                !self.types.contains(*wrapper)
                    && !foreign
                    && (*package == "scala" || imported)
            })
            .map(|(wrapper, _)| wrapper.to_string())
            .collect()
    }

    fn add(&mut self, base: &BaseExtractor, node: Node) {
        let kind = node.kind();
        if kind == "import_declaration" {
            self.imports.extend(imported_names(base, node));
            return;
        }
        let is_type = matches!(
            kind,
            "class_definition" | "trait_definition" | "enum_definition" | "type_definition"
        );
        if !is_type {
            return;
        }
        let Some(name_node) = node.child_by_field_name("name") else {
            return;
        };
        let name = base.get_node_text(&name_node);
        if kind == "type_definition" && node.child_by_field_name("type").is_none() {
            self.abstract_types.insert(name.clone());
        }
        if kind == "class_definition" {
            if has_token(node, "case") {
                self.case_classes.insert(name.clone());
            }
            self.classes.insert(name.clone());
        }
        self.types.insert(name);
    }

    fn defs(&self, scope: Node, name: &str) -> Option<&[DefEntry]> {
        self.defs
            .get(&(scope.id(), name.to_string()))
            .map(Vec::as_slice)
    }
}

/// The return type every def in `entries` agrees on, when one of them
/// accepts `applied_lists` argument lists.
fn agreed_return(entries: &[DefEntry], applied_lists: usize) -> Option<TypeShape> {
    let first = entries.first()?.shape.as_ref()?;
    let agreed = entries
        .iter()
        .all(|entry| entry.shape.as_ref() == Some(first))
        && entries.iter().any(|entry| entry.accepts(applied_lists));
    agreed.then(|| first.clone())
}

impl ReturnTypeIndex {
    fn def_entry(&self, base: &BaseExtractor, def: Node) -> DefEntry {
        let lists: Vec<Node> = def
            .children(&mut def.walk())
            .filter(|child| child.kind() == "parameters")
            .collect();
        let explicit_lists = lists
            .iter()
            .filter(|list| !has_token(**list, "implicit") && !has_token(**list, "using"))
            .count();
        let shape = def
            .child_by_field_name("return_type")
            .and_then(|return_type| self.type_shape(base, def, return_type, 0));
        DefEntry {
            explicit_lists,
            total_lists: lists.len(),
            shape,
        }
    }

    /// Whether the unqualified type `name` written in `def` names a concrete
    /// type: not a type parameter, not an abstract type member, and not a
    /// member that an enclosing template can inherit or export, unless the file
    /// declares a concrete type with that name and no abstract one.
    fn names_concrete_type(&self, base: &BaseExtractor, def: Node, name: &str) -> bool {
        let mut current = Some(def);
        let mut steps = 0;
        while let Some(node) = current {
            if !should_visit_tree_depth(steps) {
                return false;
            }
            let mut children = node
                .children(&mut node.walk())
                .collect::<Vec<_>>()
                .into_iter();
            let declared = children.find_map(|child| match child.kind() {
                "type_parameters" => type_parameter_names(base, child)
                    .contains(&name.to_string())
                    .then_some(false),
                "class_definition" | "trait_definition" | "enum_definition" | "type_definition"
                    if child
                        .child_by_field_name("name")
                        .is_some_and(|declared| base.get_node_text(&declared) == name) =>
                {
                    Some(
                        child.kind() != "type_definition"
                            || child.child_by_field_name("type").is_some(),
                    )
                }
                _ => None,
            });
            if let Some(concrete) = declared {
                return concrete;
            }
            let inherits = has_export(node)
                || (is_template_body(node)
                    && node
                        .parent()
                        .is_some_and(|template| may_inherit(base, template, name)));
            if inherits {
                return self.types.contains(name) && !self.abstract_types.contains(name);
            }
            current = node.parent();
            steps += 1;
        }
        true
    }

    fn type_shape(
        &self,
        base: &BaseExtractor,
        def: Node,
        node: Node,
        depth: u32,
    ) -> Option<TypeShape> {
        if !should_visit_tree_depth(depth) {
            return None;
        }
        let is_generic = node.kind() == "generic_type";
        let path_node = if is_generic {
            node.child_by_field_name("type")?
        } else {
            node
        };
        let path: String = base.get_node_text(&path_node).split_whitespace().collect();
        let name = base_type_name_node(node)
            .map(|name| base.get_node_text(&name))
            .filter(|name| *name != path || self.names_concrete_type(base, def, name));
        let arguments = node
            .child_by_field_name("type_arguments")
            .filter(|_| is_generic);
        let args = match (arguments, child_tree_depth(depth)) {
            (Some(arguments), Some(child_depth)) => arguments
                .named_children(&mut arguments.walk())
                .map(|argument| self.type_shape(base, def, argument, child_depth))
                .collect::<Option<_>>()?,
            _ => Vec::new(),
        };
        Some(TypeShape {
            name,
            path,
            declared: base.get_node_text(&node),
            args,
        })
    }
}

fn has_token(node: Node, token: &str) -> bool {
    node.children(&mut node.walk())
        .any(|child| child.kind() == token)
}

fn type_parameter_names(base: &BaseExtractor, parameters: Node) -> Vec<String> {
    let variant = parameters
        .named_children(&mut parameters.walk())
        .filter(|child| {
            matches!(
                child.kind(),
                "covariant_type_parameter" | "contravariant_type_parameter"
            )
        })
        .filter_map(|child| child.child_by_field_name("name"))
        .collect::<Vec<_>>();
    parameters
        .children_by_field_name("name", &mut parameters.walk())
        .chain(variant)
        .map(|name| base.get_node_text(&name))
        .collect()
}

fn is_template_body(node: Node) -> bool {
    matches!(
        node.kind(),
        "template_body" | "with_template_body" | "enum_body"
    ) && node.parent().is_some_and(|owner| {
        matches!(
            owner.kind(),
            "class_definition"
                | "object_definition"
                | "trait_definition"
                | "enum_definition"
                | "given_definition"
                | "instance_expression"
                | "package_object"
        )
    })
}

/// What a bare name means at a use site, found by walking its enclosing scopes.
enum Term<'a, 'tree> {
    Defs(&'a [DefEntry]),
    /// A same-file object definition.
    Object(Node<'tree>),
    /// A same-file case class, whose companion `apply` the name calls.
    CaseClass,
    /// A parameter, pattern, or `val`/`var` binds the name.
    Value,
    /// An enclosing template may inherit a member of this name.
    MaybeInherited,
    Unbound,
}

struct InitializerScope<'a> {
    base: &'a BaseExtractor,
    index: &'a ReturnTypeIndex,
}

impl InitializerScope<'_> {
    fn shape_of(&self, value: Node, depth: u32) -> Option<TypeShape> {
        if !should_visit_tree_depth(depth) {
            return None;
        }
        match value.kind() {
            "parenthesized_expression" => {
                self.shape_of(value.named_child(0)?, child_tree_depth(depth)?)
            }
            "identifier" => self.term_call(value, 0),
            "field_expression" => self.member_call(value, 0, depth),
            "call_expression" | "generic_function" => {
                let mut callee = value;
                let mut applied_lists = 0;
                while callee.kind() == "call_expression" {
                    applied_lists += 1;
                    callee = callee.child_by_field_name("function")?;
                }
                if callee.kind() == "generic_function" {
                    callee = callee.child_by_field_name("function")?;
                }
                match callee.kind() {
                    "identifier" => self.term_call(callee, applied_lists),
                    "field_expression" => self.member_call(callee, applied_lists, depth),
                    _ => None,
                }
            }
            _ => None,
        }
    }

    /// `name`, `name(..)`, or a companion `Name(..)`.
    fn term_call(&self, callee: Node, applied_lists: usize) -> Option<TypeShape> {
        let name = self.base.get_node_text(&callee);
        let term = self.lookup_term(callee, &name);
        if let Term::Defs(entries) = term {
            return agreed_return(entries, applied_lists);
        }
        if applied_lists == 0 {
            return None;
        }
        match term {
            Term::Object(object) => self.object_member(object, "apply", applied_lists),
            Term::CaseClass => Some(TypeShape::of_class(&name)),
            Term::MaybeInherited | Term::Unbound => self
                .index
                .classes
                .contains(&name)
                .then(|| TypeShape::of_class(&name)),
            Term::Defs(_) | Term::Value => None,
        }
    }

    /// `Object.member` with `applied_lists` argument lists. `apply` of a
    /// companion falls back to the class constructor, and a case class
    /// companion's `apply` must return the class, as its synthetic one does.
    fn object_member(&self, object: Node, member: &str, applied_lists: usize) -> Option<TypeShape> {
        let synthetic = member != "apply" && companion_synthesizes(self.base, object, member);
        if synthetic || inherits_structurally(object, member) {
            return None;
        }
        let own = object
            .child_by_field_name("body")
            .and_then(|body| self.index.defs(body, member));
        if member != "apply" {
            return agreed_return(own?, applied_lists);
        }
        let name = self
            .base
            .get_node_text(&object.child_by_field_name("name")?);
        match own {
            Some(entries) => agreed_return(entries, applied_lists).filter(|shape| {
                !self.index.case_classes.contains(&name)
                    || shape.name.as_deref() == Some(name.as_str())
            }),
            None => (applied_lists > 0 && self.index.classes.contains(&name))
                .then(|| TypeShape::of_class(&name)),
        }
    }

    /// `this.m`, `Object.m`, or `<expr>.get`, each with `applied_lists`
    /// argument lists.
    fn member_call(&self, callee: Node, applied_lists: usize, depth: u32) -> Option<TypeShape> {
        let receiver = callee.child_by_field_name("value")?;
        let member = self
            .base
            .get_node_text(&callee.child_by_field_name("field")?);
        let receiver_template = match receiver.kind() {
            "identifier" if self.base.get_node_text(&receiver) == "this" => {
                Some(enclosing_template_body(receiver)?.parent()?)
            }
            "identifier" => match self.lookup_term(receiver, &self.base.get_node_text(&receiver)) {
                Term::Object(object) => Some(object),
                _ => None,
            },
            _ => None,
        };
        if let Some(template) = receiver_template {
            if template.kind() == "object_definition" {
                return self.object_member(template, &member, applied_lists);
            }
            if may_inherit(self.base, template, &member) {
                return None;
            }
            let body = template.child_by_field_name("body")?;
            return agreed_return(self.index.defs(body, &member)?, applied_lists);
        }
        if member == "get" && applied_lists == 0 {
            return self
                .shape_of(receiver, child_tree_depth(depth)?)?
                .got(self.index);
        }
        None
    }

    /// A plain class passed on the way out is a Scala 3 constructor proxy
    /// but no term in Scala 2, so an outer def or object it would hide
    /// resolves to `Value` (no fact) instead.
    fn lookup_term<'tree>(&self, from: Node<'tree>, name: &str) -> Term<'_, 'tree> {
        let mut current = from.parent();
        let mut steps = 0;
        let mut passed_plain_class = false;
        while let Some(scope) = current {
            if !should_visit_tree_depth(steps) {
                return Term::Unbound;
            }
            if binds_value(self.base, scope, name) {
                return Term::Value;
            }
            let inherits = has_export(scope)
                || (is_template_body(scope)
                    && scope
                        .parent()
                        .is_some_and(|template| may_inherit(self.base, template, name)));
            let found = if let Some(entries) = self.index.defs(scope, name) {
                Some(if inherits {
                    Term::MaybeInherited
                } else {
                    Term::Defs(entries)
                })
            } else if let Some(object) = declared_object(self.base, scope, name) {
                Some(Term::Object(object))
            } else {
                match declared_class_kind(self.base, scope, name) {
                    Some(ClassKind::Case) => Some(Term::CaseClass),
                    Some(ClassKind::Enum) => Some(Term::Value),
                    Some(ClassKind::Plain) => {
                        passed_plain_class = true;
                        inherits.then_some(Term::MaybeInherited)
                    }
                    None => inherits.then_some(Term::MaybeInherited),
                }
            };
            match found {
                Some(Term::Defs(_) | Term::Object(_)) if passed_plain_class => return Term::Value,
                Some(term) => return term,
                None => {}
            }
            current = scope.parent();
            steps += 1;
        }
        Term::Unbound
    }
}

fn enclosing_template_body(node: Node) -> Option<Node> {
    let mut current = node.parent();
    while let Some(scope) = current {
        if is_template_body(scope) {
            return Some(scope);
        }
        current = scope.parent();
    }
    None
}

fn declared_object<'tree>(
    base: &BaseExtractor,
    scope: Node<'tree>,
    name: &str,
) -> Option<Node<'tree>> {
    scope.named_children(&mut scope.walk()).find(|child| {
        child.kind() == "object_definition"
            && child
                .child_by_field_name("name")
                .is_some_and(|object_name| base.get_node_text(&object_name) == name)
    })
}

enum ClassKind {
    Case,
    Plain,
    Enum,
}

fn declared_class_kind(base: &BaseExtractor, scope: Node, name: &str) -> Option<ClassKind> {
    scope.named_children(&mut scope.walk()).find_map(|child| {
        let kind = match child.kind() {
            "class_definition" if has_token(child, "case") => ClassKind::Case,
            "class_definition" => ClassKind::Plain,
            "enum_definition" => ClassKind::Enum,
            _ => return None,
        };
        child
            .child_by_field_name("name")
            .is_some_and(|class_name| base.get_node_text(&class_name) == name)
            .then_some(kind)
    })
}

fn has_export(scope: Node) -> bool {
    scope
        .named_children(&mut scope.walk())
        .any(|child| child.kind() == "export_declaration")
}

/// Whether `template` (a class, object, trait, anonymous class, or other
/// template owner) may have a member `name` it does not declare itself, so
/// its own defs of that name may be only some of the overloads.
fn may_inherit(base: &BaseExtractor, template: Node, name: &str) -> bool {
    inherits_structurally(template, name) || companion_synthesizes(base, template, name)
}

/// A parent, self type, `case` modifier, `export`, non-class template
/// owner, or `Any` member.
fn inherits_structurally(template: Node, name: &str) -> bool {
    match template.kind() {
        "class_definition" | "object_definition" | "trait_definition" | "package_object" => {
            template.child_by_field_name("extend").is_some()
                || has_token(template, "case")
                || template.child_by_field_name("body").is_some_and(|body| {
                    has_export(body)
                        || body
                            .named_children(&mut body.walk())
                            .any(|member| member.kind() == "self_type")
                })
                || UNIVERSAL_MEMBERS.contains(&name)
        }
        _ => true,
    }
}

/// Whether `object` is the companion of a same-scope case class or enum
/// that gives it a synthetic member `name`.
fn companion_synthesizes(base: &BaseExtractor, object: Node, name: &str) -> bool {
    if object.kind() != "object_definition" {
        return false;
    }
    let (Some(scope), Some(object_name)) = (object.parent(), object.child_by_field_name("name"))
    else {
        return false;
    };
    let members: &[&str] = match declared_class_kind(base, scope, &base.get_node_text(&object_name))
    {
        Some(ClassKind::Case) => &CASE_COMPANION_MEMBERS,
        Some(ClassKind::Enum) => &ENUM_COMPANION_MEMBERS,
        Some(ClassKind::Plain) | None => &[],
    };
    members.contains(&name)
}

/// The names and wildcards one `import` clause brings into scope, with
/// `_root_.` removed from each path.
fn imported_names(base: &BaseExtractor, import: Node) -> Vec<Imported> {
    let mut groups: Vec<(Vec<String>, Option<Node>)> = vec![(Vec::new(), None)];
    for (index, child) in import.children(&mut import.walk()).enumerate() {
        if child.kind() == "," {
            groups.push((Vec::new(), None));
            continue;
        }
        let Some((segments, tail)) = groups.last_mut() else {
            continue;
        };
        let is_path = u32::try_from(index)
            .ok()
            .and_then(|index| import.field_name_for_child(index))
            == Some("path");
        if !child.is_named() {
            continue;
        }
        if is_path {
            segments.push(base.get_node_text(&child));
        } else {
            *tail = Some(child);
        }
    }
    let mut imported = Vec::new();
    for (mut segments, tail) in groups {
        if segments.first().is_some_and(|first| first == "_root_") {
            segments.remove(0);
        }
        let Some(tail) = tail else {
            if let Some(last) = segments.last() {
                imported.push(Imported::Name {
                    local: last.clone(),
                    full: segments.join("."),
                });
            }
            continue;
        };
        let path = segments.join(".");
        let selectors = if tail.kind() == "namespace_selectors" {
            tail.named_children(&mut tail.walk()).collect()
        } else {
            vec![tail]
        };
        for selector in selectors {
            let (original, local) = match selector.kind() {
                "namespace_wildcard" if base.get_node_text(&selector) != "given" => {
                    imported.push(Imported::Wildcard(path.clone()));
                    continue;
                }
                "identifier" => (Some(selector), Some(selector)),
                "arrow_renamed_identifier" | "as_renamed_identifier" => (
                    selector.child_by_field_name("name"),
                    selector
                        .child_by_field_name("alias")
                        .filter(|alias| alias.kind() == "identifier"),
                ),
                _ => continue,
            };
            if let (Some(original), Some(local)) = (original, local) {
                let original = base.get_node_text(&original);
                imported.push(Imported::Name {
                    local: base.get_node_text(&local),
                    full: if path.is_empty() {
                        original
                    } else {
                        format!("{path}.{original}")
                    },
                });
            }
        }
    }
    imported
}

/// Whether `scope` introduces a value named `name` for the code inside it:
/// a def, lambda, extension, or class parameter, a case pattern, a `for`
/// enumerator pattern, or a `val`/`var`/named `given` declared directly in it.
fn binds_value(base: &BaseExtractor, scope: Node, name: &str) -> bool {
    let binders: Vec<Node> = scope
        .children(&mut scope.walk())
        .filter_map(|child| match child.kind() {
            "val_definition" | "var_definition" | "val_declaration" | "var_declaration" => child
                .child_by_field_name("pattern")
                .or_else(|| child.child_by_field_name("name")),
            "given_definition" => child.child_by_field_name("name"),
            "class_parameters" => Some(child),
            _ => None,
        })
        .chain(scope.children_by_field_name("parameters", &mut scope.walk()))
        .chain(
            (scope.kind() == "case_clause")
                .then(|| scope.child_by_field_name("pattern"))
                .flatten(),
        )
        .chain(for_enumerator_patterns(scope))
        .collect();
    binders
        .into_iter()
        .any(|binder| names_identifier(base, binder, name))
}

/// The pattern of every generator (`p <- xs`) and value definition
/// (`p = x`) of a `for` expression.
fn for_enumerator_patterns(scope: Node) -> Vec<Node> {
    if scope.kind() != "for_expression" {
        return Vec::new();
    }
    scope
        .named_children(&mut scope.walk())
        .filter(|child| child.kind() == "enumerators")
        .flat_map(|enumerators| {
            enumerators
                .named_children(&mut enumerators.walk())
                .collect::<Vec<_>>()
        })
        .filter_map(|enumerator| enumerator.named_child(0))
        .filter(|pattern| pattern.kind() != "guard")
        .collect()
}

fn names_identifier(base: &BaseExtractor, root: Node, name: &str) -> bool {
    let mut stack = vec![root];
    while let Some(node) = stack.pop() {
        if node.kind() == "identifier" && base.get_node_text(&node) == name {
            return true;
        }
        stack.extend(node.named_children(&mut node.walk()));
    }
    false
}

fn record_type_node(base: &mut BaseExtractor, symbol_id: &str, type_node: Node, is_inferred: bool) {
    let Some(name_node) = base_type_name_node(type_node) else {
        return;
    };
    let base_name = base.get_node_text(&name_node);
    let declared = base.get_node_text(&type_node);
    base.record_declared_type_fact_with_declared(
        symbol_id,
        &base_name,
        &declared,
        &SCALA_TYPE_NAME_RULES,
        is_inferred,
    );
}

fn base_type_name_node(node: Node) -> Option<Node> {
    let mut node = node;
    loop {
        match node.kind() {
            "type_identifier" | "identifier" => return Some(node),
            "generic_type" => {
                node = node.child_by_field_name("type")?;
            }
            "stable_type_identifier" => {
                return last_named_type_identifier(node);
            }
            _ => return None,
        }
    }
}

fn last_named_type_identifier(node: Node) -> Option<Node> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor)
        .filter(|child| matches!(child.kind(), "type_identifier" | "identifier"))
        .last()
}

fn instance_type_node(node: Node) -> Option<Node> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor).find(|child| {
        matches!(
            child.kind(),
            "type_identifier" | "generic_type" | "stable_type_identifier"
        )
    })
}
