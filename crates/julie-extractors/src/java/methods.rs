/// Method and constructor extraction
use crate::base::{Symbol, SymbolKind, SymbolOptions};
use crate::java::JavaExtractor;
use crate::test_detection::apply_callable_test_metadata;
use std::collections::HashMap;
use tree_sitter::Node;

use super::helpers;

/// Extract method declaration from a node
pub(super) fn extract_method(
    extractor: &mut JavaExtractor,
    node: Node,
    parent_id: Option<&str>,
) -> Option<Symbol> {
    let name_node = node
        .children(&mut node.walk())
        .find(|c| c.kind() == "identifier")?;

    let name = extractor.base().get_node_text(&name_node);
    let modifiers = helpers::extract_modifiers(extractor.base(), node);
    let annotations = helpers::extract_annotations(extractor.base(), node);
    let annotation_keys: Vec<String> = annotations
        .iter()
        .map(|annotation| annotation.annotation_key.clone())
        .collect();
    let visibility = helpers::determine_visibility(&modifiers, node);

    let return_type = node
        .child_by_field_name("type")
        .map(|type_node| extractor.base().get_node_text(&type_node))
        .unwrap_or_else(|| "void".to_string());

    // Get parameters
    let param_list = node
        .children(&mut node.walk())
        .find(|c| c.kind() == "formal_parameters");
    let params = param_list
        .map(|p| extractor.base().get_node_text(&p))
        .unwrap_or_else(|| "()".to_string());

    // Handle generic type parameters on the method
    let type_params = helpers::extract_type_parameters(extractor.base(), node);

    // Check for throws clause
    let throws_clause = helpers::extract_throws_clause(extractor.base(), node);

    // Build signature
    let modifier_str = if modifiers.is_empty() {
        String::new()
    } else {
        format!("{} ", modifiers.join(" "))
    };
    let type_param_str = type_params.map(|tp| format!("{} ", tp)).unwrap_or_default();
    let throws_str = throws_clause
        .map(|tc| format!(" {}", tc))
        .unwrap_or_default();

    let signature = format!(
        "{}{}{} {}{}{}",
        modifier_str, type_param_str, return_type, name, params, throws_str
    );

    // Extract JavaDoc comment
    let doc_comment = extractor.base().find_doc_comment(&node);

    let mut metadata = HashMap::new();
    apply_callable_test_metadata(
        "java",
        &name,
        &extractor.base().file_path,
        &SymbolKind::Method,
        &annotation_keys,
        doc_comment.as_deref(),
        &mut metadata,
    );

    let options = SymbolOptions {
        signature: Some(signature),
        visibility: Some(visibility),
        parent_id: parent_id.map(|s| s.to_string()),
        doc_comment,
        metadata: if metadata.is_empty() {
            None
        } else {
            Some(metadata)
        },
        annotations,
    };

    let symbol = extractor
        .base_mut()
        .create_symbol(&node, name, SymbolKind::Method, options);
    if let Some(type_node) = node.child_by_field_name("type") {
        super::type_facts::record_return_type(extractor.base_mut(), &symbol.id, type_node);
    }
    Some(symbol)
}

/// Extract constructor declaration from a node
pub(super) fn extract_constructor(
    extractor: &mut JavaExtractor,
    node: Node,
    parent_id: Option<&str>,
) -> Option<Symbol> {
    let name_node = node
        .children(&mut node.walk())
        .find(|c| c.kind() == "identifier")?;

    let name = extractor.base().get_node_text(&name_node);
    let modifiers = helpers::extract_modifiers(extractor.base(), node);
    let annotations = helpers::extract_annotations(extractor.base(), node);
    let annotation_keys: Vec<String> = annotations
        .iter()
        .map(|annotation| annotation.annotation_key.clone())
        .collect();
    let visibility = helpers::determine_visibility(&modifiers, node);

    let params = match node.kind() {
        "compact_constructor_declaration" => String::new(),
        _ => node
            .child_by_field_name("parameters")
            .map(|p| extractor.base().get_node_text(&p))
            .unwrap_or_else(|| "()".to_string()),
    };

    let modifier_str = if modifiers.is_empty() {
        String::new()
    } else {
        format!("{} ", modifiers.join(" "))
    };
    let signature = format!("{}{}{}", modifier_str, name, params);

    // Extract JavaDoc comment
    let doc_comment = extractor.base().find_doc_comment(&node);

    let mut metadata = HashMap::new();
    apply_callable_test_metadata(
        "java",
        &name,
        &extractor.base().file_path,
        &SymbolKind::Constructor,
        &annotation_keys,
        doc_comment.as_deref(),
        &mut metadata,
    );

    let options = SymbolOptions {
        signature: Some(signature),
        visibility: Some(visibility),
        parent_id: parent_id.map(|s| s.to_string()),
        doc_comment,
        metadata: if metadata.is_empty() {
            None
        } else {
            Some(metadata)
        },
        annotations,
    };

    Some(
        extractor
            .base_mut()
            .create_symbol(&node, name, SymbolKind::Constructor, options),
    )
}

/// Extract an annotation-type element (`String level() default "info";`) as a
/// method: the element is declared and read like a no-argument method.
pub(super) fn extract_annotation_element(
    extractor: &mut JavaExtractor,
    node: Node,
    parent_id: Option<&str>,
) -> Option<Symbol> {
    let name = extractor
        .base()
        .get_node_text(&node.child_by_field_name("name")?);
    let modifiers = helpers::extract_modifiers(extractor.base(), node);
    let visibility = helpers::determine_visibility(&modifiers, node);
    let type_node = node.child_by_field_name("type");
    let element_type = type_node
        .map(|type_node| extractor.base().get_node_text(&type_node))
        .unwrap_or_default();
    let default_value = node
        .child_by_field_name("value")
        .map(|value| format!(" default {}", extractor.base().get_node_text(&value)))
        .unwrap_or_default();
    let modifier_str = if modifiers.is_empty() {
        String::new()
    } else {
        format!("{} ", modifiers.join(" "))
    };
    let options = SymbolOptions {
        signature: Some(format!(
            "{modifier_str}{element_type} {name}(){default_value}"
        )),
        visibility: Some(visibility),
        parent_id: parent_id.map(|s| s.to_string()),
        doc_comment: extractor.base().find_doc_comment(&node),
        annotations: helpers::extract_annotations(extractor.base(), node),
        ..Default::default()
    };
    let symbol = extractor
        .base_mut()
        .create_symbol(&node, name, SymbolKind::Method, options);
    if let Some(type_node) = type_node {
        super::type_facts::record_return_type(extractor.base_mut(), &symbol.id, type_node);
    }
    Some(symbol)
}
