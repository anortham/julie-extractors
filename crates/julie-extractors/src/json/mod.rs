/// JSON extractor - Extract keys and objects as symbols
///
/// Extracts JSON key-value pairs as symbols for semantic search and navigation.
/// - Top-level keys and nested object keys are extracted
/// - Objects and arrays are treated as SymbolKind::Module (containers)
/// - Primitive values are treated as SymbolKind::Variable
use crate::base::{
    BaseExtractor, Identifier, PendingRelationship, Relationship, StructuredPendingRelationship,
    Symbol, SymbolKind,
};
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};
use std::collections::HashMap;
use std::path::Path;
use tree_sitter::Tree;

mod relationships;
mod test_detection;

pub struct JsonExtractor {
    pub(crate) base: BaseExtractor,
}

impl JsonExtractor {
    pub fn new(
        language: String,
        file_path: String,
        source_code: String,
        workspace_root: &Path,
    ) -> Self {
        let base = BaseExtractor::new(language, file_path, source_code, workspace_root);
        Self { base }
    }

    pub fn extract_symbols(&mut self, tree: &tree_sitter::Tree) -> Vec<Symbol> {
        let mut symbols = Vec::new();
        self.walk_tree_for_symbols(tree.root_node(), &mut symbols, None, 0);
        symbols
    }

    /// Walk the tree and extract key-value pair symbols
    fn walk_tree_for_symbols(
        &mut self,
        node: tree_sitter::Node,
        symbols: &mut Vec<Symbol>,
        parent_id: Option<String>,
        depth: u32,
    ) {
        if !should_visit_tree_depth(depth) {
            return;
        }

        let symbol = self.extract_symbol_from_node(node, parent_id.as_deref(), symbols);
        let mut current_parent_id = parent_id;

        if let Some(ref sym) = symbol {
            symbols.push(sym.clone());
            current_parent_id = Some(sym.id.clone());
        }

        // Recursively process child nodes
        let Some(child_depth) = child_tree_depth(depth) else {
            return;
        };
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.walk_tree_for_symbols(child, symbols, current_parent_id.clone(), child_depth);
        }
    }

    /// Extract symbol from a node based on its type
    fn extract_symbol_from_node(
        &mut self,
        node: tree_sitter::Node,
        parent_id: Option<&str>,
        symbols: &[Symbol],
    ) -> Option<Symbol> {
        match node.kind() {
            "pair" => self.extract_pair(node, parent_id, symbols),
            _ => None,
        }
    }

    /// Extract a key-value pair as a symbol
    fn extract_pair(
        &mut self,
        node: tree_sitter::Node,
        parent_id: Option<&str>,
        symbols: &[Symbol],
    ) -> Option<Symbol> {
        use crate::base::SymbolOptions;

        // Get children: typically [string (key), ":", value]
        let mut cursor = node.walk();
        let children: Vec<_> = node.children(&mut cursor).collect();

        if children.len() < 3 {
            return None; // Need at least key, colon, value
        }

        // Extract key name (first child, strip quotes)
        let key_node = children[0];
        let key_text = self.base.get_node_text(&key_node);
        let key_name = key_text.trim_matches('"').to_string();

        // Value is typically the last child (after key and colon)
        let value_node = *children.last().unwrap();

        // Determine the value type to choose appropriate SymbolKind
        let symbol_kind = match value_node.kind() {
            "object" | "array" => SymbolKind::Module, // Treat containers as modules
            _ => SymbolKind::Variable,                // Treat primitives as variables
        };

        // Extract string values as doc_comment for semantic search
        // This enables searching memory files by description content, config values, etc.
        let doc_comment = if value_node.kind() == "string" {
            let value_text = self.base.get_node_text(&value_node);
            let trimmed = value_text.trim_matches('"');
            // Include non-empty strings, truncating to 2000 chars (for semantic search)
            if !trimmed.is_empty() {
                if trimmed.len() <= 2000 {
                    Some(trimmed.to_string())
                } else {
                    // Truncate long strings (e.g., plan content) instead of skipping
                    Some(trimmed.chars().take(2000).collect())
                }
            } else {
                None
            }
        } else {
            None
        };

        let options = SymbolOptions {
            signature: None,
            visibility: None,
            parent_id: parent_id.map(|s| s.to_string()),
            doc_comment,
            ..Default::default()
        };

        let mut symbol = self
            .base
            .create_symbol(&node, key_name.clone(), symbol_kind, options);

        if let Some(role) = test_detection::role_for_description_pair(&self.base, node, &key_name) {
            let metadata = symbol.metadata.get_or_insert_with(HashMap::new);
            let key = match role {
                test_detection::JsonTestRole::Container => "test_container",
                test_detection::JsonTestRole::Case => "is_test",
            };
            metadata.insert(key.to_string(), serde_json::Value::Bool(true));
        }

        if value_node.kind() == "string" {
            let carrier = crate::base::config_literals::build_config_key_carrier(
                symbols, parent_id, &key_name,
            );
            crate::base::config_literals::record_config_string_literal(
                &mut self.base,
                &value_node,
                &carrier,
                Some(symbol.id.clone()),
            );
        }

        Some(symbol)
    }

    pub fn extract_identifiers(
        &mut self,
        _tree: &tree_sitter::Tree,
        _symbols: &[Symbol],
    ) -> Vec<Identifier> {
        // JSON is configuration data - no code identifiers
        Vec::new()
    }

    pub fn infer_types(&self, _symbols: &[Symbol]) -> HashMap<String, String> {
        HashMap::new()
    }

    /// Extract JSON Schema `$ref` relationships (Phase 3.2). Local pointers
    /// resolve to concrete `Relationship`s; external pointers (`<file>#/...`)
    /// emit structured pending relationships.
    pub fn extract_relationships(&mut self, tree: &Tree, symbols: &[Symbol]) -> Vec<Relationship> {
        let mut relationships = Vec::new();
        relationships::extract_relationships_internal(
            &mut self.base,
            tree.root_node(),
            symbols,
            &mut relationships,
            0,
        );
        relationships
    }

    pub fn get_pending_relationships(&self) -> Vec<PendingRelationship> {
        self.base.get_pending_relationships()
    }

    pub fn get_type_argument_usages(&self) -> Vec<crate::base::TypeArgumentUsage> {
        self.base.get_type_argument_usages()
    }

    /// Clone captured call-argument literals (Miller bridge Phase 3).
    pub fn get_literals(&self) -> Vec<crate::base::Literal> {
        self.base.get_literals()
    }

    pub fn get_structured_pending_relationships(&self) -> Vec<StructuredPendingRelationship> {
        self.base.get_structured_pending_relationships()
    }
}
