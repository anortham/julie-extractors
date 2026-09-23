/// Razor (.cshtml) language extractor with C# code blocks and HTML templates
///
/// This extractor handles Razor files which contain:
/// - Razor-specific directives (@page, @model, @using, etc.)
/// - C# code blocks (@code, @functions, @{...})
/// - HTML elements and Razor components
/// - Data bindings (@bind-Value)
/// - Event handlers (@onclick, etc.)
use crate::base::{
    BaseExtractor, Symbol, SymbolKind, SymbolOptions, Visibility, normalize_annotations,
};
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

/// The component tag of an `element` node and the byte offset where it starts.
/// An element inside a `@<...>` template has no leading `<`: the template
/// transition consumed it.
pub(crate) fn element_component_tag<'a>(node: Node, content: &'a str) -> Option<(usize, &'a str)> {
    let text = content.get(node.byte_range())?;
    let (offset, rest) = match text.strip_prefix('<') {
        Some(rest) => (1, rest),
        None if node.parent()?.kind() == "razor_template" => (0, text),
        None => return None,
    };
    let end = rest
        .find(|character: char| character.is_whitespace() || matches!(character, '/' | '>'))
        .unwrap_or(rest.len());
    let tag = &rest[..end];
    (is_component_tag_name(tag) && !is_render_fragment_parameter(node, content))
        .then_some((node.start_byte() + offset, tag))
}

/// Whether an element is a render-fragment parameter of its parent component
/// (`<Columns>`, `<Template Context="item">`), not a component: a direct child
/// of a component element, with content, and no attribute except `Context`.
fn is_render_fragment_parameter(node: Node, content: &str) -> bool {
    let Some(parent) = node.parent().filter(|parent| parent.kind() == "element") else {
        return false;
    };
    let is_parent_component = content
        .get(parent.byte_range())
        .and_then(|text| text.strip_prefix('<'))
        .map(|rest| {
            let end = rest
                .find(|character: char| character.is_whitespace() || matches!(character, '/' | '>'))
                .unwrap_or(rest.len());
            is_component_tag_name(&rest[..end])
        })
        .unwrap_or(false);
    let self_closing = content
        .get(node.byte_range())
        .is_some_and(|text| text.ends_with("/>"));
    let mut cursor = node.walk();
    let only_context_attributes = node
        .children(&mut cursor)
        .filter(|child| matches!(child.kind(), "component_attribute" | "razor_html_attribute"))
        .all(|attribute| {
            content
                .get(attribute.byte_range())
                .and_then(|text| text.split('=').next())
                .is_some_and(|name| name.trim() == "Context")
        });
    is_parent_component && !self_closing && only_context_attributes
}

pub(crate) fn is_razor_expression_node_kind(kind: &str) -> bool {
    matches!(
        kind,
        "razor_explicit_expression" | "razor_implicit_expression"
    )
}

/// The file-derived component class, which owns `@code` members and `@inject`
/// properties.
pub(crate) fn component_symbol_id(symbols: &[Symbol]) -> Option<String> {
    symbols
        .iter()
        .find(|symbol| {
            relationship_helpers::is_component_symbol(symbol) && symbol.parent_id.is_none()
        })
        .map(|symbol| symbol.id.clone())
}

/// Whether a `razor_block` is an `@code` or `@functions` block, whose direct
/// declarations are members of the component class.
pub(crate) fn is_member_block(node: Node, content: &str) -> bool {
    let mut cursor = node.walk();
    node.children(&mut cursor)
        .filter(|child| child.kind() == "at_block")
        .any(|child| matches!(content.get(child.byte_range()), Some("code" | "functions")))
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

/// The attribute whose value is `node` alone: a bare method name
/// (`"Refresh"`) or a lone implicit expression (`"@Refresh"`).
pub(crate) fn method_group_attribute(node: Node) -> Option<Node> {
    if node.kind() != "identifier" {
        return None;
    }
    let mut holder = node.parent()?;
    if holder.kind() == "razor_implicit_expression" {
        holder = holder.parent()?;
    }
    if !matches!(
        holder.kind(),
        "razor_attribute_value" | "component_attribute_value"
    ) || holder.named_child_count() != 1
    {
        return None;
    }
    holder.parent().filter(|attribute| {
        matches!(
            attribute.kind(),
            "razor_html_attribute" | "component_attribute"
        )
    })
}

/// Whether an attribute binds an event: `@onclick`, `@bind:after`,
/// `@bind:set`, or a component parameter named `On<Event>`.
pub(crate) fn is_event_attribute(attribute: Node, content: &str) -> bool {
    let text = content.get(attribute.byte_range()).unwrap_or_default();
    let name = text.split('=').next().unwrap_or_default().trim();
    if let Some(event) = name.strip_prefix("@on") {
        return !event.contains(':');
    }
    if matches!(name, "@bind:after" | "@bind:set") {
        return true;
    }
    name.strip_prefix("On")
        .and_then(|rest| rest.chars().next())
        .is_some_and(|first| first.is_ascii_uppercase())
}

fn value_body(node: Node, depth: u32) -> Option<Node> {
    if !should_visit_tree_depth(depth) {
        return None;
    }
    match node.kind() {
        "lambda_expression" | "anonymous_method_expression" => {
            return node
                .child_by_field_name("body")
                .or_else(|| node.named_child(node.named_child_count().checked_sub(1)? as u32));
        }
        "razor_template" => return Some(node),
        _ => {}
    }
    let child_depth = child_tree_depth(depth)?;
    let mut cursor = node.walk();
    node.named_children(&mut cursor)
        .find_map(|child| value_body(child, child_depth))
}

fn has_descendant_kind(node: Node, kind: &str, depth: u32) -> bool {
    let Some(child_depth) = child_tree_depth(depth).filter(|_| should_visit_tree_depth(depth))
    else {
        return false;
    };
    let mut cursor = node.walk();
    node.children(&mut cursor)
        .any(|child| child.kind() == kind || has_descendant_kind(child, kind, child_depth))
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

/// Component-level directives the file-derived class carries.
#[derive(Default)]
struct ComponentDirectives {
    type_parameters: Vec<String>,
    type_parameter_constraints: serde_json::Map<String, serde_json::Value>,
    layout: Option<String>,
    render_mode: Option<String>,
    preserve_whitespace: Option<String>,
    has_page: bool,
    attribute_lists: Vec<String>,
}

pub struct RazorExtractor {
    pub(crate) base: BaseExtractor,
    /// Names of the file's methods and local functions, so a method group
    /// bound in markup reads as a call.
    callable_names: std::collections::HashSet<String>,
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
            callable_names: std::collections::HashSet::new(),
        }
    }

    /// Extract symbols from the Razor file
    pub fn get_type_argument_usages(&self) -> Vec<crate::base::TypeArgumentUsage> {
        self.base.get_type_argument_usages()
    }

    /// Clone captured call-argument literals.
    pub fn get_literals(&self) -> Vec<crate::base::Literal> {
        self.base.get_literals()
    }

    pub fn extract_symbols(&mut self, tree: &Tree) -> Vec<Symbol> {
        let mut symbols = Vec::new();
        if let Some(component_symbol) = self.extract_component_symbol(tree.root_node()) {
            symbols.push(component_symbol);
        }
        self.visit_node(tree.root_node(), &mut symbols, None, 0);
        self.limit_value_bodies_to_lambdas(tree.root_node(), &mut symbols);
        crate::test_detection::mark_dotnet_test_containers(&mut symbols);
        symbols
    }

    /// The file-derived class: a `.razor` component or a `.cshtml` view or
    /// page. Its body is the whole file, and it carries the component-level
    /// directives: `@typeparam` in its signature, `@layout`, `@rendermode`,
    /// and `@preservewhitespace` in metadata, `@attribute` lists as
    /// annotations.
    fn extract_component_symbol(&mut self, root_node: Node) -> Option<Symbol> {
        if !self.has_file_class() {
            return None;
        }

        let component_name = self.component_name_from_file_path()?;
        let qualified_name = self
            .component_namespace()
            .map(|namespace| format!("{namespace}.{component_name}"))
            .unwrap_or_else(|| component_name.clone());
        let directives = self.component_directives(root_node);

        let is_view = !self.is_razor_component_file();
        let mut metadata = HashMap::new();
        metadata.insert(
            "type".to_string(),
            serde_json::Value::String(
                if is_view {
                    "razor-view"
                } else {
                    "razor-component"
                }
                .to_string(),
            ),
        );
        metadata.insert(
            "qualifiedName".to_string(),
            serde_json::Value::String(qualified_name.clone()),
        );
        if !directives.type_parameters.is_empty() {
            metadata.insert(
                "typeParameters".to_string(),
                serde_json::json!(directives.type_parameters),
            );
        }
        if !directives.type_parameter_constraints.is_empty() {
            metadata.insert(
                "typeParameterConstraints".to_string(),
                serde_json::Value::Object(directives.type_parameter_constraints.clone()),
            );
        }
        for (key, value) in [
            ("layout", &directives.layout),
            ("renderMode", &directives.render_mode),
            ("preserveWhitespace", &directives.preserve_whitespace),
        ] {
            if let Some(value) = value {
                metadata.insert(key.to_string(), serde_json::Value::String(value.clone()));
            }
        }

        let generics = if directives.type_parameters.is_empty() {
            String::new()
        } else {
            format!("<{}>", directives.type_parameters.join(", "))
        };
        let keyword = match (is_view, directives.has_page) {
            (false, _) => "component",
            (true, true) => "page",
            (true, false) => "view",
        };
        let mut symbol = self.base.create_symbol(
            &root_node,
            component_name,
            SymbolKind::Class,
            SymbolOptions {
                signature: Some(format!("{keyword} {qualified_name}{generics}")),
                visibility: Some(Visibility::Public),
                parent_id: None,
                metadata: Some(metadata),
                doc_comment: None,
                annotations: normalize_annotations(&directives.attribute_lists, "csharp"),
            },
        );
        let body = crate::base::NormalizedSpan::from_node(&root_node);
        symbol.body_span = Some(body);
        symbol.body_hash =
            crate::base::body::body_hash(&self.base.content, body, &self.base.language);
        Some(symbol)
    }

    fn component_directives(&self, root_node: Node) -> ComponentDirectives {
        let mut directives = ComponentDirectives::default();
        let mut cursor = root_node.walk();
        for node in root_node.named_children(&mut cursor) {
            let operand = || {
                directives::directive_type_operand(node)
                    .map(|operand| self.base.get_node_text(&operand))
            };
            match node.kind() {
                "razor_typeparam_directive" => {
                    let Some(name) = node.child_by_field_name("name") else {
                        continue;
                    };
                    let name = self.base.get_node_text(&name);
                    if let Some(clause) =
                        self.find_child_by_type(node, "type_parameter_constraints_clause")
                    {
                        directives
                            .type_parameter_constraints
                            .insert(name.clone(), self.base.get_node_text(&clause).into());
                    }
                    directives.type_parameters.push(name);
                }
                "razor_layout_directive" => directives.layout = operand(),
                "razor_rendermode_directive" => directives.render_mode = operand(),
                "razor_preservewhitespace_directive" => directives.preserve_whitespace = operand(),
                "razor_page_directive" => directives.has_page = true,
                "razor_attribute_directive" => directives.attribute_lists.extend(
                    self.find_child_by_type(node, "attribute_list")
                        .map(|list| self.base.get_node_text(&list)),
                ),
                _ => {}
            }
        }
        directives
    }

    /// Whether the file compiles to a class of its own: a `.razor` component
    /// or a `.cshtml` view or page. `_Imports`, `_ViewImports`, and
    /// `_ViewStart` only configure other files.
    pub(crate) fn has_file_class(&self) -> bool {
        let path = Path::new(&self.base.file_path);
        matches!(
            path.extension().and_then(|extension| extension.to_str()),
            Some("razor" | "cshtml")
        ) && !matches!(
            path.file_stem().and_then(|stem| stem.to_str()),
            Some("_Imports" | "_ViewImports" | "_ViewStart")
        )
    }

    pub(crate) fn is_razor_component_file(&self) -> bool {
        let path = Path::new(&self.base.file_path);
        path.extension().and_then(|extension| extension.to_str()) == Some("razor")
            && self.has_file_class()
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

        if node.kind() == "ERROR" {
            self.extract_from_text_content(node, symbols, parent_id.as_deref());
        } else if !self.is_valid_node(&node) {
            return;
        }

        let Some(current_parent_id) = self.record_node_symbol(node, symbols, parent_id) else {
            return;
        };

        let Some(child_depth) = child_tree_depth(depth) else {
            return;
        };
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.visit_node(child, symbols, current_parent_id.clone(), child_depth);
        }
    }

    /// Push the symbol `node` declares and return the parent for its children,
    /// or `None` when the node's subtree is already handled. Kept out of the
    /// recursive walker so the walker's frame holds no `Symbol`.
    #[inline(never)]
    fn record_node_symbol(
        &mut self,
        node: Node,
        symbols: &mut Vec<Symbol>,
        parent_id: Option<String>,
    ) -> Option<Option<String>> {
        let symbol = match node.kind() {
            "razor_attribute_directive" if self.has_file_class() => None,
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
                let directive_parent = if matches!(
                    node.kind(),
                    "razor_inject_directive" | "razor_model_directive"
                ) {
                    component_symbol_id(symbols).or(parent_id.clone())
                } else {
                    parent_id.clone()
                };
                self.extract_directive(node, directive_parent.as_deref())
            }
            "razor_section" => self.extract_section(node, parent_id.as_deref()),
            "razor_block" => {
                let block_parent = if is_member_block(node, &self.base.content) {
                    component_symbol_id(symbols).or(parent_id)
                } else {
                    parent_id.or_else(|| component_symbol_id(symbols))
                };
                self.extract_csharp_symbols(node, symbols, block_parent.as_deref());
                return None;
            }
            "csharp_code" => {
                let code_parent = parent_id.clone().or_else(|| component_symbol_id(symbols));
                self.extract_csharp_symbols(node, symbols, code_parent.as_deref());
                None
            }
            "using_directive" => self.extract_using(node, parent_id.as_deref()),
            "namespace_declaration" => self.extract_namespace(node, parent_id.as_deref()),
            "class_declaration" => self.extract_class(node, parent_id.as_deref()),
            "method_declaration" => self.extract_method(node, parent_id.as_deref()),
            "property_declaration" => self.extract_property(node, parent_id.as_deref()),
            "field_declaration" => self.extract_field(node, parent_id.as_deref()),
            "local_function_statement" => self.extract_local_function(node, parent_id.as_deref()),
            "local_declaration_statement" => {
                self.extract_local_variable(node, parent_id.as_deref())
            }
            _ => None,
        };

        Some(match symbol {
            Some(symbol) => {
                let id = symbol.id.clone();
                symbols.push(symbol);
                Some(id)
            }
            None => parent_id,
        })
    }

    /// A field or variable has a body only when its value runs code: a
    /// lambda, an anonymous method, or a Razor template (`@<p>...</p>`). The
    /// body is that value's body, not the first brace or parenthesis in the
    /// declaration text.
    fn limit_value_bodies_to_lambdas(&self, root: Node, symbols: &mut [Symbol]) {
        for symbol in symbols
            .iter_mut()
            .filter(|symbol| matches!(symbol.kind, SymbolKind::Field | SymbolKind::Variable))
        {
            let body = root
                .descendant_for_byte_range(symbol.start_byte as usize, symbol.end_byte as usize)
                .and_then(|node| value_body(node, 0))
                .map(|body| crate::base::NormalizedSpan::from_node(&body));
            symbol.body_span = body;
            symbol.body_hash = body.and_then(|body| {
                crate::base::body::body_hash(&self.base.content, body, &self.base.language)
            });
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

        if !has_descendant_kind(node, "razor_inherits_directive", 0)
            && let Some(captures) = INHERITS_RE.captures(&content)
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
