/// JSON extractor - Extract keys and objects as symbols
///
/// Extracts JSON key-value pairs as symbols for semantic search and navigation.
/// - Top-level keys and nested object keys are extracted
/// - Objects and arrays are treated as SymbolKind::Module (containers)
/// - Primitive values are treated as SymbolKind::Variable
use crate::base::{
    BaseExtractor, Identifier, NormalizedSpan, PendingRelationship, Relationship,
    StructuredPendingRelationship, Symbol, SymbolKind,
};
use crate::test_detection::apply_test_role;
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};
use std::collections::HashMap;
use std::path::Path;
use tree_sitter::Tree;

mod comments;
pub(crate) mod manifest;
pub(crate) mod relationships;
mod test_detection;

pub(crate) use comments::comment_documents_following_value;

const MAX_DOC_CHARS: usize = 2000;
const MAX_SIGNATURE_CHARS: usize = 80;

pub struct JsonExtractor {
    pub(crate) base: BaseExtractor,
    config_keys: crate::base::config_literals::ConfigKeyIndex,
}

impl JsonExtractor {
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
            "object" => self.extract_array_element_object(node, parent_id),
            _ => None,
        }
    }

    /// An object inside an array has no key, so it gets a container symbol named
    /// by its element index (`[0]`), matching the `$.items[0]` fact paths.
    fn extract_array_element_object(
        &mut self,
        node: tree_sitter::Node,
        parent_id: Option<&str>,
    ) -> Option<Symbol> {
        let array = node.parent().filter(|parent| parent.kind() == "array")?;
        let index = array_element_index(array, node)?;
        let options = crate::base::SymbolOptions {
            parent_id: parent_id.map(str::to_string),
            doc_comment: self.value_doc(node, node),
            ..Default::default()
        };
        let mut symbol =
            self.base
                .create_symbol(&node, format!("[{index}]"), SymbolKind::Module, options);
        self.base
            .set_body_span(&mut symbol, Some(NormalizedSpan::from_node(&node)));
        Some(symbol)
    }

    /// Extract a key-value pair as a symbol
    fn extract_pair(
        &mut self,
        node: tree_sitter::Node,
        parent_id: Option<&str>,
        symbols: &[Symbol],
    ) -> Option<Symbol> {
        use crate::base::SymbolOptions;

        let mut cursor = node.walk();
        let children: Vec<_> = node.children(&mut cursor).collect();
        if children.len() < 3 {
            return None;
        }

        let key_node = children[0];
        let key_name = decode_json_string(&self.base.get_node_text(&key_node));
        let value_node = *children.last().unwrap();

        let symbol_kind = match value_node.kind() {
            "object" | "array" => SymbolKind::Module,
            _ => SymbolKind::Variable,
        };

        let options = SymbolOptions {
            signature: scalar_signature(&self.base, key_node, value_node),
            visibility: None,
            parent_id: parent_id.map(|s| s.to_string()),
            doc_comment: self.value_doc(node, value_node),
            ..Default::default()
        };

        let mut symbol = self
            .base
            .create_symbol(&node, key_name.clone(), symbol_kind, options);
        let body_span = matches!(value_node.kind(), "object" | "array")
            .then(|| NormalizedSpan::from_node(&value_node));
        self.base.set_body_span(&mut symbol, body_span);

        if let Some(role) = test_detection::role_for_description_pair(&self.base, node, &key_name) {
            apply_test_role(symbol.metadata.get_or_insert_with(HashMap::new), role);
        }

        if value_node.kind() == "string" {
            let literal_text = decode_json_string(&self.base.get_node_text(&value_node));
            if !literal_text.is_empty() {
                let carrier = self.config_keys.carrier(symbols, parent_id, &key_name);
                self.base.record_literal(
                    &value_node,
                    literal_text,
                    Some(carrier),
                    0,
                    Some(symbol.id.clone()),
                );
            }
        }

        Some(symbol)
    }

    /// The doc of a pair or array element: a JSONC comment that documents it,
    /// else the `description`/`summary`/`title` of an object value, else the
    /// decoded text of a string value (truncated for semantic search).
    fn value_doc(&self, holder: tree_sitter::Node, value: tree_sitter::Node) -> Option<String> {
        let content = &self.base.content;
        let text = comments::leading_doc(content, holder)
            .or_else(|| comments::trailing_doc(content, holder))
            .or_else(|| match value.kind() {
                "object" => schema_description(content, value),
                "string" => Some(decode_json_string(&self.base.get_node_text(&value))),
                _ => None,
            })?;
        (!text.is_empty()).then(|| text.chars().take(MAX_DOC_CHARS).collect())
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

    /// Extract JSON Schema `$ref` relationships and package-manifest edges.
    /// Local pointers resolve to concrete `Relationship`s; references into
    /// other documents emit structured pending relationships.
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

/// Index of `element` among the value elements of `array`, skipping comments.
pub(crate) fn array_element_index(
    array: tree_sitter::Node,
    element: tree_sitter::Node,
) -> Option<usize> {
    let mut cursor = array.walk();
    array
        .named_children(&mut cursor)
        .filter(|child| child.kind() != "comment")
        .position(|child| child.id() == element.id())
}

/// The value of a JSON string token: escapes decoded, quotes removed. Text that
/// is not a valid JSON string (a parse-error fragment) only loses its quotes.
pub(crate) fn decode_json_string(raw: &str) -> String {
    let raw = raw.trim();
    serde_json::from_str::<String>(raw).unwrap_or_else(|_| {
        let inner = raw.strip_prefix('"').unwrap_or(raw);
        inner.strip_suffix('"').unwrap_or(inner).to_string()
    })
}

/// `"key": value` for a scalar pair, as written, truncated like TOML signatures.
fn scalar_signature(
    base: &BaseExtractor,
    key: tree_sitter::Node,
    value: tree_sitter::Node,
) -> Option<String> {
    if matches!(value.kind(), "object" | "array") {
        return None;
    }
    let signature = format!(
        "{}: {}",
        base.get_node_text(&key),
        base.get_node_text(&value)
    );
    if signature.chars().count() <= MAX_SIGNATURE_CHARS {
        return Some(signature);
    }
    let kept: String = signature.chars().take(MAX_SIGNATURE_CHARS - 3).collect();
    Some(format!("{kept}..."))
}

/// The first non-empty `description`, `summary`, or `title` string held
/// directly by a JSON Schema or OpenAPI object.
fn schema_description(content: &str, object: tree_sitter::Node) -> Option<String> {
    ["description", "summary", "title"]
        .iter()
        .find_map(|keyword| {
            let mut cursor = object.walk();
            object
                .named_children(&mut cursor)
                .filter(|child| child.kind() == "pair")
                .find_map(|pair| {
                    let key = pair.child_by_field_name("key")?;
                    let value = pair.child_by_field_name("value")?;
                    let key_text = content.get(key.start_byte()..key.end_byte())?;
                    if value.kind() != "string" || decode_json_string(key_text) != *keyword {
                        return None;
                    }
                    let text =
                        decode_json_string(content.get(value.start_byte()..value.end_byte())?);
                    (!text.is_empty()).then_some(text)
                })
        })
}
