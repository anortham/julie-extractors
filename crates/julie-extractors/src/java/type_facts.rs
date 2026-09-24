//! Declared and initializer-inferred type fact recording for Java.

use crate::base::BaseExtractor;
use crate::base::types::TypeNameRules;
use std::collections::HashMap;
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
/// innermost named enclosing type; `Type.method(..)` resolves in same-file
/// types named `Type`, to `static` methods only. The arity-compatible
/// candidates must agree. `void`, type-parameter returns, and any other
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
/// method name. Methods of anonymous classes and enum constant bodies are
/// left out: no call site can name their owner.
#[derive(Debug, Default)]
pub(super) struct ReturnTypeIndex(HashMap<String, Vec<ReturnEntry>>);

#[derive(Debug)]
struct ReturnEntry {
    owner: String,
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
        let mut entries: HashMap<String, Vec<ReturnEntry>> = HashMap::new();
        let mut stack = vec![root];
        while let Some(node) = stack.pop() {
            if node.kind() == "method_declaration"
                && let Some((name, entry)) = return_entry(base, node)
            {
                entries.entry(name).or_default().push(entry);
            }
            stack.extend(node.named_children(&mut node.walk()));
        }
        Self(entries)
    }

    fn call_type(&self, base: &BaseExtractor, call: Node) -> Option<&str> {
        if call.kind() != "method_invocation" {
            return None;
        }
        let (owner, type_qualified) = match call.child_by_field_name("object") {
            None => (declaring_type_name(base, call)?, false),
            Some(object) if object.kind() == "this" => (declaring_type_name(base, call)?, false),
            Some(object) if object.kind() == "identifier" => (base.get_node_text(&object), true),
            Some(_) => return None,
        };
        let name = base.get_node_text(&call.child_by_field_name("name")?);
        let argument_count = non_comment_count(call.child_by_field_name("arguments")?);
        self.lookup(&name, &owner, argument_count, type_qualified)
    }

    /// The return type every arity-compatible `owner.name` method agrees on.
    /// A type-qualified call needs every candidate to be `static`: an
    /// instance method there means the qualifier is a variable, not the type.
    fn lookup(
        &self,
        name: &str,
        owner: &str,
        argument_count: usize,
        type_qualified: bool,
    ) -> Option<&str> {
        let mut candidates = self
            .0
            .get(name)?
            .iter()
            .filter(|entry| entry.owner == owner && entry.accepts(argument_count))
            .map(|entry| {
                entry
                    .declared
                    .as_deref()
                    .filter(|_| !type_qualified || entry.is_static)
            });
        let first = candidates.next()??;
        candidates
            .all(|declared| declared == Some(first))
            .then_some(first)
    }
}

fn return_entry(base: &BaseExtractor, method: Node) -> Option<(String, ReturnEntry)> {
    let name = base.get_node_text(&method.child_by_field_name("name")?);
    let owner = declaring_type_name(base, method)?;
    let parameters = method.child_by_field_name("parameters")?;
    let parameter_kinds: Vec<&str> = parameters
        .named_children(&mut parameters.walk())
        .map(|parameter| parameter.kind())
        .filter(|kind| matches!(*kind, "formal_parameter" | "spread_parameter"))
        .collect();
    let declared = method
        .child_by_field_name("type")
        .filter(|_| method.child_by_field_name("dimensions").is_none())
        .filter(|type_node| names_single_base_type(*type_node) && !is_var_type(base, *type_node))
        .filter(|type_node| !names_type_parameter(base, *type_node, method))
        .map(|type_node| base.get_node_text(&type_node));
    Some((
        name,
        ReturnEntry {
            owner,
            declared,
            parameter_count: parameter_kinds.len(),
            variadic: parameter_kinds.contains(&"spread_parameter"),
            is_static: super::helpers::extract_modifiers(base, method)
                .iter()
                .any(|modifier| modifier == "static"),
        },
    ))
}

/// The name of the innermost type whose body holds `node`; `None` inside an
/// anonymous class or enum constant body.
fn declaring_type_name(base: &BaseExtractor, node: Node) -> Option<String> {
    let mut current = node.parent()?;
    while !matches!(
        current.kind(),
        "class_body" | "interface_body" | "enum_body" | "annotation_type_body"
    ) {
        current = current.parent()?;
    }
    let declaration = current.parent()?;
    if !matches!(
        declaration.kind(),
        "class_declaration"
            | "interface_declaration"
            | "enum_declaration"
            | "record_declaration"
            | "annotation_type_declaration"
    ) {
        return None;
    }
    Some(base.get_node_text(&declaration.child_by_field_name("name")?))
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
