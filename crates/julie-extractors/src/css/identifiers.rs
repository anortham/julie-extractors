// CSS Extractor Identifiers - Extract identifier usages (function calls, classes, IDs)

use super::custom_functions::{DashedFunction, dashed_functions, decode_css_escapes, overlapping};
use crate::base::{
    BaseExtractor, ContainingSymbolIndex, Identifier, IdentifierKind, NormalizedSpan, Symbol,
};
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};
use std::collections::HashMap;
use tree_sitter::{Node, Tree};

pub(super) struct IdentifierExtractor;

impl IdentifierExtractor {
    /// Extract all identifier usages (CSS functions, class/id selectors)
    pub(super) fn extract_identifiers(
        base: &mut BaseExtractor,
        tree: &Tree,
        symbols: &[Symbol],
    ) -> Vec<Identifier> {
        let file_path = base.file_path.clone();
        let containing_symbols = ContainingSymbolIndex::innermost(
            symbols
                .iter()
                .filter(|symbol| symbol.file_path == file_path),
        );

        let dashed = dashed_functions(&base.content);
        Self::walk_tree_for_identifiers(base, tree.root_node(), &containing_symbols, &dashed, 0);
        Self::create_dashed_call_identifiers(base, &dashed, &containing_symbols);

        // Return the collected identifiers
        base.identifiers.clone()
    }

    /// Recursively walk tree extracting identifiers from each node
    fn walk_tree_for_identifiers(
        base: &mut BaseExtractor,
        node: Node,
        containing_symbols: &ContainingSymbolIndex<'_>,
        dashed: &[DashedFunction],
        depth: u32,
    ) {
        if !should_visit_tree_depth(depth) {
            return;
        }

        Self::extract_identifier_from_node(base, node, containing_symbols, dashed);

        // Recursively walk children
        let Some(child_depth) = child_tree_depth(depth) else {
            return;
        };
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            Self::walk_tree_for_identifiers(base, child, containing_symbols, dashed, child_depth);
        }
    }

    /// Extract identifier from a single node based on its kind
    fn extract_identifier_from_node(
        base: &mut BaseExtractor,
        node: Node,
        containing_symbols: &ContainingSymbolIndex<'_>,
        dashed: &[DashedFunction],
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
                            if let Some(dashed_call) =
                                overlapping(dashed, child.start_byte(), child.end_byte())
                            {
                                function_name = Some(dashed_call.name.clone());
                            } else {
                                function_name = Some(base.get_node_text(&child));
                                base.create_identifier(
                                    &child,
                                    function_name.clone().unwrap_or_default(),
                                    IdentifierKind::Call,
                                    containing_symbol_id.clone(),
                                );
                            }
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
                                let url_path = (function_name.as_deref() == Some("url")
                                    && arg.kind() == "plain_value")
                                    .then(|| base.get_node_text(&arg));
                                if let Some(text) =
                                    url_path.or_else(|| base.decode_string_literal(&arg))
                                {
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

            "class_selector" | "id_selector" => {
                let name_kind = if node.kind() == "class_selector" {
                    "class_name"
                } else {
                    "id_name"
                };
                let mut cursor = node.walk();
                let name_node = node
                    .named_children(&mut cursor)
                    .find(|child| child.kind() == name_kind);
                if let Some(name_node) = name_node {
                    let name = decode_css_escapes(&base.get_node_text(&name_node));
                    if !name.is_empty() {
                        let containing_symbol_id =
                            Self::find_containing_symbol_id(name_node, containing_symbols);
                        base.create_identifier(
                            &name_node,
                            name,
                            IdentifierKind::MemberAccess,
                            containing_symbol_id,
                        );
                    }
                }
            }

            "pseudo_class_selector" => {
                let mut cursor = node.walk();
                let children: Vec<Node> = node.children(&mut cursor).collect();
                let is_functional = children.iter().any(|child| child.kind() == "arguments");
                if let Some(name_node) = children
                    .iter()
                    .find(|child| child.kind() == "class_name")
                    .filter(|_| is_functional)
                    .filter(|name| {
                        overlapping(dashed, name.start_byte(), name.end_byte()).is_none()
                    })
                {
                    let containing_symbol_id =
                        Self::find_containing_symbol_id(node, containing_symbols);
                    base.create_identifier(
                        name_node,
                        base.get_node_text(name_node),
                        IdentifierKind::Call,
                        containing_symbol_id,
                    );
                }
            }

            "postcss_statement" if is_apply_statement(base, node) => {
                let mut cursor = node.walk();
                let classes: Vec<Node> = node
                    .named_children(&mut cursor)
                    .filter(|child| child.kind() == "plain_value")
                    .collect();
                Self::create_class_references(base, &classes, containing_symbols);
            }

            "declaration" => {
                let composed = composed_class_nodes(base, node);
                Self::create_class_references(base, &composed, containing_symbols);
            }

            _ => {}
        }
    }

    /// One call identifier per custom function call (`--name(...)`), named
    /// with its escapes decoded and carrying its argument count.
    fn create_dashed_call_identifiers(
        base: &mut BaseExtractor,
        dashed: &[DashedFunction],
        containing_symbols: &ContainingSymbolIndex<'_>,
    ) {
        for call in dashed.iter().filter(|function| !function.is_declaration) {
            let Some(span) = NormalizedSpan::from_content_range_with_line_starts(
                &base.content,
                base.line_starts(),
                call.start_byte,
                call.end_byte,
            ) else {
                continue;
            };
            let containing_symbol_id = containing_symbols
                .find_for_span(span)
                .map(|symbol| symbol.id.clone());
            let metadata = call.arg_count.map(|count| {
                HashMap::from([("arg_count".to_string(), serde_json::Value::from(count))])
            });
            base.create_identifier_at_span(
                span,
                call.name.clone(),
                IdentifierKind::Call,
                containing_symbol_id,
                metadata,
            );
        }
    }

    /// A class name used by `@apply` or `composes:` reads as a member access,
    /// like the `.name` selector that declares it.
    fn create_class_references(
        base: &mut BaseExtractor,
        nodes: &[Node],
        containing_symbols: &ContainingSymbolIndex<'_>,
    ) {
        for class_node in nodes {
            let name = base.get_node_text(class_node);
            let containing_symbol_id =
                Self::find_containing_symbol_id(*class_node, containing_symbols);
            base.create_identifier(
                class_node,
                name,
                IdentifierKind::MemberAccess,
                containing_symbol_id,
            );
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

pub(super) fn is_apply_statement(base: &BaseExtractor, node: Node) -> bool {
    let mut cursor = node.walk();
    node.children(&mut cursor)
        .find(|child| child.kind() == "at_keyword")
        .is_some_and(|keyword| base.get_node_text(&keyword).eq_ignore_ascii_case("@apply"))
}

/// The class-name values of a CSS Modules `composes:` declaration, up to the
/// `from` keyword that names another module.
pub(super) fn composed_class_nodes<'t>(
    base: &BaseExtractor,
    declaration: Node<'t>,
) -> Vec<Node<'t>> {
    let mut cursor = declaration.walk();
    let children: Vec<Node<'t>> = declaration.named_children(&mut cursor).collect();
    let is_composes = children
        .first()
        .filter(|name| name.kind() == "property_name")
        .is_some_and(|name| base.get_node_text(name).eq_ignore_ascii_case("composes"));
    if !is_composes {
        return Vec::new();
    }
    children
        .into_iter()
        .skip(1)
        .take_while(|value| value.kind() == "plain_value" && base.get_node_text(value) != "from")
        .collect()
}

/// The quoted module path after `from` in a `composes:` declaration.
pub(super) fn composes_module_source<'t>(
    base: &BaseExtractor,
    declaration: Node<'t>,
) -> Option<(Node<'t>, String)> {
    let composed = composed_class_nodes(base, declaration);
    let last = composed.last()?;
    let from = last.next_named_sibling()?;
    if base.get_node_text(&from) != "from" {
        return None;
    }
    let source = from.next_named_sibling()?;
    let text = base.decode_string_literal(&source)?;
    Some((source, text))
}
