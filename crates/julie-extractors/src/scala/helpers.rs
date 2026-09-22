//! Helper functions for Scala symbol extraction
//!
//! Utility functions for extracting modifiers, visibility, type parameters,
//! and other metadata from Scala AST nodes.

use crate::base::{AnnotationMarker, Visibility, normalize_annotations};
use tree_sitter::Node;

type Base = super::super::base::BaseExtractor;

/// Extract modifiers from a Scala node (abstract, sealed, case, private, etc.)
pub(super) fn extract_modifiers(base: &Base, node: &Node) -> Vec<String> {
    let mut modifiers = Vec::new();
    for child in node.children(&mut node.walk()) {
        if child.kind() == "modifiers" || child.kind() == "annotation" {
            // Walk into modifiers container
            for mod_child in child.children(&mut child.walk()) {
                let text = base.get_node_text(&mod_child);
                if mod_child.kind() == "access_modifier" {
                    // `private[core]` keeps its qualifier; visibility reads the prefix.
                    modifiers.push(text.split_whitespace().collect());
                } else if is_modifier_keyword(&text) || text.starts_with('@') {
                    modifiers.push(text);
                }
            }
        }
        // Some modifiers may appear as direct children
        let text = base.get_node_text(&child);
        if child.kind() != "modifiers" && is_direct_modifier(child.kind()) {
            modifiers.push(text);
        }
    }
    modifiers
}

pub(super) fn extract_annotations(base: &Base, node: &Node) -> Vec<AnnotationMarker> {
    let mut raw_annotations = Vec::new();

    for child in node.children(&mut node.walk()) {
        if child.kind() == "annotation" {
            raw_annotations.push(base.get_node_text(&child));
        } else if child.kind() == "modifiers" {
            raw_annotations.extend(
                child
                    .children(&mut child.walk())
                    .filter(|mod_child| mod_child.kind() == "annotation")
                    .map(|mod_child| base.get_node_text(&mod_child)),
            );
        }
    }

    normalize_annotations(&raw_annotations, "scala")
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

/// Extract function parameters (e.g., `(x: Int, y: String)`)
pub(super) fn extract_parameters(base: &Base, node: &Node) -> Option<String> {
    // Scala uses `parameters` or `class_parameters`
    node.children(&mut node.walk())
        .find(|n| n.kind() == "parameters" || n.kind() == "class_parameters")
        .map(|p| base.get_node_text(&p))
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

pub(super) fn enclosing_type_name(base: &Base, node: &Node) -> Option<String> {
    get_name(base, &enclosing_type_definition(node)?)
}

fn enclosing_type_definition<'tree>(node: &Node<'tree>) -> Option<Node<'tree>> {
    std::iter::successors(node.parent(), |candidate| candidate.parent()).find(|candidate| {
        matches!(
            candidate.kind(),
            "class_definition" | "object_definition" | "trait_definition" | "enum_definition"
        )
    })
}

/// The first type in the enclosing definition's `extends` clause, without
/// type arguments: the type a `super.m()` call dispatches to.
pub(super) fn enclosing_supertype_name(base: &Base, node: &Node) -> Option<String> {
    let definition = enclosing_type_definition(node)?;
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
    fn collect(base: &Base, node: Node, names: &mut Vec<String>) {
        match node.kind() {
            "type_identifier" | "stable_type_identifier" => names.push(base.get_node_text(&node)),
            "generic_type" => {
                if let Some(inner) = node.child_by_field_name("type") {
                    collect(base, inner, names);
                }
            }
            "compound_type" | "annotated_type" => {
                let mut cursor = node.walk();
                for child in node.named_children(&mut cursor) {
                    collect(base, child, names);
                }
            }
            _ => {}
        }
    }
    let mut names = Vec::new();
    if let Some(extends) = node
        .children(&mut node.walk())
        .find(|n| n.kind() == "extends_clause")
    {
        let mut cursor = extends.walk();
        for child in extends.named_children(&mut cursor) {
            collect(base, child, &mut names);
        }
    }
    names
}
