//! PowerShell command and cmdlet extraction
//! Focuses on Azure, Windows, and cross-platform DevOps commands

use crate::base::{BaseExtractor, Symbol, SymbolKind, SymbolOptions, Visibility};
use regex::Regex;
use std::sync::LazyLock;
use tree_sitter::Node;

use super::helpers::{argument_words, command_arguments};
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};
use std::collections::HashMap;

// Static regexes compiled once for performance
static CONFIGURATION_NAME_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"Configuration\s+([A-Za-z][A-Za-z0-9-_]*)").unwrap());
static FUNCTION_NAME_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?im)^[ \t]*function\s+([A-Za-z][A-Za-z0-9-_]*)").unwrap());

const BUILTIN_CMDLETS: &[&str] = &["Write-Output", "Get-ChildItem", "Invoke-Command"];

pub(super) fn is_builtin_cmdlet(command_name: &str) -> bool {
    BUILTIN_CMDLETS
        .iter()
        .any(|builtin| builtin.eq_ignore_ascii_case(command_name))
}

/// A psake or Invoke-Build task (`task Build -depends Clean { ... }`) as a
/// function symbol, so the commands inside it have a caller. A `task` command
/// counts only with a name and a body or a dependency list.
pub(super) fn extract_build_task(
    base: &mut BaseExtractor,
    node: Node,
    parent_id: Option<&str>,
) -> Option<Symbol> {
    let command_name = node.child_by_field_name("command_name")?;
    let command = base.get_node_text(&command_name);
    if !(command.eq_ignore_ascii_case("task") || command.eq_ignore_ascii_case("Add-BuildTask")) {
        return None;
    }
    let arguments = command_arguments(base, node);
    let mut positional = arguments
        .iter()
        .filter(|(parameter, value)| {
            parameter.as_deref().is_none_or(|name| name == "name")
                && !is_script_block_argument(*value)
        })
        .map(|(_, value)| *value);
    let name = argument_words(base, positional.next()?)
        .into_iter()
        .next()?;
    let depends: Vec<String> = positional
        .chain(
            arguments
                .iter()
                .filter(|(parameter, _)| parameter.as_deref() == Some("depends"))
                .map(|(_, value)| *value),
        )
        .flat_map(|value| argument_words(base, value))
        .collect();
    let body = super::functions::block_arguments(node).into_iter().next();
    if body.is_none() && depends.is_empty() {
        return None;
    }

    let text = base.get_node_text(&node);
    let header_end = body.map_or(text.len(), |block| block.start_byte() - node.start_byte());
    let signature = text[..header_end].trim().to_string();
    let mut metadata = HashMap::from([(
        "role".to_string(),
        serde_json::Value::String("build_task".to_string()),
    )]);
    if !depends.is_empty() {
        metadata.insert("depends".to_string(), serde_json::json!(depends));
    }
    Some(base.create_symbol(
        &node,
        name,
        SymbolKind::Function,
        SymbolOptions {
            signature: Some(signature),
            visibility: Some(Visibility::Public),
            parent_id: parent_id.map(|s| s.to_string()),
            metadata: Some(metadata),
            doc_comment: super::documentation::extract_powershell_doc_comment(base, &node),
            annotations: Vec::new(),
        },
    ))
}

fn is_script_block_argument(value: Node) -> bool {
    let mut current = value;
    while matches!(
        current.kind(),
        "array_literal_expression" | "unary_expression"
    ) {
        match current.named_child(0) {
            Some(child) => current = child,
            None => return false,
        }
    }
    current.kind() == "script_block_expression"
}

/// A DSC resource instance inside a `Configuration` block
/// (`WindowsFeature IIS { Ensure = 'Present' }`): the resource type and
/// instance name, the `Node` it targets, and its `DependsOn` references.
pub(crate) struct DscResource {
    pub resource_type: String,
    pub resource_name: String,
    pub configuration: Option<String>,
    pub node_name: Option<String>,
    pub depends_on: Vec<String>,
}

pub(crate) fn dsc_resource(content: &str, node: Node) -> Option<DscResource> {
    if node.kind() != "command" {
        return None;
    }
    let text = |n: Node| content.get(n.byte_range()).unwrap_or_default().to_string();
    let resource_type = text(node.child_by_field_name("command_name")?);
    if !resource_type
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
        || ["node", "configuration"]
            .iter()
            .any(|keyword| resource_type.eq_ignore_ascii_case(keyword))
    {
        return None;
    }
    let elements = node.child_by_field_name("command_elements")?;
    let mut cursor = elements.walk();
    let values: Vec<Node> = elements
        .named_children(&mut cursor)
        .filter(|child| child.kind() != "command_argument_sep")
        .collect();
    let [name_node, block] = values.as_slice() else {
        return None;
    };
    if !matches!(
        name_node.kind(),
        "generic_token" | "array_literal_expression"
    ) || !is_script_block_argument(*block)
    {
        return None;
    }
    let resource_name = text(*name_node).trim_matches(['"', '\'']).to_string();

    let mut configuration = None;
    let mut node_name = None;
    let mut ancestor = node.parent();
    while let Some(current) = ancestor {
        if current.kind() == "command"
            && let Some(name) = current.child_by_field_name("command_name")
        {
            let keyword = text(name);
            let first_argument = || {
                let elements = current.child_by_field_name("command_elements")?;
                let mut cursor = elements.walk();
                elements
                    .named_children(&mut cursor)
                    .find(|child| child.kind() != "command_argument_sep")
                    .map(|argument| text(argument).trim_matches(['"', '\'']).to_string())
            };
            if keyword.eq_ignore_ascii_case("Node") && node_name.is_none() {
                node_name = first_argument();
            } else if keyword.eq_ignore_ascii_case("Configuration") {
                configuration = first_argument();
                break;
            }
        }
        ancestor = current.parent();
    }
    configuration.as_ref()?;

    let depends_on = dsc_property_statements(*block)
        .into_iter()
        .filter(|(key, _)| text(*key).eq_ignore_ascii_case("DependsOn"))
        .flat_map(|(_, value)| string_values(content, value))
        .collect();
    Some(DscResource {
        resource_type,
        resource_name,
        configuration,
        node_name,
        depends_on,
    })
}

/// The `Key = value` property statements of a DSC resource block, as the key
/// node and the value node. The grammar reads each one as a command named
/// after the key.
pub(super) fn dsc_property_statements(block: Node) -> Vec<(Node, Node)> {
    let mut current = block;
    while matches!(
        current.kind(),
        "array_literal_expression" | "unary_expression"
    ) {
        match current.named_child(0) {
            Some(child) => current = child,
            None => return Vec::new(),
        }
    }
    let statements = {
        let mut cursor = current.walk();
        current
            .named_children(&mut cursor)
            .find(|child| child.kind() == "script_block")
            .and_then(|script| script.child_by_field_name("script_block_body"))
            .and_then(|body| body.child_by_field_name("statement_list"))
    };
    let Some(statements) = statements else {
        return Vec::new();
    };
    let mut cursor = statements.walk();
    statements
        .named_children(&mut cursor)
        .filter_map(|statement| {
            let command = statement.named_child(0)?.named_child(0)?;
            (command.kind() == "command").then_some(command)?;
            let key = command.child_by_field_name("command_name")?;
            let elements = command.child_by_field_name("command_elements")?;
            let mut cursor = elements.walk();
            let mut values = elements
                .named_children(&mut cursor)
                .filter(|child| child.kind() != "command_argument_sep");
            let equals = values.next()?;
            (equals.kind() == "generic_token" && equals.byte_range().len() == 1).then_some(())?;
            Some((key, values.next()?))
        })
        .collect()
}

fn string_values(content: &str, value: Node) -> Vec<String> {
    let mut out = Vec::new();
    collect_string_values(content, value, &mut out, 0);
    out
}

fn collect_string_values(content: &str, node: Node, out: &mut Vec<String>, depth: u32) {
    if !should_visit_tree_depth(depth) {
        return;
    }
    if node.kind() == "string_literal" {
        let text = content.get(node.byte_range()).unwrap_or_default();
        out.push(text.trim_matches(['"', '\'']).to_string());
        return;
    }
    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        collect_string_values(content, child, out, child_depth);
    }
}

/// Extract DSC Configuration command
pub(super) fn extract_dsc_configuration(
    base: &mut BaseExtractor,
    node: Node,
    parent_id: Option<&str>,
) -> Option<Symbol> {
    // For DSC Configuration commands, extract the configuration name from command arguments
    let mut cursor = node.walk();
    let command_elements: Vec<_> = node
        .children(&mut cursor)
        .filter(|child| child.kind() == "command_elements")
        .collect();

    if command_elements.is_empty() {
        return None;
    }

    // Look for the configuration name in the command elements
    let mut elements_cursor = command_elements[0].walk();
    for element in command_elements[0].children(&mut elements_cursor) {
        if element.kind() == "command_argument_sep" {
            // Skip separators
            continue;
        }
        if element.kind() == "generic_token" || element.kind() == "command_name" {
            let token_text = base.get_node_text(&element);
            // Skip "Configuration" keyword and look for the name
            if token_text != "Configuration" && !token_text.trim().is_empty() {
                let name = token_text.trim().to_string();
                let signature = format!("Configuration {}", name);

                return Some(base.create_symbol(
                    &node,
                    name,
                    SymbolKind::Function,
                    SymbolOptions {
                        signature: Some(signature),
                        visibility: Some(Visibility::Public),
                        parent_id: parent_id.map(|s| s.to_string()),
                        metadata: None,
                        doc_comment: None,
                        annotations: Vec::new(),
                    },
                ));
            }
        }
    }

    None
}

/// Extract configuration name from ERROR node containing DSC configuration
pub(super) fn extract_configuration_from_error(
    _base: &BaseExtractor,
    node_text: &str,
) -> Option<(String, String)> {
    // Extract configuration name from text like "Configuration MyWebServer {"
    if let Some(config_match) = CONFIGURATION_NAME_RE.captures(node_text) {
        let name = config_match.get(1).unwrap().as_str().to_string();
        let signature = format!("Configuration {}", name);
        return Some((name, signature));
    }

    None
}

/// Extract function name from ERROR node containing function
pub(super) fn extract_function_from_error(
    _base: &BaseExtractor,
    node_text: &str,
) -> Option<(String, String)> {
    // Extract function name from text like "function MyFunction {"
    if let Some(func_match) = FUNCTION_NAME_RE.captures(node_text) {
        let name = func_match.get(1).unwrap().as_str().to_string();
        let signature = format!("function {}()", name);
        return Some((name, signature));
    }

    None
}
