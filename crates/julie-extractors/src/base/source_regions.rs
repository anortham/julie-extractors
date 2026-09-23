use std::collections::HashMap;

use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};
use tree_sitter::{Node, Tree};

use super::span::NormalizedSpan;
use super::types::{SourceRegion, SourceRegionKind, Symbol, stable_location_id};

#[derive(Debug, Clone, Copy)]
struct RegionLanguageConfig {
    comment_node_kinds: &'static [&'static str],
    string_literal_node_kinds: &'static [&'static str],
    quoted_string_literal_node_kinds: &'static [&'static str],
    html_comment_node_kinds: &'static [&'static str],
    embedded_node_kinds: &'static [&'static str],
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct SourceRegionLine<'a> {
    pub text: &'a str,
    pub start_byte: usize,
}

pub(crate) fn source_region_lines<'a>(
    content: &'a str,
    region: &SourceRegion,
) -> Vec<SourceRegionLine<'a>> {
    let start = region.start_byte as usize;
    let end = region.end_byte as usize;
    let Some(region_text) = content.get(start..end) else {
        return Vec::new();
    };
    let mut lines = Vec::new();
    let mut line_start = start;
    for line in region_text.split_inclusive('\n') {
        let text = line.strip_suffix('\n').unwrap_or(line);
        let text = text.strip_suffix('\r').unwrap_or(text);
        lines.push(SourceRegionLine {
            text,
            start_byte: line_start,
        });
        line_start += line.len();
    }
    lines
}

pub fn collect_source_regions(
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
    symbols: &[Symbol],
) -> Vec<SourceRegion> {
    if language == "regex" {
        let mut regions: Vec<SourceRegion> = crate::regex::verbose_comment_ranges(content)
            .into_iter()
            .filter_map(|(start, end)| NormalizedSpan::from_content_range(content, start, end))
            .map(|span| region_for_span(file_path, language, span, SourceRegionKind::Comment, None))
            .collect();
        attach_containing_symbols(&mut regions, symbols);
        return regions;
    }
    let Some(config) = config_for_language(language) else {
        return Vec::new();
    };
    let mut regions = Vec::new();
    collect_node(
        tree.root_node(),
        language,
        file_path,
        content,
        config,
        &mut regions,
        0,
    );
    if ADJACENT_DOC_REGION_LANGUAGES.contains(&language) {
        demote_detached_doc_comments(&mut regions, symbols, content);
    }
    attach_containing_symbols(&mut regions, symbols, language, content);
    regions.sort_by(|left, right| {
        left.start_byte
            .cmp(&right.start_byte)
            .then(left.end_byte.cmp(&right.end_byte))
            .then(left.kind.as_str().cmp(right.kind.as_str()))
            .then(left.id.cmp(&right.id))
    });
    regions
}

fn collect_node(
    node: Node<'_>,
    language: &str,
    file_path: &str,
    content: &str,
    config: RegionLanguageConfig,
    regions: &mut Vec<SourceRegion>,
    depth: u32,
) {
    if !should_visit_tree_depth(depth) {
        return;
    }

    let node_kind = node.kind();
    let embedded_script = match language {
        "yaml" => crate::yaml::ci::embedded_script_language(file_path, content, node),
        "xml" => crate::xml::embedded_sql_language(file_path, content, node),
        _ => None,
    };
    let markdown_body = (language == "markdown")
        .then(|| crate::markdown::blocks::embedded_body(content, node))
        .flatten();
    if let Some((embedded_language, start, end)) = markdown_body {
        let metadata = HashMap::from([
            (
                "host_node_kind".to_string(),
                serde_json::Value::String(node_kind.to_string()),
            ),
            (
                "embedded_language".to_string(),
                serde_json::Value::String(embedded_language.to_string()),
            ),
        ]);
        if let Some(span) = NormalizedSpan::from_content_range(content, start, end) {
            regions.push(region_for_span(
                file_path,
                language,
                span,
                SourceRegionKind::Embedded,
                Some(metadata),
            ));
        }
    } else if let Some(embedded_language) = embedded_script {
        let metadata = HashMap::from([
            (
                "host_node_kind".to_string(),
                serde_json::Value::String(node_kind.to_string()),
            ),
            (
                "embedded_language".to_string(),
                serde_json::Value::String(embedded_language.to_string()),
            ),
        ]);
        regions.push(region_for_node(
            file_path,
            language,
            node,
            SourceRegionKind::Embedded,
            Some(metadata),
        ));
    } else if config.embedded_node_kinds.contains(&node_kind) {
        regions.push(region_for_node(
            file_path,
            language,
            node,
            SourceRegionKind::Embedded,
            embedded_metadata(node, content),
        ));
    } else if config.comment_node_kinds.contains(&node_kind) {
        let text = node_text(content, node);
        let kind = if language == "yaml" {
            if is_yaml_key_attached_comment(content, node) {
                SourceRegionKind::DocComment
            } else {
                SourceRegionKind::Comment
            }
        } else if language == "toml" {
            if crate::toml::comment_documents_following_item(content, node) {
                SourceRegionKind::DocComment
            } else {
                SourceRegionKind::Comment
            }
        } else if language == "json" {
            if crate::json::comment_documents_following_value(content, node) {
                SourceRegionKind::DocComment
            } else {
                SourceRegionKind::Comment
            }
        } else if language == "xml" {
            if crate::xml::comment_documents_following_element(content, node) {
                SourceRegionKind::DocComment
            } else {
                SourceRegionKind::Comment
            }
        } else if is_doc_comment(language, text.unwrap_or_default()) {
            SourceRegionKind::DocComment
        } else {
            SourceRegionKind::Comment
        };
        regions.push(region_for_node(file_path, language, node, kind, None));
    } else if config.html_comment_node_kinds.contains(&node_kind) {
        if let Some(text) = node_text(content, node)
            && is_html_comment(text)
        {
            regions.push(region_for_node(
                file_path,
                language,
                node,
                SourceRegionKind::Comment,
                None,
            ));
        }
    } else if config.string_literal_node_kinds.contains(&node_kind) && node.is_named() {
        regions.push(region_for_node(
            file_path,
            language,
            node,
            SourceRegionKind::StringLiteral,
            None,
        ));
    } else if config.quoted_string_literal_node_kinds.contains(&node_kind)
        && let Some(text) = node_text(content, node)
        && is_quoted_string_literal(text)
    {
        regions.push(region_for_node(
            file_path,
            language,
            node,
            SourceRegionKind::StringLiteral,
            None,
        ));
    }

    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_node(
            child,
            language,
            file_path,
            content,
            config,
            regions,
            child_depth,
        );
    }
}

fn region_for_node(
    file_path: &str,
    language: &str,
    node: Node<'_>,
    kind: SourceRegionKind,
    metadata: Option<HashMap<String, serde_json::Value>>,
) -> SourceRegion {
    region_for_span(
        file_path,
        language,
        NormalizedSpan::from_node(&node),
        kind,
        metadata,
    )
}

fn region_for_span(
    file_path: &str,
    language: &str,
    span: NormalizedSpan,
    kind: SourceRegionKind,
    metadata: Option<HashMap<String, serde_json::Value>>,
) -> SourceRegion {
    SourceRegion {
        id: stable_location_id(file_path, kind.as_str(), span),
        file_path: file_path.to_string(),
        language: language.to_string(),
        kind,
        containing_symbol_id: None,
        start_line: span.start_line,
        start_column: span.start_column,
        end_line: span.end_line,
        end_column: span.end_column,
        start_byte: span.start_byte,
        end_byte: span.end_byte,
        metadata,
    }
}

/// Languages whose every comment reads as a doc comment by its text alone, so
/// a region is a doc comment only when it is part of a symbol's doc block.
const ADJACENT_DOC_REGION_LANGUAGES: &[&str] = &["bash"];

fn demote_detached_doc_comments(regions: &mut [SourceRegion], symbols: &[Symbol], content: &str) {
    for region in regions
        .iter_mut()
        .filter(|region| region.kind == SourceRegionKind::DocComment)
    {
        if !is_in_doc_block(region, symbols, content) {
            region.kind = SourceRegionKind::Comment;
            let span = NormalizedSpan {
                start_line: region.start_line,
                start_column: region.start_column,
                end_line: region.end_line,
                end_column: region.end_column,
                start_byte: region.start_byte,
                end_byte: region.end_byte,
            };
            region.id = stable_location_id(&region.file_path, region.kind.as_str(), span);
        }
    }
}

/// True when only comment lines separate `region` from the next documented
/// symbol, and that symbol's doc comment holds the region's text.
fn is_in_doc_block(region: &SourceRegion, symbols: &[Symbol], content: &str) -> bool {
    let Some(symbol) = documented_symbol_id(region, symbols)
        .and_then(|id| symbols.iter().find(|symbol| symbol.id == id))
    else {
        return false;
    };
    let (Some(text), Some(between)) = (
        content.get(region.start_byte as usize..region.end_byte as usize),
        content.get(region.end_byte as usize..symbol.start_byte as usize),
    ) else {
        return false;
    };
    let lines: Vec<&str> = between.split('\n').collect();
    lines.len() >= 2
        && lines[0].trim().is_empty()
        && lines[1..lines.len() - 1]
            .iter()
            .all(|line| line.trim_start().starts_with('#'))
        && symbol
            .doc_comment
            .as_deref()
            .is_some_and(|doc| doc.lines().any(|line| line == text.trim_end()))
}

fn attach_containing_symbols(
    regions: &mut [SourceRegion],
    symbols: &[Symbol],
    language: &str,
    content: &str,
) {
    let spec = crate::language_spec::language_spec(language);
    for region in regions {
        let is_trailing_doc = spec.is_some_and(|spec| {
            content
                .get(region.start_byte as usize..region.end_byte as usize)
                .is_some_and(|text| spec.is_trailing_doc_comment(text))
        });
        region.containing_symbol_id = match region.kind {
            SourceRegionKind::DocComment if is_trailing_doc => {
                trailing_documented_symbol_id(region, symbols)
            }
            SourceRegionKind::DocComment => documented_symbol_id(region, symbols),
            SourceRegionKind::Comment | SourceRegionKind::StringLiteral => {
                containing_symbol_id(region, symbols)
            }
            SourceRegionKind::Embedded => None,
        };
    }
}

fn containing_symbol_id(region: &SourceRegion, symbols: &[Symbol]) -> Option<String> {
    symbols
        .iter()
        .filter(|symbol| {
            symbol.start_byte <= region.start_byte && symbol.end_byte >= region.end_byte
        })
        .min_by_key(|symbol| symbol.end_byte.saturating_sub(symbol.start_byte))
        .map(|symbol| symbol.id.clone())
}

fn documented_symbol_id(region: &SourceRegion, symbols: &[Symbol]) -> Option<String> {
    if region.language == "sql"
        && let Some(documented) = sql_trailing_documented_symbol(region, symbols)
    {
        return Some(documented.id.clone());
    }
    symbols
        .iter()
        .filter(|symbol| symbol.doc_comment.is_some())
        .filter(|symbol| symbol.start_byte >= region.end_byte)
        .min_by_key(|symbol| symbol.start_byte.saturating_sub(region.end_byte))
        .map(|symbol| symbol.id.clone())
}

/// A SQL comment after a declaration on its last line documents it.
fn sql_trailing_documented_symbol<'a>(
    region: &SourceRegion,
    symbols: &'a [Symbol],
) -> Option<&'a Symbol> {
    symbols
        .iter()
        .filter(|symbol| symbol.doc_comment.is_some())
        .filter(|symbol| {
            symbol.end_byte <= region.start_byte && symbol.end_line == region.start_line
        })
        .max_by_key(|symbol| symbol.end_byte)
}

/// A trailing member doc (`int port; /**< TCP port. */`) documents the nearest
/// symbol that ends before it.
fn trailing_documented_symbol_id(region: &SourceRegion, symbols: &[Symbol]) -> Option<String> {
    symbols
        .iter()
        .filter(|symbol| symbol.doc_comment.is_some())
        .filter(|symbol| symbol.end_byte <= region.start_byte)
        .min_by_key(|symbol| region.start_byte.saturating_sub(symbol.end_byte))
        .map(|symbol| symbol.id.clone())
}

fn node_text<'a>(content: &'a str, node: Node<'_>) -> Option<&'a str> {
    content.get(node.start_byte()..node.end_byte())
}

fn is_doc_comment(language: &str, text: &str) -> bool {
    crate::language_spec::language_spec(language).is_some_and(|spec| spec.is_doc_comment(text))
}

fn is_yaml_key_attached_comment(content: &str, comment_node: Node<'_>) -> bool {
    comment_node.kind() == "comment"
        && crate::yaml::comment_documents_following_key(content, comment_node.start_byte())
}

fn is_html_comment(text: &str) -> bool {
    text.trim_start().starts_with("<!--")
}

fn is_quoted_string_literal(text: &str) -> bool {
    let trimmed = text.trim_start();
    let unprefixed = trimmed
        .strip_prefix(['N', 'n', 'E', 'e'])
        .filter(|rest| rest.starts_with('\''))
        .unwrap_or(trimmed);
    unprefixed.starts_with('"') || unprefixed.starts_with('\'')
}

fn embedded_metadata(node: Node<'_>, content: &str) -> Option<HashMap<String, serde_json::Value>> {
    let node_kind = node.kind();
    let mut metadata = HashMap::from([(
        "host_node_kind".to_string(),
        serde_json::Value::String(node_kind.to_string()),
    )]);

    match node_kind {
        "script_element" | "style_element" => {
            let host_tag = if node_kind == "script_element" {
                "script"
            } else {
                "style"
            };
            metadata.insert(
                "host_tag".to_string(),
                serde_json::Value::String(host_tag.to_string()),
            );
            let attributes = html_tag_attributes(content, node);
            insert_script_style_metadata(&mut metadata, node_kind, &attributes);
            if let Some(embedded_language) =
                embedded_language_for_script_style(node_kind, &attributes)
            {
                metadata.insert(
                    "embedded_language".to_string(),
                    serde_json::Value::String(embedded_language),
                );
            }
        }
        "razor_block" => {
            metadata.insert(
                "embedded_language".to_string(),
                serde_json::Value::String("csharp".to_string()),
            );
            if let Some(block_type) = razor_block_type(content, node) {
                metadata.insert(
                    "block_type".to_string(),
                    serde_json::Value::String(block_type),
                );
            }
        }
        "fenced_code_block" => {
            let info_string = fenced_code_info_string(node, content);
            if let Some(info) = info_string.as_deref().filter(|info| !info.is_empty()) {
                metadata.insert(
                    "info_string".to_string(),
                    serde_json::Value::String(info.to_string()),
                );
            }
            if let Some(language) = crate::markdown::blocks::fence_language(content, node) {
                metadata.insert(
                    "embedded_language".to_string(),
                    serde_json::Value::String(language),
                );
            }
        }
        _ => {}
    }

    Some(metadata)
}

fn html_tag_attributes(content: &str, node: Node<'_>) -> HashMap<String, String> {
    let mut attributes = HashMap::new();
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if !matches!(child.kind(), "start_tag" | "self_closing_tag") {
            continue;
        }
        let mut tag_cursor = child.walk();
        for tag_child in child.children(&mut tag_cursor) {
            if tag_child.kind() != "attribute" {
                continue;
            }
            if let (Some(name), value) = html_attribute_name_value(content, tag_child) {
                attributes.insert(name.to_ascii_lowercase(), value.unwrap_or_default());
            }
        }
    }
    attributes
}

fn html_attribute_name_value(
    content: &str,
    attr_node: Node<'_>,
) -> (Option<String>, Option<String>) {
    let mut name = None;
    let mut value = None;
    let mut cursor = attr_node.walk();
    for child in attr_node.children(&mut cursor) {
        match child.kind() {
            "attribute_name" => {
                name = node_text(content, child).map(str::to_string);
            }
            "attribute_value" | "quoted_attribute_value" => {
                let text = node_text(content, child).unwrap_or_default();
                value = Some(text.trim_matches(|c| c == '"' || c == '\'').to_string());
            }
            _ => {}
        }
    }
    (name, value)
}

fn insert_script_style_metadata(
    metadata: &mut HashMap<String, serde_json::Value>,
    node_kind: &str,
    attributes: &HashMap<String, String>,
) {
    for key in ["type", "lang", "src", "media"] {
        if let Some(value) = attributes.get(key).filter(|value| !value.is_empty()) {
            metadata.insert(key.to_string(), serde_json::Value::String(value.clone()));
        }
    }
    for key in ["scoped", "module"] {
        if attributes.contains_key(key) {
            let value = attributes
                .get(key)
                .filter(|value| !value.is_empty())
                .map(|value| serde_json::Value::String(value.clone()))
                .unwrap_or_else(|| serde_json::Value::Bool(true));
            metadata.insert(key.to_string(), value);
        }
    }
    if node_kind == "script_element" && attributes.contains_key("setup") {
        metadata.insert("setup".to_string(), serde_json::Value::Bool(true));
    }
}

fn embedded_language_for_script_style(
    node_kind: &str,
    attributes: &HashMap<String, String>,
) -> Option<String> {
    if node_kind == "style_element" {
        return Some(
            attributes
                .get("lang")
                .map(|value| value.trim().to_ascii_lowercase())
                .filter(|value| !value.is_empty())
                .unwrap_or_else(|| "css".to_string()),
        );
    }

    let script_type = attributes
        .get("type")
        .map(|value| value.trim().to_ascii_lowercase())
        .filter(|value| !value.is_empty());

    if let Some(script_type) = script_type {
        if matches!(
            script_type.as_str(),
            "application/ld+json" | "application/json" | "text/json"
        ) {
            return Some("json".to_string());
        }
        if !matches!(
            script_type.as_str(),
            "text/javascript"
                | "application/javascript"
                | "module"
                | "text/ecmascript"
                | "application/ecmascript"
        ) {
            return None;
        }
    }

    let lang = attributes
        .get("lang")
        .map(|value| value.trim().to_ascii_lowercase())
        .filter(|value| !value.is_empty());

    match lang.as_deref() {
        Some("ts") | Some("typescript") => Some("typescript".to_string()),
        Some("jsx") => Some("jsx".to_string()),
        Some("tsx") => Some("tsx".to_string()),
        _ => Some("javascript".to_string()),
    }
}

fn razor_block_type(content: &str, node: Node<'_>) -> Option<String> {
    let text = node_text(content, node)?;
    if text.contains("@code") {
        Some("code".to_string())
    } else if text.contains("@functions") {
        Some("functions".to_string())
    } else {
        None
    }
}

fn fenced_code_info_string(node: Node<'_>, content: &str) -> Option<String> {
    child_text(node, content, "info_string")
        .or_else(|| child_text(node, content, "language"))
        .map(str::trim)
        .filter(|info| !info.is_empty())
        .map(str::to_string)
}

fn child_text<'a>(node: Node<'_>, content: &'a str, child_kind: &str) -> Option<&'a str> {
    child_text_at_depth(node, content, child_kind, 0)
}

fn child_text_at_depth<'a>(
    node: Node<'_>,
    content: &'a str,
    child_kind: &str,
    depth: u32,
) -> Option<&'a str> {
    if !should_visit_tree_depth(depth) {
        return None;
    }

    let child_depth = child_tree_depth(depth);
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == child_kind {
            return node_text(content, child).map(str::trim);
        }
        if let Some(child_depth) = child_depth
            && let Some(text) = child_text_at_depth(child, content, child_kind, child_depth)
        {
            return Some(text);
        }
    }
    None
}

fn config_for_language(language: &str) -> Option<RegionLanguageConfig> {
    match language {
        "rust" => Some(RegionLanguageConfig {
            comment_node_kinds: &["line_comment", "block_comment"],
            string_literal_node_kinds: &["string_literal", "raw_string_literal"],
            quoted_string_literal_node_kinds: &[],
            html_comment_node_kinds: &[],
            embedded_node_kinds: &[],
        }),
        "c" | "cpp" => Some(RegionLanguageConfig {
            comment_node_kinds: &["comment"],
            string_literal_node_kinds: &["string_literal", "raw_string_literal"],
            quoted_string_literal_node_kinds: &[],
            html_comment_node_kinds: &[],
            embedded_node_kinds: &[],
        }),
        "javascript" | "jsx" => Some(RegionLanguageConfig {
            comment_node_kinds: &["comment"],
            string_literal_node_kinds: &["string", "template_string"],
            quoted_string_literal_node_kinds: &[],
            html_comment_node_kinds: &[],
            embedded_node_kinds: &[],
        }),
        "typescript" | "tsx" => Some(RegionLanguageConfig {
            comment_node_kinds: &["comment"],
            string_literal_node_kinds: &["string", "template_string"],
            quoted_string_literal_node_kinds: &[],
            html_comment_node_kinds: &[],
            embedded_node_kinds: &[],
        }),
        "python" => Some(RegionLanguageConfig {
            comment_node_kinds: &["comment"],
            string_literal_node_kinds: &["string"],
            quoted_string_literal_node_kinds: &[],
            html_comment_node_kinds: &[],
            embedded_node_kinds: &[],
        }),
        "xml" => Some(RegionLanguageConfig {
            comment_node_kinds: &["Comment"],
            string_literal_node_kinds: &["AttValue", "CData"],
            quoted_string_literal_node_kinds: &[],
            html_comment_node_kinds: &[],
            embedded_node_kinds: &[],
        }),
        "html" | "vue" => Some(RegionLanguageConfig {
            comment_node_kinds: &["comment"],
            string_literal_node_kinds: &["quoted_attribute_value"],
            quoted_string_literal_node_kinds: &[],
            html_comment_node_kinds: &[],
            embedded_node_kinds: &["script_element", "style_element"],
        }),
        "css" => Some(RegionLanguageConfig {
            comment_node_kinds: &["comment"],
            string_literal_node_kinds: &["string_value"],
            quoted_string_literal_node_kinds: &[],
            html_comment_node_kinds: &[],
            embedded_node_kinds: &[],
        }),
        "java" => Some(RegionLanguageConfig {
            comment_node_kinds: &["line_comment", "block_comment"],
            string_literal_node_kinds: &["string_literal"],
            quoted_string_literal_node_kinds: &[],
            html_comment_node_kinds: &[],
            embedded_node_kinds: &[],
        }),
        "csharp" => Some(RegionLanguageConfig {
            comment_node_kinds: &["comment"],
            string_literal_node_kinds: &[
                "string_literal",
                "verbatim_string_literal",
                "raw_string_literal",
                "interpolated_string_expression",
            ],
            quoted_string_literal_node_kinds: &[],
            html_comment_node_kinds: &[],
            embedded_node_kinds: &[],
        }),
        "vbnet" => Some(RegionLanguageConfig {
            comment_node_kinds: &["comment"],
            string_literal_node_kinds: &["string_literal"],
            quoted_string_literal_node_kinds: &[],
            html_comment_node_kinds: &[],
            embedded_node_kinds: &[],
        }),
        "go" => Some(RegionLanguageConfig {
            comment_node_kinds: &["comment"],
            string_literal_node_kinds: &["interpreted_string_literal", "raw_string_literal"],
            quoted_string_literal_node_kinds: &[],
            html_comment_node_kinds: &[],
            embedded_node_kinds: &[],
        }),
        "zig" => Some(RegionLanguageConfig {
            comment_node_kinds: &["comment"],
            string_literal_node_kinds: &["string", "multiline_string"],
            quoted_string_literal_node_kinds: &[],
            html_comment_node_kinds: &[],
            embedded_node_kinds: &[],
        }),
        "php" => Some(RegionLanguageConfig {
            comment_node_kinds: &["comment"],
            string_literal_node_kinds: &["encapsed_string", "string", "heredoc", "nowdoc"],
            quoted_string_literal_node_kinds: &[],
            html_comment_node_kinds: &[],
            embedded_node_kinds: &[],
        }),
        "ruby" => Some(RegionLanguageConfig {
            comment_node_kinds: &["comment"],
            string_literal_node_kinds: &["string", "heredoc_body"],
            quoted_string_literal_node_kinds: &[],
            html_comment_node_kinds: &[],
            embedded_node_kinds: &[],
        }),
        "swift" => Some(RegionLanguageConfig {
            comment_node_kinds: &["comment", "multiline_comment"],
            string_literal_node_kinds: &[
                "line_string_literal",
                "multi_line_string_literal",
                "raw_string_literal",
            ],
            quoted_string_literal_node_kinds: &[],
            html_comment_node_kinds: &[],
            embedded_node_kinds: &[],
        }),
        "kotlin" => Some(RegionLanguageConfig {
            comment_node_kinds: &["line_comment", "block_comment"],
            string_literal_node_kinds: &["string_literal", "multiline_string_literal"],
            quoted_string_literal_node_kinds: &[],
            html_comment_node_kinds: &[],
            embedded_node_kinds: &[],
        }),
        "scala" => Some(RegionLanguageConfig {
            comment_node_kinds: &["comment", "block_comment"],
            string_literal_node_kinds: &["string", "interpolated_string_expression"],
            quoted_string_literal_node_kinds: &[],
            html_comment_node_kinds: &[],
            embedded_node_kinds: &[],
        }),
        "dart" => Some(RegionLanguageConfig {
            comment_node_kinds: &["comment"],
            string_literal_node_kinds: &["string_literal"],
            quoted_string_literal_node_kinds: &[],
            html_comment_node_kinds: &[],
            embedded_node_kinds: &[],
        }),
        "erlang" => Some(RegionLanguageConfig {
            comment_node_kinds: &["comment"],
            string_literal_node_kinds: &["string"],
            quoted_string_literal_node_kinds: &[],
            html_comment_node_kinds: &[],
            embedded_node_kinds: &[],
        }),
        "fsharp" => Some(RegionLanguageConfig {
            comment_node_kinds: &["line_comment", "block_comment", "xml_doc"],
            string_literal_node_kinds: &[
                "string",
                "triple_quoted_string",
                "verbatim_string",
                "bytearray",
                "verbatim_bytearray",
            ],
            quoted_string_literal_node_kinds: &[],
            html_comment_node_kinds: &[],
            embedded_node_kinds: &[],
        }),
        "elixir" => Some(RegionLanguageConfig {
            comment_node_kinds: &["comment"],
            string_literal_node_kinds: &["string", "charlist"],
            quoted_string_literal_node_kinds: &[],
            html_comment_node_kinds: &[],
            embedded_node_kinds: &[],
        }),
        "lua" => Some(RegionLanguageConfig {
            comment_node_kinds: &["comment"],
            string_literal_node_kinds: &["string"],
            quoted_string_literal_node_kinds: &[],
            html_comment_node_kinds: &[],
            embedded_node_kinds: &[],
        }),
        "qml" => Some(RegionLanguageConfig {
            comment_node_kinds: &["comment"],
            string_literal_node_kinds: &["string", "template_string"],
            quoted_string_literal_node_kinds: &[],
            html_comment_node_kinds: &[],
            embedded_node_kinds: &[],
        }),
        "r" => Some(RegionLanguageConfig {
            comment_node_kinds: &["comment"],
            string_literal_node_kinds: &["string"],
            quoted_string_literal_node_kinds: &[],
            html_comment_node_kinds: &[],
            embedded_node_kinds: &[],
        }),
        "bash" => Some(RegionLanguageConfig {
            comment_node_kinds: &["comment"],
            string_literal_node_kinds: &["string", "raw_string", "ansi_c_string", "heredoc_body"],
            quoted_string_literal_node_kinds: &[],
            html_comment_node_kinds: &[],
            embedded_node_kinds: &[],
        }),
        "powershell" => Some(RegionLanguageConfig {
            comment_node_kinds: &["comment"],
            string_literal_node_kinds: &["string_literal", "expandable_string_literal"],
            quoted_string_literal_node_kinds: &[],
            html_comment_node_kinds: &[],
            embedded_node_kinds: &[],
        }),
        "gdscript" => Some(RegionLanguageConfig {
            comment_node_kinds: &["comment"],
            string_literal_node_kinds: &["string"],
            quoted_string_literal_node_kinds: &[],
            html_comment_node_kinds: &[],
            embedded_node_kinds: &[],
        }),
        "razor" => Some(RegionLanguageConfig {
            comment_node_kinds: &["razor_comment", "html_comment", "comment"],
            string_literal_node_kinds: &["string_literal"],
            quoted_string_literal_node_kinds: &[],
            html_comment_node_kinds: &[],
            embedded_node_kinds: &["razor_block"],
        }),
        "sql" => Some(RegionLanguageConfig {
            comment_node_kinds: &["comment", "marginalia"],
            string_literal_node_kinds: &[],
            quoted_string_literal_node_kinds: &["literal"],
            html_comment_node_kinds: &[],
            embedded_node_kinds: &[],
        }),
        "qmldir" => Some(RegionLanguageConfig {
            comment_node_kinds: &["comment"],
            string_literal_node_kinds: &[],
            quoted_string_literal_node_kinds: &[],
            html_comment_node_kinds: &[],
            embedded_node_kinds: &[],
        }),
        "markdown" => Some(RegionLanguageConfig {
            comment_node_kinds: &[],
            string_literal_node_kinds: &[],
            quoted_string_literal_node_kinds: &[],
            html_comment_node_kinds: &["html_block"],
            embedded_node_kinds: &["fenced_code_block"],
        }),
        "json" => Some(RegionLanguageConfig {
            comment_node_kinds: &["comment"],
            string_literal_node_kinds: &["string"],
            quoted_string_literal_node_kinds: &[],
            html_comment_node_kinds: &[],
            embedded_node_kinds: &[],
        }),
        "toml" => Some(RegionLanguageConfig {
            comment_node_kinds: &["comment"],
            string_literal_node_kinds: &["string"],
            quoted_string_literal_node_kinds: &[],
            html_comment_node_kinds: &[],
            embedded_node_kinds: &[],
        }),
        "yaml" => Some(RegionLanguageConfig {
            comment_node_kinds: &["comment"],
            string_literal_node_kinds: &[
                "double_quote_scalar",
                "single_quote_scalar",
                "block_scalar",
            ],
            quoted_string_literal_node_kinds: &[],
            html_comment_node_kinds: &[],
            embedded_node_kinds: &[],
        }),
        _ => None,
    }
}
