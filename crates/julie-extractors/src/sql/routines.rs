//! Stored procedure and function extraction.
//!
//! Handles extraction of CREATE PROCEDURE and CREATE FUNCTION statements,
//! including parameter extraction and function signatures.

use crate::base::{BaseExtractor, Symbol, SymbolKind, SymbolOptions};
use crate::sql::body_spans;
use crate::sql::helpers::{DECLARE_VAR_RE, VAR_DECL_RE, normalize_sql_identifier};
use crate::test_detection::is_test_symbol;
use regex::Regex;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::LazyLock;
use tree_sitter::Node;

static DECLARE_VARIABLE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"DECLARE\s+([a-zA-Z_][a-zA-Z0-9_]*)\s+(DECIMAL\([^)]+\)|JSONB|INT|BIGINT|VARCHAR\([^)]+\)|TEXT|BOOLEAN)").unwrap()
});
static ERROR_PROCEDURE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"CREATE\s+PROCEDURE\s+([a-zA-Z_][a-zA-Z0-9_]*)").unwrap());
static ERROR_FUNCTION_SIGNATURE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"CREATE\s+(?:OR\s+REPLACE\s+)?FUNCTION\s+([a-zA-Z_][a-zA-Z0-9_]*)\s*\([^)]*\)\s*RETURNS?\s+([A-Z0-9(),\s]+)",
    )
    .unwrap()
});
static ERROR_FUNCTION_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"CREATE\s+(?:OR\s+REPLACE\s+)?FUNCTION\s+([a-zA-Z_][a-zA-Z0-9_]*)").unwrap()
});
static ERROR_AGGREGATE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"CREATE\s+AGGREGATE\s+([a-zA-Z_][a-zA-Z0-9_]*)\s*\(([^)]*)\)").unwrap()
});
static ERROR_PARAMETER_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(IN|OUT|INOUT)?\s*([a-zA-Z_][a-zA-Z0-9_]*)\s+(BIGINT|INT|VARCHAR|DECIMAL|DATE|BOOLEAN|TEXT|JSONB)").unwrap()
});

/// Extract stored procedure or function from CREATE PROCEDURE/FUNCTION statement
pub(super) fn extract_stored_procedure(
    base: &mut BaseExtractor,
    node: Node,
    parent_id: Option<&str>,
) -> Option<Symbol> {
    let object_ref_node = base.find_child_by_type(&node, "object_reference");
    let name_node = if let Some(obj_ref) = object_ref_node {
        obj_ref
            .child_by_field_name("name")
            .or_else(|| base.find_child_by_type(&obj_ref, "identifier"))
    } else {
        base.find_child_by_type(&node, "identifier")
            .or_else(|| base.find_child_by_type(&node, "procedure_name"))
            .or_else(|| base.find_child_by_type(&node, "function_name"))
    }?;

    let name = normalize_sql_identifier(&base.get_node_text(&name_node));
    let is_function = node.kind().contains("function");

    let signature = extract_procedure_signature(base, &node)?;

    let mut metadata = HashMap::new();
    metadata.insert("isFunction".to_string(), Value::Bool(is_function));
    metadata.insert("isStoredProcedure".to_string(), Value::Bool(true));

    let doc_comment = base.find_doc_comment(&node);

    if is_test_symbol(
        "sql",
        &name,
        &base.file_path,
        &SymbolKind::Function,
        &[],
        doc_comment.as_deref(),
    ) {
        metadata.insert("is_test".to_string(), Value::Bool(true));
    }

    let options = SymbolOptions {
        signature: Some(signature),
        visibility: Some(crate::base::Visibility::Public),
        parent_id: parent_id.map(|s| s.to_string()),
        doc_comment,
        metadata: Some(metadata),
        annotations: Vec::new(),
    };

    let mut symbol = base.create_symbol(&node, name, SymbolKind::Function, options);
    body_spans::finalize_sql_callable_symbol(base, &mut symbol);
    Some(symbol)
}

/// Extract procedure/function signature with parameters
pub(super) fn extract_procedure_signature(base: &BaseExtractor, node: &Node) -> Option<String> {
    let object_ref_node = base.find_child_by_type(node, "object_reference");
    let name_node = if let Some(obj_ref) = object_ref_node {
        obj_ref
            .child_by_field_name("name")
            .or_else(|| base.find_child_by_type(&obj_ref, "identifier"))
    } else {
        base.find_child_by_type(node, "identifier")
            .or_else(|| base.find_child_by_type(node, "procedure_name"))
            .or_else(|| base.find_child_by_type(node, "function_name"))
    }?;
    let name = normalize_sql_identifier(&base.get_node_text(&name_node));

    let params = match direct_routine_arguments(base, node) {
        Some(arguments) => arguments
            .into_iter()
            .map(|argument| base.get_node_text(&argument).trim().to_string())
            .collect(),
        None => legacy_routine_parameters(base, node),
    };

    let is_function = node.kind().contains("function");
    let keyword = if is_function { "FUNCTION" } else { "PROCEDURE" };

    // For functions, try to extract the RETURNS clause and LANGUAGE
    let mut return_clause = String::new();
    let mut language_clause = String::new();
    if is_function {
        // Look for decimal node for RETURNS DECIMAL(10,2) - search recursively
        let decimal_nodes = base.find_nodes_by_type(node, "decimal");
        if !decimal_nodes.is_empty() {
            let decimal_text = base.get_node_text(&decimal_nodes[0]);
            return_clause = format!(" RETURNS {}", decimal_text);
        } else {
            // Look for other return types as direct children
            let return_type_nodes = [
                "keyword_boolean",
                "keyword_bigint",
                "keyword_int",
                "keyword_varchar",
                "keyword_text",
                "keyword_jsonb",
            ];
            for type_str in &return_type_nodes {
                if let Some(type_node) = base.find_child_by_type(node, type_str) {
                    let type_text = base
                        .get_node_text(&type_node)
                        .replace("keyword_", "")
                        .to_uppercase();
                    return_clause = format!(" RETURNS {}", type_text);
                    break;
                }
            }
        }

        // Look for LANGUAGE clause (PostgreSQL functions)
        if let Some(language_node) = base.find_child_by_type(node, "function_language") {
            let language_text = base.get_node_text(&language_node);
            language_clause = format!(" {}", language_text);
        }
    }

    Some(format!(
        "CREATE {} {}({}){}{}",
        keyword,
        name,
        params.join(", "),
        return_clause,
        language_clause
    ))
}

fn direct_routine_arguments<'tree>(
    base: &BaseExtractor,
    routine_node: &Node<'tree>,
) -> Option<Vec<Node<'tree>>> {
    base.find_child_by_type(routine_node, "function_arguments")
        .map(|arguments| base.find_children_by_type(&arguments, "function_argument"))
}

fn legacy_routine_parameters(base: &BaseExtractor, routine_node: &Node) -> Vec<String> {
    let mut parameters = Vec::new();
    base.traverse_tree(routine_node, &mut |child_node| {
        if child_node.kind() != "parameter_declaration" && child_node.kind() != "parameter" {
            return;
        }

        let name_node = base
            .find_child_by_type(child_node, "identifier")
            .or_else(|| base.find_child_by_type(child_node, "parameter_name"));
        let type_node = base
            .find_child_by_type(child_node, "data_type")
            .or_else(|| base.find_child_by_type(child_node, "type_name"));

        if let Some(name_node) = name_node {
            let name = normalize_sql_identifier(&base.get_node_text(&name_node));
            let parameter_type = type_node
                .map(|node| base.get_node_text(&node))
                .unwrap_or_default();
            parameters.push(if parameter_type.is_empty() {
                name
            } else {
                format!("{}: {}", name, parameter_type)
            });
        }
    });
    parameters
}

pub(super) fn extract_parameters_from_routine_node(
    base: &mut BaseExtractor,
    function_node: Node,
    symbols: &mut Vec<Symbol>,
    parent_id: &str,
) {
    for argument in direct_routine_arguments(base, &function_node).unwrap_or_default() {
        let Some(name_node) = base.find_child_by_type(&argument, "identifier") else {
            continue;
        };
        let name = normalize_sql_identifier(&base.get_node_text(&name_node));
        let signature = base.get_node_text(&argument).trim().to_string();
        let mut metadata = HashMap::new();
        metadata.insert("isParameter".to_string(), Value::Bool(true));
        let options = SymbolOptions {
            signature: Some(signature),
            visibility: Some(crate::base::Visibility::Public),
            parent_id: Some(parent_id.to_string()),
            doc_comment: None,
            metadata: Some(metadata),
            annotations: Vec::new(),
        };
        symbols.push(base.create_symbol(&argument, name, SymbolKind::Variable, options));
    }
}

/// Extract declared variables from function/procedure body
pub(super) fn extract_declare_variables(
    base: &mut BaseExtractor,
    function_node: Node,
    symbols: &mut Vec<Symbol>,
    parent_id: &str,
) {
    // Port extractDeclareVariables logic
    let function_text = base.get_node_text(&function_node);

    // Look for DECLARE statements within function bodies
    // Replaced closure with iterative approach to avoid borrow checker issues
    let mut nodes_to_process = vec![function_node];
    while let Some(node) = nodes_to_process.pop() {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            nodes_to_process.push(child);
        }
        // PostgreSQL style: function_declaration nodes like "v_current_prefs JSONB;"
        if node.kind() == "function_declaration" {
            // Parse the declaration text to extract variable name and type
            let declaration_raw = base.get_node_text(&node);
            let declaration_text = declaration_raw.trim();
            // Match patterns like "v_current_prefs JSONB;" or "v_score DECIMAL(10,2) DEFAULT 0.0;"
            if let Some(captures) = VAR_DECL_RE.captures(declaration_text) {
                let variable_name = captures.get(1).map_or("", |m| m.as_str());
                let variable_type_full = captures.get(2).map_or("", |m| m.as_str());
                let variable_type = match variable_type_full.split_whitespace().next() {
                    Some(t) => t,
                    None => continue,
                };

                // Skip if variable name is empty
                if variable_name.is_empty() {
                    continue;
                }

                let mut metadata = HashMap::new();
                metadata.insert("isLocalVariable".to_string(), serde_json::Value::Bool(true));
                metadata.insert(
                    "isDeclaredVariable".to_string(),
                    serde_json::Value::Bool(true),
                );

                let options = SymbolOptions {
                    signature: Some(format!("DECLARE {} {}", variable_name, variable_type)),
                    visibility: Some(crate::base::Visibility::Private),
                    parent_id: Some(parent_id.to_string()),
                    doc_comment: None,
                    metadata: Some(metadata),
                    annotations: Vec::new(),
                };

                let variable_symbol = base.create_symbol(
                    &node,
                    variable_name.to_string(),
                    SymbolKind::Variable,
                    options,
                );
                symbols.push(variable_symbol);
            }
        }
        // MySQL style: keyword_declare followed by identifier and type
        else if node.kind() == "keyword_declare" {
            // For MySQL DECLARE statements, look for the pattern in the surrounding text
            if let Some(parent) = node.parent() {
                let parent_text = base.get_node_text(&parent);

                // Look for DECLARE patterns in the parent text
                for captures in DECLARE_VAR_RE.captures_iter(&parent_text) {
                    let variable_name = captures.get(1).map_or("", |m| m.as_str());
                    let variable_type = captures.get(2).map_or("", |m| m.as_str());

                    // Skip if variable name or type is empty
                    if variable_name.is_empty() || variable_type.is_empty() {
                        continue;
                    }

                    let mut metadata = HashMap::new();
                    metadata.insert("isLocalVariable".to_string(), serde_json::Value::Bool(true));
                    metadata.insert(
                        "isDeclaredVariable".to_string(),
                        serde_json::Value::Bool(true),
                    );

                    let options = SymbolOptions {
                        signature: Some(format!("DECLARE {} {}", variable_name, variable_type)),
                        visibility: Some(crate::base::Visibility::Private),
                        parent_id: Some(parent_id.to_string()),
                        doc_comment: None,
                        metadata: Some(metadata),
                        annotations: Vec::new(),
                    };

                    let variable_symbol = base.create_symbol(
                        &node,
                        variable_name.to_string(),
                        SymbolKind::Variable,
                        options,
                    );
                    symbols.push(variable_symbol);
                }
            }
        }
    }

    // Also extract DECLARE variables directly from function text using regex
    for captures in DECLARE_VARIABLE_RE.captures_iter(&function_text) {
        let variable_name = captures.get(1).map_or("", |m| m.as_str());
        let variable_type = captures.get(2).map_or("", |m| m.as_str());

        // Skip if variable name or type is empty
        if variable_name.is_empty() || variable_type.is_empty() {
            continue;
        }

        // Only add if not already added from tree traversal
        if !symbols
            .iter()
            .any(|s| s.name == variable_name && s.parent_id.as_deref() == Some(parent_id))
        {
            let mut metadata = HashMap::new();
            metadata.insert("isLocalVariable".to_string(), serde_json::Value::Bool(true));
            metadata.insert(
                "isDeclaredVariable".to_string(),
                serde_json::Value::Bool(true),
            );

            let options = SymbolOptions {
                signature: Some(format!("DECLARE {} {}", variable_name, variable_type)),
                visibility: Some(crate::base::Visibility::Private),
                parent_id: Some(parent_id.to_string()),
                doc_comment: None,
                metadata: Some(metadata),
                annotations: Vec::new(),
            };

            let variable_symbol = base.create_symbol(
                &function_node,
                variable_name.to_string(),
                SymbolKind::Variable,
                options,
            );
            symbols.push(variable_symbol);
        }
    }
}

/// Extract procedures from ERROR node text
pub(super) fn extract_procedures_from_error(
    error_text: &str,
    base: &mut BaseExtractor,
    node: &Node,
    symbols: &mut Vec<Symbol>,
    parent_id: Option<&str>,
) {
    if let Some(captures) = ERROR_PROCEDURE_RE.captures(error_text)
        && let Some(procedure_name) = captures.get(1)
    {
        let name = procedure_name.as_str().to_string();

        let mut metadata = HashMap::new();
        metadata.insert(
            "isStoredProcedure".to_string(),
            serde_json::Value::Bool(true),
        );
        metadata.insert(
            "extractedFromError".to_string(),
            serde_json::Value::Bool(true),
        );

        let options = SymbolOptions {
            signature: Some(format!("CREATE PROCEDURE {}(...)", name)),
            visibility: Some(crate::base::Visibility::Public),
            parent_id: parent_id.map(|s| s.to_string()),
            doc_comment: None,
            metadata: Some(metadata),
            annotations: Vec::new(),
        };

        let mut procedure_symbol =
            base.create_symbol(node, name.clone(), SymbolKind::Function, options);
        body_spans::finalize_sql_callable_symbol(base, &mut procedure_symbol);
        symbols.push(procedure_symbol.clone());
        extract_parameters_from_error_node(base, *node, symbols, &procedure_symbol.id);
    }
}

/// Extract functions from ERROR node text
pub(super) fn extract_functions_from_error(
    error_text: &str,
    base: &mut BaseExtractor,
    node: &Node,
    symbols: &mut Vec<Symbol>,
    parent_id: Option<&str>,
) {
    if let Some(captures) = ERROR_FUNCTION_SIGNATURE_RE.captures(error_text)
        && let Some(function_name) = captures.get(1)
    {
        let name = function_name.as_str().to_string();
        let return_type = captures
            .get(2)
            .map(|m| m.as_str().trim().to_string())
            .unwrap_or_default();

        let mut metadata = HashMap::new();
        metadata.insert("isFunction".to_string(), serde_json::Value::Bool(true));
        metadata.insert(
            "extractedFromError".to_string(),
            serde_json::Value::Bool(true),
        );
        metadata.insert(
            "returnType".to_string(),
            serde_json::Value::String(return_type.clone()),
        );

        let options = SymbolOptions {
            signature: Some(format!(
                "CREATE FUNCTION {}(...) RETURNS {}",
                name, return_type
            )),
            visibility: Some(crate::base::Visibility::Public),
            parent_id: parent_id.map(|s| s.to_string()),
            doc_comment: None,
            metadata: Some(metadata),
            annotations: Vec::new(),
        };

        let mut function_symbol =
            base.create_symbol(node, name.clone(), SymbolKind::Function, options);
        body_spans::finalize_sql_callable_symbol(base, &mut function_symbol);
        symbols.push(function_symbol.clone());
        extract_declare_variables(base, *node, symbols, &function_symbol.id);
        return;
    }

    // Fallback: Extract any CREATE FUNCTION
    if let Some(captures) = ERROR_FUNCTION_RE.captures(error_text)
        && let Some(function_name) = captures.get(1)
    {
        let name = function_name.as_str().to_string();

        let mut metadata = HashMap::new();
        metadata.insert("isFunction".to_string(), serde_json::Value::Bool(true));
        metadata.insert(
            "extractedFromError".to_string(),
            serde_json::Value::Bool(true),
        );

        let options = SymbolOptions {
            signature: Some(format!("CREATE FUNCTION {}(...)", name)),
            visibility: Some(crate::base::Visibility::Public),
            parent_id: parent_id.map(|s| s.to_string()),
            doc_comment: None,
            metadata: Some(metadata),
            annotations: Vec::new(),
        };

        let mut function_symbol =
            base.create_symbol(node, name.clone(), SymbolKind::Function, options);
        body_spans::finalize_sql_callable_symbol(base, &mut function_symbol);
        symbols.push(function_symbol.clone());
        extract_declare_variables(base, *node, symbols, &function_symbol.id);
    }
}

/// Extract aggregate functions from ERROR node text
pub(super) fn extract_aggregates_from_error(
    error_text: &str,
    base: &mut BaseExtractor,
    node: &Node,
    symbols: &mut Vec<Symbol>,
    parent_id: Option<&str>,
) {
    if let Some(captures) = ERROR_AGGREGATE_RE.captures(error_text)
        && let Some(aggregate_name) = captures.get(1)
    {
        let name = aggregate_name.as_str().to_string();
        let parameters = captures.get(2).map_or("", |m| m.as_str());

        let signature = format!("CREATE AGGREGATE {}({})", name, parameters);

        let mut metadata = HashMap::new();
        metadata.insert("isAggregate".to_string(), serde_json::Value::Bool(true));
        metadata.insert(
            "extractedFromError".to_string(),
            serde_json::Value::Bool(true),
        );

        let options = SymbolOptions {
            signature: Some(signature),
            visibility: Some(crate::base::Visibility::Public),
            parent_id: parent_id.map(|s| s.to_string()),
            doc_comment: None,
            metadata: Some(metadata),
            annotations: Vec::new(),
        };

        let aggregate_symbol = base.create_symbol(node, name, SymbolKind::Function, options);
        symbols.push(aggregate_symbol);
    }
}

/// Extract parameters from ERROR nodes (for procedures/functions with parse errors)
pub(super) fn extract_parameters_from_error_node(
    base: &mut BaseExtractor,
    node: Node,
    symbols: &mut Vec<Symbol>,
    parent_id: &str,
) {
    // Port extractParametersFromErrorNode logic
    let error_text = base.get_node_text(&node);

    // Extract parameters from procedure/function definitions
    // Look for patterns like "IN p_user_id BIGINT", "OUT p_total_events INT"
    for captures in ERROR_PARAMETER_RE.captures_iter(&error_text) {
        let direction = captures.get(1).map(|m| m.as_str()).unwrap_or("IN"); // Default to IN if not specified
        let param_name = captures.get(2).map_or("", |m| m.as_str());
        let param_type = captures.get(3).map_or("", |m| m.as_str());

        // Skip if param name or type is empty
        if param_name.is_empty() || param_type.is_empty() {
            continue;
        }

        // Don't extract procedure/function names as parameters
        if !error_text.contains(&format!("PROCEDURE {}", param_name))
            && !error_text.contains(&format!("FUNCTION {}", param_name))
        {
            let signature = format!("{} {} {}", direction, param_name, param_type);

            let mut metadata = HashMap::new();
            metadata.insert("isParameter".to_string(), serde_json::Value::Bool(true));
            metadata.insert(
                "extractedFromError".to_string(),
                serde_json::Value::Bool(true),
            );

            let options = SymbolOptions {
                signature: Some(signature),
                visibility: Some(crate::base::Visibility::Public),
                parent_id: Some(parent_id.to_string()),
                doc_comment: None,
                metadata: Some(metadata),
                annotations: Vec::new(),
            };

            let param_symbol =
                base.create_symbol(&node, param_name.to_string(), SymbolKind::Variable, options);
            symbols.push(param_symbol);
        }
    }
}
