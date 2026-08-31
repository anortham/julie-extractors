mod declarations;

use crate::base::{BaseExtractor, Identifier, Relationship, Symbol};
use std::collections::HashMap;
use tree_sitter::{Node, Tree};

pub struct FSharpExtractor {
    pub(crate) base: BaseExtractor,
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
        }
    }

    pub fn extract_symbols(&mut self, tree: &Tree) -> Vec<Symbol> {
        declarations::extract_symbols(self, tree.root_node())
    }

    pub fn extract_relationships(
        &mut self,
        _tree: &Tree,
        _symbols: &[Symbol],
    ) -> Vec<Relationship> {
        Vec::new()
    }

    pub fn extract_identifiers(&mut self, _tree: &Tree, _symbols: &[Symbol]) -> Vec<Identifier> {
        Vec::new()
    }

    pub fn infer_types(&self, _symbols: &[Symbol]) -> HashMap<String, String> {
        HashMap::new()
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
