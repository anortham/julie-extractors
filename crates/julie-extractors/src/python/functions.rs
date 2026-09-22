/// Function and method extraction
/// Handles regular functions, async functions, lambdas, and method detection
use super::super::base::{Symbol, SymbolKind, SymbolOptions, Visibility, normalize_annotations};
use super::PythonExtractor;
use super::{decorators, helpers, signatures, type_facts};
use crate::test_detection::apply_callable_test_metadata;
use std::collections::HashMap;
use tree_sitter::Node;

/// Extract a regular function definition
pub fn extract_function(extractor: &mut PythonExtractor, node: Node) -> Option<Symbol> {
    // Extract function name from 'name' field
    let name_node = node.child_by_field_name("name")?;
    let name = extractor.base_mut().get_node_text(&name_node);

    // Check if it's an async function
    let is_async = signatures::has_async_keyword(&node);

    // Extract parameters from 'parameters' field
    let parameters_node = node.child_by_field_name("parameters");
    let params = if let Some(parameters_node) = parameters_node {
        signatures::extract_parameters(extractor, &parameters_node)
    } else {
        Vec::new()
    };

    // Extract return type annotation from 'return_type' field
    let return_type = if let Some(return_type_node) = node.child_by_field_name("return_type") {
        format!(
            ": {}",
            extractor.base_mut().get_node_text(&return_type_node)
        )
    } else {
        String::new()
    };

    // Extract decorators
    let decorators_list = decorators::extract_decorators(extractor, &node);
    let decorator_texts = decorators::extract_decorator_texts(extractor, &node);
    let annotations = normalize_annotations(&decorator_texts, "python");
    let annotation_keys: Vec<String> = annotations
        .iter()
        .map(|marker| marker.annotation_key.clone())
        .collect();
    let decorator_info = if decorators_list.is_empty() {
        String::new()
    } else {
        format!("@{} ", decorators_list.join(" @"))
    };

    // Build signature
    let async_prefix = if is_async { "async " } else { "" };
    let signature = format!(
        "{}{}def {}({}){}",
        decorator_info,
        async_prefix,
        name,
        params.join(", "),
        return_type
    );

    // Determine if it's a method or function based on context
    let (symbol_kind, parent_id, nested_in_function) =
        determine_function_kind(extractor, &node, &name, &decorators_list);

    // Extract docstring
    let doc_comment = super::types::extract_docstring(extractor, &node);

    // Infer visibility from name
    let visibility = signatures::infer_visibility(&name);

    let mut metadata = HashMap::new();
    metadata.insert("decorators".to_string(), serde_json::json!(decorators_list));
    metadata.insert("isAsync".to_string(), serde_json::json!(is_async));
    metadata.insert("returnType".to_string(), serde_json::json!(return_type));

    // pytest and unittest collect only module- and class-level callables.
    if !nested_in_function {
        apply_callable_test_metadata(
            "python",
            &name,
            &extractor.base().file_path,
            &symbol_kind,
            &annotation_keys,
            doc_comment.as_deref(),
            &mut metadata,
        );
    }

    let symbol = extractor.base_mut().create_symbol(
        &node,
        name,
        symbol_kind,
        SymbolOptions {
            signature: Some(signature),
            visibility: Some(visibility),
            parent_id,
            metadata: Some(metadata),
            doc_comment,
            annotations,
        },
    );
    if let Some(return_type_node) = node.child_by_field_name("return_type") {
        type_facts::record_annotation_fact(extractor.base_mut(), &symbol.id, return_type_node);
    }
    Some(symbol)
}

/// Extract an async function definition
pub fn extract_async_function(extractor: &mut PythonExtractor, node: Node) -> Option<Symbol> {
    // Async functions are handled the same way as regular functions
    // The has_async_keyword check will detect the async keyword
    extract_function(extractor, node)
}

/// Extract a lambda expression
pub(super) fn extract_lambda(extractor: &mut PythonExtractor, node: Node) -> Symbol {
    // Extract lambda parameters
    let parameters_node = node.child_by_field_name("parameters");
    let params = if let Some(parameters_node) = parameters_node {
        signatures::extract_parameters(extractor, &parameters_node)
    } else {
        Vec::new()
    };

    // Extract lambda body (simplified)
    let body_node = node.child_by_field_name("body");
    let body = if let Some(body_node) = body_node {
        extractor.base_mut().get_node_text(&body_node)
    } else {
        String::new()
    };

    // Create signature: lambda params: body
    let signature = format!("lambda {}: {}", params.join(", "), body);

    // Create name with row number: lambda_row (no angle brackets for search tokenization)
    let start_pos = node.start_position();
    let name = format!("lambda_{}", start_pos.row);

    // Extract doc comment (preceding comments)
    let doc_comment = extractor.base().find_doc_comment(&node);
    let parent_id = helpers::find_enclosing_callable_id(extractor, &node);

    extractor.base_mut().create_symbol(
        &node,
        name,
        SymbolKind::Function,
        SymbolOptions {
            signature: Some(signature),
            visibility: Some(Visibility::Public),
            parent_id,
            metadata: None,
            doc_comment,
            annotations: Vec::new(),
        },
    )
}

/// Classify a def by its nearest enclosing definition: a class body makes it a
/// method of that class, a function body makes it a nested function of that
/// function, and the module makes it a top-level function. The flag reports
/// the nested-function case.
fn determine_function_kind(
    extractor: &PythonExtractor,
    node: &Node,
    name: &str,
    decorators: &[String],
) -> (SymbolKind, Option<String>, bool) {
    let mut current = *node;
    while let Some(parent) = current.parent() {
        current = parent;
        let is_class = match parent.kind() {
            "class_definition" => true,
            "function_definition" | "async_function_definition" => false,
            _ => continue,
        };
        let Some(name_node) = parent.child_by_field_name("name") else {
            continue;
        };
        let parent_name = extractor.base().get_node_text(&name_node);
        let parent_id = Some(extractor.base().generate_id_for_node(&parent_name, &parent));
        if !is_class {
            return (SymbolKind::Function, parent_id, true);
        }
        let symbol_kind = if name == "__init__" {
            SymbolKind::Constructor
        } else if is_property_decorator(decorators) {
            SymbolKind::Property
        } else {
            SymbolKind::Method
        };
        return (symbol_kind, parent_id, false);
    }

    (SymbolKind::Function, None, false)
}

/// Check if any decorator indicates this is a property
fn is_property_decorator(decorators: &[String]) -> bool {
    decorators.iter().any(|d| {
        d == "property"
            || d.ends_with(".setter")
            || d.ends_with(".getter")
            || d.ends_with(".deleter")
    })
}
