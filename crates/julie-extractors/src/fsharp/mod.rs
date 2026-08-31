mod declarations;
mod identifiers;
mod literals;
mod relationships;
mod types;

use crate::base::{BaseExtractor, Identifier, Relationship, Symbol};
use std::collections::HashMap;
use tree_sitter::{Node, Tree};

pub struct FSharpExtractor {
    pub(crate) base: BaseExtractor,
    inferred_types: HashMap<String, String>,
}

impl FSharpExtractor {
    pub fn new(
        language: String,
        file_path: String,
        content: String,
        workspace_root: &std::path::Path,
    ) -> Self {
        Self {
            base: BaseExtractor::new(language, file_path, content, workspace_root),
            inferred_types: HashMap::new(),
        }
    }

    pub fn extract_symbols(&mut self, tree: &Tree) -> Vec<Symbol> {
        let symbols = declarations::extract_symbols(self, tree.root_node());
        self.inferred_types = types::collect_types(self, tree.root_node(), &symbols);
        symbols
    }

    pub fn extract_relationships(&mut self, tree: &Tree, symbols: &[Symbol]) -> Vec<Relationship> {
        relationships::extract_relationships(self, tree, symbols)
    }

    pub fn extract_identifiers(&mut self, tree: &Tree, symbols: &[Symbol]) -> Vec<Identifier> {
        identifiers::extract_identifiers(self, tree, symbols)
    }

    pub fn infer_types(&self, _symbols: &[Symbol]) -> HashMap<String, String> {
        self.inferred_types.clone()
    }

    pub(crate) fn visit_node(
        &mut self,
        node: Node,
        symbols: &mut Vec<Symbol>,
        parent_id: Option<String>,
        depth: u32,
    ) {
        declarations::visit_node(self, node, symbols, parent_id, depth);
    }

    pub(crate) fn base(&mut self) -> &mut BaseExtractor {
        &mut self.base
    }
}
