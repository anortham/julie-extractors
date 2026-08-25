// PHP Extractor - Function/method extraction

use super::{PhpExtractor, determine_visibility, extract_modifiers, find_child, find_child_text};
use crate::base::{AnnotationMarker, Symbol, SymbolKind, SymbolOptions, normalize_annotations};
use crate::test_detection::apply_callable_test_metadata;
use std::collections::HashMap;
use tree_sitter::Node;

/// Extract PHP function/method declarations
pub(super) fn extract_function(
    extractor: &mut PhpExtractor,
    node: Node,
    parent_id: Option<&str>,
) -> Option<Symbol> {
    let name = find_child_text(extractor, &node, "name")?;

    let modifiers = extract_modifiers(extractor, &node);
    let annotations = extract_attribute_markers(extractor, &node);
    let annotation_keys: Vec<String> = annotations
        .iter()
        .map(|annotation| annotation.annotation_key.clone())
        .collect();
    let parameters_node = find_child(extractor, &node, "formal_parameters");
    let attribute_list = find_child(extractor, &node, "attribute_list");

    // PHP return type comes after : as primitive_type, named_type, union_type, or optional_type
    let return_type_node = find_return_type(extractor, &node);

    // Check for reference modifier (&)
    let reference_modifier = find_child(extractor, &node, "reference_modifier");
    let ref_prefix = if reference_modifier.is_some() {
        "&"
    } else {
        ""
    };

    // Determine symbol kind
    let symbol_kind = match name.as_str() {
        "__construct" => SymbolKind::Constructor,
        "__destruct" => SymbolKind::Destructor,
        _ if parent_id.is_some() => SymbolKind::Method,
        _ => SymbolKind::Function,
    };

    let mut signature = String::new();

    // Add attributes if present
    if let Some(attr_node) = attribute_list {
        signature.push_str(&extractor.get_base().get_node_text(&attr_node));
        signature.push('\n');
    }

    signature.push_str(&format!("function {}{}", ref_prefix, name));

    if !modifiers.is_empty() {
        signature = signature.replace(
            &format!("function {}{}", ref_prefix, name),
            &format!("{} function {}{}", modifiers.join(" "), ref_prefix, name),
        );
    }

    if let Some(params_node) = parameters_node {
        signature.push_str(&extractor.get_base().get_node_text(&params_node));
    } else {
        signature.push_str("()");
    }

    if let Some(return_node) = return_type_node {
        signature.push_str(&format!(
            ": {}",
            extractor.get_base().get_node_text(&return_node)
        ));
    }

    let mut metadata = HashMap::new();
    metadata.insert("type".to_string(), "function".to_string());
    metadata.insert("modifiers".to_string(), modifiers.join(","));

    if let Some(params_node) = parameters_node {
        metadata.insert(
            "parameters".to_string(),
            extractor.get_base().get_node_text(&params_node),
        );
    } else {
        metadata.insert("parameters".to_string(), "()".to_string());
    }

    if let Some(return_node) = return_type_node {
        metadata.insert(
            "returnType".to_string(),
            extractor.get_base().get_node_text(&return_node),
        );
    }

    // Extract PHPDoc comment
    let doc_comment = extractor.get_base().find_doc_comment(&node);

    let mut json_metadata: HashMap<String, serde_json::Value> = metadata
        .into_iter()
        .map(|(k, v)| (k, serde_json::Value::String(v)))
        .collect();

    apply_callable_test_metadata(
        "php",
        &name,
        &extractor.get_base().file_path,
        &symbol_kind,
        &role_annotation_keys(&annotation_keys, doc_comment.as_deref()),
        doc_comment.as_deref(),
        &mut json_metadata,
    );

    Some(extractor.get_base_mut().create_symbol(
        &node,
        name,
        symbol_kind,
        SymbolOptions {
            signature: Some(signature),
            visibility: Some(determine_visibility(&modifiers)),
            parent_id: parent_id.map(|s| s.to_string()),
            metadata: Some(json_metadata),
            doc_comment,
            annotations,
        },
    ))
}

/// PHPUnit tags that carry a test role in a PHPDoc block. Each spells the same
/// metadata as an attribute of the same name — `@before` and `#[Before]` both
/// declare a setup hook — so both reach the shared detector as one key.
const PHPDOC_ROLE_TAGS: [&str; 5] = ["test", "before", "after", "beforeClass", "afterClass"];

fn role_annotation_keys(annotation_keys: &[String], doc_comment: Option<&str>) -> Vec<String> {
    let mut keys = annotation_keys.to_vec();
    let Some(doc) = doc_comment else {
        return keys;
    };
    keys.extend(
        PHPDOC_ROLE_TAGS
            .iter()
            .filter(|tag| has_phpdoc_tag(doc, tag))
            .map(|tag| tag.to_ascii_lowercase()),
    );
    keys
}

fn has_phpdoc_tag(doc_comment: &str, tag: &str) -> bool {
    doc_comment.match_indices('@').any(|(at, _)| {
        doc_comment[at + 1..]
            .strip_prefix(tag)
            .is_some_and(|rest| rest.chars().next().is_none_or(|ch| !ch.is_alphanumeric()))
    })
}

pub(super) fn extract_attribute_markers(
    extractor: &PhpExtractor,
    node: &Node,
) -> Vec<AnnotationMarker> {
    let raw_attributes: Vec<String> = node
        .children(&mut node.walk())
        .filter(|child| child.kind() == "attribute_list")
        .map(|child| extractor.get_base().get_node_text(&child))
        .collect();

    normalize_annotations(&raw_attributes, "php")
}

/// Find return type node after colon
pub(super) fn find_return_type<'a>(_extractor: &PhpExtractor, node: &Node<'a>) -> Option<Node<'a>> {
    let mut cursor = node.walk();
    let mut found_colon = false;

    for child in node.children(&mut cursor) {
        if found_colon {
            match child.kind() {
                "primitive_type" | "named_type" | "union_type" | "optional_type" => {
                    return Some(child);
                }
                _ => {}
            }
        }
        if child.kind() == ":" {
            found_colon = true;
        }
    }
    None
}
