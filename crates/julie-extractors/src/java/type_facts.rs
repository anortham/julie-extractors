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
/// only. The arity-compatible candidates of that type, its same-file
/// supertypes, and `java.lang.Object` must agree, and the return type text
/// must name the same type at the call as at the callee. `void`,
/// type-parameter returns, and any other initializer record nothing.
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
    /// pattern, enum constant, or single static import. Such a name used as
    /// a call qualifier may denote the variable, not the type.
    bound_names: HashSet<String>,
    /// An `import static pkg.Type.*;` can bring in a field with any name,
    /// and a field obscures a same-named type used as a call qualifier.
    has_static_on_demand_import: bool,
}

#[derive(Debug)]
struct TypeDeclaration {
    id: usize,
    parent_id: usize,
    start_byte: usize,
    /// A local class is in scope only from its declaration to the end of
    /// its block; a member or top-level type is in scope in its whole body.
    block_local: bool,
}

impl TypeDeclaration {
    fn in_scope(&self, site: Node) -> bool {
        (!self.block_local || self.start_byte <= site.start_byte())
            && std::iter::successors(site.parent(), Node::parent)
                .any(|ancestor| ancestor.id() == self.parent_id)
    }
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
        let mut members = Vec::new();
        let mut stack = vec![root];
        while let Some(node) = stack.pop() {
            if node.kind() == "method_declaration" || is_named_type_declaration(node.kind()) {
                members.push(node);
            }
            index.visit(base, node);
            stack.extend(node.named_children(&mut node.walk()));
        }
        for node in members {
            if node.kind() == "method_declaration" {
                if let Some((name, entry)) = method_entry(base, node) {
                    index.add_method(name, entry, node);
                }
            } else {
                index.add_implicit_methods(base, node);
            }
        }
        index
    }

    fn visit(&mut self, base: &BaseExtractor, node: Node) {
        if let Some(name) = binding_name(node) {
            self.bound_names.insert(base.get_node_text(&name));
        }
        match node.kind() {
            "import_declaration" => self.add_import(base, node),
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

    fn add_import(&mut self, base: &BaseExtractor, import: Node) {
        let mut cursor = import.walk();
        let children: Vec<Node> = import.children(&mut cursor).collect();
        if !children.iter().any(|child| child.kind() == "static") {
            return;
        }
        if children.iter().any(|child| child.kind() == "asterisk") {
            self.has_static_on_demand_import = true;
        } else if let Some(name) = children
            .iter()
            .find(|child| matches!(child.kind(), "identifier" | "scoped_identifier"))
        {
            let name = name.child_by_field_name("name").unwrap_or(*name);
            self.bound_names.insert(base.get_node_text(&name));
        }
    }

    /// Index a method whose return type text is written at `site`. The text
    /// is recorded at call sites elsewhere, so a same-file type it names
    /// must be the one declaration of that name, in scope at `site`.
    fn add_method(&mut self, name: String, mut entry: ReturnEntry, site: Node) {
        if entry
            .declared
            .as_deref()
            .is_some_and(|declared| !self.names_one_type_or_none(leading_type_name(declared), site))
        {
            entry.declared = None;
        }
        self.methods.entry(name).or_default().push(entry);
    }

    /// True when `name` is declared by no type in the file, or by exactly
    /// one that is in scope at `site`.
    fn names_one_type_or_none(&self, name: &str, site: Node) -> bool {
        match self.types.get(name).map(Vec::as_slice) {
            None => true,
            Some([declaration]) => declaration.in_scope(site),
            Some(_) => false,
        }
    }

    fn add_type(&mut self, base: &BaseExtractor, declaration: Node) {
        let (Some(name), Some(parent)) = (
            declaration.child_by_field_name("name"),
            declaration.parent(),
        ) else {
            return;
        };
        let id = declaration.id();
        self.types
            .entry(base.get_node_text(&name))
            .or_default()
            .push(TypeDeclaration {
                id,
                parent_id: parent.id(),
                start_byte: declaration.start_byte(),
                block_local: !matches!(
                    parent.kind(),
                    "program"
                        | "class_body"
                        | "interface_body"
                        | "enum_body_declarations"
                        | "annotation_type_body"
                ),
            });
        self.supertype_names
            .insert(id, written_supertype_names(base, declaration));
    }

    fn add_implicit_methods(&mut self, base: &BaseExtractor, declaration: Node) {
        match declaration.kind() {
            "record_declaration" => self.add_record_accessors(base, declaration),
            "enum_declaration" => {
                let Some(name) = declaration.child_by_field_name("name") else {
                    return;
                };
                let name = base.get_node_text(&name);
                let id = declaration.id();
                self.add_method(
                    "values".to_string(),
                    enum_method(id, 0, format!("{name}[]")),
                    declaration,
                );
                self.add_method("valueOf".to_string(), enum_method(id, 1, name), declaration);
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
                component,
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
        let declared = self.lookup(&name, owner, argument_count, type_qualified)?;
        let type_name = leading_type_name(declared);
        (self.names_one_type_or_none(type_name, call)
            && !type_parameter_in_scope(base, type_name, call))
        .then_some(declared)
    }

    /// The same-file type that the qualifier `name` denotes at `site`. The
    /// name must be declared by exactly one type in the file, that type must
    /// be in scope at `site` (top-level, a member of an enclosing type, or a
    /// local class declared earlier in an enclosing block), and no variable
    /// in the file or static import may bind the name.
    fn visible_type(&self, name: &str, site: Node) -> Option<usize> {
        if self.has_static_on_demand_import || self.bound_names.contains(name) {
            return None;
        }
        let [declaration] = self.types.get(name)?.as_slice() else {
            return None;
        };
        declaration.in_scope(site).then_some(declaration.id)
    }

    /// The return type that every arity-compatible `name` method of `owner`,
    /// of its same-file supertypes, and of `java.lang.Object` agrees on.
    /// `owner` itself must declare one: an inherited-only method records
    /// nothing. A type-qualified call needs every candidate to be `static`:
    /// an instance method there means the qualifier is a variable, not the
    /// type.
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
        let object_overload = object_method_overload(name, argument_count)
            .map(|declared| declared.filter(|_| !type_qualified));
        let mut declared = candidates
            .iter()
            .map(|entry| {
                entry
                    .declared
                    .as_deref()
                    .filter(|_| !type_qualified || entry.is_static)
            })
            .chain(object_overload);
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

/// The return type of the `java.lang.Object` instance method that a
/// same-file method named `name` with `argument_count` parameters can
/// overload without overriding, so that overload resolution picks by the
/// argument types. `Some(None)` is a `void` method.
fn object_method_overload(name: &str, argument_count: usize) -> Option<Option<&'static str>> {
    match (name, argument_count) {
        ("equals", 1) => Some(Some("boolean")),
        ("wait", 1 | 2) => Some(None),
        _ => None,
    }
}

/// The first name of a type text: `Map` in `Map.Entry<K, V>`, `Foo` in
/// `Foo[]`. It is the name that the scope where the text is written resolves.
fn leading_type_name(text: &str) -> &str {
    text.split(|c: char| !(c.is_alphanumeric() || c == '_' || c == '$'))
        .next()
        .unwrap_or(text)
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
    let text = base.get_node_text(&type_node);
    type_parameter_in_scope(base, leading_type_name(&text), method)
}

/// True when `name` is a type parameter of `scope` or of any declaration
/// around it.
fn type_parameter_in_scope(base: &BaseExtractor, name: &str, scope: Node) -> bool {
    std::iter::successors(Some(scope), Node::parent).any(|declaration| {
        declaration
            .child_by_field_name("type_parameters")
            .is_some_and(|parameters| {
                parameters
                    .named_children(&mut parameters.walk())
                    .filter_map(|parameter| {
                        parameter
                            .named_children(&mut parameter.walk())
                            .find(|child| child.kind() == "type_identifier")
                    })
                    .any(|parameter| base.get_node_text(&parameter) == name)
            })
    })
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
