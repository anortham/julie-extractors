use super::helpers;
use crate::base::BaseExtractor;
use crate::base::types::TypeNameRules;
use std::collections::{HashMap, HashSet};
use tree_sitter::Node;

pub(super) const C_TYPE_NAME_RULES: TypeNameRules = TypeNameRules {
    nullable_suffixes: &[],
    reference_prefixes: &["struct", "union", "enum", "const", "volatile"],
    generic_open: &[],
};

pub(super) fn record_declared_from_declaration(
    base: &mut BaseExtractor,
    symbol_id: &str,
    decl: Node,
    declarator: Node,
) {
    if contains_function_declarator(declarator) {
        return;
    }
    record_declared(base, symbol_id, decl, declarator);
}

/// Record a variable's declared type. A GNU `__auto_type` variable instead
/// records the inferred type of its initializer.
pub(super) fn record_variable_type(
    base: &mut BaseExtractor,
    symbol_id: &str,
    decl: Node,
    declarator: Node,
    return_types: &ReturnTypeIndex,
) {
    let auto_type = decl
        .child_by_field_name("type")
        .is_some_and(|type_node| base.get_node_text(&type_node) == "__auto_type");
    if !auto_type {
        record_declared_from_declaration(base, symbol_id, decl, declarator);
        return;
    }
    let plain_name = declarator
        .child_by_field_name("declarator")
        .is_some_and(|name| name.kind() == "identifier");
    if declarator.kind() == "init_declarator"
        && plain_name
        && let Some(value) = declarator.child_by_field_name("value")
    {
        record_initializer_type(base, symbol_id, value, return_types);
    }
}

/// Record the return type of the same-file function an `auto` or
/// `__auto_type` initializer calls (`is_inferred=true`). The value is a
/// `call_expression`, or the `function_declarator` tree-sitter-c reads a C23
/// `auto x = f();` call as. Any other initializer records nothing.
pub(super) fn record_initializer_type(
    base: &mut BaseExtractor,
    symbol_id: &str,
    value: Node,
    return_types: &ReturnTypeIndex,
) {
    let callee = match value.kind() {
        "call_expression" => value.child_by_field_name("function"),
        "function_declarator" => value.child_by_field_name("declarator"),
        _ => None,
    };
    let Some(callee) = callee else {
        return;
    };
    if let Some(return_type) = return_types.lookup(&base.get_node_text(&callee)) {
        record(base, symbol_id, return_type, true);
    }
}

/// Record a function's return type from its definition or prototype.
pub(super) fn record_return_type(
    base: &mut BaseExtractor,
    symbol_id: &str,
    decl: Node,
    declarator: Node,
) {
    if let Some(return_type) = return_type(base, decl, declarator) {
        record(base, symbol_id, &return_type, false);
    }
}

/// A declared type as its fact records it: the base name and the written text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct DeclaredType {
    base: String,
    declared: String,
}

/// Declared return types of the file's functions, by name, for `auto` and
/// `__auto_type` inference. A name is unknown anywhere in the file when it
/// also appears in any other role: a macro, variable, parameter, typedef,
/// assignment target, cast operand, or function with no plain return type,
/// since a call through a variable does not reach the function. Only the name
/// of a well-formed declaration, the callee of a call (also the call
/// tree-sitter-c reads as the declarator of a C23 `auto w = f();`), and a
/// struct, union, or enum tag count as expected uses. Any name in an ERROR node, in a malformed
/// declaration, in an expression statement with parse errors, or in a
/// file-scope expression statement is unknown, since tree-sitter-c makes
/// these from misparsed declarations and the name may be a shadowing local or
/// a disagreeing prototype.
#[derive(Debug, Default)]
pub(super) struct ReturnTypeIndex(HashMap<String, Vec<Option<DeclaredType>>>);

impl ReturnTypeIndex {
    pub(super) fn build(base: &BaseExtractor, root: Node) -> Self {
        let mut entries: HashMap<String, Vec<Option<DeclaredType>>> = HashMap::new();
        let mut expected_uses = HashSet::new();
        let mut unknown = HashSet::new();
        let mut stack = vec![(root, false, false)];
        while let Some((node, in_function, misparsed)) = stack.pop() {
            let misparsed = misparsed
                || node.is_error()
                || (node.kind() == "expression_statement" && (node.has_error() || !in_function));
            match node.kind() {
                "identifier" | "type_identifier"
                    if misparsed || !expected_uses.contains(&node.id()) =>
                {
                    unknown.insert(base.get_node_text(&node));
                }
                "call_expression" => {
                    expected_uses.extend(node.child_by_field_name("function").map(|f| f.id()));
                }
                "struct_specifier" | "union_specifier" | "enum_specifier" => {
                    expected_uses.extend(node.child_by_field_name("name").map(|n| n.id()));
                }
                "function_definition" | "declaration" => {
                    for (name, return_type) in declared_names(base, node) {
                        expected_uses.insert(name.id());
                        entries
                            .entry(base.get_node_text(&name))
                            .or_default()
                            .push(return_type);
                    }
                    expected_uses.extend(
                        helpers::c23_auto_declaration(base, node)
                            .filter(|auto| auto.value.kind() == "function_declarator")
                            .and_then(|auto| auto.value.child_by_field_name("declarator"))
                            .map(|callee| callee.id()),
                    );
                    unknown.extend(misread_function_names(base, node));
                }
                _ => {}
            }
            let in_function = in_function || node.kind() == "function_definition";
            stack.extend(
                node.named_children(&mut node.walk())
                    .map(|child| (child, in_function, misparsed)),
            );
        }
        for name in unknown {
            entries.entry(name).or_default().push(None);
        }
        Self(entries)
    }

    /// The return type every same-named declaration agrees on.
    fn lookup(&self, name: &str) -> Option<&DeclaredType> {
        let mut return_types = self.0.get(name)?.iter();
        let first = return_types.next()?.as_ref()?;
        return_types
            .all(|return_type| return_type.as_ref() == Some(first))
            .then_some(first)
    }
}

/// The name nodes a well-formed definition or declaration declares, each with
/// its plain return type when it names a function. A declaration with parse
/// errors, or a definition with no function declarator, declares no trusted
/// name, so each name in it is unknown.
fn declared_names<'tree>(
    base: &BaseExtractor,
    node: Node<'tree>,
) -> Vec<(Node<'tree>, Option<DeclaredType>)> {
    let malformed = if node.kind() == "declaration" {
        node.has_error()
    } else {
        ["type", "declarator"]
            .iter()
            .filter_map(|field| node.child_by_field_name(field))
            .any(|child| child.has_error())
            || !node
                .child_by_field_name("declarator")
                .is_some_and(contains_function_declarator)
    };
    if malformed {
        return Vec::new();
    }
    let mut cursor = node.walk();
    node.children_by_field_name("declarator", &mut cursor)
        .filter_map(|declarator| {
            let target = helpers::declarator_target(declarator)?;
            Some((target.name, return_type(base, node, declarator)))
        })
        .collect()
}

/// The callee of each call tree-sitter-c reads after a function declarator's
/// name when a parenthesized macro stands before the real name, as in
/// `NONNULL(1) make(void *p)` or `__attribute__((malloc)) make(void)`.
fn misread_function_names(base: &BaseExtractor, node: Node) -> Vec<String> {
    let mut names = Vec::new();
    let mut cursor = node.walk();
    for declarator in node.children_by_field_name("declarator", &mut cursor) {
        let mut current = Some(declarator);
        while let Some(declarator) = current {
            if declarator.kind() == "function_declarator" {
                names.extend(
                    declarator
                        .named_children(&mut declarator.walk())
                        .filter(|child| child.kind() == "call_expression")
                        .filter_map(|call| call.child_by_field_name("function"))
                        .filter(|callee| callee.kind() == "identifier")
                        .map(|callee| base.get_node_text(&callee)),
                );
            }
            current = nested_declarator(declarator);
        }
    }
    names
}

/// A function's return type. The declarator must be pointer levels around the
/// function's own declarator; a function returning a function pointer has no
/// plain return type.
fn return_type(base: &BaseExtractor, decl: Node, declarator: Node) -> Option<DeclaredType> {
    let mut current = declarator;
    while current.kind() == "pointer_declarator" {
        current = current.child_by_field_name("declarator")?;
    }
    let names_function = current.kind() == "function_declarator"
        && current
            .child_by_field_name("declarator")
            .is_some_and(|name| name.kind() != "parenthesized_declarator");
    names_function
        .then(|| declared_type(base, decl, declarator))
        .flatten()
}

fn record_declared(base: &mut BaseExtractor, symbol_id: &str, decl: Node, declarator: Node) {
    if let Some(declared) = declared_type(base, decl, declarator) {
        record(base, symbol_id, &declared, false);
    }
}

fn record(base: &mut BaseExtractor, symbol_id: &str, declared: &DeclaredType, is_inferred: bool) {
    base.record_declared_type_fact_with_declared(
        symbol_id,
        &declared.base,
        &declared.declared,
        &C_TYPE_NAME_RULES,
        is_inferred,
    );
}

fn declared_type(base: &BaseExtractor, decl: Node, declarator: Node) -> Option<DeclaredType> {
    let name_node = base_type_name_node(decl.child_by_field_name("type")?)?;
    let mut base_name = base.get_node_text(&name_node);
    let (pointers, array_declared, array_count) = declarator_decorations(base, declarator);
    for _ in 0..array_count {
        base_name.push_str("[]");
    }
    let mut declared = declared_prefix(base, decl);
    if !pointers.is_empty() {
        declared.push(' ');
        declared.push_str(&pointers);
    }
    declared.push_str(&array_declared);
    Some(DeclaredType {
        base: base_name,
        declared,
    })
}

fn base_type_name_node(node: Node) -> Option<Node> {
    match node.kind() {
        "primitive_type" | "type_identifier" => Some(node),
        "sized_type_specifier" => single_word_sized_type(node),
        "struct_specifier" | "union_specifier" | "enum_specifier" => {
            node.child_by_field_name("name")
        }
        _ => None,
    }
}

fn single_word_sized_type(node: Node) -> Option<Node> {
    let mut cursor = node.walk();
    let words = node
        .children(&mut cursor)
        .filter(|child| child.kind() != "type_qualifier")
        .count();
    (words == 1).then_some(node)
}

fn declared_prefix(base: &BaseExtractor, decl: Node) -> String {
    let mut parts = Vec::new();
    let mut cursor = decl.walk();
    for child in decl.children(&mut cursor) {
        match child.kind() {
            "type_qualifier"
            | "primitive_type"
            | "type_identifier"
            | "sized_type_specifier"
            | "struct_specifier"
            | "union_specifier"
            | "enum_specifier" => {
                parts.push(base.get_node_text(&child));
            }
            _ => {}
        }
    }
    parts.join(" ")
}

fn contains_function_declarator(node: Node) -> bool {
    let mut node = Some(node);
    while let Some(current) = node {
        if current.kind() == "function_declarator" {
            return true;
        }
        node = nested_declarator(current);
    }
    false
}

/// The pointer text of a declarator chain, each `*` with its own qualifiers
/// (`*const`), then its array suffix and array depth.
fn declarator_decorations(base: &BaseExtractor, node: Node) -> (String, String, usize) {
    let mut pointers = String::new();
    let mut array_suffix = String::new();
    let mut array_count = 0;
    let mut node = Some(node);
    while let Some(current) = node {
        match current.kind() {
            "pointer_declarator" => {
                pointers.push('*');
                let mut cursor = current.walk();
                for qualifier in current
                    .children(&mut cursor)
                    .filter(|child| child.kind() == "type_qualifier")
                {
                    pointers.push_str(&base.get_node_text(&qualifier));
                }
            }
            "array_declarator" => {
                array_count += 1;
                array_suffix.push('[');
                if let Some(size) = current.child_by_field_name("size") {
                    array_suffix.push_str(&base.get_node_text(&size));
                }
                array_suffix.push(']');
            }
            _ => {}
        }
        node = nested_declarator(current);
    }
    (pointers, array_suffix, array_count)
}

fn nested_declarator(node: Node) -> Option<Node> {
    match node.kind() {
        "init_declarator"
        | "pointer_declarator"
        | "array_declarator"
        | "parenthesized_declarator"
        | "function_declarator" => node.child_by_field_name("declarator"),
        _ => None,
    }
}
