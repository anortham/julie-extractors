use crate::base::{
    BaseExtractor, NormalizedSpan, Symbol, SymbolKind, SymbolOptions, containing_symbol_at_line,
};
use regex::Regex;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::LazyLock;
use tree_sitter::Node;

static INLINE_LINK_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(!)?\[([^\]\n]+)\]\(([^)\n]+)\)").unwrap());
static FOOTNOTE_DEFINITION_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\s*\[\^([^\]\n]+)\]:\s*(.+)$").unwrap());
static FOOTNOTE_REFERENCE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\[\^([^\]\n]+)\]").unwrap());

pub(super) fn extract_symbol_from_node(
    base: &mut BaseExtractor,
    node: Node,
    parent_id: Option<&str>,
) -> Option<Symbol> {
    match node.kind() {
        "fenced_code_block" => extract_fenced_code_block(base, node, parent_id),
        "inline_link" => extract_inline_link(base, node, parent_id),
        "full_reference_link" | "collapsed_reference_link" | "shortcut_link" => {
            extract_reference_link(base, node, parent_id)
        }
        "link_reference_definition" => extract_link_reference_definition(base, node, parent_id),
        _ => None,
    }
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
    if is_rustdoc_test_case(info_string.as_deref()) {
        metadata.insert("is_test".to_string(), json!(true));
    }

    let name = language
        .as_ref()
        .map(|language| format!("{language} code block"))
        .unwrap_or_else(|| "code block".to_string());

    Some(base.create_symbol(
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
    ))
}

fn is_rustdoc_test_case(info_string: Option<&str>) -> bool {
    let Some(info) = info_string else {
        return true;
    };

    let tokens: Vec<_> = info
        .split(|character: char| character == ',' || character.is_whitespace())
        .filter(|token| !token.is_empty())
        .collect();
    if tokens.is_empty() || tokens.contains(&"ignore") {
        return false;
    }

    if tokens[0] == "rust" {
        return true;
    }

    tokens
        .iter()
        .all(|token| matches!(*token, "no_run" | "compile_fail"))
}

fn extract_inline_link(
    base: &mut BaseExtractor,
    node: Node,
    parent_id: Option<&str>,
) -> Option<Symbol> {
    let text = first_child_text(base, node, "link_text").map(clean_link_text)?;
    let destination =
        first_child_text(base, node, "link_destination").map(clean_link_destination)?;
    let title = first_child_text(base, node, "link_title").map(clean_link_title);

    if let Some(destination_node) = first_child_node(node, "link_destination") {
        base.record_literal(
            &destination_node,
            destination.clone(),
            Some("inline_link".to_string()),
            0,
            parent_id.map(str::to_string),
        );
    }

    let mut metadata = HashMap::new();
    metadata.insert("markdown_kind".to_string(), json!("inline_link"));
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

fn extract_reference_link(
    base: &mut BaseExtractor,
    node: Node,
    parent_id: Option<&str>,
) -> Option<Symbol> {
    let label = first_child_text(base, node, "link_label")
        .or_else(|| first_child_text(base, node, "link_text"))
        .map(clean_link_label)?;
    let is_footnote = label.starts_with('^');
    let name = label.trim_start_matches('^').to_string();

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
            signature: Some(base.get_node_text(&node)),
            visibility: None,
            parent_id: parent_id.map(str::to_string),
            metadata: Some(metadata),
            doc_comment: if is_footnote { destination } else { title },
            annotations: Vec::new(),
        },
    ))
}

pub(super) fn extract_line_based_symbols(
    base: &mut BaseExtractor,
    existing_symbols: &[Symbol],
) -> Vec<Symbol> {
    let mut symbols = Vec::new();
    let mut byte_offset = 0u32;

    let lines: Vec<String> = base.content.lines().map(str::to_string).collect();
    for (line_index, line) in lines.iter().enumerate() {
        let line_number = line_index as u32 + 1;
        let parent_id =
            containing_symbol_at_line(existing_symbols, line_number).map(|s| s.id.clone());

        let is_footnote_definition = FOOTNOTE_DEFINITION_RE.is_match(line);
        for captures in INLINE_LINK_RE.captures_iter(line) {
            if captures.get(1).is_some() {
                continue;
            }
            let Some(matched) = captures.get(0) else {
                continue;
            };
            let Some(text) = captures.get(2).map(|matched| matched.as_str()) else {
                continue;
            };
            let Some(destination_match) = captures.get(3) else {
                continue;
            };
            let destination = destination_match.as_str();

            let dest_start = byte_offset as usize + destination_match.start();
            let dest_end = byte_offset as usize + destination_match.end();
            if let Some(span) = NormalizedSpan::from_content_range_with_line_starts(
                &base.content,
                base.line_starts(),
                dest_start,
                dest_end,
            ) {
                base.record_literal_at_span(
                    span,
                    clean_link_destination(destination.to_string()),
                    Some("inline_link".to_string()),
                    0,
                    parent_id.clone(),
                );
            }

            let mut metadata = HashMap::new();
            metadata.insert("markdown_kind".to_string(), json!("inline_link"));
            metadata.insert(
                "destination".to_string(),
                json!(clean_link_destination(destination.to_string())),
            );

            symbols.push(line_symbol(
                base,
                text.to_string(),
                SymbolKind::Import,
                line,
                line_number,
                byte_offset,
                matched.start() as u32,
                matched.end() as u32,
                parent_id.clone(),
                Some(matched.as_str().to_string()),
                None,
                metadata,
            ));
        }

        if let Some(captures) = FOOTNOTE_DEFINITION_RE.captures(line)
            && let (Some(matched), Some(label), Some(body)) =
                (captures.get(0), captures.get(1), captures.get(2))
        {
            let mut metadata = HashMap::new();
            metadata.insert("markdown_kind".to_string(), json!("footnote_definition"));
            metadata.insert(
                "reference_label".to_string(),
                json!(format!("^{}", label.as_str())),
            );
            metadata.insert("destination".to_string(), json!(body.as_str().trim()));

            symbols.push(line_symbol(
                base,
                label.as_str().to_string(),
                SymbolKind::Property,
                line,
                line_number,
                byte_offset,
                matched.start() as u32,
                matched.end() as u32,
                parent_id.clone(),
                Some(matched.as_str().to_string()),
                Some(body.as_str().trim().to_string()),
                metadata,
            ));
        }

        if !is_footnote_definition {
            for captures in FOOTNOTE_REFERENCE_RE.captures_iter(line) {
                let (Some(matched), Some(label)) = (captures.get(0), captures.get(1)) else {
                    continue;
                };
                let mut metadata = HashMap::new();
                metadata.insert("markdown_kind".to_string(), json!("footnote_reference"));
                metadata.insert(
                    "reference_label".to_string(),
                    json!(format!("^{}", label.as_str())),
                );

                symbols.push(line_symbol(
                    base,
                    label.as_str().to_string(),
                    SymbolKind::Property,
                    line,
                    line_number,
                    byte_offset,
                    matched.start() as u32,
                    matched.end() as u32,
                    parent_id.clone(),
                    Some(matched.as_str().to_string()),
                    None,
                    metadata,
                ));
            }
        }

        byte_offset += line.len() as u32 + 1;
    }

    symbols
}

#[allow(clippy::too_many_arguments)]
fn line_symbol(
    base: &BaseExtractor,
    name: String,
    kind: SymbolKind,
    line: &str,
    line_number: u32,
    line_byte_offset: u32,
    start_column: u32,
    end_column: u32,
    parent_id: Option<String>,
    signature: Option<String>,
    doc_comment: Option<String>,
    metadata: HashMap<String, Value>,
) -> Symbol {
    let span = NormalizedSpan {
        start_line: line_number,
        start_column,
        end_line: line_number,
        end_column,
        start_byte: line_byte_offset + start_column,
        end_byte: line_byte_offset + end_column,
    };
    let id = base.generate_id_for_span(&name, &span);
    Symbol {
        id,
        name,
        kind,
        language: base.language.clone(),
        file_path: base.file_path.clone(),
        start_line: span.start_line,
        start_column: span.start_column,
        end_line: span.end_line,
        end_column: span.end_column,
        start_byte: span.start_byte,
        end_byte: span.end_byte,
        body_span: None,
        body_hash: None,
        signature,
        doc_comment,
        visibility: None,
        parent_id,
        metadata: Some(metadata),
        annotations: Vec::new(),
        semantic_group: None,
        confidence: None,
        code_context: base
            .extract_code_context(
                line_number.saturating_sub(1) as usize,
                line_number.saturating_sub(1) as usize,
            )
            .or_else(|| Some(line.to_string())),
        content_type: Some("documentation".to_string()),
    }
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

fn clean_link_text(raw: String) -> String {
    raw.trim()
        .trim_start_matches('[')
        .trim_end_matches(']')
        .to_string()
}

fn clean_link_label(raw: String) -> String {
    clean_link_text(raw)
        .trim_end_matches(':')
        .trim()
        .to_string()
}

fn clean_link_destination(raw: String) -> String {
    raw.trim()
        .trim_start_matches('(')
        .trim_end_matches(')')
        .trim_start_matches('<')
        .trim_end_matches('>')
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
