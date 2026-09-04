use crate::base::{Symbol, SymbolKind};

pub(crate) struct ContainingSymbolIndex<'a> {
    symbols: Vec<IndexedSymbol<'a>>,
    root: Option<IntervalNode>,
}

struct IntervalNode {
    center: u32,
    by_start: Vec<usize>,
    by_end: Vec<usize>,
    left: Option<Box<IntervalNode>>,
    right: Option<Box<IntervalNode>>,
}

impl IntervalNode {
    fn build(indices: &[usize], symbols: &[IndexedSymbol<'_>]) -> Option<Self> {
        if indices.is_empty() {
            return None;
        }

        let mut starts: Vec<u32> = indices
            .iter()
            .map(|&idx| symbols[idx].symbol.start_line)
            .collect();
        starts.sort_unstable();
        let center = starts[starts.len() / 2];

        let mut overlapping = Vec::new();
        let mut left_indices = Vec::new();
        let mut right_indices = Vec::new();

        for &idx in indices {
            let sym = symbols[idx].symbol;
            if sym.start_line <= center && sym.end_line >= center {
                overlapping.push(idx);
            } else if sym.end_line < center {
                left_indices.push(idx);
            } else {
                right_indices.push(idx);
            }
        }

        if overlapping.is_empty() {
            overlapping.extend_from_slice(indices);
            left_indices.clear();
            right_indices.clear();
        }

        let mut by_start = overlapping.clone();
        by_start.sort_by_key(|&idx| symbols[idx].symbol.start_line);

        let mut by_end = overlapping;
        by_end.sort_by(|&a, &b| symbols[b].symbol.end_line.cmp(&symbols[a].symbol.end_line));

        let left = if left_indices.len() == indices.len() {
            None
        } else {
            Self::build(&left_indices, symbols).map(Box::new)
        };

        let right = if right_indices.len() == indices.len() {
            None
        } else {
            Self::build(&right_indices, symbols).map(Box::new)
        };

        Some(Self {
            center,
            by_start,
            by_end,
            left,
            right,
        })
    }

    fn query(
        &self,
        symbols: &[IndexedSymbol<'_>],
        pos_line: u32,
        pos_column: u32,
        best: &mut Option<usize>,
    ) {
        let is_better = |candidate_idx: usize, current_idx: usize| {
            is_better_containing_symbol(&symbols[candidate_idx], &symbols[current_idx])
        };

        if pos_line == self.center {
            for &idx in &self.by_start {
                let candidate = &symbols[idx];
                if symbol_contains_position(candidate.symbol, pos_line, pos_column)
                    && best.is_none_or(|current| is_better(idx, current))
                {
                    *best = Some(idx);
                }
            }
        } else if pos_line < self.center {
            for &idx in &self.by_start {
                let candidate = &symbols[idx];
                if candidate.symbol.start_line > pos_line {
                    break;
                }
                if symbol_contains_position(candidate.symbol, pos_line, pos_column)
                    && best.is_none_or(|current| is_better(idx, current))
                {
                    *best = Some(idx);
                }
            }
            if let Some(left) = &self.left {
                left.query(symbols, pos_line, pos_column, best);
            }
        } else {
            for &idx in &self.by_end {
                let candidate = &symbols[idx];
                if candidate.symbol.end_line < pos_line {
                    break;
                }
                if symbol_contains_position(candidate.symbol, pos_line, pos_column)
                    && best.is_none_or(|current| is_better(idx, current))
                {
                    *best = Some(idx);
                }
            }
            if let Some(right) = &self.right {
                right.query(symbols, pos_line, pos_column, best);
            }
        }
    }
}

pub(crate) struct IndexedSymbol<'a> {
    pub(crate) symbol: &'a Symbol,
    pub(crate) priority: u32,
    pub(crate) size: u32,
}

impl<'a> ContainingSymbolIndex<'a> {
    pub(crate) fn new(symbols: &'a [Symbol], file_path: &str) -> Self {
        Self::from_iter(
            symbols
                .iter()
                .filter(|symbol| symbol.file_path == file_path),
        )
    }

    pub(crate) fn from_iter(symbols: impl IntoIterator<Item = &'a Symbol>) -> Self {
        let mut symbols: Vec<IndexedSymbol<'a>> = symbols
            .into_iter()
            .map(|symbol| IndexedSymbol {
                symbol,
                priority: symbol_priority(&symbol.kind),
                size: symbol.end_byte.saturating_sub(symbol.start_byte),
            })
            .collect();
        symbols.sort_by(|left, right| {
            left.symbol
                .start_line
                .cmp(&right.symbol.start_line)
                .then_with(|| left.symbol.start_column.cmp(&right.symbol.start_column))
        });
        let all_indices: Vec<usize> = (0..symbols.len()).collect();
        let root = IntervalNode::build(&all_indices, &symbols);
        Self { symbols, root }
    }

    pub(crate) fn find(&self, node: tree_sitter::Node) -> Option<&'a Symbol> {
        let position = node.start_position();
        let pos_line = (position.row + 1) as u32;
        let pos_column = position.column as u32;
        self.find_at(pos_line, pos_column)
    }

    pub(crate) fn find_at(&self, pos_line: u32, pos_column: u32) -> Option<&'a Symbol> {
        let mut best: Option<usize> = None;
        if let Some(root) = &self.root {
            root.query(&self.symbols, pos_line, pos_column, &mut best);
        }
        best.map(|idx| self.symbols[idx].symbol)
    }

    pub(crate) fn find_for_span(&self, span: crate::base::NormalizedSpan) -> Option<&'a Symbol> {
        self.find_at(span.start_line, span.start_column)
    }
}

pub(crate) fn symbol_contains_position(symbol: &Symbol, pos_line: u32, pos_column: u32) -> bool {
    let line_contains = symbol.start_line <= pos_line && symbol.end_line >= pos_line;
    if !line_contains {
        return false;
    }

    if pos_line == symbol.start_line && pos_line == symbol.end_line {
        symbol.start_column <= pos_column && symbol.end_column >= pos_column
    } else if pos_line == symbol.start_line {
        symbol.start_column <= pos_column
    } else if pos_line == symbol.end_line {
        symbol.end_column >= pos_column
    } else {
        true
    }
}

pub(crate) fn is_better_containing_symbol(
    candidate: &IndexedSymbol<'_>,
    current: &IndexedSymbol<'_>,
) -> bool {
    if candidate.priority != current.priority {
        return candidate.priority < current.priority;
    }
    if candidate.size != current.size {
        return candidate.size < current.size;
    }
    if candidate.symbol.start_byte != current.symbol.start_byte {
        return candidate.symbol.start_byte < current.symbol.start_byte;
    }
    candidate.symbol.id < current.symbol.id
}

pub(crate) fn symbol_priority(kind: &SymbolKind) -> u32 {
    match kind {
        SymbolKind::Function | SymbolKind::Method | SymbolKind::Constructor => 1,
        SymbolKind::Class | SymbolKind::Interface => 2,
        SymbolKind::Namespace => 3,
        SymbolKind::Variable | SymbolKind::Constant | SymbolKind::Property => 10,
        _ => 5,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};
    use std::path::Path;
    use tree_sitter::{Node, Parser};

    #[test]
    fn containing_symbol_index_keeps_existing_priority_and_smallest_span_rules() {
        let source = "fn caller() {\n    helper();\n}\n";
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_rust::LANGUAGE.into())
            .expect("failed to set Rust language");
        let tree = parser.parse(source, None).expect("failed to parse Rust");
        let call = find_first_node_kind(tree.root_node(), "call_expression")
            .expect("call expression should parse");

        let symbols = vec![
            test_symbol(
                "module",
                SymbolKind::Namespace,
                "test.rs",
                1,
                0,
                3,
                1,
                0,
                28,
            ),
            test_symbol(
                "wide_fn",
                SymbolKind::Function,
                "test.rs",
                1,
                0,
                3,
                1,
                0,
                28,
            ),
            test_symbol(
                "narrow_fn",
                SymbolKind::Function,
                "test.rs",
                2,
                4,
                2,
                13,
                call.start_byte() as u32,
                call.end_byte() as u32,
            ),
            test_symbol(
                "other_file",
                SymbolKind::Function,
                "other.rs",
                2,
                4,
                2,
                13,
                call.start_byte() as u32,
                call.end_byte() as u32,
            ),
        ];

        let index = ContainingSymbolIndex::new(&symbols, "test.rs");

        assert_eq!(
            index.find(call).map(|symbol| symbol.id.as_str()),
            Some("narrow_fn")
        );
    }

    #[test]
    fn test_base_containing_symbols_for_nested_and_top_level_constructs() {
        let source = concat!(
            "let top_level = 1;\n",
            "\n",
            "class Container {\n",
            "    field = 2;\n",
            "    method() {\n",
            "        let in_method = 3;\n",
            "    }\n",
            "}\n",
            "\n",
            "function outer() {\n",
            "    let in_outer = 4;\n",
            "    function inner() {\n",
            "        let in_inner = 5;\n",
            "    }\n",
            "}\n",
        );
        let symbols = vec![
            test_symbol(
                "class_container",
                SymbolKind::Class,
                "test.ts",
                3,
                0,
                8,
                1,
                20,
                102,
            ),
            test_symbol("method", SymbolKind::Method, "test.ts", 5, 4, 7, 5, 57, 100),
            test_symbol(
                "fn_outer",
                SymbolKind::Function,
                "test.ts",
                10,
                0,
                15,
                1,
                104,
                201,
            ),
            test_symbol(
                "fn_inner",
                SymbolKind::Function,
                "test.ts",
                12,
                4,
                14,
                5,
                149,
                199,
            ),
        ];
        let index = ContainingSymbolIndex::new(&symbols, "test.ts");

        let mut parser = Parser::new();
        parser
            .set_language(&crate::language::get_tree_sitter_language("typescript").unwrap())
            .unwrap();
        let tree = parser.parse(source, None).unwrap();

        let top_level_node =
            find_first_node_by_text(tree.root_node(), source, "top_level").unwrap();
        assert_eq!(index.find(top_level_node), None);
        assert_eq!(old_oracle(&top_level_node, &symbols, "test.ts"), None);

        let field_node = find_first_node_by_text(tree.root_node(), source, "field").unwrap();
        assert_eq!(
            index.find(field_node).map(|s| s.id.as_str()),
            Some("class_container")
        );
        assert_eq!(
            old_oracle(&field_node, &symbols, "test.ts").map(|s| s.id.as_str()),
            Some("class_container")
        );

        let in_method_node =
            find_first_node_by_text(tree.root_node(), source, "in_method").unwrap();
        assert_eq!(
            index.find(in_method_node).map(|s| s.id.as_str()),
            Some("method")
        );
        assert_eq!(
            old_oracle(&in_method_node, &symbols, "test.ts").map(|s| s.id.as_str()),
            Some("method")
        );

        let in_outer_node = find_first_node_by_text(tree.root_node(), source, "in_outer").unwrap();
        assert_eq!(
            index.find(in_outer_node).map(|s| s.id.as_str()),
            Some("fn_outer")
        );
        assert_eq!(
            old_oracle(&in_outer_node, &symbols, "test.ts").map(|s| s.id.as_str()),
            Some("fn_outer")
        );

        let in_inner_node = find_first_node_by_text(tree.root_node(), source, "in_inner").unwrap();
        assert_eq!(
            index.find(in_inner_node).map(|s| s.id.as_str()),
            Some("fn_inner")
        );
        assert_eq!(
            old_oracle(&in_inner_node, &symbols, "test.ts").map(|s| s.id.as_str()),
            Some("fn_inner")
        );
    }

    #[test]
    fn test_oracle_comparison_over_basic_golden_sources() {
        let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
        let root = manifest_dir.parent().and_then(Path::parent).unwrap();
        let cases = [
            ("rust", "fixtures/extraction/rust/basic/source.rs"),
            (
                "typescript",
                "fixtures/extraction/typescript/basic/source.ts",
            ),
            ("python", "fixtures/extraction/python/basic/source.py"),
            ("csharp", "fixtures/extraction/csharp/basic/source.cs"),
        ];

        for (language, source_rel_path) in cases {
            let file_path = root.join(source_rel_path);
            let source = std::fs::read_to_string(&file_path).unwrap();
            let results =
                crate::pipeline::extract_canonical(source_rel_path, &source, root).unwrap();
            let mut parser = Parser::new();
            parser
                .set_language(&crate::language::get_tree_sitter_language(language).unwrap())
                .unwrap();
            let tree = parser.parse(&source, None).unwrap();
            let index = ContainingSymbolIndex::new(&results.symbols, source_rel_path);

            let nodes = collect_all_nodes(tree.root_node());

            for node in nodes {
                let oracle_result = old_oracle(&node, &results.symbols, source_rel_path);
                let index_result = index.find(node);
                let iter_result = crate::base::BaseExtractor::find_containing_symbol_from_iter(
                    &node,
                    results
                        .symbols
                        .iter()
                        .filter(|s| s.file_path == source_rel_path),
                );

                assert_eq!(index_result.map(|s| &s.id), oracle_result.map(|s| &s.id),);
                assert_eq!(iter_result.map(|s| &s.id), oracle_result.map(|s| &s.id),);
            }
        }
    }

    fn collect_all_nodes<'a>(root: Node<'a>) -> Vec<Node<'a>> {
        let mut nodes = Vec::new();
        let mut stack = vec![root];
        while let Some(node) = stack.pop() {
            nodes.push(node);
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                stack.push(child);
            }
        }
        nodes
    }

    fn old_oracle<'a>(node: &Node, symbols: &'a [Symbol], file_path: &str) -> Option<&'a Symbol> {
        let position = node.start_position();
        let pos_line = (position.row + 1) as u32;
        let pos_column = position.column as u32;

        let mut containing: Vec<&'a Symbol> = symbols
            .iter()
            .filter(|s| {
                if s.file_path != file_path {
                    return false;
                }
                let line_contains = s.start_line <= pos_line && s.end_line >= pos_line;
                let col_contains = if pos_line == s.start_line && pos_line == s.end_line {
                    s.start_column <= pos_column && s.end_column >= pos_column
                } else if pos_line == s.start_line {
                    s.start_column <= pos_column
                } else if pos_line == s.end_line {
                    s.end_column >= pos_column
                } else {
                    true
                };
                line_contains && col_contains
            })
            .collect();

        if containing.is_empty() {
            return None;
        }

        let get_priority = |kind: &SymbolKind| -> u32 {
            match kind {
                SymbolKind::Function | SymbolKind::Method | SymbolKind::Constructor => 1,
                SymbolKind::Class | SymbolKind::Interface => 2,
                SymbolKind::Namespace => 3,
                SymbolKind::Variable | SymbolKind::Constant | SymbolKind::Property => 10,
                _ => 5,
            }
        };

        containing.sort_by(|a, b| {
            let p_a = get_priority(&a.kind);
            let p_b = get_priority(&b.kind);
            if p_a != p_b {
                return p_a.cmp(&p_b);
            }
            let size_a = a.end_byte.saturating_sub(a.start_byte);
            let size_b = b.end_byte.saturating_sub(b.start_byte);
            size_a
                .cmp(&size_b)
                .then_with(|| a.start_byte.cmp(&b.start_byte))
                .then_with(|| a.id.cmp(&b.id))
        });

        Some(containing[0])
    }

    fn find_first_node_by_text<'a>(node: Node<'a>, source: &str, text: &str) -> Option<Node<'a>> {
        if node.byte_range().end <= source.len() && &source[node.byte_range()] == text {
            return Some(node);
        }
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if let Some(found) = find_first_node_by_text(child, source, text) {
                return Some(found);
            }
        }
        None
    }

    fn find_first_node_kind<'a>(
        node: tree_sitter::Node<'a>,
        kind: &str,
    ) -> Option<tree_sitter::Node<'a>> {
        find_first_node_kind_at_depth(node, kind, 0)
    }

    fn find_first_node_kind_at_depth<'a>(
        node: tree_sitter::Node<'a>,
        kind: &str,
        depth: u32,
    ) -> Option<tree_sitter::Node<'a>> {
        if !should_visit_tree_depth(depth) {
            return None;
        }

        if node.kind() == kind {
            return Some(node);
        }

        let child_depth = child_tree_depth(depth)?;
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if let Some(found) = find_first_node_kind_at_depth(child, kind, child_depth) {
                return Some(found);
            }
        }
        None
    }

    #[allow(clippy::too_many_arguments)]
    fn test_symbol(
        id: &str,
        kind: SymbolKind,
        file_path: &str,
        start_line: u32,
        start_column: u32,
        end_line: u32,
        end_column: u32,
        start_byte: u32,
        end_byte: u32,
    ) -> Symbol {
        Symbol {
            id: id.to_string(),
            name: id.to_string(),
            kind,
            language: "rust".to_string(),
            file_path: file_path.to_string(),
            start_line,
            start_column,
            end_line,
            end_column,
            start_byte,
            end_byte,
            body_span: None,
            body_hash: None,
            signature: None,
            doc_comment: None,
            visibility: None,
            parent_id: None,
            metadata: None,
            annotations: Vec::new(),
            semantic_group: None,
            confidence: None,
            content_type: None,
        }
    }
}
