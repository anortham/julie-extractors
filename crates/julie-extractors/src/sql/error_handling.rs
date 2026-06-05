//! ERROR node processing for tree-sitter parse failures.
//!
//! Handles extraction from tree-sitter ERROR nodes when the parser encounters
//! syntax it doesn't recognize. This is critical for handling diverse SQL dialects.
//!
//! Functions are dispatched to their domain modules where possible:
//! - Procedures, functions, aggregates → routines.rs
//! - Views → views.rs
//! - Constraints → constraints.rs
//!
//! Schemas, triggers, domains, and types remain here because schemas.rs
//! is at the 500-line limit and cannot accept additional functions.

use crate::base::{BaseExtractor, Symbol, SymbolKind, SymbolOptions};
use crate::sql::{constraints, routines, views};
use regex::Regex;
use std::collections::HashMap;
use std::sync::LazyLock;
use tree_sitter::Node;

static ERROR_SCHEMA_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"CREATE\s+SCHEMA\s+([a-zA-Z_][a-zA-Z0-9_]*)").unwrap());
static ERROR_TRIGGER_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"CREATE\s+TRIGGER\s+([a-zA-Z_][a-zA-Z0-9_]*)").unwrap());
static ERROR_TRIGGER_DETAILS_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"CREATE\s+TRIGGER\s+[a-zA-Z_][a-zA-Z0-9_]*\s+(BEFORE|AFTER)\s+(INSERT|UPDATE|DELETE)\s+ON\s+([a-zA-Z_][a-zA-Z0-9_]*)",
    )
    .unwrap()
});
static ERROR_DOMAIN_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"CREATE\s+DOMAIN\s+([a-zA-Z_][a-zA-Z0-9_]*)\s+AS\s+([A-Za-z]+(?:\(\d+(?:,\s*\d+)?\))?)",
    )
    .unwrap()
});
static ERROR_DOMAIN_CHECK_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"CHECK\s*\(([^)]+(?:\([^)]*\)[^)]*)*)\)").unwrap());
static ERROR_ENUM_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"CREATE\s+TYPE\s+([a-zA-Z_][a-zA-Z0-9_]*)\s+AS\s+ENUM\s*\(([\s\S]*?)\)").unwrap()
});

/// Extract multiple symbols from ERROR node
pub(super) fn extract_multiple_from_error_node(
    base: &mut BaseExtractor,
    node: Node,
    symbols: &mut Vec<Symbol>,
    parent_id: Option<&str>,
) {
    let error_text = base.get_node_text(&node);

    // Delegate to domain modules
    routines::extract_procedures_from_error(&error_text, base, &node, symbols, parent_id);
    routines::extract_functions_from_error(&error_text, base, &node, symbols, parent_id);
    views::extract_views_from_error(&error_text, base, &node, symbols, parent_id);
    constraints::extract_constraints_from_error(&error_text, base, &node, symbols, parent_id);
    routines::extract_aggregates_from_error(&error_text, base, &node, symbols, parent_id);

    // Kept here: schemas.rs is at its line limit
    extract_schemas_from_error(&error_text, base, &node, symbols, parent_id);
    extract_triggers_from_error(&error_text, base, &node, symbols, parent_id);
    extract_domains_from_error(&error_text, base, &node, symbols, parent_id);
    extract_types_from_error(&error_text, base, &node, symbols, parent_id);
}

/// Extract schemas from ERROR node text
fn extract_schemas_from_error(
    error_text: &str,
    base: &mut BaseExtractor,
    node: &Node,
    symbols: &mut Vec<Symbol>,
    parent_id: Option<&str>,
) {
    if let Some(captures) = ERROR_SCHEMA_RE.captures(error_text)
        && let Some(schema_name) = captures.get(1)
    {
        let name = schema_name.as_str().to_string();

        let mut metadata = HashMap::new();
        metadata.insert("isSchema".to_string(), serde_json::Value::Bool(true));
        metadata.insert(
            "extractedFromError".to_string(),
            serde_json::Value::Bool(true),
        );

        let options = SymbolOptions {
            signature: Some(format!("CREATE SCHEMA {}", name)),
            visibility: Some(crate::base::Visibility::Public),
            parent_id: parent_id.map(|s| s.to_string()),
            doc_comment: None,
            metadata: Some(metadata),
            annotations: Vec::new(),
        };

        let schema_symbol = base.create_symbol(node, name, SymbolKind::Namespace, options);
        symbols.push(schema_symbol);
    }
}

/// Extract triggers from ERROR node text
fn extract_triggers_from_error(
    error_text: &str,
    base: &mut BaseExtractor,
    node: &Node,
    symbols: &mut Vec<Symbol>,
    parent_id: Option<&str>,
) {
    if let Some(captures) = ERROR_TRIGGER_RE.captures(error_text)
        && let Some(trigger_name) = captures.get(1)
    {
        let name = trigger_name.as_str().to_string();

        let mut signature = format!("CREATE TRIGGER {}", name);
        if let Some(details_captures) = ERROR_TRIGGER_DETAILS_RE.captures(error_text) {
            let timing = details_captures.get(1).map_or("", |m| m.as_str());
            let event = details_captures.get(2).map_or("", |m| m.as_str());
            let table = details_captures.get(3).map_or("", |m| m.as_str());

            if !timing.is_empty() && !event.is_empty() && !table.is_empty() {
                signature = format!("CREATE TRIGGER {} {} {} ON {}", name, timing, event, table);
            }
        }

        let mut metadata = HashMap::new();
        metadata.insert("isTrigger".to_string(), serde_json::Value::Bool(true));
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

        let trigger_symbol = base.create_symbol(node, name, SymbolKind::Method, options);
        symbols.push(trigger_symbol);
    }
}

/// Extract domains from ERROR node text
fn extract_domains_from_error(
    error_text: &str,
    base: &mut BaseExtractor,
    node: &Node,
    symbols: &mut Vec<Symbol>,
    parent_id: Option<&str>,
) {
    if let Some(captures) = ERROR_DOMAIN_RE.captures(error_text)
        && let Some(domain_name) = captures.get(1)
    {
        let name = domain_name.as_str().to_string();
        let base_type = captures.get(2).map_or("", |m| m.as_str()).to_string();

        // Skip if base type is empty
        if base_type.is_empty() {
            return;
        }

        let mut signature = format!("CREATE DOMAIN {} AS {}", name, base_type);

        if let Some(check_captures) = ERROR_DOMAIN_CHECK_RE.captures(error_text) {
            let check_condition = check_captures.get(1).map_or("", |m| m.as_str()).trim();
            if !check_condition.is_empty() {
                signature.push_str(&format!(" CHECK ({})", check_condition));
            }
        }

        let mut metadata = HashMap::new();
        metadata.insert("isDomain".to_string(), serde_json::Value::Bool(true));
        metadata.insert(
            "extractedFromError".to_string(),
            serde_json::Value::Bool(true),
        );
        metadata.insert("baseType".to_string(), serde_json::Value::String(base_type));

        let options = SymbolOptions {
            signature: Some(signature),
            visibility: Some(crate::base::Visibility::Public),
            parent_id: parent_id.map(|s| s.to_string()),
            doc_comment: None,
            metadata: Some(metadata),
            annotations: Vec::new(),
        };

        let domain_symbol = base.create_symbol(node, name, SymbolKind::Class, options);
        symbols.push(domain_symbol);
    }
}

/// Extract enum/custom types from ERROR node text
fn extract_types_from_error(
    error_text: &str,
    base: &mut BaseExtractor,
    node: &Node,
    symbols: &mut Vec<Symbol>,
    parent_id: Option<&str>,
) {
    if let Some(captures) = ERROR_ENUM_RE.captures(error_text)
        && let Some(enum_name) = captures.get(1)
    {
        let name = enum_name.as_str().to_string();
        let enum_values = captures.get(2).map_or("", |m| m.as_str());

        // Skip if enum values are empty
        if enum_values.is_empty() {
            return;
        }

        let signature = format!("CREATE TYPE {} AS ENUM ({})", name, enum_values.trim());

        let mut metadata = HashMap::new();
        metadata.insert("isEnum".to_string(), serde_json::Value::Bool(true));
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

        let enum_symbol = base.create_symbol(node, name, SymbolKind::Class, options);
        symbols.push(enum_symbol);
    }
}
