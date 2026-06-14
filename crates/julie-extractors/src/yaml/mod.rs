/// YAML extractor - Extract mapping keys as symbols
///
/// Extracts YAML structure as symbols for semantic search and navigation.
/// - Mapping pairs: Individual key: value entries (the useful symbols)
/// - Anchors: Detected and included in signature (e.g., `defaults: &defaults`)
///
/// Intentionally skipped (noise):
/// - Documents: Generic container, every YAML file has one
/// - Flow mappings: Inline objects {...} — generic name, not useful
///
/// Common use cases:
/// - CI/CD configs (GitHub Actions, GitLab CI)
/// - Kubernetes manifests
/// - Docker Compose files
/// - Ansible playbooks
/// - Configuration files
mod relationships;

use crate::base::{BaseExtractor, Identifier, IdentifierKind, Relationship, Symbol, SymbolKind};
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};
use serde_json::Value;
use std::collections::HashMap;
use std::path::Path;

pub struct YamlExtractor {
    pub(crate) base: BaseExtractor,
}

impl YamlExtractor {
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

    /// Walk the tree and extract YAML symbols
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
            // Block mapping pairs are the useful symbols (key: value entries)
            "block_mapping_pair" => self.extract_mapping_pair(node, parent_id, symbols),

            // "document" and "flow_mapping" are noise — generic names with no
            // search value. Their children are still walked and extracted.
            _ => None,
        }
    }

    /// Extract a block mapping pair (key: value) as a symbol.
    /// If the value has a YAML anchor (`&name`), include it in the signature.
    fn extract_mapping_pair(
        &mut self,
        node: tree_sitter::Node,
        parent_id: Option<&str>,
        symbols: &[Symbol],
    ) -> Option<Symbol> {
        use crate::base::SymbolOptions;

        // Extract the key name
        let key_name = self.extract_mapping_key(node)?;

        // Skip merge keys (<<: *alias) — they're YAML syntax, not meaningful symbols
        if key_name == "<<" {
            return None;
        }

        // Check for anchor on the value side
        let anchor = self.extract_anchor(node);
        let signature = anchor.as_ref().map(|a| format!("{}: &{}", key_name, a));
        let metadata = anchor.as_ref().map(|anchor_name| {
            let mut metadata = HashMap::new();
            metadata.insert(
                "yaml_anchor".to_string(),
                Value::String(anchor_name.clone()),
            );
            metadata
        });

        // Determine kind: container keys (with nested mappings) are Module, leaves are Variable
        let is_leaf_value = !self.has_nested_mapping(node);
        let kind = if is_leaf_value {
            SymbolKind::Variable
        } else {
            SymbolKind::Module
        };

        let options = SymbolOptions {
            signature,
            visibility: None,
            parent_id: parent_id.map(|s| s.to_string()),
            metadata,
            doc_comment: find_yaml_key_doc_comment(&self.base, node),
            ..Default::default()
        };

        let symbol = self
            .base
            .create_symbol(&node, key_name.clone(), kind, options);

        if !is_leaf_value {
            return Some(symbol);
        }

        let mut cursor = node.walk();
        let mut saw_key_container = false;
        for child in node.children(&mut cursor) {
            if child.kind() != "flow_node" && child.kind() != "block_node" {
                continue;
            }
            if !saw_key_container {
                saw_key_container = true;
                continue;
            }
            let mut inner_cursor = child.walk();
            for scalar in child.children(&mut inner_cursor) {
                if !matches!(
                    scalar.kind(),
                    "double_quote_scalar" | "single_quote_scalar" | "plain_scalar"
                ) {
                    continue;
                }
                if scalar.kind() == "plain_scalar" {
                    let text = self.base.get_node_text(&scalar);
                    if text.contains(':') || text.starts_with('&') || text.starts_with('*') {
                        continue;
                    }
                }
                let carrier = crate::base::config_literals::build_config_key_carrier(
                    symbols, parent_id, &key_name,
                );
                crate::base::config_literals::record_config_string_literal(
                    &mut self.base,
                    &scalar,
                    &carrier,
                    Some(symbol.id.clone()),
                );
                break;
            }
            break;
        }

        Some(symbol)
    }

    /// Extract anchor name from a block_mapping_pair's value side.
    /// In `defaults: &defaults`, the AST has:
    ///   block_mapping_pair -> block_node -> anchor -> anchor_name
    fn extract_anchor(&self, node: tree_sitter::Node) -> Option<String> {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "block_node" {
                let mut block_cursor = child.walk();
                for block_child in child.children(&mut block_cursor) {
                    if block_child.kind() == "anchor" {
                        // Find the anchor_name child
                        let mut anchor_cursor = block_child.walk();
                        for anchor_child in block_child.children(&mut anchor_cursor) {
                            if anchor_child.kind() == "anchor_name" {
                                return Some(self.base.get_node_text(&anchor_child));
                            }
                        }
                    }
                }
            }
        }
        None
    }

    /// Check if a block_mapping_pair's value side contains a nested block_mapping.
    /// This distinguishes container keys (database:) from leaf keys (host: localhost).
    fn has_nested_mapping(&self, node: tree_sitter::Node) -> bool {
        yaml_pair_has_nested_mapping(node)
    }

    /// Extract the key from a block_mapping_pair
    fn extract_mapping_key(&self, node: tree_sitter::Node) -> Option<String> {
        let mut cursor = node.walk();

        for child in node.children(&mut cursor) {
            match child.kind() {
                "flow_node" | "block_node" => {
                    // Look for the actual key value
                    let mut key_cursor = child.walk();
                    for key_child in child.children(&mut key_cursor) {
                        match key_child.kind() {
                            "plain_scalar" | "single_quote_scalar" | "double_quote_scalar" => {
                                let key_text = self.base.get_node_text(&key_child);
                                // Remove quotes if present
                                let key_text = key_text.trim_matches('"').trim_matches('\'');
                                return Some(key_text.to_string());
                            }
                            _ => {}
                        }
                    }
                }
                _ => {}
            }
        }

        None
    }

    pub fn get_type_argument_usages(&self) -> Vec<crate::base::TypeArgumentUsage> {
        self.base.get_type_argument_usages()
    }

    /// Clone captured call-argument literals (Miller bridge Phase 3).
    pub fn get_literals(&self) -> Vec<crate::base::Literal> {
        self.base.get_literals()
    }

    pub fn extract_identifiers(
        &mut self,
        tree: &tree_sitter::Tree,
        symbols: &[Symbol],
    ) -> Vec<Identifier> {
        self.walk_tree_for_aliases(tree.root_node(), symbols);
        self.base.identifiers.clone()
    }

    pub fn infer_types(&self, _symbols: &[Symbol]) -> HashMap<String, String> {
        HashMap::new()
    }

    pub fn extract_relationships(
        &mut self,
        tree: &tree_sitter::Tree,
        symbols: &[Symbol],
    ) -> Vec<Relationship> {
        relationships::extract_relationships(&self.base, tree, symbols)
    }

    /// Walk the tree looking for alias nodes (*name) and create VariableRef identifiers
    fn walk_tree_for_aliases(&mut self, node: tree_sitter::Node, symbols: &[Symbol]) {
        self.walk_tree_for_aliases_at_depth(node, symbols, 0);
    }

    fn walk_tree_for_aliases_at_depth(
        &mut self,
        node: tree_sitter::Node,
        symbols: &[Symbol],
        depth: u32,
    ) {
        if !should_visit_tree_depth(depth) {
            return;
        }

        if node.kind() == "alias" {
            self.extract_alias_identifier(node, symbols);
        }

        let Some(child_depth) = child_tree_depth(depth) else {
            return;
        };
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.walk_tree_for_aliases_at_depth(child, symbols, child_depth);
        }
    }

    /// Extract an alias (*name) as a VariableRef identifier, resolving to the anchor's symbol
    fn extract_alias_identifier(&mut self, node: tree_sitter::Node, symbols: &[Symbol]) {
        // Find the alias_name child to get the actual name
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "alias_name" {
                let alias_name = self.base.get_node_text(&child);

                // Find the containing symbol (which mapping pair contains this alias)
                let containing_symbol_id = self
                    .base
                    .find_containing_symbol(&node, symbols)
                    .map(|s| s.id.clone());

                // Resolve: find the symbol whose signature contains &{alias_name}
                let target_symbol_id =
                    resolve_alias_anchor_target(symbols, &alias_name).map(|s| s.id.clone());

                let mut identifier = self.base.create_identifier(
                    &child,
                    alias_name,
                    IdentifierKind::VariableRef,
                    containing_symbol_id,
                );

                // Set the resolved target if we found the anchor symbol
                if target_symbol_id.is_some() {
                    identifier.target_symbol_id = target_symbol_id.clone();
                    // Also update in the base's identifiers vec
                    if let Some(last) = self.base.identifiers.last_mut() {
                        last.target_symbol_id = target_symbol_id;
                    }
                }

                return;
            }
        }
    }
}

pub(super) fn resolve_alias_anchor_target<'a>(
    symbols: &'a [Symbol],
    alias_name: &str,
) -> Option<&'a Symbol> {
    symbols.iter().find(|symbol| {
        symbol_anchor_name(symbol).is_some_and(|anchor_name| anchor_name == alias_name)
    })
}

fn symbol_anchor_name(symbol: &Symbol) -> Option<&str> {
    symbol
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get("yaml_anchor"))
        .and_then(Value::as_str)
        .or_else(|| {
            symbol
                .signature
                .as_deref()
                .and_then(anchor_name_from_signature)
        })
}

fn anchor_name_from_signature(signature: &str) -> Option<&str> {
    let (_, anchor_tail) = signature.rsplit_once('&')?;
    let anchor_name = anchor_tail.trim();
    if anchor_name.is_empty() {
        return None;
    }

    if anchor_name.chars().all(is_yaml_anchor_char) {
        Some(anchor_name)
    } else {
        None
    }
}

fn is_yaml_anchor_char(ch: char) -> bool {
    !ch.is_whitespace() && !matches!(ch, '[' | ']' | '{' | '}' | ',')
}

/// Attach a single `#` line only when it immediately precedes a leaf mapping key
/// at the same indentation. File headers and container-key section comments stay
/// ordinary comments, not symbol documentation.
fn find_yaml_key_doc_comment(base: &BaseExtractor, node: tree_sitter::Node) -> Option<String> {
    if node.kind() != "block_mapping_pair" || yaml_pair_has_nested_mapping(node) {
        return None;
    }

    let key_column = mapping_key_start_column(base, node)?;
    let mut comments = Vec::new();
    let mut current = node.prev_sibling();

    while let Some(sibling) = current {
        match sibling.kind() {
            "comment" => {
                let text = base.get_node_text(&sibling);
                if !text.trim_start().starts_with('#')
                    || sibling.start_position().column != key_column
                {
                    break;
                }
                comments.push(text);
                current = sibling.prev_sibling();
            }
            "blank_line" => {
                current = sibling.prev_sibling();
            }
            _ => break,
        }
    }

    comments.reverse();
    if comments.len() == 1 {
        Some(comments.into_iter().next().unwrap())
    } else {
        None
    }
}

fn mapping_key_start_column(_base: &BaseExtractor, pair: tree_sitter::Node) -> Option<usize> {
    let mut cursor = pair.walk();
    for child in pair.children(&mut cursor) {
        if !matches!(child.kind(), "flow_node" | "block_node") {
            continue;
        }
        let mut key_cursor = child.walk();
        for key_child in child.children(&mut key_cursor) {
            if matches!(
                key_child.kind(),
                "plain_scalar" | "single_quote_scalar" | "double_quote_scalar"
            ) {
                return Some(key_child.start_position().column);
            }
        }
    }
    None
}

fn yaml_pair_has_nested_mapping(pair: tree_sitter::Node) -> bool {
    let mut cursor = pair.walk();
    for child in pair.children(&mut cursor) {
        if child.kind() != "block_node" {
            continue;
        }
        let mut block_cursor = child.walk();
        for block_child in child.children(&mut block_cursor) {
            if block_child.kind() == "block_mapping" {
                return true;
            }
        }
    }
    false
}
