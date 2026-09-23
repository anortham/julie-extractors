//! Helper methods for node navigation, name extraction, and tree utilities
//!
//! This module provides utilities for finding nodes, extracting names from various
//! C constructs, and navigating the syntax tree.

use crate::base::{AnnotationMarker, BaseExtractor, normalize_annotations};
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};

/// Extract standard C attributes that decorate a declaration.
pub(super) fn extract_attributes(base: &BaseExtractor, node: tree_sitter::Node) -> Vec<String> {
    let mut attributes = Vec::new();
    collect_attributes_from_text(&base.get_node_text(&node), &mut attributes);

    let mut current = node.prev_sibling();
    while let Some(sibling) = current {
        if !matches!(
            sibling.kind(),
            "attribute_specifier" | "attribute_declaration"
        ) {
            break;
        }

        let sibling_text = base.get_node_text(&sibling);
        let trimmed = sibling_text.trim_start();
        if !(trimmed.starts_with("[[") || trimmed.starts_with("__attribute")) {
            break;
        }

        let mut sibling_attributes = Vec::new();
        collect_attributes_from_text(&sibling_text, &mut sibling_attributes);
        sibling_attributes.extend(attributes);
        attributes = sibling_attributes;
        current = sibling.prev_sibling();
    }

    attributes
}

/// Annotations from the attribute specifiers written directly on a record,
/// field, or variable declaration.
pub(super) fn child_attributes(
    base: &BaseExtractor,
    node: tree_sitter::Node,
) -> Vec<AnnotationMarker> {
    let mut attributes = Vec::new();
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if is_attribute_node(child) {
            collect_attributes_from_text(&base.get_node_text(&child), &mut attributes);
        }
    }
    normalize_annotations(&attributes, "c")
}

fn collect_attributes_from_text(text: &str, attributes: &mut Vec<String>) {
    collect_standard_attributes_from_text(text, attributes);
    collect_gnu_attributes_from_text(text, attributes);
}

fn collect_standard_attributes_from_text(text: &str, attributes: &mut Vec<String>) {
    let mut remaining = text;
    while let Some(start) = remaining.find("[[") {
        let after_start = &remaining[start + 2..];
        let Some(end) = after_start.find("]]") else {
            break;
        };
        attributes.push(format!("[[{}]]", after_start[..end].trim()));
        remaining = &after_start[end + 2..];
    }
}

fn collect_gnu_attributes_from_text(text: &str, attributes: &mut Vec<String>) {
    let mut remaining = text;
    while let Some(start) = find_gnu_attribute_keyword(remaining) {
        let after_keyword = remaining[start..]
            .strip_prefix("__attribute__")
            .or_else(|| remaining[start..].strip_prefix("__attribute"))
            .unwrap_or(&remaining[start..])
            .trim_start();

        let Some((inner, consumed)) = extract_gnu_attribute_inner(after_keyword) else {
            break;
        };

        attributes.extend(
            split_top_level_commas(inner)
                .into_iter()
                .map(str::to_string),
        );
        remaining = &after_keyword[consumed..];
    }
}

fn find_gnu_attribute_keyword(text: &str) -> Option<usize> {
    match (text.find("__attribute__"), text.find("__attribute")) {
        (Some(long), Some(short)) => Some(long.min(short)),
        (Some(index), None) | (None, Some(index)) => Some(index),
        (None, None) => None,
    }
}

fn extract_gnu_attribute_inner(text: &str) -> Option<(&str, usize)> {
    if !text.starts_with("((") {
        return None;
    }

    let mut depth = 0usize;
    for (index, character) in text.char_indices() {
        match character {
            '(' => depth += 1,
            ')' => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    if index == 0 {
                        return None;
                    }
                    return Some((&text[2..index - 1], index + character.len_utf8()));
                }
            }
            _ => {}
        }
    }

    None
}

fn split_top_level_commas(text: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut start = 0usize;
    let mut depth = 0usize;

    for (index, character) in text.char_indices() {
        match character {
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => {
                let part = text[start..index].trim();
                if !part.is_empty() {
                    parts.push(part);
                }
                start = index + character.len_utf8();
            }
            _ => {}
        }
    }

    let part = text[start..].trim();
    if !part.is_empty() {
        parts.push(part);
    }

    parts
}

/// The name a declarator declares, read the way C binds declarators: the
/// derivation applied directly to the name decides what the name is, so
/// `char **f(void)` is a function and `int (*f)(void)` is a pointer variable.
pub(super) struct DeclaratorTarget<'a> {
    pub name: tree_sitter::Node<'a>,
    /// The `function_declarator` applied directly to the name, when the name is a function.
    pub function: Option<tree_sitter::Node<'a>>,
    pub pointer_depth: usize,
    /// Whether any function declarator appears in the chain, as in a function-pointer variable.
    pub derives_function: bool,
}

pub(super) fn declarator_target(declarator: tree_sitter::Node) -> Option<DeclaratorTarget> {
    let mut current = declarator;
    let mut innermost = None;
    let mut pointer_depth = 0;
    let mut derives_function = false;
    for _ in 0..64 {
        match current.kind() {
            // tree-sitter-c reads names such as `uint64_t` as `primitive_type`,
            // including where a typedef declares them.
            "identifier" | "field_identifier" | "type_identifier" | "primitive_type" => {
                return Some(DeclaratorTarget {
                    name: current,
                    function: innermost
                        .filter(|node: &tree_sitter::Node| node.kind() == "function_declarator"),
                    pointer_depth,
                    derives_function,
                });
            }
            "init_declarator" => current = current.child_by_field_name("declarator")?,
            "parenthesized_declarator" | "attributed_declarator" => {
                current = first_declarator_child(current)?;
            }
            "pointer_declarator" | "array_declarator" | "function_declarator" => {
                match current.kind() {
                    "pointer_declarator" => pointer_depth += 1,
                    "function_declarator" => derives_function = true,
                    _ => {}
                }
                innermost = Some(current);
                current = current.child_by_field_name("declarator")?;
            }
            _ => return None,
        }
    }
    None
}

fn first_declarator_child(node: tree_sitter::Node) -> Option<tree_sitter::Node> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor).find(|child| {
        !matches!(
            child.kind(),
            "comment" | "attribute_declaration" | "attribute_specifier" | "type_qualifier"
        )
    })
}

/// The first declarator of a function definition or declaration that declares a function.
pub(super) fn function_declarator_target(node: tree_sitter::Node) -> Option<DeclaratorTarget> {
    let mut cursor = node.walk();
    let declarators: Vec<_> = node
        .children_by_field_name("declarator", &mut cursor)
        .collect();
    declarators
        .into_iter()
        .filter_map(declarator_target)
        .find(|target| target.function.is_some())
}

/// Whether a `type_identifier` is the name a struct, union, or enum body or a
/// typedef declarator introduces, rather than a use of a type.
pub(super) fn is_type_declaration_name(node: tree_sitter::Node) -> bool {
    let Some(parent) = node.parent() else {
        return false;
    };
    if matches!(
        parent.kind(),
        "struct_specifier" | "union_specifier" | "enum_specifier"
    ) {
        return parent.child_by_field_name("body").is_some();
    }
    let mut current = node;
    while let Some(parent) = current.parent() {
        match parent.kind() {
            "type_definition" => {
                let mut cursor = parent.walk();
                return parent
                    .children_by_field_name("declarator", &mut cursor)
                    .any(|declarator| declarator.id() == current.id());
            }
            "pointer_declarator" | "array_declarator" | "function_declarator" => {
                if parent
                    .child_by_field_name("declarator")
                    .is_none_or(|declarator| declarator.id() != current.id())
                {
                    return false;
                }
            }
            "parenthesized_declarator" | "attributed_declarator" => {}
            _ => return false,
        }
        current = parent;
    }
    false
}

/// Find a function declarator node within a function definition or declaration
pub(super) fn find_function_declarator(node: tree_sitter::Node) -> Option<tree_sitter::Node> {
    function_declarator_target(node)?.function
}

/// Find the deepest identifier within a declarator node tree
pub(super) fn find_deepest_identifier<'a>(
    node: tree_sitter::Node<'a>,
) -> Option<tree_sitter::Node<'a>> {
    find_deepest_identifier_at_depth(node, 0)
}

fn find_deepest_identifier_at_depth<'a>(
    node: tree_sitter::Node<'a>,
    depth: u32,
) -> Option<tree_sitter::Node<'a>> {
    if !should_visit_tree_depth(depth) {
        return None;
    }
    if node.kind() == "identifier" {
        return Some(node);
    }

    let child_depth = child_tree_depth(depth)?;
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if let Some(result) = find_deepest_identifier_at_depth(child, child_depth) {
            return Some(result);
        }
    }

    None
}

/// Find a node by its type/kind recursively
pub(super) fn find_node_by_type<'a>(
    node: tree_sitter::Node<'a>,
    node_type: &str,
) -> Option<tree_sitter::Node<'a>> {
    find_node_by_type_at_depth(node, node_type, 0)
}

fn find_node_by_type_at_depth<'a>(
    node: tree_sitter::Node<'a>,
    node_type: &str,
    depth: u32,
) -> Option<tree_sitter::Node<'a>> {
    if !should_visit_tree_depth(depth) {
        return None;
    }
    if node.kind() == node_type {
        return Some(node);
    }

    let child_depth = child_tree_depth(depth)?;
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if let Some(result) = find_node_by_type_at_depth(child, node_type, child_depth) {
            return Some(result);
        }
    }

    None
}

/// Extract macro name from a preprocessor node
pub(super) fn extract_macro_name(base: &BaseExtractor, node: tree_sitter::Node) -> Option<String> {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "identifier" {
            return Some(base.get_node_text(&child));
        }
    }
    None
}

/// Extract include path from an include directive signature
pub(super) fn extract_include_path(signature: &str) -> Option<String> {
    // Extract include path from #include statement
    if let Some(start) = signature.find('"')
        && let Some(end) = signature.rfind('"')
        && start < end
    {
        return Some(signature[start + 1..end].to_string());
    }
    if let Some(start) = signature.find('<')
        && let Some(end) = signature.rfind('>')
        && start < end
    {
        return Some(signature[start + 1..end].to_string());
    }
    None
}

/// Check if an include is a system header (uses < > instead of " ")
pub(super) fn is_system_header(signature: &str) -> bool {
    signature.contains('<') && signature.contains('>')
}

/// Extract function name from a function definition or declaration
pub(super) fn extract_function_name(
    base: &BaseExtractor,
    node: tree_sitter::Node,
) -> Option<String> {
    function_declarator_target(node).map(|target| base.get_node_text(&target.name))
}

/// Extract variable name from a declarator node
pub(super) fn extract_variable_name(
    base: &BaseExtractor,
    declarator: tree_sitter::Node,
) -> Option<String> {
    declarator_target(declarator).map(|target| base.get_node_text(&target.name))
}

/// Extract struct name from a struct specifier
pub(super) fn extract_struct_name(base: &BaseExtractor, node: tree_sitter::Node) -> Option<String> {
    let name_node = node.child_by_field_name("name")?;
    Some(base.get_node_text(&name_node))
}

/// Extract enum name from an enum specifier
pub(super) fn extract_enum_name(base: &BaseExtractor, node: tree_sitter::Node) -> Option<String> {
    let name_node = node.child_by_field_name("name")?;
    Some(base.get_node_text(&name_node))
}

/// Extract union name from a union specifier
pub(super) fn extract_union_name(base: &BaseExtractor, node: tree_sitter::Node) -> Option<String> {
    let name_node = node.child_by_field_name("name")?;
    Some(base.get_node_text(&name_node))
}

/// Whether an expression statement is the name of a typedef whose record body the
/// grammar split off: `typedef struct ALIGN(8) { ... } Name;` recovers as a
/// bodiless `type_definition`, a detached `compound_statement`, then `Name;`.
pub(super) fn follows_detached_typedef_body(node: tree_sitter::Node) -> bool {
    node.prev_named_sibling()
        .filter(|body| body.kind() == "compound_statement")
        .and_then(|body| body.prev_named_sibling())
        .is_some_and(|typedef| typedef.kind() == "type_definition")
}

/// Check if a function/variable is static
pub(super) fn is_static_function(base: &BaseExtractor, node: tree_sitter::Node) -> bool {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "storage_class_specifier" && base.get_node_text(&child) == "static" {
            return true;
        }
    }
    false
}

/// Check if a variable is extern
pub(super) fn is_extern_variable(base: &BaseExtractor, node: tree_sitter::Node) -> bool {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "storage_class_specifier" && base.get_node_text(&child) == "extern" {
            return true;
        }
    }
    false
}

/// Check if a variable is const
pub(super) fn is_const_variable(base: &BaseExtractor, node: tree_sitter::Node) -> bool {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "type_qualifier" && base.get_node_text(&child) == "const" {
            return true;
        }
    }
    false
}

/// Check if a variable is volatile
pub(super) fn is_volatile_variable(base: &BaseExtractor, node: tree_sitter::Node) -> bool {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "type_qualifier" && base.get_node_text(&child) == "volatile" {
            return true;
        }
    }
    false
}

/// Check if a declarator is an array
pub(super) fn is_array_variable(declarator: tree_sitter::Node) -> bool {
    find_node_by_type(declarator, "array_declarator").is_some()
}

/// Find the `field_identifier` name within a struct/union field declarator subtree.
///
/// In tree-sitter C, struct field declarators use `field_identifier` (not `identifier`).
/// The declarator can be a plain `field_identifier`, or wrapped in `pointer_declarator`
/// or `array_declarator`. This function recursively searches for the deepest
/// `field_identifier` in the subtree.
pub(super) fn find_field_identifier_name(
    base: &BaseExtractor,
    node: tree_sitter::Node,
) -> Option<String> {
    find_field_identifier_name_at_depth(base, node, 0)
}

fn find_field_identifier_name_at_depth(
    base: &BaseExtractor,
    node: tree_sitter::Node,
    depth: u32,
) -> Option<String> {
    if !should_visit_tree_depth(depth) {
        return None;
    }
    if node.kind() == "field_identifier" {
        return Some(base.get_node_text(&node));
    }
    let child_depth = child_tree_depth(depth)?;
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if let Some(name) = find_field_identifier_name_at_depth(base, child, child_depth) {
            return Some(name);
        }
    }
    None
}

/// The callee a call names once `(*callee)` dereference wrappers are removed.
pub(super) fn unwrapped_callee(function: tree_sitter::Node) -> tree_sitter::Node {
    let mut callee = function;
    loop {
        let inner = match callee.kind() {
            "parenthesized_expression" => callee.named_child(0),
            "pointer_expression" => callee.child_by_field_name("argument"),
            _ => None,
        };
        match inner {
            Some(inner) => callee = inner,
            None => return callee,
        }
    }
}

/// The token a call's reference site names: the field of `recv->fn(...)` or
/// `(*recv->fn)(...)`, the identifier of `fn(...)` or `(*fn)(...)`, and else
/// the whole callee expression.
pub(super) fn callee_token(function: tree_sitter::Node) -> tree_sitter::Node {
    let callee = unwrapped_callee(function);
    match callee.kind() {
        "field_expression" => callee.child_by_field_name("field").unwrap_or(function),
        "identifier" => callee,
        _ => function,
    }
}

/// Whether a node is the callee of a call, under any `(*...)` wrappers.
pub(super) fn is_call_callee(node: tree_sitter::Node) -> bool {
    let mut current = node;
    while let Some(parent) = current.parent() {
        match parent.kind() {
            "parenthesized_expression" | "pointer_expression" => current = parent,
            "call_expression" => {
                return parent
                    .child_by_field_name("function")
                    .is_some_and(|function| function.id() == current.id());
            }
            _ => return false,
        }
    }
    false
}

/// `static_assert(...)` and `_Static_assert(...)` are compile-time assertions,
/// not calls.
pub(super) fn is_static_assertion(base: &BaseExtractor, call: tree_sitter::Node) -> bool {
    call.child_by_field_name("function")
        .filter(|function| function.kind() == "identifier")
        .is_some_and(|function| {
            matches!(
                base.get_node_text(&function).as_str(),
                "static_assert" | "_Static_assert"
            )
        })
}

/// The `default` association label of `_Generic(x, int: 1, default: 0)`,
/// which the grammar parses as a type name.
pub(super) fn is_generic_default_label(base: &BaseExtractor, node: tree_sitter::Node) -> bool {
    node.parent()
        .filter(|descriptor| descriptor.kind() == "type_descriptor")
        .and_then(|descriptor| descriptor.parent())
        .is_some_and(|parent| parent.kind() == "generic_expression")
        && base.get_node_text(&node) == "default"
}

/// Attribute arguments (`__attribute__((aligned(2)))`, `[[gnu::format(...)]]`)
/// are metadata, not calls or value reads.
pub(super) fn is_attribute_node(node: tree_sitter::Node) -> bool {
    matches!(node.kind(), "attribute_specifier" | "attribute_declaration")
}
