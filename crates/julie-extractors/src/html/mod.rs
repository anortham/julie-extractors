// HTML Extractor
//
// Implementation of HTML extractor to idiomatic Rust

use crate::base::relationship_resolution::StructuredPendingRelationship;
use crate::base::{BaseExtractor, ExtractionResults, Identifier, Relationship, Symbol};
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};
use std::collections::HashMap;
use tree_sitter::{Node, Tree};

// Private modules
mod attributes;
mod body_hash;
mod elements;
mod fallback;
mod handlers;
mod helpers;
mod identifiers;
mod relationships;
mod scripts;
mod templates;
mod types;

pub struct HTMLExtractor {
    pub(crate) base: BaseExtractor,
    mocha_bdd_contract: bool,
    embedded: Vec<ExtractionResults>,
    embedded_blocks: body_hash::EmbeddedBlocks,
    handler_rows: handlers::HandlerRows,
}

impl HTMLExtractor {
    pub fn new(
        language: String,
        file_path: String,
        content: String,
        workspace_root: &std::path::Path,
    ) -> Self {
        Self {
            base: BaseExtractor::new(language, file_path, content, workspace_root),
            mocha_bdd_contract: false,
            embedded: Vec::new(),
            embedded_blocks: body_hash::EmbeddedBlocks::default(),
            handler_rows: handlers::HandlerRows::default(),
        }
    }

    /// Rows other than symbols from inline `<script>` and `<style>` blocks,
    /// collected by [`Self::extract_symbols`], in host coordinates.
    pub(crate) fn take_embedded_results(&mut self) -> Vec<ExtractionResults> {
        std::mem::take(&mut self.embedded)
    }

    pub fn extract_symbols(&mut self, tree: &Tree) -> Vec<Symbol> {
        self.embedded.clear();
        let root_node = tree.root_node();
        self.embedded_blocks = body_hash::EmbeddedBlocks::prepare(&self.base, root_node);
        self.base
            .init_body_hash_syntax(self.embedded_blocks.take_syntax());
        self.mocha_bdd_contract = self.embedded_blocks.mocha_bdd_contract();
        let mut symbols = Vec::new();

        // Check if tree is valid and has a root node - start from actual root standard format
        if root_node.child_count() > 0 {
            self.visit_node(root_node, &mut symbols, None, 0);
        } else {
            self.embedded_blocks.release_trees();
            // Fallback extraction when normal parsing fails
            return fallback::FallbackExtractor::extract_basic_structure(&mut self.base, tree);
        }
        self.embedded_blocks.release_trees();

        // If we only extracted error symbols, try basic structure fallback
        let has_only_errors = !symbols.is_empty()
            && symbols.iter().all(|s| {
                s.metadata
                    .as_ref()
                    .and_then(|m| m.get("isError"))
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false)
            });

        if has_only_errors || symbols.is_empty() {
            fallback::FallbackExtractor::extract_basic_structure(&mut self.base, tree)
        } else {
            templates::add_template_symbols(&mut self.base, root_node, &mut symbols);
            self.handler_rows = handlers::collect_handler_rows(&self.base, tree, &symbols);
            symbols
        }
    }

    fn visit_node(
        &mut self,
        node: Node,
        symbols: &mut Vec<Symbol>,
        parent_id: Option<&str>,
        depth: u32,
    ) {
        if !should_visit_tree_depth(depth) {
            return;
        }

        let node_symbols = self.extract_node_symbols(node, parent_id);
        if !node_symbols.is_empty() {
            let symbol_id = node_symbols.first().map(|symbol| symbol.id.clone());
            symbols.extend(node_symbols);

            // Recursively visit children with the new symbol as parent
            let Some(child_depth) = child_tree_depth(depth) else {
                return;
            };
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                self.visit_node(child, symbols, symbol_id.as_deref(), child_depth);
            }
        } else {
            // If no symbol was extracted, continue with children using current parent
            let Some(child_depth) = child_tree_depth(depth) else {
                return;
            };
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                self.visit_node(child, symbols, parent_id, child_depth);
            }
        }
    }

    fn extract_node_symbols(&mut self, node: Node, parent_id: Option<&str>) -> Vec<Symbol> {
        match node.kind() {
            "element" => {
                elements::ElementExtractor::extract_element(&mut self.base, node, parent_id)
                    .into_iter()
                    .collect()
            }
            "script_element" => scripts::ScriptStyleExtractor::extract_script_element(
                &mut self.base,
                node,
                parent_id,
                self.mocha_bdd_contract,
                &mut self.embedded,
                &mut self.embedded_blocks,
            ),
            "style_element" => scripts::ScriptStyleExtractor::extract_style_element(
                &mut self.base,
                node,
                parent_id,
                &mut self.embedded,
                &mut self.embedded_blocks,
            ),
            "doctype" if node.is_named() => vec![elements::ElementExtractor::extract_doctype(
                &mut self.base,
                node,
                parent_id,
            )],
            "comment" => Vec::new(),
            _ => Vec::new(),
        }
    }

    pub fn extract_relationships(
        &mut self,
        _tree: &Tree,
        _symbols: &[Symbol],
    ) -> Vec<Relationship> {
        std::mem::take(&mut self.handler_rows.relationships)
    }

    /// Phase 4b.html — emit StructuredPendingRelationship for external
    /// `<script src=...>` and `<link href=...>` references. The owning
    /// caller scope is the nearest parent element symbol if any; otherwise
    /// the document/root symbol.
    pub fn extract_structured_pending_relationships(
        &mut self,
        tree: &Tree,
        symbols: &[Symbol],
    ) -> Vec<StructuredPendingRelationship> {
        let mut pending = Vec::new();
        relationships::RelationshipExtractor::collect_structured_pending(
            &self.base,
            tree.root_node(),
            symbols,
            &mut pending,
        );
        pending.extend(templates::template_imports(&self.base, symbols));
        pending.append(&mut self.handler_rows.pending);
        pending
    }

    pub fn infer_types(&self, symbols: &[Symbol]) -> HashMap<String, String> {
        types::HTMLTypes::infer_types(symbols)
    }

    /// Extract all identifier usages (event handlers, id/class references)
    /// Following the Rust extractor reference implementation pattern
    pub fn get_type_argument_usages(&self) -> Vec<crate::base::TypeArgumentUsage> {
        self.base.get_type_argument_usages()
    }

    /// Clone captured call-argument literals.
    pub fn get_literals(&self) -> Vec<crate::base::Literal> {
        self.base.get_literals()
    }

    pub fn extract_identifiers(&mut self, tree: &Tree, symbols: &[Symbol]) -> Vec<Identifier> {
        let file_path = self.base.file_path.clone();
        let containing_symbols = crate::base::ContainingSymbolIndex::innermost(
            symbols
                .iter()
                .filter(|symbol| symbol.file_path == file_path),
        );

        // Walk the tree and extract identifiers
        identifiers::IdentifierExtractor::extract_identifiers(
            &mut self.base,
            tree.root_node(),
            &containing_symbols,
        );
        self.base
            .identifiers
            .append(&mut self.handler_rows.identifiers);
        self.base.literals.append(&mut self.handler_rows.literals);

        self.base.identifiers.clone()
    }
}
