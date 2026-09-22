/// Helper functions for Java extraction
/// Handles modifiers, visibility, and common parsing utilities
use crate::base::{
    AnnotationMarker, BaseExtractor, UnresolvedTarget, Visibility, normalize_annotations,
};
use tree_sitter::Node;

/// Extract all modifiers from a Java node (public, private, static, final, etc.)
pub(super) fn extract_modifiers(base: &BaseExtractor, node: Node) -> Vec<String> {
    node.children(&mut node.walk())
        .find(|c| c.kind() == "modifiers")
        .map(|modifiers_node| {
            modifiers_node
                .children(&mut modifiers_node.walk())
                .map(|c| base.get_node_text(&c))
                .collect()
        })
        .unwrap_or_default()
}

/// Extract canonical annotation markers from Java modifiers.
pub(super) fn extract_annotations(base: &BaseExtractor, node: Node) -> Vec<AnnotationMarker> {
    let raw_annotations: Vec<String> = node
        .children(&mut node.walk())
        .find(|c| c.kind() == "modifiers")
        .map(|modifiers_node| {
            modifiers_node
                .children(&mut modifiers_node.walk())
                .filter(|c| c.kind().contains("annotation"))
                .map(|c| base.get_node_text(&c))
                .collect()
        })
        .unwrap_or_default();

    normalize_annotations(&raw_annotations, "java")
}

/// Determine visibility from modifier list. Without an access modifier,
/// interface and annotation-type members are implicitly public, enum
/// constructors are implicitly private, local classes are private, and every
/// other declaration is package-private (`Internal`).
pub(super) fn determine_visibility(modifiers: &[String], node: Node) -> Visibility {
    let default = match node.parent().map(|parent| parent.kind()) {
        Some("interface_body" | "annotation_type_body") => Visibility::Public,
        Some("enum_body_declarations") if node.kind() == "constructor_declaration" => {
            Visibility::Private
        }
        Some("block" | "constructor_body") => Visibility::Private,
        _ => Visibility::Internal,
    };
    crate::base::visibility::visibility_from_modifiers_with_default(modifiers, default)
}

/// Extract superclass from a class declaration node
pub(super) fn extract_superclass(base: &BaseExtractor, node: Node) -> Option<String> {
    let type_node = superclass_type_node(node)?;
    Some(base.get_node_text(&type_node))
}

/// The superclass base name: `Base<T>` reduces to `Base`.
pub(super) fn extract_superclass_base_name(base: &BaseExtractor, node: Node) -> Option<String> {
    let type_node = superclass_type_node(node)?;
    Some(base.get_node_text(&generic_base_node(type_node)))
}

/// The type name without type arguments: `Map.Entry<K, V>` reduces to `Map.Entry`.
pub(super) fn generic_base_node(type_node: Node) -> Node {
    if type_node.kind() != "generic_type" {
        return type_node;
    }
    type_node
        .children(&mut type_node.walk())
        .find(|c| matches!(c.kind(), "type_identifier" | "scoped_type_identifier"))
        .unwrap_or(type_node)
}

/// A pending-relationship target for a type reference: type arguments are
/// dropped and a qualified name splits into terminal, receiver and namespace.
pub(super) fn type_reference_target(base: &BaseExtractor, type_node: Node) -> UnresolvedTarget {
    let text = base.get_node_text(&generic_base_node(type_node));
    UnresolvedTarget::from_qualified_text(&text, &["."])
        .unwrap_or_else(|| UnresolvedTarget::simple(text))
}

fn is_supertype_node(node: &Node) -> bool {
    matches!(
        node.kind(),
        "type_identifier" | "scoped_type_identifier" | "generic_type"
    )
}

pub(super) fn superclass_type_node(node: Node) -> Option<Node> {
    let superclass_node = node
        .children(&mut node.walk())
        .find(|c| c.kind() == "superclass")?;
    superclass_node
        .children(&mut superclass_node.walk())
        .find(is_supertype_node)
}

/// The type nodes listed in a `super_interfaces` or `extends_interfaces` clause.
pub(super) fn type_list_nodes<'tree>(node: Node<'tree>, clause_kind: &str) -> Vec<Node<'tree>> {
    let Some(clause) = node
        .children(&mut node.walk())
        .find(|c| c.kind() == clause_kind)
    else {
        return Vec::new();
    };
    let Some(type_list) = clause
        .children(&mut clause.walk())
        .find(|c| c.kind() == "type_list")
    else {
        return Vec::new();
    };
    type_list
        .children(&mut type_list.walk())
        .filter(is_supertype_node)
        .collect()
}

/// Extract implemented interfaces from a class/enum/record
pub(super) fn extract_implemented_interfaces(base: &BaseExtractor, node: Node) -> Vec<String> {
    type_list_nodes(node, "super_interfaces")
        .iter()
        .map(|c| base.get_node_text(c))
        .collect()
}

/// Extract extended interfaces from an interface declaration
pub(super) fn extract_extended_interfaces(base: &BaseExtractor, node: Node) -> Vec<String> {
    type_list_nodes(node, "extends_interfaces")
        .iter()
        .map(|c| base.get_node_text(c))
        .collect()
}

/// Extract type parameters from a generic type (e.g., <T, U>)
pub(super) fn extract_type_parameters(base: &BaseExtractor, node: Node) -> Option<String> {
    let type_params_node = node
        .children(&mut node.walk())
        .find(|c| c.kind() == "type_parameters")?;

    Some(base.get_node_text(&type_params_node))
}

/// Extract throws clause from a method declaration
pub(super) fn extract_throws_clause(base: &BaseExtractor, node: Node) -> Option<String> {
    let throws_node = node
        .children(&mut node.walk())
        .find(|c| c.kind() == "throws")?;

    Some(base.get_node_text(&throws_node))
}
