/// Razor-specific directive extraction (e.g., @page, @model, @using, @inject)
use crate::base::{BaseExtractor, Symbol, SymbolKind, SymbolOptions, Visibility};
use regex::Regex;
use std::collections::HashMap;
use std::sync::LazyLock;
use tree_sitter::Node;

// Static regexes compiled once for performance
static DIRECTIVE_NAME_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"@(\w+)").unwrap());
static ADD_TAG_HELPER_VALUE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"@addTagHelper\s+(.+)").unwrap());
static DIRECTIVE_VALUE_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"@\w+\s+(.*)").unwrap());

impl super::RazorExtractor {
    /// Extract Razor directives (@page, @model, @using, etc.)
    pub(super) fn extract_directive(
        &mut self,
        node: Node,
        parent_id: Option<&str>,
    ) -> Option<Symbol> {
        let directive_name = self.extract_directive_name(node)?;
        let directive_value = self.extract_directive_value(node);

        let mut signature = format!("@{}", directive_name);
        if let Some(value) = &directive_value {
            signature.push_str(&format!(" {}", value));
        }

        let symbol_kind = if directive_name == "model" {
            SymbolKind::Property
        } else {
            self.get_directive_symbol_kind(&directive_name)
        };

        let using_alias = (node.kind() == "razor_using_directive")
            .then(|| node.child_by_field_name("name"))
            .flatten()
            .filter(|alias| alias.next_sibling().is_some_and(|next| next.kind() == "="))
            .map(|alias| self.base.get_node_text(&alias));
        let signature = match &using_alias {
            Some(alias) => signature.replacen("@using ", &format!("@using {alias} = "), 1),
            None => signature,
        };

        // For certain directives, use the value as the symbol name
        let symbol_name = match directive_name.as_str() {
            "using" => using_alias
                .clone()
                .or_else(|| directive_value.clone())
                .unwrap_or_else(|| format!("@{}", directive_name)),
            "inject" => {
                // Extract property name from "@inject IService PropertyName"
                if let Some(value) = &directive_value {
                    let parts: Vec<&str> = value.split_whitespace().collect();
                    if parts.len() >= 2 {
                        parts.last().unwrap().to_string()
                    } else {
                        format!("@{}", directive_name)
                    }
                } else {
                    format!("@{}", directive_name)
                }
            }
            "model" => "Model".to_string(),
            _ => format!("@{}", directive_name),
        };

        // Extract Razor doc comment
        let doc_comment = self.base.find_doc_comment(&node);

        let mut symbol = self.base.create_symbol(
            &node,
            symbol_name,
            symbol_kind,
            SymbolOptions {
                signature: Some(signature),
                visibility: Some(Visibility::Public),
                parent_id: parent_id.map(|s| s.to_string()),
                metadata: Some({
                    let mut metadata = HashMap::new();
                    metadata.insert(
                        "type".to_string(),
                        serde_json::Value::String("razor-directive".to_string()),
                    );
                    metadata.insert(
                        "directiveName".to_string(),
                        serde_json::Value::String(directive_name.clone()),
                    );
                    if let Some(value) = directive_value {
                        metadata.insert(
                            "directiveValue".to_string(),
                            serde_json::Value::String(value),
                        );
                    }
                    if let Some(alias) = using_alias {
                        metadata.insert("alias".to_string(), serde_json::Value::String(alias));
                    }
                    metadata
                }),
                doc_comment,
                annotations: Vec::new(),
            },
        );
        symbol.body_span = None;
        symbol.body_hash = None;
        if let Some(type_node) = self.directive_declared_type_node(node) {
            super::type_facts::record_declared_type(&mut self.base, &symbol.id, type_node);
        }
        Some(symbol)
    }

    /// The type an `@inject` property or the `@model` property is declared with.
    fn directive_declared_type_node<'a>(&self, node: Node<'a>) -> Option<Node<'a>> {
        match node.kind() {
            "razor_inject_directive" => self
                .find_child_by_type(node, "variable_declaration")?
                .child_by_field_name("type"),
            "razor_model_directive" => directive_type_operand(node),
            _ => None,
        }
    }

    /// Extract directive name from node kind or text
    pub(super) fn extract_directive_name(&self, node: Node) -> Option<String> {
        match node.kind() {
            "razor_page_directive" => Some("page".to_string()),
            "razor_model_directive" => Some("model".to_string()),
            "razor_using_directive" => Some("using".to_string()),
            "razor_inject_directive" => Some("inject".to_string()),
            "razor_attribute_directive" => Some("attribute".to_string()),
            "razor_namespace_directive" => Some("namespace".to_string()),
            "razor_inherits_directive" => Some("inherits".to_string()),
            "razor_implements_directive" => Some("implements".to_string()),
            "razor_addtaghelper_directive" => Some("addTagHelper".to_string()),
            _ => {
                let text = self.base.get_node_text(&node);
                if text.contains("@addTagHelper") {
                    Some("addTagHelper".to_string())
                } else {
                    DIRECTIVE_NAME_RE
                        .captures(&text)
                        .map(|captures| captures[1].to_string())
                }
            }
        }
    }

    /// Extract directive value from node
    pub(super) fn extract_directive_value(&self, node: Node) -> Option<String> {
        match node.kind() {
            "razor_page_directive" => self
                .find_child_by_type(node, "string_literal")
                .map(|n| self.base.get_node_text(&n)),
            "razor_model_directive" | "razor_inherits_directive" | "razor_implements_directive" => {
                directive_type_operand(node).map(|n| self.base.get_node_text(&n))
            }
            "razor_using_directive" | "razor_namespace_directive" => {
                let mut cursor = node.walk();
                node.named_children(&mut cursor)
                    .filter(|child| {
                        matches!(
                            child.kind(),
                            "qualified_name" | "identifier" | "generic_name"
                        )
                    })
                    .last()
                    .map(|n| self.base.get_node_text(&n))
            }
            "razor_inject_directive" => self
                .find_child_by_type(node, "variable_declaration")
                .map(|n| self.base.get_node_text(&n)),
            "razor_attribute_directive" => self
                .find_child_by_type(node, "attribute_list")
                .map(|n| self.base.get_node_text(&n)),
            "razor_addtaghelper_directive" => {
                let text = self.base.get_node_text(&node);
                ADD_TAG_HELPER_VALUE_RE
                    .captures(&text)
                    .map(|captures| captures[1].trim().to_string())
            }
            _ => {
                let text = self.base.get_node_text(&node);
                if text.contains("@addTagHelper") {
                    ADD_TAG_HELPER_VALUE_RE
                        .captures(&text)
                        .map(|captures| captures[1].trim().to_string())
                } else {
                    DIRECTIVE_VALUE_RE
                        .captures(&text)
                        .map(|captures| captures[1].trim().to_string())
                }
            }
        }
    }

    /// Map directive name to symbol kind
    pub(super) fn get_directive_symbol_kind(&self, directive_name: &str) -> SymbolKind {
        match directive_name.to_lowercase().as_str() {
            "model" | "layout" => SymbolKind::Class,
            "page" | "using" | "namespace" => SymbolKind::Import,
            "inherits" | "implements" => SymbolKind::Import,
            "inject" | "attribute" => SymbolKind::Property,
            "code" | "functions" => SymbolKind::Function,
            _ => SymbolKind::Variable,
        }
    }

    /// Extract section (@section) directives
    pub(super) fn extract_section(
        &mut self,
        node: Node,
        parent_id: Option<&str>,
    ) -> Option<Symbol> {
        let identifier_node = self.find_child_by_type(node, "identifier")?;
        let section_name = self.base.get_node_text(&identifier_node);
        let signature = format!("@section {}", section_name);

        // Extract Razor doc comment
        let doc_comment = self.base.find_doc_comment(&node);

        Some(self.base.create_symbol(
            &node,
            section_name,
            SymbolKind::Module,
            SymbolOptions {
                signature: Some(signature),
                visibility: Some(Visibility::Public),
                parent_id: parent_id.map(|s| s.to_string()),
                metadata: Some({
                    let mut metadata = HashMap::new();
                    metadata.insert(
                        "type".to_string(),
                        serde_json::Value::String("razor-section".to_string()),
                    );
                    metadata
                }),
                doc_comment,
                annotations: Vec::new(),
            },
        ))
    }

    /// Extract code blocks (@code, @functions, @{...})
    #[allow(dead_code)]
    pub(super) fn extract_code_block(
        &mut self,
        node: Node,
        parent_id: Option<&str>,
    ) -> Option<Symbol> {
        let block_type = self.get_code_block_type(node);
        let content = self.base.get_node_text(&node);
        // Safely truncate UTF-8 string at character boundary
        let truncated_content = BaseExtractor::truncate_string(&content, 50);

        let signature = format!("@{{ {} }}", truncated_content);

        // Extract Razor doc comment
        let doc_comment = self.base.find_doc_comment(&node);

        Some(self.base.create_symbol(
            &node,
            format!("{}Block", block_type),
            SymbolKind::Function,
            SymbolOptions {
                signature: Some(signature),
                visibility: Some(Visibility::Public),
                parent_id: parent_id.map(|s| s.to_string()),
                metadata: Some({
                    let mut metadata = HashMap::new();
                    metadata.insert(
                        "type".to_string(),
                        serde_json::Value::String("razor-code-block".to_string()),
                    );
                    metadata.insert(
                        "blockType".to_string(),
                        serde_json::Value::String(block_type.clone()),
                    );
                    metadata.insert(
                        "content".to_string(),
                        serde_json::Value::String(BaseExtractor::truncate_string(&content, 200)),
                    );
                    metadata
                }),
                doc_comment,
                annotations: Vec::new(),
            },
        ))
    }

    /// Determine code block type from node content
    #[allow(dead_code)]
    pub(super) fn get_code_block_type(&self, node: Node) -> String {
        let text = self.base.get_node_text(&node);
        if text.contains("@code") {
            "code".to_string()
        } else if text.contains("@functions") {
            "functions".to_string()
        } else if text.contains("@{") {
            "expression".to_string()
        } else {
            "block".to_string()
        }
    }
}

/// The type operand of `@model`, `@inherits`, or `@implements`: the first
/// named child that is not part of the `@keyword` transition.
pub(super) fn directive_type_operand(node: Node) -> Option<Node> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor)
        .find(|child| !child.kind().starts_with("at_"))
}
