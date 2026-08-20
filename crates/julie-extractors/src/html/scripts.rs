use crate::base::{
    BaseExtractor, EmbeddedSpanOffset, NormalizedSpan, Symbol, SymbolKind, SymbolOptions,
    Visibility,
};
use crate::css::CSSExtractor;
use crate::javascript::JavaScriptExtractor;
use std::collections::HashMap;
use tree_sitter::Node;

use super::attributes::AttributeHandler;
use super::helpers::HTMLHelpers;

/// Script and style tag extraction
pub(super) struct ScriptStyleExtractor;

impl ScriptStyleExtractor {
    /// Extract a script element and create a symbol
    pub(super) fn extract_script_element(
        base: &mut BaseExtractor,
        node: Node,
        parent_id: Option<&str>,
        mocha_bdd_contract: bool,
    ) -> Vec<Symbol> {
        let attributes = HTMLHelpers::extract_attributes(base, node);
        let content = HTMLHelpers::extract_text_content(base, node);

        // Only delegate to the embedded JS extractor when the script type is
        // actually JavaScript.  Non-JS types (e.g. application/ld+json,
        // text/html, text/template) should produce a script-tag symbol so that
        // attributes like `type` are preserved in the symbol's signature.
        let is_javascript = is_javascript_script_type(&attributes);

        if !attributes.contains_key("src") && is_javascript {
            let symbols = content
                .as_deref()
                .map(|content| {
                    extract_embedded_javascript_symbols(base, node, content, mocha_bdd_contract)
                })
                .unwrap_or_default();
            if !symbols.is_empty() {
                return symbols;
            }
        }

        let signature =
            AttributeHandler::build_element_signature("script", &attributes, content.as_deref());

        // Determine symbol kind based on src attribute
        let symbol_kind = if attributes.contains_key("src") {
            SymbolKind::Import
        } else {
            SymbolKind::Variable
        };

        let mut metadata = HashMap::new();
        metadata.insert(
            "type".to_string(),
            serde_json::Value::String("script-element".to_string()),
        );
        metadata.insert(
            "isInline".to_string(),
            serde_json::Value::Bool(!attributes.contains_key("src")),
        );

        if !attributes.is_empty() {
            metadata.insert(
                "attributes".to_string(),
                serde_json::to_value(&attributes).unwrap_or_default(),
            );
        }

        let script_type = attributes
            .get("type")
            .cloned()
            .unwrap_or_else(|| "text/javascript".to_string());
        metadata.insert(
            "scriptType".to_string(),
            serde_json::Value::String(script_type),
        );

        if let Some(content) = content {
            // Safely truncate UTF-8 string at character boundary
            let truncated_content = BaseExtractor::truncate_string(&content, 100);
            metadata.insert(
                "content".to_string(),
                serde_json::Value::String(truncated_content),
            );
        }

        // Extract HTML comment
        let doc_comment = base.find_doc_comment(&node);

        vec![base.create_symbol(
            &node,
            "script".to_string(),
            symbol_kind,
            SymbolOptions {
                signature: Some(signature),
                visibility: Some(Visibility::Public),
                parent_id: parent_id.map(|s| s.to_string()),
                metadata: Some(metadata),
                doc_comment,
                annotations: Vec::new(),
            },
        )]
    }

    /// Extract a style element and create a symbol
    pub(super) fn extract_style_element(
        base: &mut BaseExtractor,
        node: Node,
        parent_id: Option<&str>,
    ) -> Vec<Symbol> {
        let attributes = HTMLHelpers::extract_attributes(base, node);
        let content = HTMLHelpers::extract_text_content(base, node);

        // Always extract the style-tag symbol so that preceding HTML comments
        // (<!-- … -->) can be attached to it as doc_comment.  Embedded CSS
        // symbols (class selectors, custom properties, etc.) are appended
        // afterwards so callers that look for either find what they expect.
        let embedded_css_symbols = content
            .as_deref()
            .map(|content| extract_embedded_css_symbols(base, node, content))
            .unwrap_or_default();

        let signature =
            AttributeHandler::build_element_signature("style", &attributes, content.as_deref());

        let mut metadata = HashMap::new();
        metadata.insert(
            "type".to_string(),
            serde_json::Value::String("style-element".to_string()),
        );
        metadata.insert("isInline".to_string(), serde_json::Value::Bool(true));

        if !attributes.is_empty() {
            metadata.insert(
                "attributes".to_string(),
                serde_json::to_value(&attributes).unwrap_or_default(),
            );
        }

        if let Some(ref content) = content {
            // Safely truncate UTF-8 string at character boundary
            let truncated_content = BaseExtractor::truncate_string(content, 100);
            metadata.insert(
                "content".to_string(),
                serde_json::Value::String(truncated_content),
            );
        }

        // Extract HTML comment (e.g. <!-- Theme overrides for dark mode -->)
        let doc_comment = base.find_doc_comment(&node);

        let style_symbol = base.create_symbol(
            &node,
            "style".to_string(),
            SymbolKind::Variable,
            SymbolOptions {
                signature: Some(signature),
                visibility: Some(Visibility::Public),
                parent_id: parent_id.map(|s| s.to_string()),
                metadata: Some(metadata),
                doc_comment,
                annotations: Vec::new(),
            },
        );

        let mut result = vec![style_symbol];
        result.extend(embedded_css_symbols);
        result
    }
}

pub(super) fn is_javascript_script_type(attributes: &HashMap<String, String>) -> bool {
    let script_type = attributes.get("type").map(String::as_str).unwrap_or("");
    script_type.is_empty()
        || matches!(
            script_type,
            "text/javascript"
                | "application/javascript"
                | "module"
                | "text/ecmascript"
                | "application/ecmascript"
        )
}

pub(super) fn is_mocha_script_source(source: &str) -> bool {
    let source = source.split(['?', '#']).next().unwrap_or(source);
    source.rsplit('/').next() == Some("mocha.js")
}

pub(super) fn contains_mocha_bdd_setup(content: &str) -> bool {
    let mut parser = tree_sitter::Parser::new();
    if parser
        .set_language(&tree_sitter_javascript::LANGUAGE.into())
        .is_err()
    {
        return false;
    }
    let Some(tree) = parser.parse(content, None) else {
        return false;
    };

    let mut nodes = vec![tree.root_node()];
    while let Some(node) = nodes.pop() {
        if node.kind() == "call_expression"
            && let Some(function) = node.child_by_field_name("function")
            && function.kind() == "member_expression"
            && function
                .child_by_field_name("object")
                .is_some_and(|object| node_text(content, object) == "mocha")
            && function
                .child_by_field_name("property")
                .is_some_and(|property| node_text(content, property) == "setup")
        {
            let arguments = node.child_by_field_name("arguments");
            if !arguments.is_some_and(|arguments| {
                arguments
                    .named_child(0)
                    .is_some_and(|argument| is_bdd_setup_argument(content, argument))
            }) {
                continue;
            }
            return true;
        }

        let mut cursor = node.walk();
        nodes.extend(node.children(&mut cursor));
    }

    false
}

fn is_bdd_setup_argument(content: &str, argument: Node) -> bool {
    if js_string_value(node_text(content, argument).as_str()) == Some("bdd") {
        return true;
    }
    if argument.kind() != "object" {
        return false;
    }

    let mut cursor = argument.walk();
    argument.named_children(&mut cursor).any(|pair| {
        pair.kind() == "pair"
            && pair.child_by_field_name("key").is_some_and(|key| {
                let key_text = node_text(content, key);
                key_text.trim() == "ui" || js_string_value(key_text.as_str()) == Some("ui")
            })
            && pair.child_by_field_name("value").is_some_and(|value| {
                js_string_value(node_text(content, value).as_str()) == Some("bdd")
            })
    })
}

fn node_text(content: &str, node: Node) -> String {
    content[node.byte_range()].to_string()
}

fn js_string_value(text: &str) -> Option<&str> {
    let text = text.trim();
    let bytes = text.as_bytes();
    if bytes.len() >= 2
        && matches!(bytes[0], b'\'' | b'"' | b'`')
        && bytes.last() == Some(&bytes[0])
    {
        Some(&text[1..text.len() - 1])
    } else {
        None
    }
}

fn extract_embedded_javascript_symbols(
    base: &BaseExtractor,
    node: Node,
    content: &str,
    mocha_bdd_contract: bool,
) -> Vec<Symbol> {
    let mut parser = tree_sitter::Parser::new();
    if parser
        .set_language(&tree_sitter_javascript::LANGUAGE.into())
        .is_err()
    {
        return Vec::new();
    }
    let Some(tree) = parser.parse(content, None) else {
        return Vec::new();
    };
    let mut extractor = JavaScriptExtractor::new(
        "javascript".to_string(),
        base.file_path.clone(),
        content.to_string(),
        std::path::Path::new(""),
    );
    let mut symbols = extractor.extract_symbols(&tree);
    symbols.retain(|symbol| {
        !has_test_role_metadata(symbol) || (mocha_bdd_contract && is_supported_mocha_role(symbol))
    });
    let Some(offset) = embedded_content_offset(base, node, content) else {
        return Vec::new();
    };
    apply_embedded_offsets(&mut symbols, base, offset);
    symbols
}

fn has_test_role_metadata(symbol: &Symbol) -> bool {
    symbol.metadata.as_ref().is_some_and(|metadata| {
        metadata.contains_key("is_test") || metadata.contains_key("test_container")
    })
}

fn is_supported_mocha_role(symbol: &Symbol) -> bool {
    let Some(metadata) = symbol.metadata.as_ref() else {
        return false;
    };
    let Some(signature) = symbol.signature.as_deref() else {
        return false;
    };
    let Some(callee) = signature.split('(').next() else {
        return false;
    };

    if metadata
        .get("test_container")
        .and_then(|value| value.as_bool())
        == Some(true)
    {
        return matches!(callee, "describe" | "context");
    }
    if metadata
        .get("test_lifecycle")
        .and_then(|value| value.as_bool())
        == Some(true)
    {
        return matches!(callee, "before" | "after" | "beforeEach" | "afterEach");
    }
    metadata.get("is_test").and_then(|value| value.as_bool()) == Some(true) && callee == "it"
}

fn extract_embedded_css_symbols(base: &BaseExtractor, node: Node, content: &str) -> Vec<Symbol> {
    let mut parser = tree_sitter::Parser::new();
    if parser
        .set_language(&tree_sitter_css::LANGUAGE.into())
        .is_err()
    {
        return Vec::new();
    }
    let Some(tree) = parser.parse(content, None) else {
        return Vec::new();
    };
    let mut extractor = CSSExtractor::new(
        "css".to_string(),
        base.file_path.clone(),
        content.to_string(),
        std::path::Path::new(""),
    );
    let mut symbols = extractor.extract_symbols(&tree);
    let Some(offset) = embedded_content_offset(base, node, content) else {
        return Vec::new();
    };
    apply_embedded_offsets(&mut symbols, base, offset);
    symbols
}

fn embedded_content_offset(base: &BaseExtractor, node: Node, _content: &str) -> Option<u32> {
    let content_node = node
        .children(&mut node.walk())
        .find(|child| matches!(child.kind(), "text" | "raw_text"))?;
    let raw_content = base.get_node_text(&content_node);
    let trimmed_start = raw_content.trim_start();
    if trimmed_start.is_empty() {
        return None;
    }

    let leading_trim_bytes = raw_content.len() - trimmed_start.len();
    Some((content_node.start_byte() + leading_trim_bytes) as u32)
}

fn apply_embedded_offsets(symbols: &mut [Symbol], base: &BaseExtractor, byte_offset: u32) {
    let Some(offset) = EmbeddedSpanOffset::from_host_byte(&base.content, byte_offset as usize)
    else {
        return;
    };

    let mut symbol_id_map = HashMap::new();
    for symbol in symbols.iter_mut() {
        let old_id = symbol.id.clone();
        let span = NormalizedSpan {
            start_line: symbol.start_line,
            start_column: symbol.start_column,
            end_line: symbol.end_line,
            end_column: symbol.end_column,
            start_byte: symbol.start_byte,
            end_byte: symbol.end_byte,
        };
        symbol.file_path = base.file_path.clone();
        symbol.apply_normalized_span(offset.apply(span));
        symbol.refresh_id();
        symbol_id_map.insert(old_id, symbol.id.clone());
    }

    for symbol in symbols {
        if let Some(parent_id) = symbol.parent_id.as_mut()
            && let Some(new_parent_id) = symbol_id_map.get(parent_id)
        {
            *parent_id = new_parent_id.clone();
        }
    }
}
