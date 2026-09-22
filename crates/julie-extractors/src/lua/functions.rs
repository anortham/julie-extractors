use super::helpers;
use super::parameters;
use super::scope;
/// Function and method definition extraction
///
/// Handles extraction of:
/// - Regular functions: `function name() end`
/// - Local functions: `local function name() end`
/// - Methods with colon syntax: `function obj:method() end`
/// - Methods with dot syntax: `function obj.method() end`
use crate::base::{BaseExtractor, Symbol, SymbolKind, SymbolOptions, Visibility};
use crate::test_detection::apply_callable_test_metadata;
use std::collections::HashMap;
use tree_sitter::Node;

fn collapse_signature_whitespace(text: &str) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .replace("( ", "(")
        .replace(" )", ")")
}

fn build_function_signature(base: &BaseExtractor, node: Node, name_node: Node) -> String {
    let node_text = base.get_node_text(&node);
    let prefix = if node_text.trim_start().starts_with("local function") {
        "local function"
    } else {
        "function"
    };
    let parameters = node
        .child_by_field_name("parameters")
        .map(|parameters| base.get_node_text(&parameters))
        .unwrap_or_else(|| "()".to_string());

    collapse_signature_whitespace(&format!(
        "{prefix} {}{}",
        base.get_node_text(&name_node).trim(),
        parameters
    ))
}

/// Extract regular function definition statement
/// Handles both `function name()` and method definitions
pub(super) fn extract_function_definition_statement(
    symbols: &mut Vec<Symbol>,
    base: &mut BaseExtractor,
    node: Node,
    parent_id: Option<&str>,
) -> Option<Symbol> {
    let declared_name = node.child_by_field_name("name")?;
    let (name, kind, method_parent_id) = match declared_name.kind() {
        "identifier" => (
            base.get_node_text(&declared_name),
            SymbolKind::Function,
            None,
        ),
        "dot_index_expression" | "method_index_expression" => {
            let member_field = if declared_name.kind() == "dot_index_expression" {
                "field"
            } else {
                "method"
            };
            let member = declared_name.child_by_field_name(member_field)?;
            let owner_id = declared_name
                .child_by_field_name("table")
                .and_then(|table| scope::resolve_table_symbol_id(base, table, symbols));
            (base.get_node_text(&member), SymbolKind::Method, owner_id)
        }
        _ => return None,
    };
    let signature = build_function_signature(base, node, declared_name);

    // Determine visibility: check if function is local (contains "local" keyword) or uses underscore prefix
    let node_text = base.get_node_text(&node);
    let is_local = node_text.trim_start().starts_with("local function");
    let has_underscore = name.starts_with('_');
    let visibility = if is_local || has_underscore {
        Visibility::Private
    } else {
        Visibility::Public
    };

    // Extract LuaDoc comment
    let doc_comment = base.find_doc_comment(&node);

    // Test detection
    let mut metadata = HashMap::new();
    apply_callable_test_metadata(
        "lua",
        &name,
        &base.file_path,
        &kind,
        &[],
        doc_comment.as_deref(),
        &mut metadata,
    );

    let options = SymbolOptions {
        signature: Some(signature),
        parent_id: method_parent_id.or_else(|| parent_id.map(|s| s.to_string())),
        visibility: Some(visibility),
        doc_comment,
        metadata: if metadata.is_empty() {
            None
        } else {
            Some(metadata)
        },
        ..Default::default()
    };

    let symbol = base.create_symbol(&node, name, kind, options);
    symbols.push(symbol.clone());
    symbols.extend(parameters::extract_parameter_symbols(
        base, node, &symbol.id,
    ));
    Some(symbol)
}

/// Extract local function definition statement
/// Local functions are always private
pub(super) fn extract_local_function_definition_statement(
    symbols: &mut Vec<Symbol>,
    base: &mut BaseExtractor,
    node: Node,
    parent_id: Option<&str>,
) -> Option<Symbol> {
    let name_node = helpers::find_child_by_type(&node, "identifier")?;
    let name = base.get_node_text(&name_node);
    let signature = build_function_signature(base, node, name_node);

    // Extract LuaDoc comment
    let doc_comment = base.find_doc_comment(&node);

    // Local functions are always private (regardless of underscore prefix)
    let options = SymbolOptions {
        signature: Some(signature),
        parent_id: parent_id.map(|s| s.to_string()),
        visibility: Some(Visibility::Private),
        doc_comment,
        ..Default::default()
    };

    let symbol = base.create_symbol(&node, name, SymbolKind::Function, options);
    symbols.push(symbol.clone());
    symbols.extend(parameters::extract_parameter_symbols(
        base, node, &symbol.id,
    ));
    Some(symbol)
}
