// CSS Extractor Identifiers - Extract identifier usages (function calls, classes, IDs)

use crate::base::{BaseExtractor, ContainingSymbolIndex, Identifier, IdentifierKind, Symbol};
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};
use tree_sitter::{Node, Tree};

pub(super) struct IdentifierExtractor;

impl IdentifierExtractor {
    /// Extract all identifier usages (CSS functions, class/id selectors)
    pub(super) fn extract_identifiers(
        base: &mut BaseExtractor,
        tree: &Tree,
        symbols: &[Symbol],
    ) -> Vec<Identifier> {
        let containing_symbols = base.containing_symbol_index(symbols);

        // Walk the tree and extract identifiers
        Self::walk_tree_for_identifiers(base, tree.root_node(), &containing_symbols, 0);

        // Return the collected identifiers
        base.identifiers.clone()
    }

    /// Recursively walk tree extracting identifiers from each node
    fn walk_tree_for_identifiers(
        base: &mut BaseExtractor,
        node: Node,
        containing_symbols: &ContainingSymbolIndex<'_>,
        depth: u32,
    ) {
        if !should_visit_tree_depth(depth) {
            return;
        }

        // Extract identifier from this node if applicable
        Self::extract_identifier_from_node(base, node, containing_symbols);

        // Recursively walk children
        let Some(child_depth) = child_tree_depth(depth) else {
            return;
        };
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            Self::walk_tree_for_identifiers(base, child, containing_symbols, child_depth);
        }
    }

    /// Extract identifier from a single node based on its kind
    fn extract_identifier_from_node(
        base: &mut BaseExtractor,
        node: Node,
        containing_symbols: &ContainingSymbolIndex<'_>,
    ) {
        match node.kind() {
            // CSS function calls: calc(), var(), rgb(), etc.
            "call_expression" => {
                let mut cursor = node.walk();
                let mut function_name = None;
                let containing_symbol_id =
                    Self::find_containing_symbol_id(node, containing_symbols);

                for child in node.children(&mut cursor) {
                    match child.kind() {
                        "function_name" => {
                            function_name = Some(base.get_node_text(&child));
                            base.create_identifier(
                                &child,
                                function_name.clone().unwrap_or_default(),
                                IdentifierKind::Call,
                                containing_symbol_id.clone(),
                            );
                        }
                        "arguments" => {
                            let mut arg_cursor = child.walk();
                            for (pos, arg) in child.named_children(&mut arg_cursor).enumerate() {
                                // variable_ref (Batch F): `var(--x)` is the read of a
                                // custom property declared as `--x: value`. Only the
                                // FIRST argument is the reference — later arguments
                                // are fallback values, substituted as literal text,
                                // never dereferenced. The name keeps its `--` prefix
                                // to match the declared Property symbol's name.
                                if pos == 0
                                    && function_name.as_deref() == Some("var")
                                    && arg.kind() == "plain_value"
                                {
                                    let arg_text = base.get_node_text(&arg);
                                    if arg_text.starts_with("--") {
                                        base.create_identifier(
                                            &arg,
                                            arg_text,
                                            IdentifierKind::VariableRef,
                                            containing_symbol_id.clone(),
                                        );
                                    }
                                }
                                if let Some(text) = base.decode_string_literal(&arg) {
                                    base.record_literal(
                                        &arg,
                                        text,
                                        function_name.clone(),
                                        pos as u32,
                                        containing_symbol_id.clone(),
                                    );
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }

            // Class selectors: .button, .nav-item (treated as member access for HTML tracking)
            "class_selector" => {
                let text = base.get_node_text(&node);
                // Remove the leading dot from class name
                let class_name = text.strip_prefix('.').unwrap_or(&text);

                if !class_name.is_empty() {
                    let containing_symbol_id =
                        Self::find_containing_symbol_id(node, containing_symbols);

                    base.create_identifier(
                        &node,
                        class_name.to_string(),
                        IdentifierKind::MemberAccess,
                        containing_symbol_id,
                    );
                }
            }

            // ID selectors: #header, #main-content (treated as member access for HTML tracking)
            "id_selector" => {
                let text = base.get_node_text(&node);
                // Remove the leading hash from ID name
                let id_name = text.strip_prefix('#').unwrap_or(&text);

                if !id_name.is_empty() {
                    let containing_symbol_id =
                        Self::find_containing_symbol_id(node, containing_symbols);

                    base.create_identifier(
                        &node,
                        id_name.to_string(),
                        IdentifierKind::MemberAccess,
                        containing_symbol_id,
                    );
                }
            }

            "pseudo_class_selector" => {
                let text = base.get_node_text(&node);
                if let Some(name) = pseudo_call_name(&text) {
                    let containing_symbol_id =
                        Self::find_containing_symbol_id(node, containing_symbols);
                    base.create_identifier(
                        &node,
                        name.to_string(),
                        IdentifierKind::Call,
                        containing_symbol_id,
                    );
                }
            }

            _ => {
                let text = base.get_node_text(&node);
                if node.kind().contains("selector") {
                    extract_pseudo_calls_from_selector_node(base, node, &text, containing_symbols);
                }
            }
        }
    }

    /// Find the ID of the symbol that contains this node
    fn find_containing_symbol_id(
        node: Node,
        containing_symbols: &ContainingSymbolIndex<'_>,
    ) -> Option<String> {
        containing_symbols.find(node).map(|s| s.id.clone())
    }
}

fn extract_pseudo_calls_from_selector_node(
    base: &mut BaseExtractor,
    node: Node,
    text: &str,
    containing_symbols: &ContainingSymbolIndex<'_>,
) {
    for pseudo in [":has(", ":is(", ":where(", ":not("] {
        let mut search_start = 0usize;
        while let Some(relative) = text[search_start..].find(pseudo) {
            let local_start = search_start + relative + 1;
            let start_byte = node.start_byte() + local_start;
            let name = pseudo.trim_start_matches(':').trim_end_matches('(');
            if !base.identifiers.iter().any(|identifier| {
                identifier.name == name && identifier.start_byte == start_byte as u32
            }) && let Some((line, column)) = line_column_for_byte(&base.content, start_byte)
            {
                let containing_symbol_id =
                    IdentifierExtractor::find_containing_symbol_id(node, containing_symbols);
                let end_byte = start_byte + name.len();
                base.identifiers.push(Identifier {
                    id: base.generate_id(name, line, column),
                    name: name.to_string(),
                    kind: IdentifierKind::Call,
                    language: base.language.clone(),
                    file_path: base.file_path.clone(),
                    start_line: line,
                    start_column: column,
                    end_line: line,
                    end_column: column + name.len() as u32,
                    start_byte: start_byte as u32,
                    end_byte: end_byte as u32,
                    containing_symbol_id,
                    target_symbol_id: None,
                    confidence: 1.0,
                    receiver_type: None,
                    code_context: None,
                });
            }
            search_start = local_start + name.len();
        }
    }
}

fn pseudo_call_name(text: &str) -> Option<&str> {
    let selector = text.strip_prefix(':')?;
    let name = selector.split('(').next()?;
    if name.is_empty() || name == selector {
        None
    } else {
        Some(name)
    }
}

fn line_column_for_byte(content: &str, target: usize) -> Option<(u32, u32)> {
    let mut line = 1u32;
    let mut line_start = 0usize;
    for (idx, byte) in content.bytes().enumerate() {
        if idx == target {
            return Some((line, (idx - line_start) as u32));
        }
        if byte == b'\n' {
            line += 1;
            line_start = idx + 1;
        }
    }
    (target == content.len()).then_some((line, (target - line_start) as u32))
}
