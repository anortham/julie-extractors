pub(crate) mod classes;
pub(crate) mod complexity_metrics;
pub(crate) mod flags;
pub(crate) mod groups;
pub(crate) mod helpers;
pub(crate) mod identifiers;
mod patterns;
mod relationships;
pub(crate) mod signatures;

use crate::base::{BaseExtractor, Identifier, NormalizedSpan, Relationship, Symbol, SymbolKind};
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};
use std::collections::{HashMap, HashSet};
use tree_sitter::{Node, Parser, Point, Range, Tree};

pub struct RegexExtractor {
    pub(crate) base: BaseExtractor,
    pattern_trees: Vec<Tree>,
}

/// Parses each independent pattern of a `.regex` file into its own tree.
///
/// The grammar treats newlines as extras, so one parse joins every line into a
/// single pattern. A pattern-list file has one pattern per non-blank line; a
/// file that opens with an inline flag group containing `x` (verbose mode) is
/// one pattern whose `#` comments are left out of the parse. Each tree keeps
/// the file's byte and point coordinates.
pub(crate) fn pattern_trees(content: &str) -> Vec<Tree> {
    let lines = pattern_line_ranges(content);
    let patterns: Vec<Vec<Range>> = if is_verbose_file(content, &lines) {
        let code: Vec<Range> = lines
            .iter()
            .filter_map(|line| verbose_code_range(content, line))
            .collect();
        vec![code]
    } else {
        lines.into_iter().map(|line| vec![line]).collect()
    };
    let mut parser = Parser::new();
    if parser
        .set_language(&tree_sitter_regex::LANGUAGE.into())
        .is_err()
    {
        return Vec::new();
    }
    patterns
        .into_iter()
        .filter(|ranges| !ranges.is_empty())
        .filter_map(|ranges| {
            parser.set_included_ranges(&ranges).ok()?;
            parser.parse(content, None)
        })
        .collect()
}

/// Byte ranges of the `#` comments in a verbose-mode file, one per line.
pub(crate) fn verbose_comment_ranges(content: &str) -> Vec<(usize, usize)> {
    let lines = pattern_line_ranges(content);
    if !is_verbose_file(content, &lines) {
        return Vec::new();
    }
    lines
        .iter()
        .filter_map(|line| {
            let text = &content[line.start_byte..line.end_byte];
            let comment_start = verbose_comment_start(text)?;
            Some((line.start_byte + comment_start, line.end_byte))
        })
        .collect()
}

/// Folds a pattern onto one line: a verbose pattern loses its comments and the
/// whitespace around each line.
pub(crate) fn folded_pattern_text(pattern_text: &str) -> String {
    let verbose = pattern_text
        .lines()
        .find(|line| !line.trim().is_empty())
        .is_some_and(is_verbose_flag_group);
    pattern_text
        .lines()
        .map(|line| {
            let code = if verbose {
                &line[..verbose_comment_start(line).unwrap_or(line.len())]
            } else {
                line
            };
            code.trim()
        })
        .collect()
}

fn is_verbose_file(content: &str, lines: &[Range]) -> bool {
    lines
        .first()
        .is_some_and(|first| is_verbose_flag_group(&content[first.start_byte..first.end_byte]))
}

fn verbose_code_range(content: &str, line: &Range) -> Option<Range> {
    let text = &content[line.start_byte..line.end_byte];
    let code = &text[..verbose_comment_start(text).unwrap_or(text.len())];
    let leading = code.len() - code.trim_start().len();
    let end = code.trim_end().len();
    (leading < end).then(|| Range {
        start_byte: line.start_byte + leading,
        end_byte: line.start_byte + end,
        start_point: Point::new(line.start_point.row, leading),
        end_point: Point::new(line.start_point.row, end),
    })
}

/// Offset of the `#` that opens a verbose-mode comment: unescaped and outside a
/// character class.
fn verbose_comment_start(line: &str) -> Option<usize> {
    let mut escaped = false;
    let mut in_class = false;
    for (offset, ch) in line.char_indices() {
        match ch {
            _ if escaped => escaped = false,
            '\\' => escaped = true,
            '[' => in_class = true,
            ']' => in_class = false,
            '#' if !in_class => return Some(offset),
            _ => {}
        }
    }
    None
}

/// The capture groups one pattern declares: how many, and their names.
pub(crate) struct CaptureInventory {
    pub(crate) count: usize,
    pub(crate) names: HashSet<String>,
}

impl CaptureInventory {
    pub(crate) fn of(root: Node, content: &str) -> Self {
        let mut inventory = Self {
            count: 0,
            names: HashSet::new(),
        };
        inventory.collect(root, content, 0);
        inventory
    }

    fn collect(&mut self, node: Node, content: &str, depth: u32) {
        if !should_visit_tree_depth(depth) {
            return;
        }
        match node.kind() {
            "anonymous_capturing_group" => self.count += 1,
            "named_capturing_group" => {
                self.count += 1;
                if let Some(name) = groups::group_name_node(node)
                    .and_then(|name| content.get(name.start_byte()..name.end_byte()))
                {
                    self.names.insert(name.to_string());
                }
            }
            _ => {}
        }
        let Some(child_depth) = child_tree_depth(depth) else {
            return;
        };
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.collect(child, content, child_depth);
        }
    }
}

/// Binds each regex structural fact to the innermost symbol holding its bytes.
///
/// The shared binder skips value-holder kinds, but the root pattern symbol is a
/// variable and is the scope of every construct in its line.
pub(crate) fn attach_fact_symbols(facts: &mut [crate::base::StructuralFact], symbols: &[Symbol]) {
    for fact in facts {
        fact.containing_symbol_id =
            helpers::innermost_symbol_for_bytes(symbols, fact.start_byte, fact.end_byte)
                .map(|symbol| symbol.id.clone());
    }
}

fn pattern_line_ranges(content: &str) -> Vec<Range> {
    let mut ranges = Vec::new();
    let mut line_start = 0;
    for (row, line) in content.split('\n').enumerate() {
        let text = line.strip_suffix('\r').unwrap_or(line);
        if !text.trim().is_empty() {
            ranges.push(Range {
                start_byte: line_start,
                end_byte: line_start + text.len(),
                start_point: Point::new(row, 0),
                end_point: Point::new(row, text.len()),
            });
        }
        line_start += line.len() + 1;
    }
    ranges
}

fn is_verbose_flag_group(line: &str) -> bool {
    line.trim_start()
        .strip_prefix("(?")
        .and_then(|rest| rest.split([')', ':']).next())
        .is_some_and(|flags| {
            flags.contains('x') && flags.chars().all(|c| c.is_ascii_alphabetic() || c == '-')
        })
}

impl RegexExtractor {
    pub fn new(
        language: String,
        file_path: String,
        content: String,
        workspace_root: &std::path::Path,
    ) -> Self {
        let pattern_trees = pattern_trees(&content);
        Self {
            base: BaseExtractor::new(language, file_path, content, workspace_root),
            pattern_trees,
        }
    }

    /// Extracts symbols from the file's pattern trees; the whole-file `tree`
    /// joins every line into one pattern, so it is not used.
    pub fn extract_symbols(&mut self, _tree: &Tree) -> Vec<Symbol> {
        let mut symbols = Vec::new();
        let pattern_trees = std::mem::take(&mut self.pattern_trees);
        for pattern_tree in &pattern_trees {
            let referenced_capture_numbers =
                relationships::referenced_capture_numbers(&self.base, pattern_tree);
            let mut capture_index = 0;
            self.visit_node(
                pattern_tree.root_node(),
                &mut symbols,
                None,
                &referenced_capture_numbers,
                &mut capture_index,
                0,
            );
        }
        self.pattern_trees = pattern_trees;
        symbols
    }

    fn visit_node(
        &mut self,
        node: Node,
        symbols: &mut Vec<Symbol>,
        parent_id: Option<String>,
        referenced_capture_numbers: &HashSet<usize>,
        capture_index: &mut usize,
        depth: u32,
    ) -> Option<String> {
        if !should_visit_tree_depth(depth) {
            return parent_id;
        }

        let symbol = match node.kind() {
            "pattern" if parent_id.is_none() => {
                patterns::extract_pattern(&mut self.base, node, parent_id.clone())
            }
            "character_class" => {
                patterns::extract_character_class(&mut self.base, node, parent_id.clone())
            }
            "named_capturing_group" => {
                *capture_index += 1;
                patterns::extract_group(&mut self.base, node, parent_id.clone()).map(
                    |mut symbol| {
                        symbol.kind = SymbolKind::Function;
                        add_capture_index(&mut symbol, *capture_index);
                        symbol
                    },
                )
            }
            // Anonymous capture groups are symbols only when a numeric backreference targets them.
            "anonymous_capturing_group" => {
                *capture_index += 1;
                if referenced_capture_numbers.contains(capture_index) {
                    patterns::extract_group(&mut self.base, node, parent_id.clone()).map(
                        |mut symbol| {
                            symbol.kind = SymbolKind::Function;
                            add_capture_index(&mut symbol, *capture_index);
                            symbol
                        },
                    )
                } else {
                    None
                }
            }
            "lookaround_assertion" => {
                patterns::extract_lookaround(&mut self.base, node, parent_id.clone())
            }
            "character_class_escape" if is_unicode_property_escape(&self.base, node) => {
                patterns::extract_unicode_property(&mut self.base, node, parent_id.clone())
            }
            "term" => {
                record_literal_runs(&mut self.base, node, parent_id.clone());
                None
            }
            _ => None,
        };

        let current_parent_id = if let Some(symbol) = symbol {
            let id = symbol.id.clone();
            symbols.push(symbol);
            Some(id)
        } else {
            // When a node is skipped (noise), its children inherit the grandparent's
            // parent_id. This "skip-through parenting" flattens the tree — e.g. a
            // character_class inside an unnamed group gets parented to the top-level
            // pattern, not the skipped group. This is the desired behavior.
            parent_id
        };

        // Recursively visit children
        let Some(child_depth) = child_tree_depth(depth) else {
            return current_parent_id;
        };
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.visit_node(
                child,
                symbols,
                current_parent_id.clone(),
                referenced_capture_numbers,
                capture_index,
                child_depth,
            );
        }

        current_parent_id
    }

    pub fn extract_relationships(&mut self, _tree: &Tree, symbols: &[Symbol]) -> Vec<Relationship> {
        self.pattern_trees
            .iter()
            .flat_map(|pattern_tree| {
                let root = pattern_tree.root_node();
                let pattern_symbols: Vec<Symbol> = symbols
                    .iter()
                    .filter(|symbol| {
                        root.start_byte() as u32 <= symbol.start_byte
                            && symbol.end_byte <= root.end_byte() as u32
                    })
                    .cloned()
                    .collect();
                relationships::extract_relationships(&self.base, pattern_tree, &pattern_symbols)
            })
            .collect()
    }

    pub fn infer_types(&self, symbols: &[Symbol]) -> HashMap<String, String> {
        let mut types = HashMap::new();
        for symbol in symbols {
            if let Some(symbol_type) = symbol.metadata.as_ref().and_then(|m| m.get("type")) {
                if let Some(type_str) = symbol_type.as_str() {
                    types.insert(symbol.id.clone(), format!("regex:{}", type_str));
                }
            } else if symbol.kind == SymbolKind::Variable {
                types.insert(symbol.id.clone(), "regex:pattern".to_string());
            }
        }
        types
    }

    /// Extract all identifier usages (backreferences and named groups)
    /// Following the Rust extractor reference implementation pattern
    pub fn get_type_argument_usages(&self) -> Vec<crate::base::TypeArgumentUsage> {
        self.base.get_type_argument_usages()
    }

    /// Clone captured call-argument literals.
    pub fn get_literals(&self) -> Vec<crate::base::Literal> {
        self.base.get_literals()
    }

    pub fn extract_identifiers(&mut self, _tree: &Tree, symbols: &[Symbol]) -> Vec<Identifier> {
        identifiers::extract_identifiers(&mut self.base, &self.pattern_trees, symbols)
    }
}

fn add_capture_index(symbol: &mut Symbol, capture_index: usize) {
    let metadata = symbol.metadata.get_or_insert_with(HashMap::new);
    metadata.insert(
        "captureIndex".to_string(),
        serde_json::Value::Number((capture_index as u64).into()),
    );
}

fn is_unicode_property_escape(base: &BaseExtractor, node: Node) -> bool {
    let text = base.get_node_text(&node);
    text.starts_with("\\p{") || text.starts_with("\\P{")
}

/// Records each run of two or more word characters in a term as a pattern
/// literal. A quantifier binds only to the character before it, so that
/// character ends the run without joining it.
fn record_literal_runs(base: &mut BaseExtractor, term: Node, parent_id: Option<String>) {
    let mut cursor = term.walk();
    let children: Vec<Node> = term.children(&mut cursor).collect();
    let mut run: Vec<Node> = Vec::new();
    for (index, child) in children.iter().enumerate() {
        let quantified = children
            .get(index + 1)
            .is_some_and(|next| is_quantifier_kind(next.kind()));
        if is_word_character(base, child) && !quantified {
            run.push(*child);
            continue;
        }
        flush_literal_run(base, &mut run, parent_id.clone());
    }
    flush_literal_run(base, &mut run, parent_id);
}

fn flush_literal_run(base: &mut BaseExtractor, run: &mut Vec<Node>, parent_id: Option<String>) {
    if let (Some(first), Some(last)) = (run.first(), run.last())
        && run.len() >= 2
        && let Some(span) =
            NormalizedSpan::from_content_range(&base.content, first.start_byte(), last.end_byte())
    {
        let text = base.content[first.start_byte()..last.end_byte()].to_string();
        base.record_literal_at_span(span, text, Some("pattern".to_string()), 0, parent_id);
    }
    run.clear();
}

fn is_word_character(base: &BaseExtractor, node: &Node) -> bool {
    node.kind() == "pattern_character"
        && base
            .get_node_text(node)
            .chars()
            .all(|c| c.is_alphanumeric() || c == '_')
}

pub(crate) fn is_quantifier_kind(kind: &str) -> bool {
    matches!(
        kind,
        "zero_or_more" | "one_or_more" | "optional" | "count_quantifier"
    )
}
