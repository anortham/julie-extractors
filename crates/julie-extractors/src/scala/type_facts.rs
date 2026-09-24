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

const GET_UNWRAPS: [&str; 4] = ["Option", "Some", "Try", "Success"];

/// A type reduced to what initializer inference needs: the bindable base
/// name (`None` for type parameters, abstract type members, and shapes
/// without one), the written text, and the type arguments.
#[derive(Debug, Clone, PartialEq, Eq)]
struct TypeShape {
    name: Option<String>,
    declared: String,
    args: Vec<TypeShape>,
}

impl TypeShape {
    /// The wrapped type `.get` returns, unless the file declares its own
    /// type with the wrapper's name.
    fn got(self, file_types: &HashSet<String>) -> Option<TypeShape> {
        let unwraps = self
            .name
            .as_deref()
            .is_some_and(|name| GET_UNWRAPS.contains(&name) && !file_types.contains(name));
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
}

impl ReturnTypeIndex {
    pub(super) fn build(base: &BaseExtractor, root: Node) -> Self {
        let mut index = Self::default();
        let mut stack = vec![(root, root.id())];
        while let Some((node, scope)) = stack.pop() {
            index.add(base, node, scope);
            stack.extend(
                node.named_children(&mut node.walk())
                    .map(|child| (child, node.id())),
            );
        }
        index
    }

    fn add(&mut self, base: &BaseExtractor, node: Node, scope: usize) {
        let kind = node.kind();
        let is_def = matches!(kind, "function_definition" | "function_declaration");
        let is_type = matches!(
            kind,
            "class_definition" | "trait_definition" | "enum_definition" | "type_definition"
        );
        if !is_def && !is_type {
            return;
        }
        let Some(name_node) = node.child_by_field_name("name") else {
            return;
        };
        let name = base.get_node_text(&name_node);
        if is_def {
            self.defs
                .entry((scope, name))
                .or_default()
                .push(def_entry(base, node));
            return;
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

fn def_entry(base: &BaseExtractor, def: Node) -> DefEntry {
    let lists: Vec<Node> = def
        .children(&mut def.walk())
        .filter(|child| child.kind() == "parameters")
        .collect();
    let explicit_lists = lists
        .iter()
        .filter(|list| !has_token(**list, "implicit") && !has_token(**list, "using"))
        .count();
    let generics = generic_names_in_scope(base, def);
    let shape = def
        .child_by_field_name("return_type")
        .and_then(|return_type| type_shape(base, return_type, &generics, 0));
    DefEntry {
        explicit_lists,
        total_lists: lists.len(),
        shape,
    }
}

fn has_token(node: Node, token: &str) -> bool {
    node.children(&mut node.walk())
        .any(|child| child.kind() == token)
}

/// Type parameters of the def and every enclosing definition, plus the
/// abstract type members of every enclosing template.
fn generic_names_in_scope(base: &BaseExtractor, def: Node) -> Vec<String> {
    let mut names = Vec::new();
    let mut current = Some(def);
    while let Some(node) = current {
        for parameters in node
            .children(&mut node.walk())
            .filter(|child| child.kind() == "type_parameters")
        {
            names.extend(type_parameter_names(base, parameters));
        }
        if is_template_body(node) {
            names.extend(
                node.named_children(&mut node.walk())
                    .filter(|member| {
                        member.kind() == "type_definition"
                            && member.child_by_field_name("type").is_none()
                    })
                    .filter_map(|member| member.child_by_field_name("name"))
                    .map(|name| base.get_node_text(&name)),
            );
        }
        current = node.parent();
    }
    names
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

fn type_shape(
    base: &BaseExtractor,
    node: Node,
    generics: &[String],
    depth: u32,
) -> Option<TypeShape> {
    if !should_visit_tree_depth(depth) {
        return None;
    }
    let name = base_type_name_node(node)
        .map(|name| base.get_node_text(&name))
        .filter(|name| !generics.contains(name));
    let arguments = node
        .child_by_field_name("type_arguments")
        .filter(|_| node.kind() == "generic_type");
    let args = match (arguments, child_tree_depth(depth)) {
        (Some(arguments), Some(child_depth)) => arguments
            .named_children(&mut arguments.walk())
            .map(|argument| type_shape(base, argument, generics, child_depth))
            .collect::<Option<_>>()?,
        _ => Vec::new(),
    };
    Some(TypeShape {
        name,
        declared: base.get_node_text(&node),
        args,
    })
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
        let object = match self.lookup_term(callee, &name) {
            Term::Defs(entries) => return agreed_return(entries, applied_lists),
            Term::Value => return None,
            Term::Object(object) => Some(object),
            Term::MaybeInherited | Term::Unbound => None,
        };
        if applied_lists == 0 || object.is_some_and(|object| may_inherit(object, "apply")) {
            return None;
        }
        let applies = object
            .and_then(|object| object.child_by_field_name("body"))
            .and_then(|body| self.index.defs(body, "apply"));
        match applies {
            Some(entries) => agreed_return(entries, applied_lists).filter(|shape| {
                !self.index.case_classes.contains(&name)
                    || shape.name.as_deref() == Some(name.as_str())
            }),
            None => self.index.classes.contains(&name).then(|| TypeShape {
                declared: name.clone(),
                name: Some(name),
                args: Vec::new(),
            }),
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
            if may_inherit(template, &member) {
                return None;
            }
            let body = template.child_by_field_name("body")?;
            return agreed_return(self.index.defs(body, &member)?, applied_lists);
        }
        if member == "get" && applied_lists == 0 {
            return self
                .shape_of(receiver, child_tree_depth(depth)?)?
                .got(&self.index.types);
        }
        None
    }

    fn lookup_term<'tree>(&self, from: Node<'tree>, name: &str) -> Term<'_, 'tree> {
        let mut current = from.parent();
        let mut steps = 0;
        while let Some(scope) = current {
            if !should_visit_tree_depth(steps) {
                return Term::Unbound;
            }
            if binds_value(self.base, scope, name) {
                return Term::Value;
            }
            let inherits = is_template_body(scope)
                && scope
                    .parent()
                    .is_some_and(|template| may_inherit(template, name));
            if let Some(entries) = self.index.defs(scope, name) {
                return if inherits {
                    Term::MaybeInherited
                } else {
                    Term::Defs(entries)
                };
            }
            if let Some(object) = declared_object(self.base, scope, name) {
                return Term::Object(object);
            }
            if inherits {
                return Term::MaybeInherited;
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

/// Whether `template` (a class, object, trait, anonymous class, or other
/// template owner) may have a member `name` it does not declare itself, so
/// its own defs of that name may be only some of the overloads.
fn may_inherit(template: Node, name: &str) -> bool {
    match template.kind() {
        "class_definition" | "object_definition" | "trait_definition" | "package_object" => {
            template.child_by_field_name("extend").is_some()
                || has_token(template, "case")
                || template.child_by_field_name("body").is_some_and(|body| {
                    body.named_children(&mut body.walk())
                        .any(|member| member.kind() == "self_type")
                })
                || UNIVERSAL_MEMBERS.contains(&name)
        }
        _ => true,
    }
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
