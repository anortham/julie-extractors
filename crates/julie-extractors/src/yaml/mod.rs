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
pub(crate) mod ansible;
pub(crate) mod ci;
mod cloudformation;
pub(crate) mod compose;
pub(crate) mod kubernetes;
mod relationships;

/// Domain facts for Docker Compose, Kubernetes, and Ansible documents.
pub(crate) fn domain_facts(
    tree: &tree_sitter::Tree,
    file_path: &str,
    content: &str,
) -> Vec<crate::base::StructuralFact> {
    let mut facts = compose::compose_facts(tree, file_path, content);
    facts.extend(kubernetes::k8s_facts(tree, file_path, content));
    facts.extend(ansible::ansible_facts(tree, file_path, content));
    facts
}

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
            "block_mapping_pair" | "flow_pair" => {
                self.extract_mapping_pair(node, parent_id, symbols)
            }
            "block_sequence_item" => self.extract_sequence_item(node, parent_id),

            // "document" and "flow_mapping" are noise — generic names with no
            // search value. Their children are still walked and extracted.
            _ => None,
        }
    }

    /// A block sequence item has no key. An item that holds a mapping, or one
    /// that carries an anchor, gets a symbol named by its index (`[0]`), like
    /// JSON array elements, so keys and aliases can point at it.
    fn extract_sequence_item(
        &mut self,
        node: tree_sitter::Node,
        parent_id: Option<&str>,
    ) -> Option<Symbol> {
        let value = node.named_child(0)?;
        let is_mapping = has_named_child(value, &["block_mapping", "flow_mapping"]);
        let anchor = value_anchor(&self.base.content, value);
        if !is_mapping && anchor.is_none() {
            return None;
        }
        let sequence = node.parent()?;
        let mut cursor = sequence.walk();
        let index = sequence
            .named_children(&mut cursor)
            .filter(|item| item.kind() == "block_sequence_item")
            .position(|item| item.id() == node.id())?;
        let is_container =
            is_mapping || has_named_child(value, &["block_sequence", "flow_sequence"]);
        let options = crate::base::SymbolOptions {
            parent_id: parent_id.map(str::to_string),
            signature: anchor
                .as_ref()
                .and_then(|_| first_line_signature(&self.base.content, value)),
            metadata: anchor.map(anchor_metadata),
            ..Default::default()
        };
        let kind = if is_container {
            SymbolKind::Module
        } else {
            SymbolKind::Variable
        };
        let mut symbol = self
            .base
            .create_symbol(&node, format!("[{index}]"), kind, options);
        let body_span = is_container
            .then(|| trimmed_span(&self.base, value))
            .flatten();
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

        let anchor = node
            .child_by_field_name("value")
            .and_then(|value| value_anchor(&self.base.content, value));
        let container_value = container_value(node);
        let is_leaf_value = container_value.is_none();
        let signature = (is_leaf_value || anchor.is_some())
            .then(|| first_line_signature(&self.base.content, node))
            .flatten();
        let mut metadata = anchor.map(anchor_metadata);
        if let Some(role) = self.test_role_for_mapping_pair(node, &key_name) {
            apply_test_role(metadata.get_or_insert_with(HashMap::new), role);
        }

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

        let scalar = node.child_by_field_name("value").and_then(|value| {
            let mut cursor = value.walk();
            value
                .named_children(&mut cursor)
                .find(|child| is_scalar_kind(child.kind()))
        });
        if let Some(scalar) = scalar {
            let text = decode_scalar(scalar.kind(), &self.base.get_node_text(&scalar));
            if !text.is_empty() {
                let carrier = crate::base::config_literals::build_config_key_carrier(
                    symbols, parent_id, &key_name,
                );
                self.base
                    .record_literal(&scalar, text, Some(carrier), 0, Some(symbol.id.clone()));
            }
        }

        Some(symbol)
    }

    /// The decoded scalar key of a mapping pair. A complex key (a sequence or
    /// mapping) or an alias key has no name, so the pair makes no symbol.
    fn extract_mapping_key(&self, node: tree_sitter::Node) -> Option<String> {
        mapping_key(&self.base.content, node)
    }

    /// Test roles for container-structure-test v2 documents
    /// (`schemaVersion: 2.0.0`) and Tavern `*.tavern.yaml` tests. Each CST test
    /// list is a test container whose named items are test cases;
    /// `metadataTest` is one test case.
    fn test_role_for_mapping_pair(
        &self,
        node: tree_sitter::Node,
        key_name: &str,
    ) -> Option<TestRole> {
        if self.is_document_root_pair(node) {
            if CST_TEST_LISTS.contains(&key_name)
                && self.is_google_command_tests_root(node)
                && mapping_pair_value_is_sequence(node)
            {
                return Some(TestRole::TestContainer);
            }
            if key_name == "metadataTest" && self.is_google_command_tests_root(node) {
                return Some(TestRole::TestCase);
            }
            if key_name == "test_name"
                && is_tavern_path(&self.base.file_path)
                && mapping_pair_has_string_value(&self.base, node)
            {
                return Some(TestRole::TestCase);
            }
            return None;
        }
        let list = self.cst_list_of_item_pair(node)?;
        match key_name {
            "name" if mapping_pair_has_string_value(&self.base, node) => Some(TestRole::TestCase),
            "setup" if list == "commandTests" => Some(TestRole::FixtureSetup),
            "teardown" if list == "commandTests" => Some(TestRole::FixtureTeardown),
            _ => None,
        }
    }

    fn is_document_root_pair(&self, node: tree_sitter::Node) -> bool {
        node.parent()
            .filter(|mapping| mapping.kind() == "block_mapping")
            .and_then(|mapping| mapping.parent())
            .and_then(|block| block.parent())
            .is_some_and(|document| document.kind() == "document")
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

    /// The CST test list (`commandTests`, ...) whose item mapping holds `node`.
    fn cst_list_of_item_pair(&self, node: tree_sitter::Node) -> Option<String> {
        let mapping = node.parent().filter(|n| n.kind() == "block_mapping")?;
        let item_block = mapping.parent().filter(|n| n.kind() == "block_node")?;
        let item = item_block
            .parent()
            .filter(|n| n.kind() == "block_sequence_item")?;
        let sequence = item.parent().filter(|n| n.kind() == "block_sequence")?;
        let list_value = sequence.parent().filter(|n| n.kind() == "block_node")?;
        let list_pair = list_value
            .parent()
            .filter(|n| n.kind() == "block_mapping_pair")?;
        let list = self.extract_mapping_key(list_pair)?;
        (CST_TEST_LISTS.contains(&list.as_str()) && self.is_google_command_tests_root(list_pair))
            .then_some(list)
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

                let containing_symbol_id = innermost_symbol(symbols, node).map(|s| s.id.clone());
                let target_symbol_id =
                    resolve_alias_anchor_target(symbols, node, &alias_name).map(|s| s.id.clone());

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

/// The anchored symbol an alias names: per YAML, the nearest anchor with that
/// name before the alias in the same document.
pub(super) fn resolve_alias_anchor_target<'a>(
    symbols: &'a [Symbol],
    alias: tree_sitter::Node,
    alias_name: &str,
) -> Option<&'a Symbol> {
    let document = std::iter::successors(alias.parent(), |node| node.parent())
        .find(|node| node.kind() == "document");
    let (start, end) = document.map_or((0, alias.start_byte()), |document| {
        (document.start_byte(), document.end_byte())
    });
    symbols
        .iter()
        .filter(|symbol| {
            let at = symbol.start_byte as usize;
            at >= start && at < end && at < alias.start_byte()
        })
        .filter(|symbol| {
            symbol
                .metadata
                .as_ref()
                .and_then(|metadata| metadata.get("yaml_anchor"))
                .and_then(Value::as_str)
                == Some(alias_name)
        })
        .max_by_key(|symbol| symbol.start_byte)
}

/// The narrowest symbol whose span holds `node`: the key or item that owns it.
pub(super) fn innermost_symbol<'a>(
    symbols: &'a [Symbol],
    node: tree_sitter::Node,
) -> Option<&'a Symbol> {
    let (start, end) = (node.start_byte() as u32, node.end_byte() as u32);
    symbols
        .iter()
        .filter(|symbol| symbol.start_byte <= start && symbol.end_byte >= end)
        .min_by_key(|symbol| symbol.end_byte - symbol.start_byte)
}

const CST_TEST_LISTS: &[&str] = &[
    "commandTests",
    "fileExistenceTests",
    "fileContentTests",
    "licenseTests",
];

fn is_tavern_path(file_path: &str) -> bool {
    file_path.ends_with(".tavern.yaml") || file_path.ends_with(".tavern.yml")
}

fn is_scalar_kind(kind: &str) -> bool {
    matches!(
        kind,
        "plain_scalar" | "double_quote_scalar" | "single_quote_scalar"
    )
}

fn has_named_child(node: tree_sitter::Node, kinds: &[&str]) -> bool {
    let mut cursor = node.walk();
    node.named_children(&mut cursor)
        .any(|child| kinds.contains(&child.kind()))
}

/// The anchor name on a value node (`&name` before a scalar or collection).
pub(crate) fn value_anchor(content: &str, value: tree_sitter::Node) -> Option<String> {
    let mut cursor = value.walk();
    let anchor = value
        .named_children(&mut cursor)
        .find(|child| child.kind() == "anchor")?;
    let mut anchor_cursor = anchor.walk();
    let name = anchor
        .named_children(&mut anchor_cursor)
        .find(|child| child.kind() == "anchor_name")?;
    content.get(name.byte_range()).map(str::to_string)
}

fn anchor_metadata(anchor: String) -> HashMap<String, Value> {
    HashMap::from([("yaml_anchor".to_string(), Value::String(anchor))])
}

/// The first source line of a node, as written, truncated like TOML signatures.
fn first_line_signature(content: &str, node: tree_sitter::Node) -> Option<String> {
    let text = content.get(node.byte_range())?;
    let line = text.lines().next()?.trim_end();
    if line.is_empty() {
        return None;
    }
    if line.chars().count() <= 80 {
        return Some(line.to_string());
    }
    let kept: String = line.chars().take(77).collect();
    Some(format!("{kept}..."))
}

/// The decoded key of a mapping pair, or `None` for a complex or alias key.
pub(crate) fn mapping_key(content: &str, pair: tree_sitter::Node) -> Option<String> {
    let key = pair.child_by_field_name("key")?;
    let mut cursor = key.walk();
    let scalar = key
        .named_children(&mut cursor)
        .find(|child| !matches!(child.kind(), "tag" | "anchor"))
        .filter(|child| is_scalar_kind(child.kind()))?;
    Some(decode_scalar(
        scalar.kind(),
        content.get(scalar.byte_range())?,
    ))
}

/// The value of a flow scalar: quotes removed, escapes decoded, and line
/// breaks folded to spaces.
pub(crate) fn decode_scalar(kind: &str, raw: &str) -> String {
    let raw = raw.trim();
    let text = match kind {
        "double_quote_scalar" => unescape_double_quoted(strip_quotes(raw, '"')),
        "single_quote_scalar" => strip_quotes(raw, '\'').replace("''", "'"),
        _ => raw.to_string(),
    };
    if !text.contains('\n') {
        return text;
    }
    text.split('\n')
        .map(str::trim)
        .collect::<Vec<_>>()
        .join(" ")
}

fn strip_quotes(raw: &str, quote: char) -> &str {
    let inner = raw.strip_prefix(quote).unwrap_or(raw);
    inner.strip_suffix(quote).unwrap_or(inner)
}

fn unescape_double_quoted(inner: &str) -> String {
    let mut out = String::with_capacity(inner.len());
    let mut chars = inner.chars();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            out.push(ch);
            continue;
        }
        let Some(escape) = chars.next() else {
            out.push('\\');
            break;
        };
        let simple = match escape {
            '0' => Some('\0'),
            'a' => Some('\u{7}'),
            'b' => Some('\u{8}'),
            't' | '\t' => Some('\t'),
            'n' => Some('\n'),
            'v' => Some('\u{b}'),
            'f' => Some('\u{c}'),
            'r' => Some('\r'),
            'e' => Some('\u{1b}'),
            ' ' => Some(' '),
            '"' => Some('"'),
            '/' => Some('/'),
            '\\' => Some('\\'),
            'N' => Some('\u{85}'),
            '_' => Some('\u{a0}'),
            'L' => Some('\u{2028}'),
            'P' => Some('\u{2029}'),
            _ => None,
        };
        if let Some(decoded) = simple {
            out.push(decoded);
            continue;
        }
        let width = match escape {
            'x' => 2,
            'u' => 4,
            'U' => 8,
            _ => 0,
        };
        let hex: String = chars.by_ref().take(width).collect();
        match u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32) {
            Some(decoded) if width > 0 => out.push(decoded),
            _ => {
                out.push('\\');
                out.push(escape);
                out.push_str(&hex);
            }
        }
    }
    out
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
