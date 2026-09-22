use crate::base::{
    BaseExtractor, ExtractionLevel, ExtractionResults, Symbol, SymbolKind, SymbolOptions,
    Visibility,
};
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
        embedded: &mut Vec<ExtractionResults>,
    ) -> Vec<Symbol> {
        let attributes = HTMLHelpers::extract_attributes(base, node);
        let content = HTMLHelpers::extract_text_content(base, node);

        // Only delegate to the embedded JS extractor when the script type is
        // actually JavaScript.  Non-JS types (e.g. application/ld+json,
        // text/html, text/template) should produce a script-tag symbol so that
        // attributes like `type` are preserved in the symbol's signature.
        let is_javascript = is_javascript_script_type(&attributes);

        if !attributes.contains_key("src")
            && is_javascript
            && let Some(mut results) = extract_embedded_block(base, node, "javascript")
        {
            crate::embedded::retain_symbols(&mut results, |symbol| {
                !has_test_role_metadata(symbol)
                    || (mocha_bdd_contract && is_supported_mocha_role(symbol))
            });
            let symbols = std::mem::take(&mut results.symbols);
            embedded.push(results);
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
        embedded: &mut Vec<ExtractionResults>,
    ) -> Vec<Symbol> {
        let attributes = HTMLHelpers::extract_attributes(base, node);
        let content = HTMLHelpers::extract_text_content(base, node);

        // Always extract the style-tag symbol so that preceding HTML comments
        // (<!-- … -->) can be attached to it as doc_comment.  Embedded CSS
        // symbols (class selectors, custom properties, etc.) are appended
        // afterwards so callers that look for either find what they expect.
        let embedded_css_symbols = extract_embedded_block(base, node, "css")
            .map(|mut results| {
                let symbols = std::mem::take(&mut results.symbols);
                embedded.push(results);
                symbols
            })
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

/// Runs the full `language` pipeline over the element's raw text, in host
/// coordinates, with structural facts published under the `html` language.
fn extract_embedded_block(
    base: &BaseExtractor,
    node: Node,
    language: &str,
) -> Option<ExtractionResults> {
    let mut cursor = node.walk();
    let raw_text = node
        .children(&mut cursor)
        .find(|child| matches!(child.kind(), "text" | "raw_text"))?;
    let source = base.content.get(raw_text.byte_range())?;
    if source.trim().is_empty() {
        return None;
    }
    let mut results = crate::embedded::extract_embedded(
        language,
        source,
        &base.content,
        raw_text.start_byte(),
        &base.file_path,
        std::path::Path::new(""),
        ExtractionLevel::Full,
    )?;
    for fact in &mut results.structural_facts {
        fact.language = base.language.clone();
    }
    Some(results)
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
