use crate::base::{BaseExtractor, Symbol, SymbolKind, SymbolOptions};
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};
use serde_json::json;
use std::collections::{HashMap, HashSet};
use tree_sitter::{Node, Tree};

/// Links, images, autolinks, and footnotes from the inline trees. A reference
/// link counts only when the document defines its label, as in CommonMark;
/// otherwise `[text]` is plain bracketed prose.
pub(super) fn extract_inline_symbols(
    base: &mut BaseExtractor,
    block_tree: &Tree,
    block_symbols: &[Symbol],
) -> Vec<Symbol> {
    let defined_labels: HashSet<String> = block_symbols
        .iter()
        .filter_map(|symbol| symbol.metadata.as_ref()?.get("reference_label")?.as_str())
        .map(normalize_label)
        .collect();
    let mut symbols = Vec::new();
    for tree in super::inline::parse_inline_trees(block_tree, &base.content) {
        walk_inline(
            base,
            tree.root_node(),
            block_symbols,
            &defined_labels,
            &mut symbols,
            0,
        );
        for link in super::inline::nested_bracket_links(&tree, &base.content) {
            symbols.extend(nested_bracket_link(
                base,
                tree.root_node(),
                link,
                block_symbols,
            ));
        }
    }
    symbols
}

fn nested_bracket_link(
    base: &mut BaseExtractor,
    inline_root: Node,
    link: super::inline::NestedBracketLink,
    block_symbols: &[Symbol],
) -> Option<Symbol> {
    let span = base.span_for_byte_range(link.start, link.end)?;
    let parent_id =
        super::containing_heading(block_symbols, span.start_byte).map(|symbol| symbol.id.clone());
    if let Some(destination_span) = base.span_for_byte_range(
        link.destination_start,
        link.destination_start + link.destination.len(),
    ) {
        base.record_literal_at_span(
            destination_span,
            link.destination.clone(),
            Some("inline_link".to_string()),
            0,
            parent_id.clone(),
        );
    }
    let mut metadata = HashMap::new();
    metadata.insert("markdown_kind".to_string(), json!("inline_link"));
    metadata.insert("destination".to_string(), json!(link.destination));
    let mut symbol = base.create_symbol_from_span(
        &inline_root,
        span,
        link.label,
        SymbolKind::Import,
        SymbolOptions {
            signature: Some(base.content[link.start..link.end].to_string()),
            parent_id,
            metadata: Some(metadata),
            ..Default::default()
        },
    );
    base.set_body_span(&mut symbol, None);
    Some(symbol)
}

fn walk_inline(
    base: &mut BaseExtractor,
    node: Node,
    block_symbols: &[Symbol],
    defined_labels: &HashSet<String>,
    symbols: &mut Vec<Symbol>,
    depth: u32,
) {
    if !should_visit_tree_depth(depth) {
        return;
    }
    let is_reference = matches!(
        node.kind(),
        "full_reference_link" | "collapsed_reference_link" | "shortcut_link"
    );
    let wanted = !is_reference
        || reference_label(base, node).is_some_and(|label| {
            label.starts_with('^') || defined_labels.contains(&normalize_label(&label))
        });
    if wanted {
        let parent_id = super::containing_heading(block_symbols, node.start_byte() as u32)
            .map(|symbol| symbol.id.clone());
        if let Some(symbol) = extract_symbol_from_node(base, node, parent_id.as_deref()) {
            symbols.push(symbol);
        }
    }
    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        walk_inline(
            base,
            child,
            block_symbols,
            defined_labels,
            symbols,
            child_depth,
        );
    }
}

fn reference_label(base: &BaseExtractor, node: Node) -> Option<String> {
    first_child_text(base, node, "link_label")
        .or_else(|| first_child_text(base, node, "link_text"))
        .map(clean_link_label)
}

/// CommonMark label matching: case-insensitive, whitespace runs collapsed.
fn normalize_label(label: &str) -> String {
    label
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

pub(super) fn extract_symbol_from_node(
    base: &mut BaseExtractor,
    node: Node,
    parent_id: Option<&str>,
) -> Option<Symbol> {
    let mut symbol = match node.kind() {
        "fenced_code_block" => return extract_fenced_code_block(base, node, parent_id),
        "inline_link" | "image" => extract_inline_link(base, node, parent_id),
        "uri_autolink" | "email_autolink" => extract_autolink(base, node, parent_id),
        "full_reference_link" | "collapsed_reference_link" | "shortcut_link" => {
            extract_reference_link(base, node, parent_id)
        }
        "link_reference_definition" => extract_link_reference_definition(base, node, parent_id),
        _ => None,
    }?;
    base.set_body_span(&mut symbol, None);
    Some(symbol)
}

fn extract_fenced_code_block(
    base: &mut BaseExtractor,
    node: Node,
    parent_id: Option<&str>,
) -> Option<Symbol> {
    let info_string = first_child_text(base, node, "info_string")
        .map(|text| text.trim().to_string())
        .filter(|text| !text.is_empty());
    let language = info_string
        .as_deref()
        .and_then(|info| info.split_whitespace().next())
        .filter(|language| !language.is_empty())
        .map(str::to_string);
    let code = child_texts(base, node, "code_fence_content").join("\n");

    let mut metadata = HashMap::new();
    metadata.insert("markdown_kind".to_string(), json!("code_block"));
    if let Some(info) = &info_string {
        metadata.insert("info_string".to_string(), json!(info));
    }
    if let Some(language) = &language {
        metadata.insert("language".to_string(), json!(language));
    }
    let name = language
        .as_ref()
        .map(|language| format!("{language} code block"))
        .unwrap_or_else(|| "code block".to_string());

    let mut symbol = base.create_symbol(
        &node,
        name,
        SymbolKind::Property,
        SymbolOptions {
            signature: info_string.map(|info| format!("```{info}")),
            visibility: None,
            parent_id: parent_id.map(str::to_string),
            metadata: Some(metadata),
            doc_comment: (!code.trim().is_empty()).then(|| code.trim().to_string()),
            annotations: Vec::new(),
        },
    );
    let body_span = first_child_node(node, "code_fence_content")
        .and_then(|code| base.span_for_byte_range(code.start_byte(), trimmed_end(base, code)));
    base.set_body_span(&mut symbol, body_span);
    Some(symbol)
}

/// Code fence content runs through the line break before the closing fence.
fn trimmed_end(base: &BaseExtractor, node: Node) -> usize {
    let text = base.content.get(node.byte_range()).unwrap_or_default();
    node.start_byte() + text.trim_end_matches(['\n', '\r']).len()
}

fn extract_inline_link(
    base: &mut BaseExtractor,
    node: Node,
    parent_id: Option<&str>,
) -> Option<Symbol> {
    let markdown_kind = node.kind();
    let label_kind = if markdown_kind == "image" {
        "image_description"
    } else {
        "link_text"
    };
    let text = first_child_node(node, label_kind)
        .map(|label| super::inline::plain_text(&base.content, label))
        .unwrap_or_default();
    let destination =
        first_child_text(base, node, "link_destination").map(clean_link_destination)?;
    let title = first_child_text(base, node, "link_title").map(clean_link_title);
    let text = if text.is_empty() {
        destination.clone()
    } else {
        text
    };

    if let Some(destination_node) = first_child_node(node, "link_destination") {
        base.record_literal(
            &destination_node,
            destination.clone(),
            Some(markdown_kind.to_string()),
            0,
            parent_id.map(str::to_string),
        );
    }

    let mut metadata = HashMap::new();
    metadata.insert("markdown_kind".to_string(), json!(markdown_kind));
    metadata.insert("destination".to_string(), json!(destination));
    if let Some(title) = &title {
        metadata.insert("title".to_string(), json!(title));
    }

    Some(base.create_symbol(
        &node,
        text,
        SymbolKind::Import,
        SymbolOptions {
            signature: Some(base.get_node_text(&node)),
            visibility: None,
            parent_id: parent_id.map(str::to_string),
            metadata: Some(metadata),
            doc_comment: title,
            annotations: Vec::new(),
        },
    ))
}

fn extract_autolink(
    base: &mut BaseExtractor,
    node: Node,
    parent_id: Option<&str>,
) -> Option<Symbol> {
    let destination = clean_link_destination(base.get_node_text(&node));
    if destination.is_empty() {
        return None;
    }
    base.record_literal(
        &node,
        destination.clone(),
        Some("autolink".to_string()),
        0,
        parent_id.map(str::to_string),
    );
    let mut metadata = HashMap::new();
    metadata.insert("markdown_kind".to_string(), json!("autolink"));
    metadata.insert("destination".to_string(), json!(destination));
    Some(base.create_symbol(
        &node,
        destination,
        SymbolKind::Import,
        SymbolOptions {
            signature: Some(base.get_node_text(&node)),
            parent_id: parent_id.map(str::to_string),
            metadata: Some(metadata),
            ..Default::default()
        },
    ))
}

fn extract_reference_link(
    base: &mut BaseExtractor,
    node: Node,
    parent_id: Option<&str>,
) -> Option<Symbol> {
    let label = reference_label(base, node)?;
    let is_footnote = label.starts_with('^');
    let name = label.trim_start_matches('^').to_string();
    if is_footnote && let Some(definition) = footnote_definition(base, node, &label, parent_id) {
        return Some(definition);
    }

    let mut metadata = HashMap::new();
    metadata.insert(
        "markdown_kind".to_string(),
        if is_footnote {
            json!("footnote_reference")
        } else {
            json!("reference_link")
        },
    );
    metadata.insert("reference_label".to_string(), json!(label));

    Some(base.create_symbol(
        &node,
        name,
        if is_footnote {
            SymbolKind::Property
        } else {
            SymbolKind::Import
        },
        SymbolOptions {
            signature: Some(base.get_node_text(&node)),
            visibility: None,
            parent_id: parent_id.map(str::to_string),
            metadata: Some(metadata),
            doc_comment: None,
            annotations: Vec::new(),
        },
    ))
}

/// A GFM footnote definition (`[^n]: text` at the start of a line) is not
/// CommonMark, so the grammars read it as a `[^n]` shortcut link followed by
/// `:`. The definition runs to the end of the line.
fn footnote_definition(
    base: &mut BaseExtractor,
    node: Node,
    label: &str,
    parent_id: Option<&str>,
) -> Option<Symbol> {
    let content = &base.content;
    let line_start = content[..node.start_byte()]
        .rfind('\n')
        .map_or(0, |i| i + 1);
    if !content[line_start..node.start_byte()].trim().is_empty() {
        return None;
    }
    let rest = content[node.end_byte()..].strip_prefix(':')?;
    let line = rest
        .split('\n')
        .next()
        .unwrap_or_default()
        .trim_end_matches('\r');
    let line_end = node.end_byte() + 1 + line.len();
    let body = content[node.end_byte() + 1..line_end].trim().to_string();
    let span = base.span_for_byte_range(node.start_byte(), line_end)?;
    let signature = content[node.start_byte()..line_end].trim_end().to_string();

    let mut metadata = HashMap::new();
    metadata.insert("markdown_kind".to_string(), json!("footnote_definition"));
    metadata.insert("reference_label".to_string(), json!(label));
    metadata.insert("destination".to_string(), json!(body));
    Some(base.create_symbol_from_span(
        &node,
        span,
        label.trim_start_matches('^').to_string(),
        SymbolKind::Property,
        SymbolOptions {
            signature: Some(signature),
            parent_id: parent_id.map(str::to_string),
            metadata: Some(metadata),
            doc_comment: (!body.is_empty()).then_some(body),
            ..Default::default()
        },
    ))
}

fn extract_link_reference_definition(
    base: &mut BaseExtractor,
    node: Node,
    parent_id: Option<&str>,
) -> Option<Symbol> {
    let label = first_child_text(base, node, "link_label").map(clean_link_label)?;
    let is_footnote = label.starts_with('^');
    let name = label.trim_start_matches('^').to_string();
    let destination = if is_footnote {
        footnote_definition_text(&base.get_node_text(&node))
    } else {
        first_child_text(base, node, "link_destination").map(clean_link_destination)
    };
    let title = first_child_text(base, node, "link_title").map(clean_link_title);
    if !is_footnote
        && let (Some(destination_node), Some(destination)) =
            (first_child_node(node, "link_destination"), &destination)
    {
        base.record_literal(
            &destination_node,
            destination.clone(),
            Some("link_definition".to_string()),
            0,
            parent_id.map(str::to_string),
        );
    }

    let mut metadata = HashMap::new();
    metadata.insert(
        "markdown_kind".to_string(),
        if is_footnote {
            json!("footnote_definition")
        } else {
            json!("link_reference_definition")
        },
    );
    metadata.insert("reference_label".to_string(), json!(label));
    if let Some(destination) = &destination {
        metadata.insert("destination".to_string(), json!(destination));
    }
    if let Some(title) = &title {
        metadata.insert("title".to_string(), json!(title));
    }

    Some(base.create_symbol(
        &node,
        name,
        if is_footnote {
            SymbolKind::Property
        } else {
            SymbolKind::Import
        },
        SymbolOptions {
            signature: Some(base.get_node_text(&node).trim_end().to_string()),
            visibility: None,
            parent_id: parent_id.map(str::to_string),
            metadata: Some(metadata),
            doc_comment: if is_footnote { destination } else { title },
            annotations: Vec::new(),
        },
    ))
}

fn first_child_node<'a>(node: Node<'a>, kind: &str) -> Option<Node<'a>> {
    let mut cursor = node.walk();
    node.children(&mut cursor)
        .find(|child| child.kind() == kind)
}

fn first_child_text(base: &BaseExtractor, node: Node, kind: &str) -> Option<String> {
    first_child_node(node, kind).map(|child| base.get_node_text(&child))
}

fn child_texts(base: &BaseExtractor, node: Node, kind: &str) -> Vec<String> {
    let mut cursor = node.walk();
    node.children(&mut cursor)
        .filter(|child| child.kind() == kind)
        .map(|child| base.get_node_text(&child))
        .collect()
}

fn clean_link_label_text(raw: String) -> String {
    raw.trim()
        .trim_start_matches('[')
        .trim_end_matches(']')
        .to_string()
}

fn clean_link_label(raw: String) -> String {
    clean_link_label_text(raw)
        .trim_end_matches(':')
        .trim()
        .to_string()
}

fn clean_link_destination(raw: String) -> String {
    let raw = raw.trim();
    raw.strip_prefix('<')
        .and_then(|inner| inner.strip_suffix('>'))
        .unwrap_or(raw)
        .trim()
        .to_string()
}

fn clean_link_title(raw: String) -> String {
    raw.trim()
        .trim_matches(|ch| matches!(ch, '"' | '\'' | '(' | ')'))
        .to_string()
}

fn footnote_definition_text(raw: &str) -> Option<String> {
    raw.split_once(':')
        .map(|(_, body)| body.trim().to_string())
        .filter(|body| !body.is_empty())
}
