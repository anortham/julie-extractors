//! PowerShell function extraction
//! Handles simple functions, advanced functions with `[CmdletBinding()]`, and parameters

use crate::base::{
    BaseExtractor, Symbol, SymbolKind, SymbolOptions, Visibility, normalize_annotations,
};
use crate::test_detection::apply_callable_test_metadata;
use regex::Regex;
use std::collections::HashMap;
use std::sync::LazyLock;
use tree_sitter::Node;

use super::documentation;
use super::helpers::{
    extract_command_annotation_attributes, extract_function_name_from_param_block,
    extract_parameter_annotation_attributes, find_function_name_node, has_attribute,
    split_function_scope, variable_name,
};
use super::type_facts;

static FUNCTION_NAME_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?im)^[ \t]*function\s+([A-Za-z][A-Za-z0-9-_]*)").unwrap());

/// Extract function symbols (simple functions)
pub(super) fn extract_function(
    base: &mut BaseExtractor,
    node: Node,
    parent_id: Option<&str>,
) -> Option<Symbol> {
    let name_node = find_function_name_node(node)?;
    let raw_name = base.get_node_text(&name_node);
    let (scope, name) = split_function_scope(&raw_name);
    let name = name.to_string();

    let signature = extract_function_signature(base, node)?;

    // Extract doc comment (PowerShell comment-based help)
    let doc_comment = documentation::extract_powershell_doc_comment(base, &node);
    let annotations = normalize_annotations(
        &extract_command_annotation_attributes(base, node),
        "powershell",
    );
    let annotation_keys = annotations
        .iter()
        .map(|annotation| annotation.annotation_key.clone())
        .collect::<Vec<_>>();

    let mut metadata = HashMap::new();
    if let Some(scope) = &scope {
        metadata.insert(
            "scope".to_string(),
            serde_json::Value::String(scope.clone()),
        );
    }
    apply_callable_test_metadata(
        "powershell",
        &name,
        &base.file_path,
        &SymbolKind::Function,
        &annotation_keys,
        doc_comment.as_deref(),
        &mut metadata,
    );

    let visibility = if scope.as_deref() == Some("private") {
        Visibility::Private
    } else {
        Visibility::Public
    };
    let symbol = base.create_symbol(
        &node,
        name,
        SymbolKind::Function,
        SymbolOptions {
            signature: Some(signature),
            visibility: Some(visibility),
            parent_id: parent_id.map(|s| s.to_string()),
            metadata: if metadata.is_empty() {
                None
            } else {
                Some(metadata)
            },
            doc_comment,
            annotations,
        },
    );
    record_output_type(base, &symbol.id, node);
    Some(symbol)
}

/// Extract advanced function symbols (from param_block nodes)
pub(super) fn extract_advanced_function(
    base: &mut BaseExtractor,
    node: Node,
    parent_id: Option<&str>,
) -> Option<Symbol> {
    // For param_block nodes (advanced functions), extract function name from ERROR node content
    let function_name = extract_function_name_from_param_block(base, node, &FUNCTION_NAME_RE)?;

    let signature = extract_advanced_function_signature(base, node, &function_name);

    // Extract doc comment (PowerShell comment-based help)
    let doc_comment = documentation::extract_powershell_doc_comment(base, &node);
    let annotations = normalize_annotations(
        &extract_command_annotation_attributes(base, node),
        "powershell",
    );

    let symbol = base.create_symbol(
        &node,
        function_name,
        SymbolKind::Function,
        SymbolOptions {
            signature: Some(signature),
            visibility: Some(Visibility::Public),
            parent_id: parent_id.map(|s| s.to_string()),
            metadata: None,
            doc_comment,
            annotations,
        },
    );
    record_output_type(base, &symbol.id, node);
    Some(symbol)
}

/// The parameter symbols a callable owns: the parameters of its own
/// `param()` block or parameter list. Nested functions and script blocks own
/// theirs, so their parameters never attach to the enclosing callable.
pub(super) fn extract_function_parameters(
    base: &mut BaseExtractor,
    owner: Node,
    parent_id: Option<&str>,
) -> Vec<Symbol> {
    let owner_doc = documentation::extract_powershell_doc_comment(base, &owner);
    let mut parameters = Vec::new();
    for parameter in own_parameter_nodes(owner) {
        let mut cursor = parameter.walk();
        let Some(variable_node) = parameter
            .children(&mut cursor)
            .find(|child| child.kind() == "variable")
        else {
            continue;
        };
        let param_name = variable_name(&base.get_node_text(&variable_node));
        let signature = if parameter.kind() == "class_method_parameter" {
            base.get_node_text(&parameter)
        } else {
            extract_script_parameter_signature(base, parameter, variable_node)
        };
        let annotations = normalize_annotations(
            &extract_parameter_annotation_attributes(base, parameter),
            "powershell",
        );
        let doc_comment =
            documentation::extract_powershell_doc_comment(base, &parameter).or_else(|| {
                owner_doc
                    .as_deref()
                    .and_then(|doc| documentation::parameter_help(doc, &param_name))
            });

        let mut param_symbol = base.create_symbol(
            &parameter,
            param_name,
            SymbolKind::Variable,
            SymbolOptions {
                signature: Some(signature),
                visibility: Some(Visibility::Public),
                parent_id: parent_id.map(str::to_string),
                metadata: Some(parameter_role_metadata()),
                doc_comment,
                annotations,
            },
        );
        param_symbol.body_span = None;
        param_symbol.body_hash = None;
        type_facts::record_declared_type_literal(base, &param_symbol.id, parameter);
        parameters.push(param_symbol);
    }
    parameters
}

/// The `script_parameter` / `class_method_parameter` nodes that belong to
/// `owner` itself.
fn own_parameter_nodes(owner: Node) -> Vec<Node> {
    let lists: Vec<Node> = match owner.kind() {
        "function_statement" => direct_children(owner, "function_parameter_declaration")
            .into_iter()
            .chain(
                direct_children(owner, "script_block")
                    .into_iter()
                    .flat_map(|block| direct_children(block, "param_block")),
            )
            .flat_map(|holder| direct_children(holder, "parameter_list"))
            .collect(),
        "param_block" => direct_children(owner, "parameter_list"),
        "class_method_definition" => direct_children(owner, "class_method_parameter_list"),
        "command" => block_arguments(owner)
            .into_iter()
            .flat_map(|block| {
                let mut holders = direct_children(block, "param_block");
                holders.extend(
                    direct_children(block, "script_block")
                        .into_iter()
                        .flat_map(|inner| direct_children(inner, "param_block")),
                );
                holders
            })
            .flat_map(|holder| direct_children(holder, "parameter_list"))
            .collect(),
        _ => Vec::new(),
    };
    lists
        .into_iter()
        .flat_map(|list| {
            let mut cursor = list.walk();
            list.named_children(&mut cursor)
                .filter(|child| {
                    matches!(child.kind(), "script_parameter" | "class_method_parameter")
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

/// The script-block arguments of a command (`It 'x' { ... }`), unwrapped from
/// their expression wrappers.
pub(super) fn block_arguments(command: Node) -> Vec<Node> {
    let Some(elements) = command.child_by_field_name("command_elements") else {
        return Vec::new();
    };
    let mut cursor = elements.walk();
    elements
        .named_children(&mut cursor)
        .filter_map(|element| {
            let mut current = element;
            while matches!(
                current.kind(),
                "array_literal_expression" | "unary_expression"
            ) {
                current = current.named_child(0)?;
            }
            (current.kind() == "script_block_expression").then_some(current)
        })
        .collect()
}

fn direct_children<'a>(node: Node<'a>, kind: &str) -> Vec<Node<'a>> {
    let mut cursor = node.walk();
    node.children(&mut cursor)
        .filter(|child| child.kind() == kind)
        .collect()
}

/// Record the `[OutputType([T])]` a function declares as its return type.
fn record_output_type(base: &mut BaseExtractor, symbol_id: &str, function: Node) {
    let param_blocks: Vec<Node> = match function.kind() {
        "function_statement" => direct_children(function, "script_block")
            .into_iter()
            .flat_map(|block| direct_children(block, "param_block"))
            .collect(),
        _ => vec![function],
    };
    for attribute in param_blocks
        .into_iter()
        .flat_map(|block| direct_children(block, "attribute_list"))
        .flat_map(|list| direct_children(list, "attribute"))
    {
        let is_output_type = direct_children(attribute, "attribute_name")
            .first()
            .is_some_and(|name| base.get_node_text(name).eq_ignore_ascii_case("OutputType"));
        if let (true, Some(arguments)) = (
            is_output_type,
            direct_children(attribute, "attribute_arguments").first(),
        ) {
            type_facts::record_declared_type_literal(base, symbol_id, *arguments);
            return;
        }
    }
}

/// Extract function signature
fn extract_function_signature(base: &BaseExtractor, node: Node) -> Option<String> {
    let name = find_function_name_node(node).map(|n| base.get_node_text(&n))?;

    let has_attributes = has_attribute(base, node, "CmdletBinding");
    let prefix = if has_attributes {
        "[CmdletBinding()] "
    } else {
        ""
    };

    Some(format!("{}function {}()", prefix, name))
}

/// Extract advanced function signature
fn extract_advanced_function_signature(
    base: &BaseExtractor,
    node: Node,
    function_name: &str,
) -> String {
    let has_cmdlet_binding = has_attribute(base, node, "CmdletBinding");
    let has_output_type = has_attribute(base, node, "OutputType");

    let mut signature = String::new();
    if has_cmdlet_binding {
        signature.push_str("[CmdletBinding()] ");
    }
    if has_output_type {
        signature.push_str("[OutputType([void])] ");
    }
    signature.push_str(&format!("function {}()", function_name));

    signature
}

/// A `param()` parameter's signature: its `[Parameter(...)]` and type
/// attributes, then its own variable.
fn extract_script_parameter_signature(base: &BaseExtractor, node: Node, variable: Node) -> String {
    let name = base.get_node_text(&variable);
    let attributes: Vec<String> = direct_children(node, "attribute_list")
        .into_iter()
        .flat_map(|list| direct_children(list, "attribute"))
        .map(|attribute| base.get_node_text(&attribute))
        .filter(|text| text.contains("Parameter") || super::types::is_type_bracket(text))
        .collect();
    if attributes.is_empty() {
        name
    } else {
        format!("{} {}", attributes.join(" "), name)
    }
}

fn parameter_role_metadata() -> HashMap<String, serde_json::Value> {
    HashMap::from([(
        "role".to_string(),
        serde_json::Value::String("parameter".to_string()),
    )])
}
