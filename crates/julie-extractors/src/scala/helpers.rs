//! Helper functions for Scala symbol extraction
//!
//! Utility functions for extracting modifiers, visibility, type parameters,
//! and other metadata from Scala AST nodes.

use crate::base::{AnnotationMarker, Visibility, normalize_annotations};
use tree_sitter::Node;

type Base = super::super::base::BaseExtractor;

/// Extract modifiers from a Scala node (abstract, sealed, case, private, etc.).
/// Annotations are not modifiers; [`extract_annotations`] reads them.
pub(super) fn extract_modifiers(base: &Base, node: &Node) -> Vec<String> {
    let mut modifiers = Vec::new();
    for child in node.children(&mut node.walk()) {
        if child.kind() == "modifiers" {
            for mod_child in child.children(&mut child.walk()) {
                let text = base.get_node_text(&mod_child);
                if mod_child.kind() == "access_modifier" {
                    // `private[core]` keeps its qualifier; visibility reads the prefix.
                    modifiers.push(text.split_whitespace().collect());
                } else if is_modifier_keyword(&text) {
                    modifiers.push(text);
                }
            }
        } else if is_direct_modifier(child.kind()) {
            modifiers.push(base.get_node_text(&child));
        }
    }
    modifiers
}

pub(super) fn extract_annotations(base: &Base, node: &Node) -> Vec<AnnotationMarker> {
    let mut raw_annotations = Vec::new();

    for child in node.children(&mut node.walk()) {
        if child.kind() == "annotation" {
            raw_annotations.push(annotation_text(base, child));
        } else if child.kind() == "modifiers" {
            raw_annotations.extend(
                child
                    .children(&mut child.walk())
                    .filter(|mod_child| mod_child.kind() == "annotation")
                    .map(|mod_child| annotation_text(base, mod_child)),
            );
        }
    }

    normalize_annotations(&raw_annotations, "scala")
}

/// The annotation source without the type arguments of its name:
/// `@throws[java.io.IOException]` reads as `@throws`.
fn annotation_text(base: &Base, annotation: Node) -> String {
    let text = base.get_node_text(&annotation);
    let Some(type_arguments) = annotation
        .child_by_field_name("name")
        .filter(|name| name.kind() == "generic_type")
        .and_then(|name| name.child_by_field_name("type_arguments"))
    else {
        return text;
    };
    let start = type_arguments.start_byte() - annotation.start_byte();
    let end = type_arguments.end_byte() - annotation.start_byte();
    match (text.get(..start), text.get(end..)) {
        (Some(head), Some(tail)) => format!("{head}{tail}"),
        _ => text,
    }
}

fn is_modifier_keyword(s: &str) -> bool {
    matches!(
        s,
        "abstract"
            | "sealed"
            | "case"
            | "private"
            | "protected"
            | "override"
            | "final"
            | "lazy"
            | "implicit"
            | "inline"
            | "open"
            | "opaque"
            | "transparent"
            | "erased"
            | "given"
            | "using"
    )
}

fn is_direct_modifier(kind: &str) -> bool {
    matches!(
        kind,
        "abstract"
            | "sealed"
            | "case"
            | "private"
            | "protected"
            | "override"
            | "final"
            | "lazy"
            | "implicit"
    )
}

/// Extract type parameters (e.g., `[T, U]`)
pub(super) fn extract_type_parameters(base: &Base, node: &Node) -> Option<String> {
    node.children(&mut node.walk())
        .find(|n| n.kind() == "type_parameters")
        .map(|tp| base.get_node_text(&tp))
}

/// Extract every parameter list (e.g., `(x: Int)(using ec: Ec)`).
pub(super) fn extract_parameters(base: &Base, node: &Node) -> Option<String> {
    let lists: Vec<String> = node
        .children(&mut node.walk())
        .filter(|n| n.kind() == "parameters" || n.kind() == "class_parameters")
        .map(|p| base.get_node_text(&p))
        .collect();
    (!lists.is_empty()).then(|| lists.concat())
}

/// Extract return type after `:` (e.g., `: String`)
pub(super) fn extract_return_type(base: &Base, node: &Node) -> Option<String> {
    // In Scala, return type typically appears as a child after `:`
    let mut found_colon = false;
    for child in node.children(&mut node.walk()) {
        if base.get_node_text(&child) == ":" {
            found_colon = true;
            continue;
        }
        if found_colon && is_type_node(child.kind()) {
            return Some(base.get_node_text(&child));
        }
    }
    None
}

/// Extract extends clause (e.g., `extends Animal with Serializable`)
pub(super) fn extract_extends(base: &Base, node: &Node) -> Option<String> {
    node.children(&mut node.walk())
        .find(|n| n.kind() == "extends_clause")
        .map(|ec| base.get_node_text(&ec))
}

/// Determine visibility from modifiers
pub(super) fn determine_visibility(modifiers: &[String]) -> Visibility {
    crate::base::visibility::visibility_from_modifiers(modifiers)
}

fn is_type_node(kind: &str) -> bool {
    matches!(
        kind,
        "type_identifier"
            | "generic_type"
            | "compound_type"
            | "infix_type"
            | "function_type"
            | "tuple_type"
            | "stable_type_identifier"
            | "parameter" // sometimes used
    )
}

/// Get the name identifier from a node
///
/// For most Scala nodes, the name is an `identifier` child.
/// For `type_definition`, the name is a `type_identifier` child.
pub(super) fn get_name(base: &Base, node: &Node) -> Option<String> {
    node.child_by_field_name("name")
        .map(|n| base.get_node_text(&n))
        .or_else(|| {
            node.children(&mut node.walk())
                .find(|n| n.kind() == "identifier" || n.kind() == "type_identifier")
                .map(|n| base.get_node_text(&n))
        })
}

/// The name of the type whose body holds `node`; `None` inside the body of an
/// anonymous class, which has no written name.
pub(super) fn enclosing_type_name(base: &Base, node: &Node) -> Option<String> {
    let definition = enclosing_type_definition(node)?;
    if definition.kind() == "instance_expression" {
        return None;
    }
    get_name(base, &definition)
}

/// The nearest named type definition around `node`, or the instance
/// expression whose anonymous class body holds it.
fn enclosing_type_definition<'tree>(node: &Node<'tree>) -> Option<Node<'tree>> {
    let mut child = *node;
    while let Some(candidate) = child.parent() {
        if matches!(
            candidate.kind(),
            "class_definition" | "object_definition" | "trait_definition" | "enum_definition"
        ) || (candidate.kind() == "instance_expression" && child.kind() == "template_body")
        {
            return Some(candidate);
        }
        child = candidate;
    }
    None
}

/// The first type in the enclosing definition's `extends` clause, or the
/// first type an anonymous class instantiates, without type arguments: the
/// type a `super.m()` call dispatches to.
pub(super) fn enclosing_supertype_name(base: &Base, node: &Node) -> Option<String> {
    let definition = enclosing_type_definition(node)?;
    if definition.kind() == "instance_expression" {
        return instance_type_names(base, &definition).into_iter().next();
    }
    let extends = definition
        .children(&mut definition.walk())
        .find(|child| child.kind() == "extends_clause")?;
    let mut supertype = extends.child_by_field_name("type")?;
    if supertype.kind() == "generic_type" {
        supertype = supertype.child_by_field_name("type")?;
    }
    Some(base.get_node_text(&supertype))
}

/// `private`, `protected` and their qualified forms such as `private[core]`.
pub(super) fn is_access_modifier(modifier: &str) -> bool {
    modifier.starts_with("private") || modifier.starts_with("protected")
}

/// The supertypes named in a definition's `extends` clause, without type
/// arguments: `extends munit.FunSuite with Matchers[Int]` gives
/// `munit.FunSuite` and `Matchers`.
pub(super) fn extends_type_names(base: &Base, node: &Node) -> Vec<String> {
    let mut names = Vec::new();
    if let Some(extends) = node
        .children(&mut node.walk())
        .find(|n| n.kind() == "extends_clause")
    {
        let mut cursor = extends.walk();
        for child in extends.named_children(&mut cursor) {
            collect_type_names(base, child, &mut names, 0);
        }
    }
    names
}

/// The types an anonymous class instance (`new Task with Named { … }`)
/// extends, without type arguments.
pub(super) fn instance_type_names(base: &Base, node: &Node) -> Vec<String> {
    let mut names = Vec::new();
    for type_node in instance_type_nodes(node) {
        collect_type_names(base, type_node, &mut names, 0);
    }
    names
}

/// The type nodes written between `new` and the body of an instance
/// expression; constructor arguments are left out.
pub(super) fn instance_type_nodes<'tree>(node: &Node<'tree>) -> Vec<Node<'tree>> {
    node.named_children(&mut node.walk())
        .filter(|child| !matches!(child.kind(), "arguments" | "template_body"))
        .filter(|child| !child.kind().contains("comment"))
        .collect()
}

/// The body of an instance expression that declares an anonymous class:
/// `new Task { … }` and `new { … }`, not `new Task(1)`.
pub(super) fn anonymous_template_body<'tree>(node: &Node<'tree>) -> Option<Node<'tree>> {
    if node.kind() != "instance_expression" {
        return None;
    }
    node.named_children(&mut node.walk())
        .find(|child| child.kind() == "template_body")
}

/// True when the nearest scope around `node` is a function or a `val`/`var`
/// initializer rather than a template body or the top level.
pub(super) fn is_in_local_scope(node: &Node) -> bool {
    for ancestor in std::iter::successors(node.parent(), Node::parent) {
        match ancestor.kind() {
            "function_definition" | "val_definition" | "var_definition" => return true,
            "template_body"
            | "with_template_body"
            | "enum_body"
            | "extension_definition"
            | "compilation_unit" => return false,
            _ => {}
        }
    }
    false
}

fn collect_type_names(base: &Base, node: Node, names: &mut Vec<String>, depth: u32) {
    let Some(child_depth) = crate::tree_traversal::child_tree_depth(depth) else {
        return;
    };
    match node.kind() {
        "type_identifier" | "stable_type_identifier" => names.push(base.get_node_text(&node)),
        "generic_type" => {
            if let Some(inner) = node.child_by_field_name("type") {
                collect_type_names(base, inner, names, child_depth);
            }
        }
        "compound_type" | "annotated_type" => {
            let mut cursor = node.walk();
            for child in node.named_children(&mut cursor) {
                collect_type_names(base, child, names, child_depth);
            }
        }
        _ => {}
    }
}
