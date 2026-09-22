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
pub(crate) mod ci;
mod cloudformation;
mod relationships;

use crate::base::{
    BaseExtractor, Identifier, IdentifierKind, NormalizedSpan, Relationship, Symbol, SymbolKind,
    TestRole,
};
use crate::test_detection::apply_test_role;
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
            "block_sequence_item" => self.extract_sequence_item_mapping(node, parent_id),

            // "document" and "flow_mapping" are noise — generic names with no
            // search value. Their children are still walked and extracted.
            _ => None,
        }
    }

    /// A mapping inside a block sequence has no key, so it gets a container
    /// symbol named by its item index (`[0]`), like JSON array elements.
    fn extract_sequence_item_mapping(
        &mut self,
        node: tree_sitter::Node,
        parent_id: Option<&str>,
    ) -> Option<Symbol> {
        let value = first_child_of_kind(node, "block_node")?;
        first_child_of_kind(value, "block_mapping")?;
        let sequence = node.parent()?;
        let mut cursor = sequence.walk();
        let index = sequence
            .named_children(&mut cursor)
            .filter(|item| item.kind() == "block_sequence_item")
            .position(|item| item.id() == node.id())?;
        let options = crate::base::SymbolOptions {
            parent_id: parent_id.map(str::to_string),
            ..Default::default()
        };
        let mut symbol =
            self.base
                .create_symbol(&node, format!("[{index}]"), SymbolKind::Module, options);
        let body_span = trimmed_span(&self.base, value);
        self.base.set_body_span(&mut symbol, body_span);
        Some(symbol)
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
        let mut metadata = anchor.as_ref().map(|anchor_name| {
            let mut metadata = HashMap::new();
            metadata.insert(
                "yaml_anchor".to_string(),
                Value::String(anchor_name.clone()),
            );
            metadata
        });
        if let Some(role) = self.test_role_for_mapping_pair(node, &key_name) {
            apply_test_role(metadata.get_or_insert_with(HashMap::new), role);
        }

        let container_value = container_value(node);
        let is_leaf_value = container_value.is_none();
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

        let mut symbol = self
            .base
            .create_symbol(&node, key_name.clone(), kind, options);
        let body_span = container_value.and_then(|value| trimmed_span(&self.base, value));
        self.base.set_body_span(&mut symbol, body_span);

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

    fn test_role_for_mapping_pair(
        &self,
        node: tree_sitter::Node,
        key_name: &str,
    ) -> Option<TestRole> {
        if key_name == "commandTests"
            && self.is_google_command_tests_root(node)
            && mapping_pair_value_is_sequence(node)
        {
            return Some(TestRole::TestContainer);
        }
        if !self.is_direct_command_test_pair(node) {
            return None;
        }

        match key_name {
            "name" if mapping_pair_has_string_value(&self.base, node) => Some(TestRole::TestCase),
            "setup" => Some(TestRole::FixtureSetup),
            "teardown" => Some(TestRole::FixtureTeardown),
            _ => None,
        }
    }

    fn is_google_command_tests_root(&self, node: tree_sitter::Node) -> bool {
        let Some(mapping) = node.parent() else {
            return false;
        };
        if mapping.kind() != "block_mapping" {
            return false;
        }
        let Some(block_node) = mapping.parent() else {
            return false;
        };
        if block_node.kind() != "block_node" {
            return false;
        }
        let Some(document) = block_node.parent() else {
            return false;
        };
        document.kind() == "document" && self.document_has_schema_version(document)
    }

    fn document_has_schema_version(&self, document: tree_sitter::Node) -> bool {
        let Some(block_node) = first_child_of_kind(document, "block_node") else {
            return false;
        };
        let Some(mapping) = first_child_of_kind(block_node, "block_mapping") else {
            return false;
        };
        let mut cursor = mapping.walk();
        mapping.children(&mut cursor).any(|pair| {
            pair.kind() == "block_mapping_pair"
                && self.extract_mapping_key(pair).as_deref() == Some("schemaVersion")
                && mapping_pair_value_text(&self.base, pair).as_deref() == Some("2.0.0")
        })
    }

    fn is_direct_command_test_pair(&self, node: tree_sitter::Node) -> bool {
        let Some(mapping) = node.parent() else {
            return false;
        };
        if mapping.kind() != "block_mapping" {
            return false;
        }
        let Some(item_block) = mapping.parent() else {
            return false;
        };
        if item_block.kind() != "block_node" {
            return false;
        }
        let Some(item) = item_block.parent() else {
            return false;
        };
        if item.kind() != "block_sequence_item" {
            return false;
        }
        let Some(sequence) = item.parent() else {
            return false;
        };
        if sequence.kind() != "block_sequence" {
            return false;
        }
        let Some(command_value) = sequence.parent() else {
            return false;
        };
        if command_value.kind() != "block_node" {
            return false;
        }
        let Some(command_pair) = command_value.parent() else {
            return false;
        };
        command_pair.kind() == "block_mapping_pair"
            && self.extract_mapping_key(command_pair).as_deref() == Some("commandTests")
            && self.is_google_command_tests_root(command_pair)
    }

    pub fn get_type_argument_usages(&self) -> Vec<crate::base::TypeArgumentUsage> {
        self.base.get_type_argument_usages()
    }

    /// Clone captured call-argument literals.
    pub fn get_literals(&self) -> Vec<crate::base::Literal> {
        self.base.get_literals()
    }

    pub fn extract_identifiers(
        &mut self,
        tree: &tree_sitter::Tree,
        symbols: &[Symbol],
    ) -> Vec<Identifier> {
        self.walk_tree_for_aliases(tree.root_node(), symbols);
        cloudformation::extract_identifiers(&mut self.base, tree, symbols);
        ci::extract_identifiers(&mut self.base, tree, symbols);
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
        relationships::extract_relationships(&mut self.base, tree, symbols)
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

/// The contiguous `#` comment lines directly above a mapping key, at the key's
/// column. A blank line, a code line, or a comment at another column ends the block.
fn find_yaml_key_doc_comment(base: &BaseExtractor, node: tree_sitter::Node) -> Option<String> {
    if node.kind() != "block_mapping_pair" {
        return None;
    }
    let key_column = mapping_key_start_column(base, node)?;
    let line_start = node.start_byte() - node.start_position().column;
    doc_comment_block_above(&base.content, line_start, key_column)
}

fn doc_comment_block_above(content: &str, line_start: usize, key_column: usize) -> Option<String> {
    let mut lines = Vec::new();
    for line in content.get(..line_start)?.lines().rev() {
        let indent = line.len() - line.trim_start().len();
        if indent != key_column || !line.trim_start().starts_with('#') {
            break;
        }
        lines.push(line.trim());
    }
    lines.reverse();
    (!lines.is_empty()).then(|| lines.join("\n"))
}

/// True when a whole-line `#` comment is part of the doc block of the mapping
/// key that follows it, by the same rule as [`find_yaml_key_doc_comment`].
pub(crate) fn comment_documents_following_key(content: &str, comment_start: usize) -> bool {
    let line_start = content[..comment_start]
        .rfind('\n')
        .map_or(0, |index| index + 1);
    let column = comment_start - line_start;
    if !content[line_start..comment_start].trim().is_empty() {
        return false;
    }
    for line in content[line_start..].lines().skip(1) {
        let indent = line.len() - line.trim_start().len();
        let text = line.trim_start();
        if indent != column || text.is_empty() {
            return false;
        }
        if !text.starts_with('#') {
            return is_mapping_key_line(text);
        }
    }
    false
}

fn is_mapping_key_line(text: &str) -> bool {
    if text.starts_with(['-', '[', '{', '&', '*', '!', '|', '>']) {
        return false;
    }
    let key_end = match text.chars().next() {
        Some(quote @ ('"' | '\'')) => text[1..].find(quote).map(|index| index + 2),
        _ => text
            .find(": ")
            .or_else(|| text.strip_suffix(':').map(str::len)),
    };
    key_end.is_some_and(|end| text[end..].starts_with(':'))
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

fn first_child_of_kind<'tree>(
    node: tree_sitter::Node<'tree>,
    kind: &str,
) -> Option<tree_sitter::Node<'tree>> {
    let mut cursor = node.walk();
    node.children(&mut cursor)
        .find(|child| child.kind() == kind)
}

fn mapping_pair_value_scalar<'tree>(
    pair: tree_sitter::Node<'tree>,
) -> Option<tree_sitter::Node<'tree>> {
    let value_container = mapping_pair_value_container(pair)?;
    first_scalar_descendant(value_container, 0)
}

fn mapping_pair_value_container<'tree>(
    pair: tree_sitter::Node<'tree>,
) -> Option<tree_sitter::Node<'tree>> {
    let mut value_container = None;
    let mut cursor = pair.walk();
    for child in pair.children(&mut cursor) {
        if !matches!(child.kind(), "flow_node" | "block_node") {
            continue;
        }
        if value_container.is_some() {
            value_container = Some(child);
            break;
        }
        value_container = Some(child);
    }
    value_container
}

fn mapping_pair_value_is_sequence(pair: tree_sitter::Node<'_>) -> bool {
    let Some(value_container) = mapping_pair_value_container(pair) else {
        return false;
    };
    contains_node_kind(value_container, "block_sequence", 0)
        || contains_node_kind(value_container, "flow_sequence", 0)
}

fn contains_node_kind(node: tree_sitter::Node<'_>, kind: &str, depth: u32) -> bool {
    if !should_visit_tree_depth(depth) {
        return false;
    }
    if node.kind() == kind {
        return true;
    }
    let Some(child_depth) = child_tree_depth(depth) else {
        return false;
    };
    let mut cursor = node.walk();
    node.children(&mut cursor)
        .any(|child| contains_node_kind(child, kind, child_depth))
}

fn first_scalar_descendant<'tree>(
    node: tree_sitter::Node<'tree>,
    depth: u32,
) -> Option<tree_sitter::Node<'tree>> {
    if !should_visit_tree_depth(depth) {
        return None;
    }
    if matches!(
        node.kind(),
        "plain_scalar" | "single_quote_scalar" | "double_quote_scalar"
    ) {
        return Some(node);
    }
    let child_depth = child_tree_depth(depth)?;
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if let Some(scalar) = first_scalar_descendant(child, child_depth) {
            return Some(scalar);
        }
    }
    None
}

fn mapping_pair_value_text(base: &BaseExtractor, pair: tree_sitter::Node) -> Option<String> {
    mapping_pair_value_scalar(pair).map(|scalar| {
        base.get_node_text(&scalar)
            .trim()
            .trim_matches(|character| character == '\'' || character == '"')
            .to_string()
    })
}

fn mapping_pair_has_string_value(base: &BaseExtractor, pair: tree_sitter::Node) -> bool {
    let Some(scalar) = mapping_pair_value_scalar(pair) else {
        return false;
    };
    match scalar.kind() {
        "single_quote_scalar" | "double_quote_scalar" => true,
        "plain_scalar" => is_plain_yaml_string(&base.get_node_text(&scalar)),
        _ => false,
    }
}

fn is_plain_yaml_string(value: &str) -> bool {
    let value = value.trim();
    if value.is_empty()
        || matches!(
            value.to_ascii_lowercase().as_str(),
            "null" | "~" | "true" | "false" | "yes" | "no" | "on" | "off"
        )
    {
        return false;
    }
    let normalized = value.replace('_', "");
    normalized.parse::<i64>().is_err() && normalized.parse::<f64>().is_err()
}

/// A block node's span runs over the trailing line break; the body ends at its
/// last non-whitespace byte.
fn trimmed_span(base: &BaseExtractor, node: tree_sitter::Node) -> Option<NormalizedSpan> {
    let text = base.content.get(node.byte_range())?;
    let end = node.start_byte() + text.trim_end().len();
    base.span_for_byte_range(node.start_byte(), end)
}

/// The value node of a mapping pair when it holds a mapping or a sequence.
fn container_value(pair: tree_sitter::Node) -> Option<tree_sitter::Node> {
    let value = pair.child_by_field_name("value")?;
    let mut cursor = value.walk();
    value
        .named_children(&mut cursor)
        .any(|child| {
            matches!(
                child.kind(),
                "block_mapping" | "block_sequence" | "flow_mapping" | "flow_sequence"
            )
        })
        .then_some(value)
}
