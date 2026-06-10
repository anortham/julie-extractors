use std::collections::HashMap;

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

pub fn collect_source_regions(
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
    symbols: &[Symbol],
) -> Vec<SourceRegion> {
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
    );
    attach_containing_symbols(&mut regions, symbols);
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
) {
    let node_kind = node.kind();
    if config.embedded_node_kinds.contains(&node_kind) {
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
    } else if config.string_literal_node_kinds.contains(&node_kind) {
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

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_node(child, language, file_path, content, config, regions);
    }
}

fn region_for_node(
    file_path: &str,
    language: &str,
    node: Node<'_>,
    kind: SourceRegionKind,
    metadata: Option<HashMap<String, serde_json::Value>>,
) -> SourceRegion {
    let span = NormalizedSpan::from_node(&node);
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

fn attach_containing_symbols(regions: &mut [SourceRegion], symbols: &[Symbol]) {
    for region in regions {
        region.containing_symbol_id = match region.kind {
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
    symbols
        .iter()
        .filter(|symbol| symbol.doc_comment.is_some())
        .filter(|symbol| symbol.start_byte >= region.end_byte)
        .min_by_key(|symbol| symbol.start_byte.saturating_sub(region.end_byte))
        .map(|symbol| symbol.id.clone())
}

fn node_text<'a>(content: &'a str, node: Node<'_>) -> Option<&'a str> {
    content.get(node.start_byte()..node.end_byte())
}

fn is_doc_comment(language: &str, text: &str) -> bool {
    crate::language_spec::language_spec(language).is_some_and(|spec| spec.is_doc_comment(text))
}

fn is_yaml_key_attached_comment(content: &str, comment_node: Node<'_>) -> bool {
    if comment_node.kind() != "comment" {
        return false;
    }
    let Some(text) = node_text(content, comment_node) else {
        return false;
    };
    if !text.trim_start().starts_with('#') {
        return false;
    }
    // Multi-line header blocks stay ordinary comments.
    if comment_node
        .next_sibling()
        .is_some_and(|sibling| sibling.kind() == "comment")
    {
        return false;
    }

    let key_column = comment_node.start_position().column;
    let mut next = comment_node.next_sibling();
    while let Some(sibling) = next {
        if sibling.kind() == "blank_line" {
            next = sibling.next_sibling();
            continue;
        }
        if sibling.kind() != "block_mapping_pair" {
            return false;
        }
        return !yaml_pair_has_nested_mapping(sibling)
            && yaml_mapping_key_start_column(content, sibling) == Some(key_column);
    }
    false
}

fn yaml_pair_has_nested_mapping(pair: Node<'_>) -> bool {
    let mut cursor = pair.walk();
    for child in pair.children(&mut cursor) {
        if child.kind() != "block_node" {
            continue;
        }
        let mut block_cursor = child.walk();
        for block_child in child.children(&mut block_cursor) {
            if block_child.kind() == "block_mapping" {
                return true;
            }
        }
    }
    false
}

fn yaml_mapping_key_start_column(_content: &str, pair: Node<'_>) -> Option<usize> {
    let mut cursor = pair.walk();
    for child in pair.children(&mut cursor) {
        if !matches!(child.kind(), "flow_node" | "block_node") {
            continue;
        }
        let mut key_cursor = child.walk();
        for key_child in child.children(&mut key_cursor) {
            if matches!(
                key_child.kind(),
                "plain_scalar" | "single_quote_scalar" | "double_quote_scalar"
            ) {
                return Some(key_child.start_position().column);
            }
        }
    }
    None
}

fn is_html_comment(text: &str) -> bool {
    text.trim_start().starts_with("<!--")
}

fn is_quoted_string_literal(text: &str) -> bool {
    let trimmed = text.trim_start();
    trimmed.starts_with('"') || trimmed.starts_with('\'')
}

fn embedded_metadata(node: Node<'_>, content: &str) -> Option<HashMap<String, serde_json::Value>> {
    let node_kind = node.kind();
    let mut metadata = HashMap::from([(
        "host_node_kind".to_string(),
        serde_json::Value::String(node_kind.to_string()),
    )]);

    let embedded_language = match node_kind {
        "script_element" => Some("javascript".to_string()),
        "style_element" => Some("css".to_string()),
        "razor_block" => Some("csharp".to_string()),
        "fenced_code_block" => child_text(node, content, "language")
            .filter(|language| !language.is_empty())
            .map(str::to_string),
        _ => None,
    };

    if let Some(embedded_language) = embedded_language {
        metadata.insert(
            "embedded_language".to_string(),
            serde_json::Value::String(embedded_language),
        );
    }
    Some(metadata)
}

fn child_text<'a>(node: Node<'_>, content: &'a str, child_kind: &str) -> Option<&'a str> {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == child_kind {
            return node_text(content, child).map(str::trim);
        }
        if let Some(text) = child_text(child, content, child_kind) {
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
            comment_node_kinds: &["comment"],
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
            string_literal_node_kinds: &["string"],
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
