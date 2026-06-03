use std::collections::HashMap;

use tree_sitter::{Node, Tree};

use super::span::NormalizedSpan;
use super::types::{SourceRegion, SourceRegionKind, Symbol, stable_location_id};

#[derive(Debug, Clone, Copy)]
struct RegionLanguageConfig {
    comment_node_kinds: &'static [&'static str],
    string_literal_node_kinds: &'static [&'static str],
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
            embedded_metadata(node_kind),
        ));
    } else if config.comment_node_kinds.contains(&node_kind) {
        let text = node_text(content, node);
        let kind = if is_doc_comment(language, text.unwrap_or_default()) {
            SourceRegionKind::DocComment
        } else {
            SourceRegionKind::Comment
        };
        regions.push(region_for_node(file_path, language, node, kind, None));
    } else if config.string_literal_node_kinds.contains(&node_kind) {
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
    let trimmed = text.trim_start();
    match language {
        "rust" => {
            trimmed.starts_with("///")
                || trimmed.starts_with("//!")
                || trimmed.starts_with("/**")
                || trimmed.starts_with("/*!")
        }
        "javascript" | "jsx" | "typescript" | "tsx" | "java" | "c" | "cpp" | "csharp" => {
            trimmed.starts_with("/**") || trimmed.starts_with("///")
        }
        _ => false,
    }
}

fn embedded_metadata(node_kind: &str) -> Option<HashMap<String, serde_json::Value>> {
    let embedded_language = match node_kind {
        "script_element" => "javascript",
        "style_element" => "css",
        _ => return None,
    };
    Some(HashMap::from([
        (
            "embedded_language".to_string(),
            serde_json::Value::String(embedded_language.to_string()),
        ),
        (
            "host_node_kind".to_string(),
            serde_json::Value::String(node_kind.to_string()),
        ),
    ]))
}

fn config_for_language(language: &str) -> Option<RegionLanguageConfig> {
    match language {
        "rust" => Some(RegionLanguageConfig {
            comment_node_kinds: &["line_comment", "block_comment"],
            string_literal_node_kinds: &["string_literal", "raw_string_literal"],
            embedded_node_kinds: &[],
        }),
        "javascript" | "jsx" => Some(RegionLanguageConfig {
            comment_node_kinds: &["comment"],
            string_literal_node_kinds: &["string", "template_string"],
            embedded_node_kinds: &[],
        }),
        "typescript" | "tsx" => Some(RegionLanguageConfig {
            comment_node_kinds: &["comment"],
            string_literal_node_kinds: &["string", "template_string"],
            embedded_node_kinds: &[],
        }),
        "python" => Some(RegionLanguageConfig {
            comment_node_kinds: &["comment"],
            string_literal_node_kinds: &["string"],
            embedded_node_kinds: &[],
        }),
        "html" | "vue" => Some(RegionLanguageConfig {
            comment_node_kinds: &["comment"],
            string_literal_node_kinds: &["quoted_attribute_value"],
            embedded_node_kinds: &["script_element", "style_element"],
        }),
        _ => None,
    }
}
