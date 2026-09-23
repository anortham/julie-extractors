// CSS Extractor At-Rules - Extract @media, @import, @keyframes, etc.

use crate::base::{
    BaseExtractor, RelationshipKind, StructuredPendingRelationship, Symbol, SymbolKind,
    SymbolOptions, UnresolvedTarget, Visibility,
};
use std::collections::HashMap;
use tree_sitter::Node;

pub(super) struct AtRuleExtractor;

impl AtRuleExtractor {
    /// Extract at-rule - Implementation of extractAtRule
    pub(super) fn extract_at_rule(
        base: &mut BaseExtractor,
        node: Node,
        parent_id: Option<&str>,
    ) -> Option<Symbol> {
        if node.kind() == "import_statement" {
            return Self::extract_import(base, node, parent_id);
        }
        let keyword = at_rule_keyword(base, node)?;
        let prelude = at_rule_prelude(&base.content, node);
        let font_family = (keyword.eq_ignore_ascii_case("@font-face"))
            .then(|| font_face_family(&base.content, node))
            .flatten();
        let rule_name = match prelude.as_deref().or(font_family.as_deref()) {
            Some(detail) => format!("{keyword} {detail}"),
            None => keyword.clone(),
        };
        let signature = base.get_node_text(&node);
        let is_registration = keyword.eq_ignore_ascii_case("@property");
        let symbol_kind = if is_registration {
            SymbolKind::Property
        } else if node.kind() == "charset_statement" {
            SymbolKind::Variable
        } else {
            SymbolKind::Namespace
        };

        let mut metadata = HashMap::new();
        let mut insert = |key: &str, value: &str| {
            metadata.insert(
                key.to_string(),
                serde_json::Value::String(value.to_string()),
            );
        };
        insert("type", "at-rule");
        insert("ruleName", &rule_name);
        insert("atRuleType", keyword.strip_prefix('@').unwrap_or(&keyword));
        if let Some(prelude) = prelude.as_deref() {
            insert("prelude", prelude);
            if is_registration {
                insert("property", prelude);
            }
        }
        if let Some(font_family) = font_family.as_deref() {
            insert("fontFamily", font_family);
        }

        // Extract CSS comment
        let doc_comment = base.find_doc_comment(&node);

        Some(base.create_symbol(
            &node,
            rule_name,
            symbol_kind,
            SymbolOptions {
                signature: Some(signature),
                visibility: Some(Visibility::Public),
                parent_id: parent_id.map(|id| id.to_string()),
                metadata: Some(metadata),
                doc_comment,
                annotations: Vec::new(),
            },
        ))
    }

    /// An `@import` becomes an import symbol named by its target path, like a C
    /// `#include`, plus an `imports` pending edge to that raw target.
    fn extract_import(
        base: &mut BaseExtractor,
        node: Node,
        parent_id: Option<&str>,
    ) -> Option<Symbol> {
        let (url, media) = import_target(&base.content, node)?;

        let mut metadata = HashMap::new();
        for (key, value) in [
            ("type", "at-rule"),
            ("ruleName", "@import"),
            ("atRuleType", "import"),
            ("url", url.as_str()),
        ] {
            metadata.insert(
                key.to_string(),
                serde_json::Value::String(value.to_string()),
            );
        }
        if let Some(media) = media {
            metadata.insert("media".to_string(), serde_json::Value::String(media));
        }

        let doc_comment = base.find_doc_comment(&node);
        let mut symbol = base.create_symbol(
            &node,
            url.clone(),
            SymbolKind::Import,
            SymbolOptions {
                signature: Some(base.get_node_text(&node)),
                visibility: Some(Visibility::Public),
                parent_id: parent_id.map(|id| id.to_string()),
                metadata: Some(metadata),
                doc_comment,
                annotations: Vec::new(),
            },
        );
        symbol.body_span = None;
        symbol.body_hash = None;

        let mut target = UnresolvedTarget::simple(url);
        target.import_context = Some("css-import".to_string());
        base.add_structured_pending_relationship(StructuredPendingRelationship::new(
            symbol.id.clone(),
            target,
            Some(symbol.id.clone()),
            RelationshipKind::Imports,
            base.file_path.clone(),
            symbol.start_line,
            0.9,
        ));
        Some(symbol)
    }
}

/// The at-keyword (`@layer`, `@scope`, `@charset`) that opens an at-rule node.
pub(super) fn at_rule_keyword(base: &BaseExtractor, node: Node) -> Option<String> {
    let mut cursor = node.walk();
    node.children(&mut cursor)
        .map(|child| base.get_node_text(&child))
        .find(|text| text.starts_with('@'))
}

/// The whitespace-collapsed text between the at-keyword and the block or `;`.
pub(super) fn at_rule_prelude(content: &str, node: Node) -> Option<String> {
    let mut cursor = node.walk();
    let children: Vec<Node> = node.children(&mut cursor).collect();
    let keyword_end = children.iter().position(|child| {
        content
            .get(child.byte_range())
            .is_some_and(|t| t.starts_with('@'))
    })?;
    let rest: Vec<&Node> = children[keyword_end + 1..]
        .iter()
        .take_while(|child| child.kind() != "block" && child.kind() != ";")
        .collect();
    let (first, last) = (rest.first()?, rest.last()?);
    let text = content.get(first.start_byte()..last.end_byte())?;
    let collapsed = text.split_whitespace().collect::<Vec<_>>().join(" ");
    (!collapsed.is_empty()).then_some(collapsed)
}

/// The unquoted `font-family` descriptor of an `@font-face` block.
pub(crate) fn font_face_family(content: &str, node: Node) -> Option<String> {
    let block = child_of_kind(node, "block")?;
    let mut cursor = block.walk();
    let declaration = block.named_children(&mut cursor).find(|child| {
        child.kind() == "declaration"
            && child_of_kind(*child, "property_name")
                .and_then(|name| content.get(name.byte_range()))
                .is_some_and(|name| name.eq_ignore_ascii_case("font-family"))
    })?;
    let name = child_of_kind(declaration, "property_name")?;
    let value = content
        .get(name.end_byte()..declaration.end_byte())?
        .trim_start()
        .trim_start_matches(':')
        .trim()
        .trim_end_matches(';')
        .trim()
        .trim_matches(|c| c == '"' || c == '\'')
        .to_string();
    (!value.is_empty()).then_some(value)
}

fn child_of_kind<'t>(node: Node<'t>, kind: &str) -> Option<Node<'t>> {
    let mut cursor = node.walk();
    node.children(&mut cursor)
        .find(|child| child.kind() == kind)
}

/// The unquoted `@import` path (a string, or `url(...)` holding a string or a
/// bare path) and the media/supports/layer text that follows it.
pub(crate) fn import_target(content: &str, node: Node<'_>) -> Option<(String, Option<String>)> {
    let mut cursor = node.walk();
    let target = node
        .named_children(&mut cursor)
        .find(|child| matches!(child.kind(), "string_value" | "call_expression"))?;
    let value_node = if target.kind() == "call_expression" {
        let arguments = target.child_by_field_name("arguments").or_else(|| {
            let mut cursor = target.walk();
            target
                .named_children(&mut cursor)
                .find(|child| child.kind() == "arguments")
        })?;
        let mut cursor = arguments.walk();
        arguments.named_children(&mut cursor).next()?
    } else {
        target
    };
    let url = content
        .get(value_node.byte_range())?
        .trim()
        .trim_matches(|c| c == '"' || c == '\'')
        .trim()
        .to_string();
    let media = content
        .get(target.end_byte()..node.end_byte())?
        .trim()
        .trim_end_matches(';')
        .trim();
    (!url.is_empty()).then(|| (url, (!media.is_empty()).then(|| media.to_string())))
}
