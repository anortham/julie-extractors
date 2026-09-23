// CSS Extractor Animations - Extract @keyframes and animation-related symbols

use crate::base::{BaseExtractor, Symbol, SymbolKind, SymbolOptions, Visibility};
use std::collections::HashMap;
use tree_sitter::Node;

pub(super) struct AnimationExtractor;

impl AnimationExtractor {
    /// Extract keyframes rule - Implementation of extractKeyframesRule
    pub(super) fn extract_keyframes_rule(
        base: &mut BaseExtractor,
        node: Node,
        parent_id: Option<&str>,
    ) -> Option<Symbol> {
        let keyword = super::at_rules::at_rule_keyword(base, node)?;
        let keyframes_name = Self::extract_keyframes_name(base, &node)?;
        let signature = base.get_node_text(&node);
        let symbol_name = format!("{keyword} {keyframes_name}");

        // Create metadata
        let mut metadata = HashMap::new();
        metadata.insert(
            "type".to_string(),
            serde_json::Value::String("keyframes".to_string()),
        );
        metadata.insert(
            "animationName".to_string(),
            serde_json::Value::String(keyframes_name),
        );

        // Extract CSS comment
        let doc_comment = base.find_doc_comment(&node);

        Some(base.create_symbol(
            &node,
            symbol_name,
            SymbolKind::Function, // Animations as functions as designed
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

    /// Extract individual keyframes - intentionally a no-op.
    ///
    /// Individual keyframe blocks (0%, 50%, 100%, from, to) are not useful as
    /// symbols for code intelligence. They pollute search results and have no
    /// meaningful name. The @keyframes rule itself (including the animation name)
    /// is extracted by `extract_keyframes_rule`.
    pub(super) fn extract_keyframes(
        _base: &mut BaseExtractor,
        _node: Node,
        _symbols: &mut Vec<Symbol>,
        _parent_id: Option<&str>,
    ) {
        // Intentionally empty — keyframe percentages/keywords are noise, not symbols.
    }

    /// Extract keyframes name - port of extractKeyframesName
    pub(super) fn extract_keyframes_name(base: &BaseExtractor, node: &Node) -> Option<String> {
        let mut cursor = node.walk();
        node.children(&mut cursor)
            .find(|child| child.kind() == "keyframes_name")
            .map(|name| base.get_node_text(&name))
    }
}
