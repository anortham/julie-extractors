use serde_json::{Number, Value};
use tree_sitter::{Node, Tree};

use super::fact_builders::{base_metadata, child_by_kind, fact_for_node, insert_string, node_text};
use super::{
    CSS_CHARSET_PATTERN_ID, CSS_CONTAINER_PATTERN_ID, CSS_CUSTOM_PROPERTY_PATTERN_ID,
    CSS_FONT_FACE_PATTERN_ID, CSS_KEYFRAMES_PATTERN_ID, CSS_LAYER_PATTERN_ID,
    CSS_MEDIA_QUERY_PATTERN_ID, CSS_NAMESPACE_PATTERN_ID, CSS_SELECTOR_RULE_PATTERN_ID,
    CSS_SUPPORTS_PATTERN_ID,
};
use crate::base::embedded_span::EmbeddedSpanOffset;
use crate::base::span::NormalizedSpan;
use crate::base::types::StructuralFact;
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};

pub(super) fn collect_css_structural_facts(
    tree: &Tree,
    file_path: &str,
    content: &str,
) -> Vec<StructuralFact> {
    collect_css_structural_facts_with_host(tree, file_path, content, "css", None)
}

pub(super) fn collect_css_structural_facts_with_host(
    tree: &Tree,
    file_path: &str,
    content: &str,
    language: &str,
    host_offset: Option<EmbeddedSpanOffset>,
) -> Vec<StructuralFact> {
    let mut facts = Vec::new();
    collect_css_node(
        tree.root_node(),
        file_path,
        content,
        language,
        &mut facts,
        0,
    );
    if let Some(offset) = host_offset {
        for fact in &mut facts {
            let span = NormalizedSpan {
                start_line: fact.start_line,
                start_column: fact.start_column,
                end_line: fact.end_line,
                end_column: fact.end_column,
                start_byte: fact.start_byte,
                end_byte: fact.end_byte,
            };
            let adjusted = offset.apply(span);
            fact.start_line = adjusted.start_line;
            fact.start_column = adjusted.start_column;
            fact.end_line = adjusted.end_line;
            fact.end_column = adjusted.end_column;
            fact.start_byte = adjusted.start_byte;
            fact.end_byte = adjusted.end_byte;
            fact.refresh_id();
        }
    }
    facts
}

fn collect_css_node(
    node: Node<'_>,
    file_path: &str,
    content: &str,
    language: &str,
    facts: &mut Vec<StructuralFact>,
    depth: u32,
) {
    if !should_visit_tree_depth(depth) {
        return;
    }

    match node.kind() {
        "rule_set" => {
            if let Some(fact) = css_selector_rule_fact(file_path, content, language, node) {
                facts.push(fact);
            }
        }
        "property_name" => {
            if let Some(fact) = css_custom_property_fact(file_path, content, language, node) {
                facts.push(fact);
            }
        }
        "media_statement" => {
            if let Some(fact) = css_media_query_fact(file_path, content, language, node) {
                facts.push(fact);
            }
        }
        "keyframes_statement" => {
            if let Some(fact) = css_keyframes_fact(file_path, content, language, node) {
                facts.push(fact);
            }
        }
        "supports_statement" => {
            if let Some(fact) = css_supports_fact(file_path, content, language, node) {
                facts.push(fact);
            }
        }
        "charset_statement" => {
            if let Some(fact) = css_charset_fact(file_path, content, language, node) {
                facts.push(fact);
            }
        }
        "namespace_statement" => {
            if let Some(fact) = css_namespace_fact(file_path, content, language, node) {
                facts.push(fact);
            }
        }
        "at_rule" => {
            if let Some(fact) = css_generic_at_rule_fact(file_path, content, language, node) {
                facts.push(fact);
            }
        }
        _ => {}
    }

    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_css_node(child, file_path, content, language, facts, child_depth);
    }
}

fn css_selector_rule_fact(
    file_path: &str,
    content: &str,
    language: &str,
    node: Node<'_>,
) -> Option<StructuralFact> {
    let selectors = child_by_kind(node, "selectors")?;
    let selector_text = node_text(content, selectors)?.trim().to_string();
    if selector_text.is_empty() {
        return None;
    }

    let mut metadata = base_metadata("stylesheet_structure");
    insert_string(&mut metadata, "selector", &selector_text);
    insert_string(
        &mut metadata,
        "selector_kind",
        css_selector_kind(&selector_text),
    );
    metadata.insert(
        "declaration_count".to_string(),
        Value::Number(Number::from(count_css_declarations(node))),
    );

    Some(fact_for_node(
        file_path,
        language,
        CSS_SELECTOR_RULE_PATTERN_ID,
        "rule_set",
        node,
        metadata,
    ))
}

fn css_custom_property_fact(
    file_path: &str,
    content: &str,
    language: &str,
    node: Node<'_>,
) -> Option<StructuralFact> {
    let property_name = node_text(content, node)?.trim();
    if !property_name.starts_with("--") {
        return None;
    }

    let mut metadata = base_metadata("stylesheet_structure");
    insert_string(&mut metadata, "property_name", property_name);

    Some(fact_for_node(
        file_path,
        language,
        CSS_CUSTOM_PROPERTY_PATTERN_ID,
        "custom_property",
        node,
        metadata,
    ))
}

fn css_media_query_fact(
    file_path: &str,
    content: &str,
    language: &str,
    node: Node<'_>,
) -> Option<StructuralFact> {
    let text = node_text(content, node)?;
    let query = css_at_rule_prelude(text, "@media");
    let mut metadata = base_metadata("responsive_design");
    if let Some(query) = query {
        insert_string(&mut metadata, "query", query);
    }

    Some(fact_for_node(
        file_path,
        language,
        CSS_MEDIA_QUERY_PATTERN_ID,
        "media_query",
        node,
        metadata,
    ))
}

fn css_keyframes_fact(
    file_path: &str,
    content: &str,
    language: &str,
    node: Node<'_>,
) -> Option<StructuralFact> {
    let text = node_text(content, node)?;
    let animation_name = css_at_rule_prelude(text, "@keyframes");
    let mut metadata = base_metadata("animation");
    if let Some(animation_name) = animation_name {
        insert_string(&mut metadata, "animation_name", animation_name);
    }

    Some(fact_for_node(
        file_path,
        language,
        CSS_KEYFRAMES_PATTERN_ID,
        "keyframes",
        node,
        metadata,
    ))
}

fn css_supports_fact(
    file_path: &str,
    content: &str,
    language: &str,
    node: Node<'_>,
) -> Option<StructuralFact> {
    let text = node_text(content, node)?;
    let condition = css_at_rule_prelude(text, "@supports");
    let mut metadata = base_metadata("feature_query");
    if let Some(condition) = condition {
        insert_string(&mut metadata, "condition", condition);
    }

    Some(fact_for_node(
        file_path,
        language,
        CSS_SUPPORTS_PATTERN_ID,
        "supports",
        node,
        metadata,
    ))
}

fn css_charset_fact(
    file_path: &str,
    content: &str,
    language: &str,
    node: Node<'_>,
) -> Option<StructuralFact> {
    let encoding = child_by_kind(node, "string_value")
        .and_then(|value| node_text(content, value))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .or_else(|| {
            node_text(content, node)
                .and_then(|text| css_at_rule_prelude(text, "@charset"))
                .map(|prelude| prelude.trim_end_matches(';').trim().to_string())
        })?;
    let mut metadata = base_metadata("stylesheet_structure");
    insert_string(&mut metadata, "encoding", &encoding);

    Some(fact_for_node(
        file_path,
        language,
        CSS_CHARSET_PATTERN_ID,
        "charset",
        node,
        metadata,
    ))
}

fn css_namespace_fact(
    file_path: &str,
    content: &str,
    language: &str,
    node: Node<'_>,
) -> Option<StructuralFact> {
    let text = node_text(content, node)?;
    let namespace = css_at_rule_prelude(text, "@namespace")
        .map(|prelude| prelude.trim_end_matches(';').trim().to_string())
        .filter(|value| !value.is_empty())?;
    let mut metadata = base_metadata("stylesheet_structure");
    insert_string(&mut metadata, "namespace", &namespace);

    Some(fact_for_node(
        file_path,
        language,
        CSS_NAMESPACE_PATTERN_ID,
        "namespace",
        node,
        metadata,
    ))
}

fn css_generic_at_rule_fact(
    file_path: &str,
    content: &str,
    language: &str,
    node: Node<'_>,
) -> Option<StructuralFact> {
    let keyword = child_by_kind(node, "at_keyword")
        .and_then(|keyword| node_text(content, keyword))
        .map(str::trim)
        .map(str::to_ascii_lowercase)?;
    let text = node_text(content, node)?;

    match keyword.as_str() {
        "@container" => {
            let condition = css_at_rule_prelude(text, "@container");
            let mut metadata = base_metadata("responsive_design");
            if let Some(condition) = condition {
                insert_string(&mut metadata, "condition", condition);
            }
            Some(fact_for_node(
                file_path,
                language,
                CSS_CONTAINER_PATTERN_ID,
                "container",
                node,
                metadata,
            ))
        }
        "@font-face" => {
            let mut metadata = base_metadata("stylesheet_structure");
            insert_string(&mut metadata, "at_rule", "@font-face");
            Some(fact_for_node(
                file_path,
                language,
                CSS_FONT_FACE_PATTERN_ID,
                "font_face",
                node,
                metadata,
            ))
        }
        "@layer" => {
            let layer_name = css_at_rule_prelude(text, "@layer");
            let mut metadata = base_metadata("stylesheet_structure");
            if let Some(layer_name) = layer_name {
                insert_string(&mut metadata, "layer_name", layer_name);
            }
            Some(fact_for_node(
                file_path,
                language,
                CSS_LAYER_PATTERN_ID,
                "layer",
                node,
                metadata,
            ))
        }
        _ => None,
    }
}

fn css_selector_kind(selector: &str) -> &'static str {
    let selector = selector.trim();
    if selector_has_top_level_comma(selector) {
        "selector_list"
    } else if selector.starts_with('.') {
        "class"
    } else if selector.starts_with('#') {
        "id"
    } else if selector.starts_with(':') {
        "pseudo"
    } else {
        "compound"
    }
}

fn selector_has_top_level_comma(selector: &str) -> bool {
    let bytes = selector.as_bytes();
    let mut bracket_depth = 0usize;
    let mut paren_depth = 0usize;
    let mut quote = None;
    let mut escaped = false;

    for byte in bytes {
        if let Some(active_quote) = quote {
            if escaped {
                escaped = false;
            } else if *byte == b'\\' {
                escaped = true;
            } else if *byte == active_quote {
                quote = None;
            }
            continue;
        }

        match *byte {
            b'\'' | b'"' => quote = Some(*byte),
            b'[' => bracket_depth += 1,
            b']' => bracket_depth = bracket_depth.saturating_sub(1),
            b'(' => paren_depth += 1,
            b')' => paren_depth = paren_depth.saturating_sub(1),
            b',' if bracket_depth == 0 && paren_depth == 0 => return true,
            _ => {}
        }
    }

    false
}

fn count_css_declarations(node: Node<'_>) -> usize {
    count_css_declarations_at_depth(node, 0)
}

fn count_css_declarations_at_depth(node: Node<'_>, depth: u32) -> usize {
    if !should_visit_tree_depth(depth) {
        return 0;
    }
    let mut count = usize::from(node.kind() == "declaration");
    let Some(child_depth) = child_tree_depth(depth) else {
        return count;
    };
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        count += count_css_declarations_at_depth(child, child_depth);
    }
    count
}

fn css_at_rule_prelude<'a>(text: &'a str, keyword: &str) -> Option<&'a str> {
    let trimmed = text.trim();
    let rest = trimmed.strip_prefix(keyword)?.trim();
    let prelude = rest.split('{').next().unwrap_or(rest).trim();
    let prelude = prelude.strip_suffix(';').unwrap_or(prelude).trim();
    (!prelude.is_empty()).then_some(prelude)
}
