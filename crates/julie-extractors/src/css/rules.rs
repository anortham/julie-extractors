// CSS Extractor Rules - Extract CSS rules and their properties

use super::helpers::PropertyHelper;
use super::identifiers::composes_module_source;
use crate::base::{
    BaseExtractor, NormalizedSpan, RelationshipKind, StructuredPendingRelationship, Symbol,
    SymbolKind, SymbolOptions, UnresolvedTarget, Visibility,
};
use std::collections::HashMap;
use tree_sitter::Node;

pub(super) struct RuleExtractor;

impl RuleExtractor {
    /// Extract CSS rule - Implementation of extractRule
    pub(super) fn extract_rule(
        base: &mut BaseExtractor,
        node: Node,
        parent_id: Option<&str>,
    ) -> Option<Symbol> {
        // Find selectors and declaration block
        let mut selectors_node = None;
        let mut declaration_block = None;

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            match child.kind() {
                "selectors" => selectors_node = Some(child),
                "block" => declaration_block = Some(child),
                _ => {}
            }
        }

        let selectors = selectors_node?;
        let selector_text = base.get_node_text(&selectors);

        let signature = Self::build_rule_signature(base, &node, &selector_text);

        // Create metadata
        let mut metadata = HashMap::new();
        metadata.insert(
            "type".to_string(),
            serde_json::Value::String("css-rule".to_string()),
        );
        metadata.insert(
            "selector".to_string(),
            serde_json::Value::String(selector_text.clone()),
        );

        let properties = PropertyHelper::extract_properties(base, declaration_block.as_ref());
        metadata.insert(
            "properties".to_string(),
            serde_json::Value::Array(
                properties
                    .into_iter()
                    .map(serde_json::Value::String)
                    .collect(),
            ),
        );

        // Extract CSS comment
        let doc_comment = base.find_doc_comment(&node);

        let symbol = base.create_symbol(
            &node,
            selector_text,
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
        if let Some(block) = declaration_block {
            Self::add_composes_imports(base, block, &symbol.id);
        }
        Some(symbol)
    }

    /// `composes: name from "./other.module.css"` imports that CSS Module.
    fn add_composes_imports(base: &mut BaseExtractor, block: Node, rule_id: &str) {
        let mut cursor = block.walk();
        let declarations: Vec<Node> = block
            .named_children(&mut cursor)
            .filter(|child| child.kind() == "declaration")
            .collect();
        for declaration in declarations {
            let Some((source, path)) = composes_module_source(base, declaration) else {
                continue;
            };
            let mut target = UnresolvedTarget::simple(path);
            target.import_context = Some("css-modules-composes".to_string());
            base.add_structured_pending_relationship(StructuredPendingRelationship::new(
                rule_id.to_string(),
                target,
                Some(rule_id.to_string()),
                RelationshipKind::Imports,
                base.file_path.clone(),
                NormalizedSpan::from_node(&source).start_line,
                0.9,
            ));
        }
    }

    /// Build rule signaimplementation's buildRuleSignature
    pub(super) fn build_rule_signature(
        base: &BaseExtractor,
        node: &Node,
        selector: &str,
    ) -> String {
        let declaration_block = PropertyHelper::find_declaration_block(node);

        if let Some(block) = declaration_block {
            let key_properties =
                PropertyHelper::extract_key_properties(base, &block, Some(selector));
            if !key_properties.is_empty() {
                return format!("{} {{ {} }}", selector, key_properties.join("; "));
            }
        }

        selector.to_string()
    }
}
