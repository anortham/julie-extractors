//! Declared and initializer-inferred type fact recording for Java.

use crate::base::BaseExtractor;
use crate::base::types::TypeNameRules;
use std::collections::{HashMap, HashSet};
use tree_sitter::Node;

pub(super) const JAVA_TYPE_NAME_RULES: TypeNameRules = TypeNameRules {
    nullable_suffixes: &[],
    reference_prefixes: &[],
    generic_open: &['<'],
};

/// Record a syntactically stated type for a symbol (`is_inferred=false`).
pub(super) fn record_declared_type(base: &mut BaseExtractor, symbol_id: &str, type_node: Node) {
    record_type_node(base, symbol_id, type_node, false);
}

/// Record the type a `var` initializer produces (`is_inferred=true`): the
/// constructed type of `new Foo(..)`, or the declared return type of a call
/// to a same-file method. An unqualified or `this.` call resolves only in the
/// innermost named enclosing type; `Type.method(..)` resolves in the one
/// same-file type named `Type` in scope at the call, to `static` methods
/// only. The arity-compatible candidates of that type and its same-file
/// supertypes must agree. `void`, type-parameter returns, and any other
/// initializer record nothing.
pub(super) fn record_initializer_type(
    base: &mut BaseExtractor,
    symbol_id: &str,
    value: Node,
    return_types: &ReturnTypeIndex,
) {
    if value.kind() == "object_creation_expression" {
        if let Some(type_node) = value.child_by_field_name("type") {
            record_type_node(base, symbol_id, type_node, true);
        }
        return;
    }
    let Some(declared) = return_types.call_type(base, value) else {
        return;
    };
    let declared = declared.to_string();
    base.record_declared_type_fact(symbol_id, &declared, &JAVA_TYPE_NAME_RULES, true);
}

/// Declared return types of the methods of the file's named types, by
/// method name, with the facts needed to find the type a call resolves in.
/// Types are identified by their declaration node, never by name, so
/// same-named nested or local types never share methods. Methods of
/// anonymous classes and enum constant bodies are left out.
#[derive(Debug, Default)]
pub(super) struct ReturnTypeIndex {
    methods: HashMap<String, Vec<ReturnEntry>>,
    types: HashMap<String, Vec<TypeDeclaration>>,
    /// Every type name written in the `extends`/`implements` clauses of a
    /// type declaration, keyed by the declaration's node id.
    supertype_names: HashMap<usize, Vec<String>>,
    /// Names bound anywhere in the file by a variable, parameter, field,
    /// pattern, or enum constant. Such a name used as a call qualifier may
    /// denote the variable, not the type.
    bound_names: HashSet<String>,
}

#[derive(Debug)]
struct TypeDeclaration {
    id: usize,
    parent_id: usize,
}

#[derive(Debug)]
struct ReturnEntry {
    owner: usize,
    /// `None` for `void`, type-parameter returns, and types with no single base name.
    declared: Option<String>,
    parameter_count: usize,
    variadic: bool,
    is_static: bool,
}

impl ReturnEntry {
    fn accepts(&self, argument_count: usize) -> bool {
        if self.variadic {
            argument_count + 1 >= self.parameter_count
        } else {
            argument_count == self.parameter_count
        }
    }
}

impl ReturnTypeIndex {
    pub(super) fn build(base: &BaseExtractor, root: Node) -> Self {
        let mut index = Self::default();
        let mut stack = vec![root];
        while let Some(node) = stack.pop() {
            index.visit(base, node);
            stack.extend(node.named_children(&mut node.walk()));
        }
        index
    }

    fn visit(&mut self, base: &BaseExtractor, node: Node) {
        if let Some(name) = binding_name(node) {
            self.bound_names.insert(base.get_node_text(&name));
        }
        match node.kind() {
            "method_declaration" => {
                if let Some((name, entry)) = method_entry(base, node) {
                    self.add_method(name, entry);
                }
            }
            "inferred_parameters" | "type_pattern" | "record_pattern_component" => {
                for identifier in node
                    .named_children(&mut node.walk())
                    .filter(|child| child.kind() == "identifier")
                {
                    self.bound_names.insert(base.get_node_text(&identifier));
                }
            }
            kind if is_named_type_declaration(kind) => self.add_type(base, node),
            _ => {}
        }
    }

    fn add_method(&mut self, name: String, entry: ReturnEntry) {
        self.methods.entry(name).or_default().push(entry);
    }

    fn add_type(&mut self, base: &BaseExtractor, declaration: Node) {
        let (Some(name), Some(parent)) = (
            declaration.child_by_field_name("name"),
            declaration.parent(),
        ) else {
            return;
        };
        let name = base.get_node_text(&name);
        let id = declaration.id();
        self.types
            .entry(name.clone())
            .or_default()
            .push(TypeDeclaration {
                id,
                parent_id: parent.id(),
            });
        self.supertype_names
            .insert(id, written_supertype_names(base, declaration));
        match declaration.kind() {
            "record_declaration" => self.add_record_accessors(base, declaration),
            "enum_declaration" => {
                self.add_method(
                    "values".to_string(),
                    enum_method(id, 0, format!("{name}[]")),
                );
                self.add_method("valueOf".to_string(), enum_method(id, 1, name));
            }
            _ => {}
        }
    }

    /// The implicit accessor of each record component: no parameters, the
    /// component's type.
    fn add_record_accessors(&mut self, base: &BaseExtractor, record: Node) {
        let Some(components) = record.child_by_field_name("parameters") else {
            return;
        };
        for component in components
            .named_children(&mut components.walk())
            .filter(|component| component.kind() == "formal_parameter")
        {
            let Some(name) = component.child_by_field_name("name") else {
                continue;
            };
            let declared = declared_return(base, component, component);
            self.add_method(
                base.get_node_text(&name),
                ReturnEntry {
                    owner: record.id(),
                    declared,
                    parameter_count: 0,
                    variadic: false,
                    is_static: false,
                },
            );
        }
    }

    fn call_type(&self, base: &BaseExtractor, call: Node) -> Option<&str> {
        if call.kind() != "method_invocation" {
            return None;
        }
        let (owner, type_qualified) = match call.child_by_field_name("object") {
            None => (declaring_type(call)?.id(), false),
            Some(object) if object.kind() == "this" => (declaring_type(call)?.id(), false),
            Some(object) if object.kind() == "identifier" => {
                (self.visible_type(&base.get_node_text(&object), call)?, true)
            }
            Some(_) => return None,
        };
        let name = base.get_node_text(&call.child_by_field_name("name")?);
        let argument_count = non_comment_count(call.child_by_field_name("arguments")?);
        self.lookup(&name, owner, argument_count, type_qualified)
    }

    /// The same-file type that the qualifier `name` denotes at `site`. The
    /// name must be declared by exactly one type in the file, that type must
    /// be in scope at `site` (top-level, a member of an enclosing type, or a
    /// local class of an enclosing block), and no variable in the file may
    /// bind the name.
    fn visible_type(&self, name: &str, site: Node) -> Option<usize> {
        if self.bound_names.contains(name) {
            return None;
        }
        let [declaration] = self.types.get(name)?.as_slice() else {
            return None;
        };
        std::iter::successors(site.parent(), Node::parent)
            .any(|ancestor| ancestor.id() == declaration.parent_id)
            .then_some(declaration.id)
    }

    /// The return type that every arity-compatible `name` method of `owner`
    /// and of its same-file supertypes agrees on. `owner` itself must declare
    /// one: an inherited-only method records nothing. A type-qualified call
    /// needs every candidate to be `static`: an instance method there means
    /// the qualifier is a variable, not the type.
    fn lookup(
        &self,
        name: &str,
        owner: usize,
        argument_count: usize,
        type_qualified: bool,
    ) -> Option<&str> {
        let owners = self.with_supertypes(owner);
        let candidates: Vec<&ReturnEntry> = self
            .methods
            .get(name)?
            .iter()
            .filter(|entry| owners.contains(&entry.owner) && entry.accepts(argument_count))
            .collect();
        if !candidates.iter().any(|entry| entry.owner == owner) {
            return None;
        }
        let mut declared = candidates.iter().map(|entry| {
            entry
                .declared
                .as_deref()
                .filter(|_| !type_qualified || entry.is_static)
        });
        let first = declared.next()??;
        declared.all(|other| other == Some(first)).then_some(first)
    }

    /// `owner` and every same-file type that a written supertype name of it,
    /// followed transitively, could denote.
    fn with_supertypes(&self, owner: usize) -> HashSet<usize> {
        let mut seen = HashSet::from([owner]);
        let mut pending = vec![owner];
        while let Some(id) = pending.pop() {
            for name in self.supertype_names.get(&id).into_iter().flatten() {
                for declaration in self.types.get(name).into_iter().flatten() {
                    if seen.insert(declaration.id) {
                        pending.push(declaration.id);
                    }
                }
            }
        }
        seen
    }
}

fn is_named_type_declaration(kind: &str) -> bool {
    matches!(
        kind,
        "class_declaration"
            | "interface_declaration"
            | "enum_declaration"
            | "record_declaration"
            | "annotation_type_declaration"
    )
}

/// The identifier a declaration node binds through its `name` field.
fn binding_name(node: Node) -> Option<Node> {
    let name = match node.kind() {
        "variable_declarator"
        | "formal_parameter"
        | "catch_formal_parameter"
        | "enhanced_for_statement"
        | "resource"
        | "enum_constant"
        | "instanceof_expression" => node.child_by_field_name("name"),
        "lambda_expression" => node.child_by_field_name("parameters"),
        _ => None,
    }?;
    (name.kind() == "identifier").then_some(name)
}

/// Every type name in the `extends`, `implements`, and interface `extends`
/// clauses, type arguments included. Extra names only add candidates, which
/// can only turn a fact into no fact.
fn written_supertype_names(base: &BaseExtractor, declaration: Node) -> Vec<String> {
    let mut names = Vec::new();
    let mut stack: Vec<Node> = declaration
        .named_children(&mut declaration.walk())
        .filter(|child| {
            matches!(
                child.kind(),
                "superclass" | "super_interfaces" | "extends_interfaces"
            )
        })
        .collect();
    while let Some(node) = stack.pop() {
        if node.kind() == "type_identifier" {
            names.push(base.get_node_text(&node));
        }
        stack.extend(node.named_children(&mut node.walk()));
    }
    names
}

fn enum_method(owner: usize, parameter_count: usize, declared: String) -> ReturnEntry {
    ReturnEntry {
        owner,
        declared: Some(declared),
        parameter_count,
        variadic: false,
        is_static: true,
    }
}

fn method_entry(base: &BaseExtractor, method: Node) -> Option<(String, ReturnEntry)> {
    let name = base.get_node_text(&method.child_by_field_name("name")?);
    let owner = declaring_type(method)?.id();
    let parameters = method.child_by_field_name("parameters")?;
    let parameter_kinds: Vec<&str> = parameters
        .named_children(&mut parameters.walk())
        .map(|parameter| parameter.kind())
        .filter(|kind| matches!(*kind, "formal_parameter" | "spread_parameter"))
        .collect();
    Some((
        name,
        ReturnEntry {
            owner,
            declared: declared_return(base, method, method),
            parameter_count: parameter_kinds.len(),
            variadic: parameter_kinds.contains(&"spread_parameter"),
            is_static: super::helpers::extract_modifiers(base, method)
                .iter()
                .any(|modifier| modifier == "static"),
        },
    ))
}

/// The `type` of a method or record component as a recordable fact, or
/// `None` for `void`, old-style `[]` dimensions, `var`, and a type parameter
/// of `scope` or of any declaration around it.
fn declared_return(base: &BaseExtractor, declaration: Node, scope: Node) -> Option<String> {
    declaration
        .child_by_field_name("type")
        .filter(|_| declaration.child_by_field_name("dimensions").is_none())
        .filter(|type_node| names_single_base_type(*type_node) && !is_var_type(base, *type_node))
        .filter(|type_node| !names_type_parameter(base, *type_node, scope))
        .map(|type_node| base.get_node_text(&type_node))
}

/// The declaration of the innermost named type whose body holds `node`;
/// `None` inside an anonymous class or enum constant body.
fn declaring_type(node: Node) -> Option<Node> {
    let mut current = node.parent()?;
    while !matches!(
        current.kind(),
        "class_body" | "interface_body" | "enum_body" | "annotation_type_body"
    ) {
        current = current.parent()?;
    }
    current
        .parent()
        .filter(|declaration| is_named_type_declaration(declaration.kind()))
}

/// True when the base name of `type_node` is a type parameter of the method
/// or of any enclosing declaration.
fn names_type_parameter(base: &BaseExtractor, type_node: Node, method: Node) -> bool {
    let mut element = type_node;
    while element.kind() == "array_type" {
        let Some(inner) = element.child_by_field_name("element") else {
            return false;
        };
        element = inner;
    }
    let base_name = match element.kind() {
        "type_identifier" => Some(element),
        "generic_type" => element
            .named_children(&mut element.walk())
            .find(|child| child.kind() == "type_identifier"),
        _ => None,
    };
    let Some(base_name) = base_name.map(|name| base.get_node_text(&name)) else {
        return false;
    };
    let mut scope = Some(method);
    while let Some(declaration) = scope {
        if let Some(parameters) = declaration.child_by_field_name("type_parameters")
            && parameters
                .named_children(&mut parameters.walk())
                .filter_map(|parameter| {
                    parameter
                        .named_children(&mut parameter.walk())
                        .find(|child| child.kind() == "type_identifier")
                })
                .any(|name| base.get_node_text(&name) == base_name)
        {
            return true;
        }
        scope = declaration.parent();
    }
    false
}

fn non_comment_count(node: Node) -> usize {
    node.named_children(&mut node.walk())
        .filter(|child| !matches!(child.kind(), "line_comment" | "block_comment"))
        .count()
}

/// Record a method's declared return type (`is_inferred=false`). `void`
/// is not a type fact and records nothing.
pub(super) fn record_return_type(base: &mut BaseExtractor, symbol_id: &str, type_node: Node) {
    if type_node.kind() == "void_type" {
        return;
    }
    record_type_node(base, symbol_id, type_node, false);
}

/// True when a stated type is the `var` keyword, which tree-sitter-java
/// parses as a `type_identifier` named `var`. `var` is a reserved type name
/// in Java, so no real type can collide with it.
pub(super) fn is_var_type(base: &BaseExtractor, type_node: Node) -> bool {
    type_node.kind() == "type_identifier" && base.get_node_text(&type_node) == "var"
}

fn record_type_node(base: &mut BaseExtractor, symbol_id: &str, type_node: Node, is_inferred: bool) {
    if !names_single_base_type(type_node) || is_var_type(base, type_node) {
        return;
    }
    let declared = base.get_node_text(&type_node);
    base.record_declared_type_fact(symbol_id, &declared, &JAVA_TYPE_NAME_RULES, is_inferred);
}

/// True for type nodes whose text reduces to one base type name. Generics
/// over dotted bases, wildcards, `void`, and annotated types do not, so they
/// record nothing. Array types record their full `Foo[]` text unstripped.
fn names_single_base_type(node: Node) -> bool {
    match node.kind() {
        "type_identifier"
        | "scoped_type_identifier"
        | "integral_type"
        | "floating_point_type"
        | "boolean_type" => true,
        "generic_type" => {
            let mut cursor = node.walk();
            node.children(&mut cursor)
                .any(|child| child.kind() == "type_identifier")
        }
        "array_type" => node
            .child_by_field_name("element")
            .is_some_and(names_single_base_type),
        _ => false,
    }
}
