/// Razor (.cshtml) language extractor with C# code blocks and HTML templates
///
/// This extractor handles Razor files which contain:
/// - Razor-specific directives (@page, @model, @using, etc.)
/// - C# code blocks (@code, @functions, @{...})
/// - HTML elements and Razor components
/// - Data bindings (@bind-Value)
/// - Event handlers (@onclick, etc.)
use crate::base::{BaseExtractor, Symbol, SymbolKind, SymbolOptions, Visibility};
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};
use regex::Regex;
use std::collections::HashMap;
use std::path::Path;
use std::sync::LazyLock;
use tree_sitter::{Node, Tree};

// Static regexes compiled once for performance
static INHERITS_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"@inherits\s+(\S+)").unwrap());
static NAMESPACE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?m)^\s*@namespace\s+([A-Za-z_][A-Za-z0-9_]*(?:\.[A-Za-z_][A-Za-z0-9_]*)*)\b")
        .unwrap()
});
static RENDERMODE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"@rendermode="([^"]+)""#).unwrap());

pub(crate) fn component_tag_name(element: &str) -> Option<&str> {
    let remainder = element.strip_prefix('<')?;
    let end = remainder
        .find(|character: char| character.is_whitespace() || matches!(character, '/' | '>'))
        .unwrap_or(remainder.len());
    let tag = &remainder[..end];
    is_component_tag_name(tag).then_some(tag)
}

pub(crate) fn is_razor_expression_node_kind(kind: &str) -> bool {
    matches!(
        kind,
        "razor_explicit_expression" | "razor_implicit_expression"
    )
}

fn is_component_tag_name(tag: &str) -> bool {
    tag.split('.').all(is_pascal_case_component_segment)
}

fn is_pascal_case_component_segment(segment: &str) -> bool {
    let mut characters = segment.chars();
    characters
        .next()
        .is_some_and(|first| first.is_ascii_uppercase())
        && characters.all(|character| character.is_ascii_alphanumeric())
}

// Module declarations
mod csharp;
mod directives;
mod expressions;
mod helpers;
mod identifiers;
mod parameters;
mod relationship_helpers;
mod relationships;
mod stubs;
mod type_facts;

mod type_inference;

pub struct RazorExtractor {
    pub(crate) base: BaseExtractor,
}

impl RazorExtractor {
    /// Create a new Razor extractor
    pub fn new(
        language: String,
        file_path: String,
        content: String,
        workspace_root: &std::path::Path,
    ) -> Self {
        Self {
            base: BaseExtractor::new(language, file_path, content, workspace_root),
        }
    }

    /// Extract symbols from the Razor file
    pub fn get_type_argument_usages(&self) -> Vec<crate::base::TypeArgumentUsage> {
        self.base.get_type_argument_usages()
    }

    /// Clone captured call-argument literals (Miller bridge Phase 3).
    pub fn get_literals(&self) -> Vec<crate::base::Literal> {
        self.base.get_literals()
    }

    pub fn extract_symbols(&mut self, tree: &Tree) -> Vec<Symbol> {
        let mut symbols = Vec::new();
        if let Some(component_symbol) = self.extract_component_symbol(tree.root_node()) {
            symbols.push(component_symbol);
        }
        self.visit_node(tree.root_node(), &mut symbols, None, 0);
        crate::test_detection::mark_dotnet_test_containers(&mut symbols);
        symbols
    }

    fn extract_component_symbol(&mut self, root_node: Node) -> Option<Symbol> {
        if !self.is_razor_component_file() {
            return None;
        }

        let component_name = self.component_name_from_file_path()?;
        let qualified_name = self
            .component_namespace()
            .map(|namespace| format!("{namespace}.{component_name}"))
            .unwrap_or_else(|| component_name.clone());

        let mut metadata = HashMap::new();
        metadata.insert(
            "type".to_string(),
            serde_json::Value::String("razor-component".to_string()),
        );
        metadata.insert(
            "qualifiedName".to_string(),
            serde_json::Value::String(qualified_name.clone()),
        );

        Some(self.base.create_symbol(
            &root_node,
            component_name,
            SymbolKind::Class,
            SymbolOptions {
                signature: Some(format!("component {qualified_name}")),
                visibility: Some(Visibility::Public),
                parent_id: None,
                metadata: Some(metadata),
                doc_comment: None,
                annotations: Vec::new(),
            },
        ))
    }

    fn is_razor_component_file(&self) -> bool {
        let path = Path::new(&self.base.file_path);
        path.extension().and_then(|extension| extension.to_str()) == Some("razor")
            && !matches!(
                path.file_stem().and_then(|stem| stem.to_str()),
                Some("_Imports" | "_ViewImports")
            )
    }

    fn component_name_from_file_path(&self) -> Option<String> {
        Path::new(&self.base.file_path)
            .file_stem()
            .and_then(|stem| stem.to_str())
            .filter(|stem| !stem.is_empty())
            .map(ToOwned::to_owned)
    }

    fn component_namespace(&self) -> Option<String> {
        NAMESPACE_RE
            .captures(&self.base.content)
            .and_then(|captures| captures.get(1))
            .map(|namespace| namespace.as_str().to_string())
    }

    /// Visit a node and extract symbols recursively
    fn visit_node(
        &mut self,
        node: Node,
        symbols: &mut Vec<Symbol>,
        parent_id: Option<String>,
        depth: u32,
    ) {
        if !should_visit_tree_depth(depth) {
            return;
        }

        // Handle ERROR nodes by falling back to text-based extraction
        if node.kind() == "ERROR" {
            self.extract_from_text_content(node, symbols, parent_id.as_deref());
            return;
        }

        if !self.is_valid_node(&node) {
            return;
        }

        let mut symbol = None;
        let node_type = node.kind();

        match node_type {
            "razor_directive"
            | "razor_inject_directive"
            | "razor_using_directive"
            | "razor_page_directive"
            | "razor_namespace_directive"
            | "razor_model_directive"
            | "razor_attribute_directive"
            | "razor_inherits_directive"
            | "razor_implements_directive"
            | "razor_addtaghelper_directive" => {
                symbol = self.extract_directive(node, parent_id.as_deref());
            }
            "at_namespace" | "at_inherits" | "at_implements" => {
                symbol = self.extract_token_directive(node, parent_id.as_deref());
            }
            "razor_section" => {
                symbol = self.extract_section(node, parent_id.as_deref());
            }
            "razor_block" => {
                // Extract C# symbols from within the block.
                // Use the outer parent_id (not a code block symbol) so children
                // appear as top-level file symbols — the @code block is just a
                // container, not a meaningful symbol for search/navigation.
                self.extract_csharp_symbols(node, symbols, parent_id.as_deref());
                // Don't visit children since we already extracted them
                return;
            }
            kind if is_razor_expression_node_kind(kind) && !self.contains_invocation(node) => {
                symbol = self.extract_expression(node, parent_id.as_deref());
            }
            // Template component references (<PageTitle>, <EditForm>, etc.) are USAGES
            // not definitions — skip them. Component definitions come from the
            // component's own .razor file via @code block extraction.
            "html_element" | "element" | "razor_component" => {}
            "csharp_code" => {
                self.extract_csharp_symbols(node, symbols, parent_id.as_deref());
            }
            "using_directive" => {
                symbol = self.extract_using(node, parent_id.as_deref());
            }
            "namespace_declaration" => {
                symbol = self.extract_namespace(node, parent_id.as_deref());
            }
            "class_declaration" => {
                symbol = self.extract_class(node, parent_id.as_deref());
            }
            "method_declaration" => {
                symbol = self.extract_method(node, parent_id.as_deref());
            }
            "property_declaration" => {
                symbol = self.extract_property(node, parent_id.as_deref());
            }
            "field_declaration" => {
                symbol = self.extract_field(node, parent_id.as_deref());
            }
            "local_function_statement" => {
                symbol = self.extract_local_function(node, parent_id.as_deref());
            }
            "local_declaration_statement" => {
                symbol = self.extract_local_variable(node, parent_id.as_deref());
            }
            // Assignment expressions (ViewData["Title"] = "Home", Layout = "_Layout", etc.)
            // are USAGES, not definitions. Tracked via identifier extraction for reference relationships.
            "assignment_expression" => {}
            // Invocation expressions (Html.Raw(), RenderBody(), etc.) are USAGES, not definitions.
            // They are tracked via identifier extraction for call relationships.
            "invocation_expression" => {}
            // HTML/Razor attributes (@onclick, @bind, class, id, etc.) are template
            // markup, not code symbols. Meaningful directives (@inject, @page, etc.)
            // are handled via their own directive node types above.
            "razor_html_attribute" | "attribute" => {}
            _ => {}
        }

        let current_parent_id = if let Some(sym) = &symbol {
            symbols.push(sym.clone());
            Some(sym.id.clone())
        } else {
            parent_id
        };

        // Recursively visit children
        let Some(child_depth) = child_tree_depth(depth) else {
            return;
        };
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.visit_node(child, symbols, current_parent_id.clone(), child_depth);
        }
    }

    /// Extract symbols from ERROR nodes using regex-based text parsing
    fn extract_from_text_content(
        &mut self,
        node: Node,
        symbols: &mut Vec<Symbol>,
        parent_id: Option<&str>,
    ) {
        let content = self.base.get_node_text(&node);

        // Extract Razor directives from text

        // Look for @inherits directive
        if let Some(captures) = INHERITS_RE.captures(&content)
            && let Some(base_class) = captures.get(1)
        {
            let symbol = self.base.create_symbol(
                &node,
                format!("inherits {}", base_class.as_str()),
                SymbolKind::Import,
                SymbolOptions {
                    signature: Some(format!("@inherits {}", base_class.as_str())),
                    visibility: Some(Visibility::Public),
                    parent_id: parent_id.map(|s| s.to_string()),
                    metadata: None,
                    doc_comment: None,
                    annotations: Vec::new(),
                },
            );
            symbols.push(symbol);
        }

        // Look for @rendermode directives
        for captures in RENDERMODE_RE.captures_iter(&content) {
            if let Some(mode) = captures.get(1) {
                let symbol = self.base.create_symbol(
                    &node,
                    format!("rendermode {}", mode.as_str()),
                    SymbolKind::Property,
                    SymbolOptions {
                        signature: Some(format!("@rendermode=\"{}\"", mode.as_str())),
                        visibility: Some(Visibility::Public),
                        parent_id: parent_id.map(|s| s.to_string()),
                        metadata: None,
                        doc_comment: None,
                        annotations: Vec::new(),
                    },
                );
                symbols.push(symbol);
            }
        }
    }
}
