//! Static template attribute values recorded as literals (`href`, `src`,
//! `action`, ...), carried by their tag and attribute name.

use super::parsing::VueSection;
use crate::base::config_literals::{enclosing_element_tag_name, tag_attribute_carrier};
use crate::base::{BaseExtractor, ContainingSymbolIndex, EmbeddedSpanOffset, NormalizedSpan};
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};
use tree_sitter::{Node, Parser};

pub(super) fn extract_template_attribute_literals(
    base: &mut BaseExtractor,
    section: &VueSection,
    containing_symbols: &ContainingSymbolIndex<'_>,
) {
    let mut parser = Parser::new();
    if parser
        .set_language(&tree_sitter_html::LANGUAGE.into())
        .is_err()
    {
        return;
    }
    let Some(tree) = parser.parse(&section.content, None) else {
        return;
    };
    let Some(offset) = EmbeddedSpanOffset::from_host_byte(&base.content, section.content_start)
    else {
        return;
    };
    walk_template_for_literals(
        base,
        tree.root_node(),
        &section.content,
        containing_symbols,
        offset,
        0,
    );
}

fn walk_template_for_literals(
    base: &mut BaseExtractor,
    node: Node,
    template_content: &str,
    containing_symbols: &ContainingSymbolIndex<'_>,
    offset: EmbeddedSpanOffset,
    depth: u32,
) {
    if !should_visit_tree_depth(depth) {
        return;
    }

    if node.kind() == "attribute" {
        record_template_attribute_literal(base, node, template_content, containing_symbols, offset);
    }

    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk_template_for_literals(
            base,
            child,
            template_content,
            containing_symbols,
            offset,
            child_depth,
        );
    }
}

fn record_template_attribute_literal(
    base: &mut BaseExtractor,
    node: Node,
    template_content: &str,
    containing_symbols: &ContainingSymbolIndex<'_>,
    offset: EmbeddedSpanOffset,
) {
    let mut attr_name = None;
    let mut attr_value = None;
    let mut attr_value_node = None;
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        match child.kind() {
            "attribute_name" => {
                attr_name = Some(get_node_text_from_content(&child, template_content));
            }
            "attribute_value" | "quoted_attribute_value" => {
                let text = get_node_text_from_content(&child, template_content);
                attr_value = Some(text.trim_matches(|c| c == '"' || c == '\'').to_string());
                attr_value_node = Some(child);
            }
            _ => {}
        }
    }

    let (name, value, value_node) = match (attr_name, attr_value, attr_value_node) {
        (Some(name), Some(value), Some(value_node)) if !value.is_empty() => {
            (name, value, value_node)
        }
        _ => return,
    };
    if !is_static_template_attribute(&name) {
        return;
    }

    let tag_name =
        enclosing_element_tag_name(template_content, node).unwrap_or_else(|| "element".to_string());
    let carrier = tag_attribute_carrier(&tag_name, &name);
    let span = offset.apply(NormalizedSpan::from_node(&value_node));
    let containing_symbol_id = containing_symbols.find_for_span(span).map(|s| s.id.clone());
    base.record_literal_at_span(span, value, Some(carrier), 0, containing_symbol_id);
}

fn is_static_template_attribute(name: &str) -> bool {
    let name = name.trim();
    !(name.starts_with(':')
        || name.starts_with('@')
        || name.starts_with('#')
        || name.starts_with("v-"))
}

fn get_node_text_from_content(node: &Node, content: &str) -> String {
    content
        .get(node.start_byte()..node.end_byte())
        .unwrap_or_default()
        .to_string()
}
