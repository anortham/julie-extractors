// CSS Extractor Properties - Extract CSS custom properties and @supports rules

use crate::base::body::body_hash;
use crate::base::{BaseExtractor, NormalizedSpan, Symbol, SymbolKind, SymbolOptions, Visibility};
use regex::Regex;
use std::collections::HashMap;
use std::sync::LazyLock;
use tree_sitter::Node;

/// Matches `@supports` condition
static SUPPORTS_CONDITION_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"@supports\s+([^{]+)").unwrap());

pub(super) struct PropertyExtractor;

impl PropertyExtractor {
    /// A custom property symbol spans its whole declaration, which is also its
    /// body, and keeps the full value text.
    pub(super) fn extract_custom_property(
        base: &mut BaseExtractor,
        node: Node,
        parent_id: Option<&str>,
    ) -> Option<Symbol> {
        let property_name = base.get_node_text(&node);
        let declaration = node
            .parent()
            .filter(|parent| parent.kind() == "declaration")?;
        let declaration_text = base.get_node_text(&declaration);
        let value = declaration_text
            .split_once(':')
            .map(|(_, value)| value.trim().trim_end_matches(';').trim_end())
            .unwrap_or_default()
            .to_string();

        let signature = format!("{}: {}", property_name, value);

        let mut metadata = HashMap::new();
        metadata.insert(
            "type".to_string(),
            serde_json::Value::String("custom-property".to_string()),
        );
        metadata.insert(
            "property".to_string(),
            serde_json::Value::String(property_name.clone()),
        );
        metadata.insert("value".to_string(), serde_json::Value::String(value));

        let doc_comment = base.find_doc_comment(&declaration);

        let mut symbol = base.create_symbol(
            &declaration,
            property_name,
            SymbolKind::Property,
            SymbolOptions {
                signature: Some(signature),
                visibility: Some(Visibility::Public),
                parent_id: parent_id.map(|id| id.to_string()),
                metadata: Some(metadata),
                doc_comment,
                annotations: Vec::new(),
            },
        );
        let body = NormalizedSpan::from_node(&declaration);
        symbol.body_hash = body_hash(&base.content, body, &base.language);
        symbol.body_span = Some(body);
        Some(symbol)
    }

    /// Extract supports rule - port of extractSupportsRule
    pub(super) fn extract_supports_rule(
        base: &mut BaseExtractor,
        node: Node,
        parent_id: Option<&str>,
    ) -> Option<Symbol> {
        let condition = Self::extract_supports_condition(base, &node)?;
        let signature = base.get_node_text(&node);

        // Create metadata
        let mut metadata = HashMap::new();
        metadata.insert(
            "type".to_string(),
            serde_json::Value::String("supports-rule".to_string()),
        );
        metadata.insert(
            "condition".to_string(),
            serde_json::Value::String(condition.clone()),
        );

        // Extract CSS comment
        let doc_comment = base.find_doc_comment(&node);

        Some(base.create_symbol(
            &node,
            condition,
            SymbolKind::Variable,
            SymbolOptions {
                signature: Some(signature),
                visibility: Some(Visibility::Public),
                parent_id: parent_id.map(|id| id.to_string()),
                metadata: Some(metadata),
                doc_comment,
                annotations: Vec::new(),
            },
        ))
    }

    /// Extract supports condition - port of extractSupportsCondition
    pub(super) fn extract_supports_condition(base: &BaseExtractor, node: &Node) -> Option<String> {
        let text = base.get_node_text(node);
        let captures = SUPPORTS_CONDITION_RE.captures(&text)?;
        // Safe: capture group 1 exists if regex matched (pattern has one capture group)
        let condition = captures.get(1).map_or("", |m| m.as_str()).trim();
        Some(format!("@supports {}", condition))
    }
}
