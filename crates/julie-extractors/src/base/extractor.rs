// BaseExtractor implementation for Julie
//
// Lines 399-1090 from original base.rs
// Contains the BaseExtractor struct and all its methods

use md5;
use std::collections::HashMap;
use tracing::debug;
use tree_sitter::Node;

use super::relationship_resolution::StructuredPendingRelationship;
use super::span::{NormalizedSpan, normalize_file_path};
use super::string_literals;
use super::type_models::{Literal, LiteralKind, TypeArgument, TypeArgumentUsage};
use super::types::{Identifier, PendingRelationship, Relationship, TypeInfo, stable_location_id};

/// Base implementation for language extractors
///
/// Implementation of BaseExtractor class with all utility methods
pub struct BaseExtractor {
    pub language: String,
    pub file_path: String,
    /// Source text. Set once at construction. Do NOT mutate after `new()`.
    /// Build a fresh extractor per file instead (the production pattern).
    pub content: String,
    line_starts: Vec<usize>,
    pub relationships: Vec<Relationship>,
    pub pending_relationships: Vec<PendingRelationship>,
    pub structured_pending_relationships: Vec<StructuredPendingRelationship>,
    pub type_info: HashMap<String, TypeInfo>,
    pub identifiers: Vec<Identifier>, // NEW: Reference extraction for LSP-quality tools
    /// Ordered/nested generic type arguments captured at use sites, keyed to the
    /// use-site identifier by id. Populated by language readers via
    /// `record_type_arguments`; flattened into the `type_arguments` table.
    pub type_argument_usages: Vec<TypeArgumentUsage>,
    /// String literals captured at call-argument sites (Miller bridge Phase 3).
    /// Populated config-free by language readers via `record_literal`; the
    /// artifact language-policy pass classifies + gates them by carrier before
    /// persistence.
    pub literals: Vec<Literal>,
}

impl BaseExtractor {
    /// Create new abstract extractor - port of constructor
    ///
    /// Accepts a path that is already root-relative and normalized to `/`.
    /// Performs no filesystem canonicalization.
    pub fn new(
        language: String,
        file_path: String,
        content: String,
        workspace_root: &std::path::Path,
    ) -> Self {
        let relative_unix_path = normalize_file_path(&file_path, workspace_root);
        let line_starts = content_line_starts(&content);

        debug!(
            "BaseExtractor path: '{}' -> '{}' (relative)",
            file_path, relative_unix_path
        );

        Self {
            language,
            file_path: relative_unix_path, // Phase 2: Store relative Unix-style path
            content,
            line_starts,
            relationships: Vec::new(),
            pending_relationships: Vec::new(),
            structured_pending_relationships: Vec::new(),
            type_info: HashMap::new(),
            identifiers: Vec::new(), // NEW: Initialize empty identifier list
            type_argument_usages: Vec::new(),
            literals: Vec::new(),
        }
    }

    pub(crate) fn line_starts(&self) -> &[usize] {
        &self.line_starts
    }

    /// Record ordered/nested generic type arguments for a use-site identifier.
    ///
    /// No-op when `arguments` is empty, so non-generic uses (`List` with no
    /// `<...>`) produce zero `type_arguments` rows. `identifier` must be one
    /// that was (or will be) persisted — the `type_arguments.identifier_id` FK
    /// depends on it.
    pub fn record_type_arguments(&mut self, identifier: &Identifier, arguments: Vec<TypeArgument>) {
        if arguments.is_empty() {
            return;
        }
        self.type_argument_usages.push(TypeArgumentUsage {
            identifier_id: identifier.id.clone(),
            file_path: identifier.file_path.clone(),
            language: identifier.language.clone(),
            arguments,
        });
    }

    /// Clone the accumulated type-argument usages (mirrors `get_pending_relationships`).
    pub fn get_type_argument_usages(&self) -> Vec<TypeArgumentUsage> {
        self.type_argument_usages.clone()
    }

    pub(crate) fn take_type_argument_usages(&mut self) -> Vec<TypeArgumentUsage> {
        std::mem::take(&mut self.type_argument_usages)
    }

    /// Record a string literal captured at a call-argument site (Phase 3).
    ///
    /// Config-free: `kind` is always [`LiteralKind::Other`] here; the artifact
    /// language-policy pass reclassifies and gates by `carrier`. `node` is the
    /// string-literal argument node (used for the span and stable id). Returns
    /// the created `Literal` (also pushed to `self.literals`).
    pub fn record_literal(
        &mut self,
        node: &Node,
        literal_text: String,
        carrier: Option<String>,
        arg_position: u32,
        containing_symbol_id: Option<String>,
    ) -> Literal {
        let span = NormalizedSpan::from_node(node);
        let id = self.generate_id_for_span(&literal_text, &span);
        let literal = Literal {
            id,
            literal_text,
            kind: LiteralKind::Other,
            carrier,
            arg_position,
            language: self.language.clone(),
            file_path: self.file_path.clone(),
            start_line: span.start_line,
            start_column: span.start_column,
            end_line: span.end_line,
            end_column: span.end_column,
            start_byte: span.start_byte,
            end_byte: span.end_byte,
            containing_symbol_id,
            confidence: 1.0,
        };
        self.literals.push(literal.clone());
        literal
    }

    pub fn record_literal_at_span(
        &mut self,
        span: NormalizedSpan,
        literal_text: String,
        carrier: Option<String>,
        arg_position: u32,
        containing_symbol_id: Option<String>,
    ) -> Literal {
        let id = self.generate_id_for_span(&literal_text, &span);
        let literal = Literal {
            id,
            literal_text,
            kind: LiteralKind::Other,
            carrier,
            arg_position,
            language: self.language.clone(),
            file_path: self.file_path.clone(),
            start_line: span.start_line,
            start_column: span.start_column,
            end_line: span.end_line,
            end_column: span.end_column,
            start_byte: span.start_byte,
            end_byte: span.end_byte,
            containing_symbol_id,
            confidence: 1.0,
        };
        self.literals.push(literal.clone());
        literal
    }

    /// Clone the accumulated call-argument literals.
    pub fn get_literals(&self) -> Vec<Literal> {
        self.literals.clone()
    }

    pub(crate) fn take_literals(&mut self) -> Vec<Literal> {
        std::mem::take(&mut self.literals)
    }

    /// Decode a string-literal node's contents for capture: strip delimiters and
    /// replace interpolation/substitution holes with `{}` so a resolver sees the
    /// static shape (`/api/users/{}`, `SELECT ... FROM Users`). Returns `None`
    /// for non-string nodes.
    ///
    /// Language-agnostic by design. Recognizes string-bearing nodes by kind
    /// substring (`string`/`char`), then recursively classifies each *named*
    /// descendant precisely:
    /// - an interpolation/substitution **hole** (any kind containing `interpolat`
    ///   or `substitution` — e.g. `interpolation`, `template_substitution`,
    ///   Swift's `interpolated_expression`, Dart's `template_substitution`,
    ///   Kotlin's `interpolated_identifier`) becomes `{}`;
    /// - a content/fragment child (kind contains `content`/`fragment`/`text`/
    ///   `template_chars`, or is `escape_sequence`) is appended verbatim;
    /// - a **wrapper** node with its own named children (e.g. Dart's
    ///   `string_literal_double_quotes`, which nests the real content one level
    ///   below the `string_literal`) is descended into;
    /// - every other (leaf) named child is a **delimiter marker**
    ///   (`raw_string_start`, `interpolation_start`, `interpolation_quote`,
    ///   encoding suffixes, …) and is skipped — so triple-quote and interpolation
    ///   delimiters never leak.
    ///
    /// When that yields nothing (a flat token whose body is anonymous, e.g. TS-less
    /// `string` tokens or C# `verbatim_string_literal`), it falls back to stripping
    /// one matching outer delimiter pair (plus any string prefix) from the raw text.
    pub fn decode_string_literal(&self, node: &Node) -> Option<String> {
        string_literals::decode_string_literal(self, node)
    }

    pub fn add_pending_relationship(&mut self, pending: PendingRelationship) {
        self.pending_relationships.push(pending);
    }

    pub fn add_structured_pending_relationship(&mut self, pending: StructuredPendingRelationship) {
        self.pending_relationships.push(pending.pending.clone());
        self.structured_pending_relationships.push(pending);
    }

    pub fn get_pending_relationships(&self) -> Vec<PendingRelationship> {
        self.pending_relationships.clone()
    }

    pub fn get_structured_pending_relationships(&self) -> Vec<StructuredPendingRelationship> {
        self.structured_pending_relationships.clone()
    }

    pub(crate) fn take_pending_relationships(&mut self) -> Vec<PendingRelationship> {
        std::mem::take(&mut self.pending_relationships)
    }

    pub(crate) fn take_structured_pending_relationships(
        &mut self,
    ) -> Vec<StructuredPendingRelationship> {
        std::mem::take(&mut self.structured_pending_relationships)
    }

    pub fn clear_pending_relationships(&mut self) {
        self.pending_relationships.clear();
        self.structured_pending_relationships.clear();
    }

    /// Get text from a tree-sitter node - exact port of getNodeText
    pub fn get_node_text(&self, node: &Node) -> String {
        let start_byte = node.start_byte();
        let end_byte = node.end_byte();

        // Use byte slice but handle UTF-8 boundaries properly
        let content_bytes = self.content.as_bytes();
        if start_byte < content_bytes.len() && end_byte <= content_bytes.len() {
            String::from_utf8_lossy(&content_bytes[start_byte..end_byte]).to_string()
        } else {
            String::new()
        }
    }

    /// Find documentation comment for a node - exact port of findDocComment
    pub fn find_doc_comment(&self, node: &Node) -> Option<String> {
        // First try to find comments as siblings of this node
        let comments = self.previous_comment_texts(node.prev_named_sibling());
        if let Some(doc_comment) = select_doc_comment_block(&self.language, &comments) {
            return Some(doc_comment);
        }

        // If no comments found as direct siblings, try looking at wrapper ancestors.
        // This handles declarations wrapped by templates, attributes, or language-specific
        // statement nodes without letting container docs bleed onto child members.
        let mut current_node = *node;
        for _ in 0..3 {
            // Try up to 3 ancestor levels
            if let Some(parent) = current_node.parent() {
                if !self.should_search_ancestor_doc_comments(&parent) {
                    break;
                }

                let comments = self.previous_comment_texts(parent.prev_named_sibling());
                if let Some(doc_comment) = select_doc_comment_block(&self.language, &comments) {
                    return Some(doc_comment);
                }
                current_node = parent;
            } else {
                break;
            }
        }

        // For certain nodes (like cte), also check for comments as children (e.g., inside parentheses)
        if node.kind() == "cte" {
            // Look for first comments among direct children
            let mut comments = Vec::new();
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                if is_comment_node(&child) {
                    comments.push(self.get_node_text(&child));
                }
            }
            comments.reverse();
            if let Some(doc_comment) = select_doc_comment_block(&self.language, &comments) {
                return Some(doc_comment);
            }
        }

        None
    }

    fn should_search_ancestor_doc_comments(&self, ancestor: &Node) -> bool {
        if matches!(self.language.as_str(), "dart" | "sql") {
            return true;
        }

        matches!(
            ancestor.kind(),
            "attributed_declarator"
                | "attributed_statement"
                | "declaration"
                | "field_declaration"
                | "function_declarator"
                | "package_clause"
                | "template_declaration"
        )
    }

    pub(crate) fn previous_comment_texts<'a>(&self, mut current: Option<Node<'a>>) -> Vec<String> {
        let mut comments = Vec::new();

        while let Some(sibling) = current {
            if is_comment_node(&sibling) {
                comments.push(self.get_node_text(&sibling));
                current = sibling.prev_named_sibling();
            } else {
                break;
            }
        }

        comments
    }

    /// Generate ID for a symbol - exact port of generateId (MD5 hash)
    pub fn generate_id(&self, name: &str, line: u32, column: u32) -> String {
        let input = format!("{}:{}:{}:{}", self.file_path, name, line, column);
        let digest = md5::compute(input.as_bytes());
        format!("{:x}", digest)
    }

    pub fn generate_id_for_span(&self, name: &str, span: &NormalizedSpan) -> String {
        stable_location_id(self.file_path.as_str(), name, *span)
    }

    pub fn generate_id_for_node(&self, name: &str, node: &Node) -> String {
        self.generate_id_for_span(name, &NormalizedSpan::from_node(node))
    }

    /// Safely truncate a string to a maximum number of characters (not bytes)
    /// This handles UTF-8 multi-byte characters correctly by truncating at character boundaries
    pub fn truncate_string(text: &str, max_chars: usize) -> String {
        let char_count = text.chars().count();
        if char_count <= max_chars {
            text.to_string()
        } else {
            text.chars().take(max_chars).collect::<String>() + "..."
        }
    }
}

fn is_comment_node(node: &Node) -> bool {
    node.kind().contains("comment") || node.kind() == "marginalia"
}

pub(crate) fn select_doc_comment_block(
    language: &str,
    comments_nearest_first: &[String],
) -> Option<String> {
    let spec = crate::language::language_spec(language)?;
    if comments_nearest_first.is_empty() {
        return None;
    }

    let comments_top_down = comments_nearest_first.iter().rev().collect::<Vec<_>>();
    for start_index in 0..comments_top_down.len() {
        let first = comments_top_down[start_index];
        if !spec.is_doc_comment(first) {
            continue;
        }

        if comments_top_down[start_index + 1..]
            .iter()
            .all(|comment| spec.continues_doc_comment(comment))
        {
            return Some(
                comments_top_down[start_index..]
                    .iter()
                    .map(|comment| comment.as_str())
                    .collect::<Vec<_>>()
                    .join("\n"),
            );
        }
    }

    None
}

fn content_line_starts(content: &str) -> Vec<usize> {
    let mut starts = vec![0];
    for (index, byte) in content.as_bytes().iter().enumerate() {
        if *byte == b'\n' {
            starts.push(index + 1);
        }
    }
    starts
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn test_base_extractor_new_operates_without_filesystem_access_on_nonexistent_path() {
        let nonexistent_root = Path::new("/definitely/nonexistent/workspace/root");
        let nonexistent_file = "nonexistent/deeply/nested/file.rs";
        let extractor = BaseExtractor::new(
            "rust".to_string(),
            nonexistent_file.to_string(),
            "fn dummy() {}".to_string(),
            nonexistent_root,
        );

        assert_eq!(extractor.file_path, nonexistent_file);
    }
}
