/// TOML extractor - Extract tables and key-value pairs as symbols
///
/// Extracts TOML tables and key-value pairs for semantic search and navigation.
/// - Regular tables: [table_name] -> SymbolKind::Module
/// - Nested tables: [parent.child] -> SymbolKind::Module
/// - Array tables: [[array_table]] -> SymbolKind::Module
/// - Key-value pairs: key = value -> SymbolKind::Property
use crate::base::{
    BaseExtractor, Identifier, NormalizedSpan, PendingRelationship, Relationship,
    StructuredPendingRelationship, Symbol, SymbolKind,
};
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};
use std::collections::HashMap;
use std::path::Path;
use tree_sitter::Tree;

pub(crate) mod dependencies;
mod relationships;
mod test_detection;
pub(crate) mod text;

pub(crate) use text::comment_documents_following_item;

pub struct TomlExtractor {
    pub(crate) base: BaseExtractor,
    config_keys: crate::base::config_literals::ConfigKeyIndex,
}

impl TomlExtractor {
    pub fn new(
        language: String,
        file_path: String,
        source_code: String,
        workspace_root: &Path,
    ) -> Self {
        let base = BaseExtractor::new(language, file_path, source_code, workspace_root);
        Self {
            base,
            config_keys: Default::default(),
        }
    }

    pub fn extract_symbols(&mut self, tree: &tree_sitter::Tree) -> Vec<Symbol> {
        let mut symbols = Vec::new();
        let test_context = test_detection::TomlTestContext::from_tree(
            tree,
            &self.base.content,
            &self.base.file_path,
        );
        self.walk_tree_for_symbols(tree.root_node(), &mut symbols, None, 0, None, &test_context);
        symbols
    }

    /// Walk the tree and extract table symbols
    fn walk_tree_for_symbols(
        &mut self,
        node: tree_sitter::Node,
        symbols: &mut Vec<Symbol>,
        parent_id: Option<String>,
        depth: u32,
        table_name: Option<String>,
        test_context: &test_detection::TomlTestContext,
    ) {
        if !should_visit_tree_depth(depth) {
            return;
        }

        let symbol = self.extract_symbol_from_node(
            node,
            parent_id.as_deref(),
            symbols,
            table_name.as_deref(),
            test_context,
        );
        let mut current_parent_id = parent_id;

        if let Some(ref sym) = symbol {
            symbols.push(sym.clone());
            current_parent_id = Some(sym.id.clone());
        }

        let Some(child_depth) = child_tree_depth(depth) else {
            return;
        };
        let child_table_name = if matches!(node.kind(), "table" | "table_array_element") {
            header_name(node, &self.base.content)
        } else {
            table_name
        };
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.walk_tree_for_symbols(
                child,
                symbols,
                current_parent_id.clone(),
                child_depth,
                child_table_name.clone(),
                test_context,
            );
        }
    }

    /// Extract symbol from a node based on its type
    fn extract_symbol_from_node(
        &mut self,
        node: tree_sitter::Node,
        parent_id: Option<&str>,
        symbols: &[Symbol],
        table_name: Option<&str>,
        test_context: &test_detection::TomlTestContext,
    ) -> Option<Symbol> {
        match node.kind() {
            "table" => self.extract_table(node, parent_id, false, test_context),
            "table_array_element" => self.extract_table(node, parent_id, true, test_context),
            "pair" => self.extract_pair(node, parent_id, symbols, table_name, test_context),
            _ => None,
        }
    }

    /// Extract a table (regular or array) as a symbol
    fn extract_table(
        &mut self,
        node: tree_sitter::Node,
        parent_id: Option<&str>,
        _is_array: bool,
        test_context: &test_detection::TomlTestContext,
    ) -> Option<Symbol> {
        use crate::base::SymbolOptions;

        let table_name = header_name(node, &self.base.content)?;

        let options = SymbolOptions {
            signature: None,
            visibility: None,
            parent_id: parent_id.map(|s| s.to_string()),
            doc_comment: text::leading_comment_doc(&self.base.content, node.start_byte()),
            metadata: test_detection::role_metadata(test_context.table_role(&table_name)),
            ..Default::default()
        };

        let pairs = dependencies::pairs(node);
        let span = self
            .base
            .span_for_byte_range(node.start_byte(), table_end_byte(node))?;
        let mut symbol =
            self.base
                .create_symbol_from_span(&node, span, table_name, SymbolKind::Module, options);
        let body_span = match (pairs.first(), pairs.last()) {
            (Some(first), Some(last)) => self
                .base
                .span_for_byte_range(first.start_byte(), last.end_byte()),
            _ => None,
        };
        self.base.set_body_span(&mut symbol, body_span);

        Some(symbol)
    }

    /// Extract a key-value pair as a Property symbol
    fn extract_pair(
        &mut self,
        node: tree_sitter::Node,
        parent_id: Option<&str>,
        symbols: &[Symbol],
        table_name: Option<&str>,
        test_context: &test_detection::TomlTestContext,
    ) -> Option<Symbol> {
        use crate::base::SymbolOptions;

        let mut cursor = node.walk();
        let children: Vec<_> = node.children(&mut cursor).collect();

        // Need at least key, =, value
        if children.len() < 3 {
            return None;
        }

        let key_node = children[0];
        let key_name = dependencies::pair_key_parts(node, &self.base.content)?.join(".");
        let value_node = pair_value(node)?;
        let value_text = self.base.get_node_text(&value_node);

        // Build signature as "key = value", truncating long values
        let max_sig_len = 80;
        let prefix = format!("{} = ", self.base.get_node_text(&key_node));
        let signature = if prefix.len() + value_text.len() > max_sig_len {
            let available = max_sig_len.saturating_sub(prefix.len() + 3); // 3 for "..."
            // Find a safe char boundary for truncation
            let truncated: String = value_text.chars().take(available).collect();
            format!("{}{}...", prefix, truncated)
        } else {
            format!("{}{}", prefix, value_text)
        };

        let decoded_string = (value_node.kind() == "string")
            .then(|| text::decode_toml_string(&value_text))
            .flatten()
            .map(|(value, _)| value);
        let doc_comment = text::leading_comment_doc(&self.base.content, node.start_byte())
            .or_else(|| decoded_string.clone().filter(|value| !value.is_empty()))
            .map(|doc| doc.chars().take(2000).collect());

        let options = SymbolOptions {
            signature: Some(signature),
            visibility: None,
            parent_id: parent_id.map(|s| s.to_string()),
            doc_comment,
            metadata: test_detection::role_metadata(test_context.pair_role(table_name, &key_name)),
            ..Default::default()
        };

        let mut symbol =
            self.base
                .create_symbol(&node, key_name.clone(), SymbolKind::Property, options);
        self.base
            .set_body_span(&mut symbol, Some(NormalizedSpan::from_node(&value_node)));

        if let Some(literal_text) = decoded_string.filter(|value| !value.is_empty()) {
            let carrier = self.config_keys.carrier(symbols, parent_id, &key_name);
            self.base.record_literal(
                &value_node,
                literal_text,
                Some(carrier),
                0,
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
        // TOML is configuration data - no code identifiers
        Vec::new()
    }

    pub fn infer_types(&self, _symbols: &[Symbol]) -> HashMap<String, String> {
        HashMap::new()
    }

    /// Extract domain-aware relationships (Phase 3.3): Cargo dependency
    /// imports and pyproject tool-table references. Other tables emit
    /// nothing; see `crates/julie-extractors/src/toml/relationships.rs`
    /// for the file-name dispatch contract.
    pub fn extract_relationships(&mut self, tree: &Tree, symbols: &[Symbol]) -> Vec<Relationship> {
        let mut relationships = Vec::new();
        relationships::extract_relationships_internal(
            &mut self.base,
            tree.root_node(),
            symbols,
            &mut relationships,
        );
        relationships
    }

    pub fn get_pending_relationships(&self) -> Vec<PendingRelationship> {
        self.base.get_pending_relationships()
    }

    pub fn get_type_argument_usages(&self) -> Vec<crate::base::TypeArgumentUsage> {
        self.base.get_type_argument_usages()
    }

    /// Clone captured call-argument literals.
    pub fn get_literals(&self) -> Vec<crate::base::Literal> {
        self.base.get_literals()
    }

    pub fn get_structured_pending_relationships(&self) -> Vec<StructuredPendingRelationship> {
        self.base.get_structured_pending_relationships()
    }
}

/// The dotted name of a table header, each segment unquoted:
/// `[project.entry-points."pytest11"]` -> `project.entry-points.pytest11`.
pub(crate) fn header_name(table: tree_sitter::Node, content: &str) -> Option<String> {
    dependencies::header_parts(table, content).map(|parts| parts.join("."))
}

/// The value node of a TOML `pair`: the first named child after the key.
/// A trailing `# comment` is also a named child of the pair, so "last child" is wrong.
pub(crate) fn pair_value(pair: tree_sitter::Node) -> Option<tree_sitter::Node> {
    let mut cursor = pair.walk();
    pair.named_children(&mut cursor)
        .skip(1)
        .find(|child| child.kind() != "comment")
}

/// A table's syntax node runs up to the next header, so it also holds the
/// comments that lead into the next table. A table ends at its last pair, or at
/// its header when it has none.
pub(crate) fn table_end_byte(table: tree_sitter::Node) -> usize {
    let mut cursor = table.walk();
    table
        .children(&mut cursor)
        .filter(|child| child.kind() != "comment")
        .map(|child| child.end_byte())
        .max()
        .unwrap_or(table.end_byte())
}
