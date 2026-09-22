/// Markdown extractor - Extract sections as symbols for documentation embedding
///
/// This extractor treats markdown sections (headings) as symbols, enabling:
/// 1. Semantic search across documentation
/// 2. goto definition for heading navigation
/// 3. Knowledge graph connections between code and docs
pub(crate) mod inline;
mod relationships;
mod semantic_symbols;

use crate::base::{BaseExtractor, Identifier, Relationship, Symbol, SymbolKind, SymbolOptions};
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};
use serde_json::json;
use std::collections::HashMap;
use std::path::Path;
use tree_sitter::Tree;

pub struct MarkdownExtractor {
    pub(crate) base: BaseExtractor,
}

impl MarkdownExtractor {
    pub fn new(
        language: String,
        file_path: String,
        source_code: String,
        workspace_root: &Path,
    ) -> Self {
        let base = BaseExtractor::new(language, file_path, source_code, workspace_root);
        Self { base }
    }

    pub fn extract_symbols(&mut self, tree: &Tree) -> Vec<Symbol> {
        let blocks = outline_blocks(tree.root_node());
        let mut symbols = self.extract_headings(&blocks, tree.root_node().end_byte());
        let headings = symbols.clone();
        self.walk_tree_for_symbols(tree.root_node(), &blocks, &headings, &mut symbols, 0);
        let inline_symbols =
            semantic_symbols::extract_inline_symbols(&mut self.base, tree, &symbols);
        symbols.extend(inline_symbols);
        symbols
    }

    /// One Module symbol per ATX or setext heading. The block grammar nests
    /// `section` nodes by ATX level only and never opens one for a setext
    /// heading, so the outline is rebuilt here: a heading's span runs to the
    /// next heading of the same or a higher level, its parent is the nearest
    /// earlier heading of a lower level, and its doc comment is the content
    /// before the next heading of any level.
    fn extract_headings(
        &mut self,
        blocks: &[tree_sitter::Node],
        document_end: usize,
    ) -> Vec<Symbol> {
        let mut symbols: Vec<Symbol> = Vec::new();
        let mut open: Vec<(usize, String)> = Vec::new();
        for (index, heading) in blocks.iter().enumerate() {
            if !is_heading(heading.kind()) {
                continue;
            }
            let level = self.determine_heading_level(*heading);
            let later = &blocks[index + 1..];
            let end = later
                .iter()
                .find(|block| {
                    is_heading(block.kind()) && self.determine_heading_level(**block) <= level
                })
                .map_or(document_end, |block| block.start_byte());
            let content =
                self.content_text(later.iter().take_while(|block| !is_heading(block.kind())));
            while open
                .last()
                .is_some_and(|(open_level, _)| *open_level >= level)
            {
                open.pop();
            }
            let parent_id = open.last().map(|(_, id)| id.clone());
            let Some(mut symbol) =
                self.extract_heading(*heading, parent_id.as_deref(), Some(content))
            else {
                continue;
            };
            if let Some(span) = self.base.span_for_byte_range(heading.start_byte(), end) {
                symbol.start_line = span.start_line;
                symbol.start_column = span.start_column;
                symbol.end_line = span.end_line;
                symbol.end_column = span.end_column;
                symbol.start_byte = span.start_byte;
                symbol.end_byte = span.end_byte;
            }
            open.push((level, symbol.id.clone()));
            symbols.push(symbol);
        }
        symbols
    }

    fn walk_tree_for_symbols(
        &mut self,
        node: tree_sitter::Node,
        blocks: &[tree_sitter::Node],
        headings: &[Symbol],
        symbols: &mut Vec<Symbol>,
        depth: u32,
    ) {
        if !should_visit_tree_depth(depth) {
            return;
        }
        let symbol = match node.kind() {
            "fenced_code_block" | "link_reference_definition" => {
                let parent_id = containing_heading(headings, node.start_byte() as u32)
                    .map(|heading| heading.id.clone());
                semantic_symbols::extract_symbol_from_node(
                    &mut self.base,
                    node,
                    parent_id.as_deref(),
                )
            }
            "minus_metadata" | "plus_metadata" => self.extract_frontmatter(node, blocks),
            _ => None,
        };
        symbols.extend(symbol);

        let Some(child_depth) = child_tree_depth(depth) else {
            return;
        };
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.walk_tree_for_symbols(child, blocks, headings, symbols, child_depth);
        }
    }

    fn content_text<'a>(&self, blocks: impl Iterator<Item = &'a tree_sitter::Node<'a>>) -> String {
        blocks
            .filter(|block| self.is_content_node(block))
            .map(|block| self.base.get_node_text(block).trim_end().to_string())
            .collect::<Vec<_>>()
            .join("\n\n")
    }

    /// Extract frontmatter (YAML or TOML) as a symbol
    ///
    /// Frontmatter contains document metadata like title, author, tags, etc.
    /// Also captures body content following frontmatter (before any heading)
    /// for semantic search of memory files, blog posts, etc.
    ///
    /// This is valuable for:
    /// 1. Semantic search (find docs by metadata AND content)
    /// 2. Documentation organization
    /// 3. Blog/static site content discovery
    /// 4. Development memory checkpoint search
    fn extract_frontmatter(
        &mut self,
        node: tree_sitter::Node,
        blocks: &[tree_sitter::Node],
    ) -> Option<Symbol> {
        let raw_text = self.base.get_node_text(&node);

        // Strip the delimiters (--- or +++) from start and end
        let frontmatter_content = self.strip_frontmatter_delimiters(&raw_text);

        // Skip empty frontmatter
        if frontmatter_content.trim().is_empty() {
            return None;
        }

        let body_content = self.content_text(
            blocks
                .iter()
                .skip_while(|block| block.id() != node.id())
                .skip(1)
                .take_while(|block| !is_heading(block.kind())),
        );

        // Combine frontmatter and body content for rich semantic search
        let doc_comment = if body_content.is_empty() {
            frontmatter_content
        } else {
            format!("{}\n\n---\n\n{}", frontmatter_content, body_content)
        };

        let options = SymbolOptions {
            signature: None,
            visibility: None,
            parent_id: None, // Frontmatter is always top-level
            doc_comment: Some(doc_comment),
            ..Default::default()
        };

        let mut symbol = self.base.create_symbol(
            &node,
            "frontmatter".to_string(),
            SymbolKind::Property, // Metadata property
            options,
        );
        self.base.set_body_span(&mut symbol, None);

        Some(symbol)
    }

    /// Strip frontmatter delimiters (--- or +++) from raw text
    fn strip_frontmatter_delimiters(&self, text: &str) -> String {
        let lines: Vec<&str> = text.lines().collect();

        if lines.len() < 2 {
            return String::new();
        }

        // Skip first line (opening delimiter) and last line (closing delimiter)
        // The closing delimiter might be on the last line or second-to-last if there's a trailing newline
        let start = 1;
        let end = if lines.last().map(|l| l.trim()).unwrap_or("") == "---"
            || lines.last().map(|l| l.trim()).unwrap_or("") == "+++"
        {
            lines.len() - 1
        } else if lines.len() > 2
            && (lines[lines.len() - 2].trim() == "---" || lines[lines.len() - 2].trim() == "+++")
        {
            lines.len() - 2
        } else {
            lines.len()
        };

        lines[start..end].join("\n")
    }

    /// Check if a node contains content that should be included in section body
    /// This captures all markdown content types for comprehensive RAG embeddings
    fn is_content_node(&self, node: &tree_sitter::Node) -> bool {
        matches!(
            node.kind(),
            "paragraph"
                | "list"              // Unordered/ordered lists
                | "list_item"
                | "fenced_code_block" // ```code blocks```
                | "indented_code_block"
                | "block_quote"       // > quotes
                | "table"             // Tables
                | "thematic_break"    // ---
                | "html_block" // Raw HTML
        )
    }

    /// Extract heading text and create symbol
    fn extract_heading(
        &mut self,
        node: tree_sitter::Node,
        parent_id: Option<&str>,
        section_content: Option<String>,
    ) -> Option<Symbol> {
        // Extract the heading text (skip the # markers)
        let heading_text = self.extract_heading_text(node)?;

        let level = self.determine_heading_level(node);
        let mut metadata = HashMap::new();
        metadata.insert("markdown_kind".to_string(), json!("heading"));
        metadata.insert("heading_level".to_string(), json!(level));

        // Include section content as doc_comment for RAG embedding
        let doc_comment = section_content.filter(|s| !s.is_empty());

        let options = SymbolOptions {
            signature: None, // No signature for headings
            visibility: None,
            parent_id: parent_id.map(|s| s.to_string()),
            doc_comment,
            metadata: Some(metadata),
            ..Default::default()
        };

        let mut symbol = self.base.create_symbol(
            &node,
            heading_text,
            SymbolKind::Module, // Treat sections as modules for semantic grouping
            options,
        );
        self.base.set_body_span(&mut symbol, None);

        Some(symbol)
    }

    /// Extract the text content of a heading (without # markers)
    fn extract_heading_text(&self, node: tree_sitter::Node) -> Option<String> {
        if node.kind() == "setext_heading" {
            let content = node.child_by_field_name("heading_content")?;
            let text = self.base.get_node_text(&content);
            return Some(text.split_whitespace().collect::<Vec<_>>().join(" "));
        }
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            // Look for inline content or heading_content
            if child.kind() == "inline" || child.kind() == "heading_content" {
                let text = self.base.get_node_text(&child);
                return Some(text);
            }
        }

        // Fallback: get entire node text and strip # markers
        let text = self.base.get_node_text(&node);
        Some(strip_atx_heading_marker(&text))
    }

    /// Determine heading level from number of # markers
    fn determine_heading_level(&self, node: tree_sitter::Node) -> usize {
        if let Some(level) = setext_level(node) {
            return level;
        }
        let text = self.base.get_node_text(&node);

        // Count leading # characters
        text.chars().take_while(|&c| c == '#').count().clamp(1, 6)
    }

    pub fn get_type_argument_usages(&self) -> Vec<crate::base::TypeArgumentUsage> {
        self.base.get_type_argument_usages()
    }

    /// Clone captured call-argument literals.
    pub fn get_literals(&self) -> Vec<crate::base::Literal> {
        self.base.get_literals()
    }

    pub fn extract_identifiers(&mut self, _tree: &Tree, _symbols: &[Symbol]) -> Vec<Identifier> {
        // Markdown is documentation - no code identifiers
        Vec::new()
    }

    pub fn infer_types(&self, _symbols: &[Symbol]) -> HashMap<String, String> {
        HashMap::new()
    }

    pub fn extract_relationships(&mut self, _tree: &Tree, symbols: &[Symbol]) -> Vec<Relationship> {
        relationships::extract_relationships(&self.base, symbols)
    }
}

/// The children of the document and of every nested `section`, in document
/// order, with the sections themselves flattened away.
fn outline_blocks(root: tree_sitter::Node) -> Vec<tree_sitter::Node> {
    fn collect<'tree>(
        container: tree_sitter::Node<'tree>,
        blocks: &mut Vec<tree_sitter::Node<'tree>>,
        depth: u32,
    ) {
        let Some(child_depth) = crate::tree_traversal::child_tree_depth(depth) else {
            return;
        };
        let mut cursor = container.walk();
        for child in container.children(&mut cursor) {
            if child.kind() == "section" {
                collect(child, blocks, child_depth);
            } else {
                blocks.push(child);
            }
        }
    }
    let mut blocks = Vec::new();
    collect(root, &mut blocks, 0);
    blocks
}

/// The innermost heading whose section holds `byte`.
fn containing_heading(symbols: &[Symbol], byte: u32) -> Option<&Symbol> {
    symbols
        .iter()
        .filter(|symbol| {
            symbol.kind == SymbolKind::Module && symbol.start_byte <= byte && byte < symbol.end_byte
        })
        .min_by_key(|symbol| symbol.end_byte - symbol.start_byte)
}

fn is_heading(kind: &str) -> bool {
    matches!(kind, "atx_heading" | "setext_heading" | "heading")
}

/// `===` underlines make level 1 and `---` underlines level 2.
pub(crate) fn setext_level(node: tree_sitter::Node) -> Option<usize> {
    let mut cursor = node.walk();
    node.children(&mut cursor)
        .find_map(|child| match child.kind() {
            "setext_h1_underline" => Some(1),
            "setext_h2_underline" => Some(2),
            _ => None,
        })
}

fn strip_atx_heading_marker(raw: &str) -> String {
    let trimmed = raw.trim_start();
    let marker_len = trimmed.chars().take_while(|ch| *ch == '#').count();
    if marker_len == 0 {
        return trimmed.trim().to_string();
    }

    let marker_len = marker_len.min(6);
    trimmed[marker_len..].trim_start().trim_end().to_string()
}
