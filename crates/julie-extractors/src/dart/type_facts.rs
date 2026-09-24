use super::helpers::find_child_by_type;
use crate::base::BaseExtractor;
use crate::base::types::TypeNameRules;
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};
use std::collections::{HashMap, HashSet};
use tree_sitter::Node;

pub(super) const DART_TYPE_NAME_RULES: TypeNameRules = TypeNameRules {
    nullable_suffixes: &["?"],
    reference_prefixes: &[],
    generic_open: &['<'],
};

pub(super) fn record_declared_type(base: &mut BaseExtractor, symbol_id: &str, type_node: Node) {
    let Some(base_name) = base_type_name(base, type_node) else {
        return;
    };
    let declared = base.get_node_text(&type_node);
    base.record_declared_type_fact_with_declared(
        symbol_id,
        &base_name,
        &declared,
        &DART_TYPE_NAME_RULES,
        false,
    );
}

fn base_type_name(base: &BaseExtractor, type_node: Node) -> Option<String> {
    let container = match type_node.kind() {
        "type" | "nullable_type" => type_node,
        "type_identifier" => return Some(base.get_node_text(&type_node)),
        _ => return None,
    };
    let mut segments = Vec::new();
    let mut cursor = container.walk();
    for child in container.named_children(&mut cursor) {
        match child.kind() {
            "type_identifier" => segments.push(base.get_node_text(&child)),
            "void_type" => segments.push("void".to_string()),
            "type_arguments" => {}
            _ => return None,
        }
    }
    if segments.is_empty() {
        None
    } else {
        Some(segments.join("."))
    }
}

pub(super) fn record_constructor_fact(base: &mut BaseExtractor, symbol_id: &str, class_name: &str) {
    base.record_declared_type_fact(symbol_id, class_name, &DART_TYPE_NAME_RULES, true);
}

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
    if matches!(
        node.kind(),
        "class_definition" | "class_declaration" | "mixin_declaration" | "enum_declaration"
    ) && let Some(name_node) = node
        .child_by_field_name("name")
        .or_else(|| find_child_by_type(&node, "identifier"))
    {
        names.insert(base.get_node_text(&name_node));
    }
    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_type_names_into(base, child, child_depth, names);
    }
}

pub(super) fn inferred_constructor_name(
    base: &BaseExtractor,
    value: Node,
    same_file_types: &HashSet<String>,
) -> Option<String> {
    match value.kind() {
        "new_expression" | "const_object_expression" => {
            let type_node = value.child_by_field_name("type")?;
            let name = constructor_type_name(base, type_node)?;
            same_file_types.contains(&name).then_some(name)
        }
        "call_expression" => {
            let function = value.child_by_field_name("function")?;
            let name = match function.kind() {
                "identifier" => base.get_node_text(&function),
                "member_expression" | "null_aware_member_expression" => {
                    let object = function.child_by_field_name("object")?;
                    if object.kind() != "identifier" {
                        return None;
                    }
                    base.get_node_text(&object)
                }
                _ => return None,
            };
            same_file_types.contains(&name).then_some(name)
        }
        _ => None,
    }
}

fn constructor_type_name(base: &BaseExtractor, type_node: Node) -> Option<String> {
    let declared = base.get_node_text(&type_node);
    let stripped = crate::base::types::strip_type_decorations(&declared, &DART_TYPE_NAME_RULES);
    if stripped.is_empty() || stripped.contains('.') {
        None
    } else {
        Some(stripped)
    }
}

/// Record the type a local initializer produces (`is_inferred=true`): a
/// same-file constructor call, or a call to a same-file function or method
/// with a declared return type. Unqualified calls follow Dart's lexical
/// scope (enclosing type member, then library function); `this.m()` resolves
/// in the enclosing type, except in an extension, and `Type.m()` in the named
/// same-file type. `await`
/// removes one `Future`/`FutureOr` layer and `!` removes nullability; any
/// other receiver, method, or operator records nothing.
pub(super) fn record_initializer_type(
    base: &mut BaseExtractor,
    symbol_id: &str,
    value: Node,
    return_types: &ReturnTypeIndex,
    same_file_types: &HashSet<String>,
) {
    let declaration = enclosing_type_declaration(value);
    let scope = InitializerScope {
        base,
        return_types,
        same_file_types,
        owner: declaration.map_or(Owner::Library, |declaration| {
            declaration_owner(base, declaration)
        }),
        this_is_owner: declaration
            .is_some_and(|declaration| declaration.kind() != "extension_declaration"),
    };
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
        &DART_TYPE_NAME_RULES,
        true,
    );
}

/// A type reduced to what initializer inference needs: the bindable base
/// name (`None` for type parameters, `void`, `dynamic`, and function or record
/// types), the written text, and the type arguments.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct TypeShape {
    name: Option<String>,
    declared: String,
    args: Vec<TypeShape>,
}

impl TypeShape {
    fn named(name: String) -> Self {
        Self {
            declared: name.clone(),
            name: Some(name),
            args: Vec::new(),
        }
    }

    /// The result type of `await` on a `Future<T>` or `FutureOr<T>`.
    fn awaited(self) -> Option<TypeShape> {
        if !matches!(self.name.as_deref(), Some("Future" | "FutureOr")) || self.args.len() != 1 {
            return None;
        }
        let nullable = self.declared.ends_with('?');
        let mut inner = self.args.into_iter().next()?;
        if nullable && !inner.declared.ends_with('?') {
            inner.declared.push('?');
        }
        Some(inner)
    }

    fn non_nullable(mut self) -> TypeShape {
        if let Some(stripped) = self.declared.strip_suffix('?') {
            self.declared = stripped.trim_end().to_string();
        }
        self
    }
}

/// The declaration scope a call resolves in.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Owner {
    Library,
    Type(String),
    /// An unnamed extension: its members cannot be named from outside.
    Unnamed,
}

fn is_type_declaration(kind: &str) -> bool {
    matches!(
        kind,
        "class_declaration"
            | "mixin_declaration"
            | "enum_declaration"
            | "extension_declaration"
            | "extension_type_declaration"
    )
}

fn declaration_owner(base: &BaseExtractor, declaration: Node) -> Owner {
    declaration
        .child_by_field_name("name")
        .map(|name| extension_type_name(name).unwrap_or(name))
        .map_or(Owner::Unnamed, |name| {
            Owner::Type(base.get_node_text(&name))
        })
}

/// The type identifier of an `extension_type_name`, which also holds the
/// type parameters and the representation constructor name (`Id<T>._`).
fn extension_type_name(name: Node) -> Option<Node> {
    (name.kind() == "extension_type_name")
        .then(|| name.named_child(0))
        .flatten()
}

fn declaration_generics(base: &BaseExtractor, declaration: Node) -> Vec<String> {
    let mut generics = type_parameter_names(base, declaration);
    if let Some(name) = declaration.child_by_field_name("name")
        && name.kind() == "extension_type_name"
    {
        generics.extend(type_parameter_names(base, name));
    }
    generics
}

fn enclosing_type_declaration(node: Node) -> Option<Node> {
    let mut current = node;
    while let Some(parent) = current.parent() {
        if is_type_declaration(parent.kind()) {
            return Some(parent);
        }
        current = parent;
    }
    None
}

/// Declared return types of the file's functions and type members, keyed by
/// owner (`None` for the library) and name, plus every name a parameter,
/// local variable, local function, or pattern binds. Getters, setters,
/// fields, and enum constants are members with no return type, so they
/// block inference.
#[derive(Debug, Default)]
pub(super) struct ReturnTypeIndex {
    members: HashMap<(Option<String>, String), Vec<Option<TypeShape>>>,
    local_names: HashSet<String>,
}

#[derive(Clone)]
struct IndexScope {
    owner: Owner,
    generics: Vec<String>,
    in_function: bool,
    in_parameter: bool,
}

impl ReturnTypeIndex {
    pub(super) fn build(base: &BaseExtractor, root: Node) -> Self {
        let mut index = Self::default();
        let scope = IndexScope {
            owner: Owner::Library,
            generics: Vec::new(),
            in_function: false,
            in_parameter: false,
        };
        index.visit(base, root, &scope, 0);
        index
    }

    fn visit(&mut self, base: &BaseExtractor, node: Node, scope: &IndexScope, depth: u32) {
        if !should_visit_tree_depth(depth) {
            return;
        }
        let mut inner = scope.clone();
        let name_of = |field: &str| {
            node.child_by_field_name(field)
                .map(|name| base.get_node_text(&name))
        };
        match node.kind() {
            kind if is_type_declaration(kind) => {
                inner.owner = declaration_owner(base, node);
                inner.generics = declaration_generics(base, node);
            }
            "extension_type_representation" => {
                if let Some(name) = name_of("name") {
                    self.add_member(scope, name, None);
                }
            }
            "function_signature" => {
                if let Some(name) = name_of("name") {
                    match node.parent().map(|parent| parent.kind()) {
                        Some("function_declaration" | "method_signature" | "declaration")
                            if !scope.in_function =>
                        {
                            let mut generics = scope.generics.clone();
                            generics.extend(type_parameter_names(base, node));
                            let shape =
                                node.child_by_field_name("return_type")
                                    .and_then(|return_type| {
                                        type_shape(base, return_type, &generics, 0)
                                    });
                            self.add_member(scope, name, shape);
                        }
                        Some("local_function_declaration") => {
                            self.local_names.insert(name);
                        }
                        _ => {}
                    }
                }
            }
            "getter_signature" | "setter_signature" if !scope.in_function => {
                if let Some(name) = name_of("name") {
                    self.add_member(scope, name, None);
                }
            }
            "initialized_variable_definition"
            | "initialized_identifier"
            | "static_final_declaration" => {
                if let Some(name) = name_of("name") {
                    if scope.in_function {
                        self.local_names.insert(name);
                    } else {
                        self.add_member(scope, name, None);
                    }
                }
            }
            "enum_constant" => {
                if let Some(name) = name_of("name") {
                    self.add_member(scope, name, None);
                }
            }
            "identifier_list" if !scope.in_function => {
                for field in node.named_children(&mut node.walk()) {
                    self.add_member(scope, base.get_node_text(&field), None);
                }
            }
            "for_statement" | "variable_pattern" => {
                self.local_names.extend(name_of("name"));
            }
            "catch_clause" => {
                self.local_names.extend(name_of("exception"));
                self.local_names.extend(name_of("stack_trace"));
            }
            "constant_pattern" => {
                self.local_names.extend(
                    node.named_child(0)
                        .filter(|child| child.kind() == "identifier")
                        .map(|child| base.get_node_text(&child)),
                );
            }
            "formal_parameter" => inner.in_parameter = true,
            "identifier" if scope.in_parameter => {
                self.local_names.insert(base.get_node_text(&node));
            }
            "function_body" | "function_expression" | "function_expression_body" => {
                inner.in_function = true;
            }
            _ => {}
        }
        let Some(child_depth) = child_tree_depth(depth) else {
            return;
        };
        for child in node.named_children(&mut node.walk()) {
            self.visit(base, child, &inner, child_depth);
        }
    }

    fn add_member(&mut self, scope: &IndexScope, name: String, shape: Option<TypeShape>) {
        let owner = match &scope.owner {
            Owner::Library => None,
            Owner::Type(owner) => Some(owner.clone()),
            Owner::Unnamed => return,
        };
        self.members.entry((owner, name)).or_default().push(shape);
    }

    fn entries(&self, owner: Option<&str>, name: &str) -> Option<&Vec<Option<TypeShape>>> {
        self.members
            .get(&(owner.map(str::to_string), name.to_string()))
    }

    /// The return type every same-named member of this owner agrees on.
    fn lookup(&self, owner: Option<&str>, name: &str) -> Option<TypeShape> {
        let mut shapes = self.entries(owner, name)?.iter();
        let first = shapes.next()?.as_ref()?;
        shapes
            .all(|shape| shape.as_ref() == Some(first))
            .then(|| first.clone())
    }

    fn declares(&self, owner: &str, name: &str) -> bool {
        self.entries(Some(owner), name).is_some()
    }
}

fn type_parameter_names(base: &BaseExtractor, node: Node) -> Vec<String> {
    let Some(parameters) = find_child_by_type(&node, "type_parameters") else {
        return Vec::new();
    };
    parameters
        .named_children(&mut parameters.walk())
        .filter(|parameter| parameter.kind() == "type_parameter")
        .filter_map(|parameter| parameter.child_by_field_name("name"))
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
    let name = base_type_name(base, node)
        .filter(|name| !matches!(name.as_str(), "void" | "dynamic") && !generics.contains(name));
    let args = match (
        find_child_by_type(&node, "type_arguments"),
        child_tree_depth(depth),
    ) {
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

struct InitializerScope<'a> {
    base: &'a BaseExtractor,
    return_types: &'a ReturnTypeIndex,
    same_file_types: &'a HashSet<String>,
    owner: Owner,
    /// False inside an extension: `this.m()` there uses normal member lookup
    /// on the on-type, which usually lives in another file or the SDK, and
    /// the extension member applies only when the on-type lacks `m`.
    this_is_owner: bool,
}

impl InitializerScope<'_> {
    fn shape_of(&self, value: Node, depth: u32) -> Option<TypeShape> {
        if !should_visit_tree_depth(depth) {
            return None;
        }
        let child_depth = child_tree_depth(depth)?;
        match value.kind() {
            "parenthesized_expression" => self.shape_of(value.named_child(0)?, child_depth),
            "unary_expression" if value.named_child_count() == 1 => {
                let operand = value.named_child(0)?;
                (operand.kind() == "await_expression")
                    .then(|| self.shape_of(operand, child_depth))
                    .flatten()
            }
            "await_expression" => {
                let operand = value.named_children(&mut value.walk()).last()?;
                self.shape_of(operand, child_depth)?.awaited()
            }
            "null_assertion_expression" => self
                .shape_of(value.child_by_field_name("value")?, child_depth)
                .map(TypeShape::non_nullable),
            "new_expression" | "const_object_expression" => {
                inferred_constructor_name(self.base, value, self.same_file_types)
                    .map(TypeShape::named)
            }
            "call_expression" => self.call_shape(value),
            _ => None,
        }
    }

    fn call_shape(&self, call: Node) -> Option<TypeShape> {
        let function = call.child_by_field_name("function")?;
        let function = if function.kind() == "instantiation_expression" {
            function.child_by_field_name("function")?
        } else {
            function
        };
        match function.kind() {
            "identifier" => {
                let name = self.base.get_node_text(&function);
                if !self.binds_value(&name) && self.same_file_types.contains(&name) {
                    Some(TypeShape::named(name))
                } else {
                    self.unqualified_call(&name)
                }
            }
            "member_expression" | "null_aware_member_expression" => {
                let object = function.child_by_field_name("object")?;
                let member = self
                    .base
                    .get_node_text(&function.child_by_field_name("property")?);
                match object.kind() {
                    "this" if function.kind() == "member_expression" && self.this_is_owner => {
                        match &self.owner {
                            Owner::Type(owner) => self.return_types.lookup(Some(owner), &member),
                            _ => None,
                        }
                    }
                    "identifier" => self.static_call(self.base.get_node_text(&object), &member),
                    _ => None,
                }
            }
            _ => None,
        }
    }

    /// Whether a local, a parameter, or a member of the enclosing type may bind
    /// `name` before Dart's lexical lookup reaches a library declaration.
    /// Unnamed extension members are not indexed, so any name may be bound.
    fn binds_value(&self, name: &str) -> bool {
        self.return_types.local_names.contains(name)
            || match &self.owner {
                Owner::Library => false,
                Owner::Unnamed => true,
                Owner::Type(owner) => self.return_types.declares(owner, name),
            }
    }

    fn unqualified_call(&self, name: &str) -> Option<TypeShape> {
        if self.return_types.local_names.contains(name) {
            return None;
        }
        match &self.owner {
            Owner::Unnamed => None,
            Owner::Type(owner) if self.return_types.declares(owner, name) => {
                self.return_types.lookup(Some(owner), name)
            }
            _ => self.return_types.lookup(None, name),
        }
    }

    /// `Type.member()`: a same-file static member, else a named constructor.
    fn static_call(&self, owner: String, member: &str) -> Option<TypeShape> {
        if self.binds_value(&owner) {
            None
        } else if self.return_types.declares(&owner, member) {
            self.return_types.lookup(Some(&owner), member)
        } else if self.same_file_types.contains(&owner) {
            Some(TypeShape::named(owner))
        } else {
            None
        }
    }
}
