use super::functions;
use super::tables;
use super::variables;
/// Core symbol extraction and tree traversal
///
/// Handles the main tree traversal logic and dispatches to appropriate
/// extraction functions based on node types.
use crate::base::{
    BaseExtractor, RelationshipKind, Symbol, SymbolKind, SymbolOptions, UnresolvedTarget,
    Visibility,
};
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};
use serde_json::Value;
use std::collections::HashMap;
use tree_sitter::Node;

/// Function and table value nodes mapped to the symbol that binds them, keyed by node id.
pub(super) type ValueOwners = HashMap<usize, String>;

/// Recursively traverse the tree and extract symbols
pub(super) fn traverse_tree(
    symbols: &mut Vec<Symbol>,
    base: &mut BaseExtractor,
    owners: &mut ValueOwners,
    node: Node,
    parent_id: Option<String>,
    depth: u32,
) {
    if !should_visit_tree_depth(depth) {
        return;
    }
    let parent_id = owners.remove(&node.id()).or(parent_id);

    let mut symbol: Option<Symbol> = None;

    match node.kind() {
        "function_call" => {
            // busted call-style tests (`describe`/`it`/`before_each` etc.) take
            // priority; the shared core pushes nothing, so push here. Non-DSL
            // calls fall through to the bare-`require` import detector (which
            // pushes internally). Either result becomes the parent for nested
            // test calls below.
            if let Some(test_sym) =
                super::test_calls::extract_lua_test_call(base, node, parent_id.as_deref())
            {
                symbols.push(test_sym.clone());
                symbol = Some(test_sym);
            } else {
                symbol = extract_bare_require_import(symbols, base, node, parent_id.as_deref());
            }
        }
        "function_definition_statement" | "function_declaration" => {
            symbol = functions::extract_function_definition_statement(
                symbols,
                base,
                node,
                parent_id.as_deref(),
            );
        }
        "local_function_definition_statement" | "local_function_declaration" => {
            symbol = functions::extract_local_function_definition_statement(
                symbols,
                base,
                node,
                parent_id.as_deref(),
            );
        }
        "variable_declaration" => {
            variables::extract_local_variable_declaration(
                symbols,
                base,
                owners,
                node,
                parent_id.as_deref(),
            );
        }
        "assignment_statement"
            if node
                .parent()
                .is_none_or(|parent| parent.kind() != "variable_declaration") =>
        {
            variables::extract_assignment_statement(
                symbols,
                base,
                owners,
                node,
                parent_id.as_deref(),
            );
        }
        "table_constructor" => {
            tables::extract_table_fields(symbols, base, owners, node, parent_id.as_deref());
            let mut cursor = node.walk();
            let owned_values: Vec<Node> = node
                .children(&mut cursor)
                .filter_map(|field| field.child_by_field_name("value"))
                .filter(|value| owners.contains_key(&value.id()))
                .collect();
            if let Some(child_depth) = child_tree_depth(depth) {
                for value in owned_values {
                    traverse_tree(symbols, base, owners, value, parent_id.clone(), child_depth);
                }
            }
            return;
        }
        _ => {}
    }

    // Traverse children with current symbol as parent (if extracted) or keep same parent
    let current_parent_id = symbol.as_ref().map(|s| s.id.clone()).or(parent_id);
    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        traverse_tree(
            symbols,
            base,
            owners,
            child,
            current_parent_id.clone(),
            child_depth,
        );
    }
}

fn extract_bare_require_import(
    symbols: &mut Vec<Symbol>,
    base: &mut BaseExtractor,
    node: Node,
    parent_id: Option<&str>,
) -> Option<Symbol> {
    if !is_bare_require_statement(base, node) {
        return None;
    }

    let require = require_import(base, node)?;
    let options = SymbolOptions {
        signature: Some(format!("require({:?})", require.module_path)),
        visibility: Some(Visibility::Public),
        parent_id: parent_id.map(|s| s.to_string()),
        metadata: Some(require.metadata()),
        ..Default::default()
    };

    let symbol = base.create_symbol(
        &node,
        require.terminal_name.clone(),
        SymbolKind::Import,
        options,
    );
    record_require_pending(base, &symbol.id, require, node, parent_id);
    symbols.push(symbol.clone());
    Some(symbol)
}

pub(super) struct RequireImport {
    module_path: String,
    terminal_name: String,
    import_context: String,
}

impl RequireImport {
    pub(super) fn metadata(&self) -> HashMap<String, Value> {
        HashMap::from([
            (
                "source".to_string(),
                Value::String(self.module_path.clone()),
            ),
            (
                "importContext".to_string(),
                Value::String(self.import_context.clone()),
            ),
        ])
    }
}

/// Parse `require "mod"` / `require("mod")` into its module path and import context.
pub(super) fn require_import(base: &BaseExtractor, call_node: Node) -> Option<RequireImport> {
    if call_node.kind() != "function_call"
        || base.get_node_text(&call_node.child_by_field_name("name")?) != "require"
    {
        return None;
    }
    let require_call = parse_require_call(base, call_node)?;
    Some(RequireImport {
        module_path: require_call.module_path,
        terminal_name: require_call.terminal_name,
        import_context: base.get_node_text(&call_node),
    })
}

pub(super) fn record_require_pending(
    base: &mut BaseExtractor,
    symbol_id: &str,
    require: RequireImport,
    call_node: Node,
    parent_id: Option<&str>,
) {
    let pending = base.create_pending_relationship(
        symbol_id.to_string(),
        UnresolvedTarget {
            display_name: require.module_path,
            terminal_name: require.terminal_name,
            receiver: None,
            namespace_path: Vec::new(),
            import_context: Some(require.import_context),
        },
        RelationshipKind::Imports,
        &call_node,
        Some(parent_id.unwrap_or(symbol_id).to_string()),
        Some(0.7),
    );
    base.add_structured_pending_relationship(pending);
}

fn is_bare_require_statement(base: &BaseExtractor, node: Node) -> bool {
    let Some(name_node) = node.child_by_field_name("name") else {
        return false;
    };

    if base.get_node_text(&name_node) != "require" {
        return false;
    }

    !node
        .parent()
        .map(|parent| matches!(parent.kind(), "expression_list" | "arguments"))
        .unwrap_or(false)
}

struct RequireCall {
    module_path: String,
    terminal_name: String,
}

fn parse_require_call(base: &BaseExtractor, node: Node) -> Option<RequireCall> {
    let arguments = node.child_by_field_name("arguments")?;
    let module_path = extract_first_string_argument(base, arguments)?;
    let terminal_name = terminal_require_name(&module_path);

    if terminal_name.is_empty() {
        return None;
    }

    Some(RequireCall {
        module_path,
        terminal_name,
    })
}

fn extract_first_string_argument(base: &BaseExtractor, arguments: Node) -> Option<String> {
    let mut cursor = arguments.walk();
    let string_node = arguments
        .children(&mut cursor)
        .find(|child| child.kind() == "string")?;
    normalize_lua_string_literal(&base.get_node_text(&string_node))
}

fn normalize_lua_string_literal(raw: &str) -> Option<String> {
    let trimmed = raw.trim();

    if let Some(stripped) = trimmed
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
    {
        return Some(stripped.to_string());
    }

    if let Some(stripped) = trimmed
        .strip_prefix('\'')
        .and_then(|value| value.strip_suffix('\''))
    {
        return Some(stripped.to_string());
    }

    if let Some(stripped) = trimmed
        .strip_prefix("[[")
        .and_then(|value| value.strip_suffix("]]"))
    {
        return Some(stripped.to_string());
    }

    None
}

fn terminal_require_name(module_path: &str) -> String {
    module_path
        .split(['/', '.'])
        .rfind(|segment| !segment.is_empty() && *segment != ".")
        .unwrap_or(module_path)
        .to_string()
}
