use super::helpers::extract_attribute_name_value;
use super::relationships::is_templated;
use crate::base::config_literals::{enclosing_element_tag_name, tag_attribute_carrier};
use crate::base::{BaseExtractor, ContainingSymbolIndex, IdentifierKind};
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};
use tree_sitter::Node;

/// Identifier extraction for LSP find_references functionality
pub(super) struct IdentifierExtractor;

impl IdentifierExtractor {
    /// Extract all identifier usages from HTML tree
    pub(super) fn extract_identifiers(
        base: &mut BaseExtractor,
        node: Node,
        containing_symbols: &ContainingSymbolIndex<'_>,
    ) {
        Self::extract_identifiers_at_depth(base, node, containing_symbols, 0);
    }

    fn extract_identifiers_at_depth(
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
            Self::extract_identifiers_at_depth(base, child, containing_symbols, child_depth);
        }
    }

    /// Attributes record a literal for their value; `id` and `class` declare
    /// member names; id-reference attributes (`for`, `href="#x"`) use them.
    fn extract_identifier_from_node(
        base: &mut BaseExtractor,
        node: Node,
        containing_symbols: &ContainingSymbolIndex<'_>,
    ) {
        if node.kind() != "attribute" {
            return;
        }
        let (Some(name), Some(value)) = extract_attribute_name_value(base, node) else {
            return;
        };
        let mut cursor = node.walk();
        let value_node = node
            .children(&mut cursor)
            .find(|child| matches!(child.kind(), "attribute_value" | "quoted_attribute_value"));
        let containing_symbol_id = Self::find_containing_symbol_id(node, containing_symbols);

        if let Some(value_node) = value_node
            && !value.is_empty()
        {
            let tag_name = enclosing_element_tag_name(&base.content, node)
                .map(|tag_name| tag_name.to_ascii_lowercase())
                .unwrap_or_else(|| "element".to_string());
            let carrier = tag_attribute_carrier(&tag_name, &name);
            base.record_literal(
                &value_node,
                value.clone(),
                Some(carrier),
                0,
                containing_symbol_id.clone(),
            );
        }

        let declared: Vec<String> = match name.as_str() {
            "class" => class_tokens(&value),
            "id" if !is_templated(&value) && !value.trim().is_empty() => vec![value.clone()],
            _ => Vec::new(),
        };
        for member_name in declared {
            base.create_identifier(
                &node,
                member_name,
                IdentifierKind::MemberAccess,
                containing_symbol_id.clone(),
            );
        }

        let Some(value_start) = value_node.map(inner_value_start) else {
            return;
        };
        for (offset, id) in id_references(&name, &value) {
            let start = value_start + offset;
            if let Some(span) = base.span_for_byte_range(start, start + id.len()) {
                base.create_identifier_at_span(
                    span,
                    id.to_string(),
                    IdentifierKind::MemberAccess,
                    containing_symbol_id.clone(),
                    None,
                );
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

/// Class names in a `class` value, with server-template segments removed.
fn class_tokens(value: &str) -> Vec<String> {
    strip_template_segments(value)
        .split_whitespace()
        .filter(|token| {
            !token.chars().any(|c| {
                matches!(
                    c,
                    '{' | '}' | '%' | '<' | '>' | '=' | '"' | '\'' | '(' | ')'
                )
            })
        })
        .map(str::to_string)
        .collect()
}

fn strip_template_segments(value: &str) -> String {
    let mut stripped = String::with_capacity(value.len());
    let mut rest = value;
    while let Some((open, close)) = [("{{", "}}"), ("{%", "%}"), ("<%", "%>"), ("{#", "#}")]
        .iter()
        .filter_map(|(open, close)| rest.find(open).map(|at| (at, *close)))
        .min_by_key(|(at, _)| *at)
    {
        stripped.push_str(&rest[..open]);
        stripped.push(' ');
        rest = match rest[open + 2..].find(close) {
            Some(end) => &rest[open + 2 + end + close.len()..],
            None => "",
        };
    }
    stripped.push_str(rest);
    stripped
}

/// Attributes whose value names one element id.
const SINGLE_ID_ATTRIBUTES: &[&str] = &[
    "for",
    "list",
    "form",
    "popovertarget",
    "commandfor",
    "anchor",
    "aria-activedescendant",
    "aria-details",
    "aria-errormessage",
];

/// Attributes whose value is a whitespace-separated list of element ids.
const ID_LIST_ATTRIBUTES: &[&str] = &[
    "aria-describedby",
    "aria-labelledby",
    "aria-controls",
    "aria-owns",
    "aria-flowto",
    "headers",
    "itemref",
];

/// Attributes whose value is a CSS selector; only a bare `#id` names an id.
const ID_SELECTOR_ATTRIBUTES: &[&str] = &["hx-target", "hx-include", "hx-indicator"];

/// The ids an attribute refers to, with their byte offsets in the value.
fn id_references<'v>(name: &str, value: &'v str) -> Vec<(usize, &'v str)> {
    if is_templated(value) {
        return Vec::new();
    }
    let is_fragment_link = matches!(name, "href" | "xlink:href");
    if is_fragment_link || ID_SELECTOR_ATTRIBUTES.contains(&name) {
        return value
            .strip_prefix('#')
            .filter(|id| is_plain_id(id))
            .map(|id| vec![(1, id)])
            .unwrap_or_default();
    }
    if SINGLE_ID_ATTRIBUTES.contains(&name) || ID_LIST_ATTRIBUTES.contains(&name) {
        let base = value.as_ptr() as usize;
        return value
            .split_ascii_whitespace()
            .filter(|id| is_plain_id(id))
            .take(if ID_LIST_ATTRIBUTES.contains(&name) {
                usize::MAX
            } else {
                1
            })
            .map(|id| (id.as_ptr() as usize - base, id))
            .collect();
    }
    Vec::new()
}

fn is_plain_id(id: &str) -> bool {
    !id.is_empty() && !id.contains(['/', '#', '?', '=', '(', ' ', '.'])
}

/// Byte where the attribute text starts, inside any quotes.
fn inner_value_start(value_node: Node) -> usize {
    if value_node.kind() == "quoted_attribute_value" {
        value_node.start_byte() + 1
    } else {
        value_node.start_byte()
    }
}
