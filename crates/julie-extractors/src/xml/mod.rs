//! XML extractor — name-promoted elements as symbols, QName attribute references as identifiers.
//!
//! An element becomes a symbol only when it carries a `name` or `id` attribute; the attribute
//! value is the symbol name (`<xs:complexType name="AddPhone">` → `AddPhone`). Anonymous
//! structural elements (`<xs:sequence>`, `<item>`, `<row>`) emit nothing, so a document with
//! thousands of repeated rows still yields only the handful of named components. Named elements
//! chain to their nearest named ancestor the way YAML mapping keys chain.
//!
//! Attribute values of `type`, `ref`, `base`, and `element` become `type_usage` identifiers
//! named by the QName's local part, with the raw QName and prefix in metadata, but only in
//! schema context — the owning element must sit in a declared XML Schema or WSDL namespace, or
//! the attribute itself must (`xsi:type`), because those four names are ordinary words that a
//! generic document uses for its own purposes. Every non-empty attribute value is captured as a
//! literal under a `tag.attribute` carrier regardless of dialect.
//!
//! Build manifests (see [`build`]) add a root symbol (MSBuild project, NuGet package, Maven
//! artifact), MSBuild target `Calls` edges and identifiers, structured pending `Imports` rows for
//! referenced project, import, and module files, and dependency and property facts.
//!
//! Common use cases:
//! - XSD schemas (complexType/element/simpleType declarations and their references)
//! - WSDL service definitions (service/port/operation/message)
//! - Project and application configuration documents

pub(crate) mod build;
mod elements;
mod identifiers;

use std::collections::HashMap;
use std::path::Path;

use crate::base::{
    BaseExtractor, Identifier, IdentifierKind, Relationship, RelationshipKind,
    StructuredPendingRelationship, Symbol, UnresolvedTarget,
};
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};

pub struct XmlExtractor {
    pub(crate) base: BaseExtractor,
}

impl XmlExtractor {
    pub fn new(
        language: String,
        file_path: String,
        source_code: String,
        workspace_root: &Path,
    ) -> Self {
        Self {
            base: BaseExtractor::new(language, file_path, source_code, workspace_root),
        }
    }

    pub fn extract_symbols(&mut self, tree: &tree_sitter::Tree) -> Vec<Symbol> {
        let mut symbols = Vec::new();
        self.walk_elements(tree.root_node(), None, 0, &mut symbols);
        symbols
    }

    pub fn extract_identifiers(
        &mut self,
        tree: &tree_sitter::Tree,
        symbols: &[Symbol],
    ) -> Vec<Identifier> {
        let symbols_by_start_byte: HashMap<u32, &str> = symbols
            .iter()
            .map(|symbol| (symbol.start_byte, symbol.id.as_str()))
            .collect();
        let namespaces = identifiers::SchemaNamespaces::scan(&self.base, tree.root_node());
        self.walk_references(
            tree.root_node(),
            None,
            0,
            &namespaces,
            &symbols_by_start_byte,
        );
        self.extract_msbuild_identifiers(tree, symbols);
        self.base.identifiers.clone()
    }

    /// MSBuild target names (`DependsOnTargets`, `CallTarget`) as `call`
    /// identifiers and `$(Property)` uses as `variable_ref` identifiers.
    fn extract_msbuild_identifiers(&mut self, tree: &tree_sitter::Tree, symbols: &[Symbol]) {
        if build::BuildDialect::for_path(&self.base.file_path) != Some(build::BuildDialect::MsBuild)
        {
            return;
        }
        let Some(root) = build::root_element(tree) else {
            return;
        };
        let content = self.base.content.clone();
        let sites = build::target_references(&content, root)
            .into_iter()
            .map(|site| (site, IdentifierKind::Call))
            .chain(
                build::property_references(&content, root)
                    .into_iter()
                    .map(|site| (site, IdentifierKind::VariableRef)),
            );
        for (site, kind) in sites {
            let Some(span) = self.base.span_for_byte_range(site.start, site.end) else {
                continue;
            };
            let containing = symbols
                .iter()
                .filter(|symbol| {
                    symbol.start_byte as usize <= site.start && site.end <= symbol.end_byte as usize
                })
                .min_by_key(|symbol| symbol.end_byte - symbol.start_byte)
                .map(|symbol| symbol.id.clone());
            let target = (kind == IdentifierKind::Call)
                .then(|| target_symbol(symbols, &site.name))
                .flatten()
                .map(|symbol| symbol.id.clone());
            self.base
                .create_identifier_at_span(span, site.name, kind, containing, None);
            if let Some(last) = self.base.identifiers.last_mut() {
                last.target_symbol_id = target;
            }
        }
    }

    /// MSBuild target-to-target `Calls` edges within the file, plus structured
    /// pending `Imports` rows for references to other project, import,
    /// module, and schema files.
    pub fn extract_relationships(
        &mut self,
        tree: &tree_sitter::Tree,
        symbols: &[Symbol],
    ) -> Vec<Relationship> {
        let Some(dialect) = build::BuildDialect::for_path(&self.base.file_path) else {
            return Vec::new();
        };
        let Some(root) = build::root_element(tree) else {
            return Vec::new();
        };
        let content = self.base.content.clone();
        let mut relationships = Vec::new();
        if dialect == build::BuildDialect::MsBuild {
            for site in build::target_references(&content, root) {
                let from = site
                    .element
                    .and_then(|id| element_symbol(tree, symbols, id));
                let to = target_symbol(symbols, &site.name);
                let (Some(from), Some(to), Some(span)) = (
                    from,
                    to,
                    self.base.span_for_byte_range(site.start, site.end),
                ) else {
                    continue;
                };
                relationships.push(Relationship {
                    id: format!("{}_{}_Calls_{}", from.id, to.id, site.start),
                    from_symbol_id: from.id.clone(),
                    to_symbol_id: to.id.clone(),
                    kind: RelationshipKind::Calls,
                    file_path: self.base.file_path.clone(),
                    line_number: span.start_line,
                    span: Some(span),
                    reference_site_is_exact: true,
                    confidence: 1.0,
                    metadata: None,
                });
            }
        }
        for reference in build::file_references(dialect, &content, root) {
            let from = std::iter::successors(Some(reference.node), |node| node.parent())
                .find_map(|node| symbol_at(symbols, node));
            let Some(from) = from else {
                continue;
            };
            let terminal_name = reference
                .path
                .rsplit('/')
                .next()
                .unwrap_or(&reference.path)
                .to_string();
            let pending = StructuredPendingRelationship::new(
                from.id.clone(),
                UnresolvedTarget {
                    display_name: reference.path.clone(),
                    terminal_name,
                    receiver: None,
                    namespace_path: Vec::new(),
                    import_context: Some(reference.path.clone()),
                },
                Some(from.id.clone()),
                RelationshipKind::Imports,
                self.base.file_path.clone(),
                reference.node.start_position().row as u32 + 1,
                1.0,
            );
            self.base.add_structured_pending_relationship(pending);
        }
        relationships
    }

    pub fn infer_types(&self, _symbols: &[Symbol]) -> HashMap<String, String> {
        HashMap::new()
    }

    pub fn get_type_argument_usages(&self) -> Vec<crate::base::TypeArgumentUsage> {
        self.base.get_type_argument_usages()
    }

    pub fn get_literals(&self) -> Vec<crate::base::Literal> {
        self.base.get_literals()
    }

    fn walk_elements(
        &mut self,
        node: tree_sitter::Node,
        parent_id: Option<String>,
        depth: u32,
        symbols: &mut Vec<Symbol>,
    ) {
        if !should_visit_tree_depth(depth) {
            return;
        }

        let mut child_parent_id = parent_id;
        if node.kind() == "element" {
            if let Some(symbol) = self.extract_element_symbol(node, child_parent_id.as_deref()) {
                child_parent_id = Some(symbol.id.clone());
                symbols.push(symbol);
            }
        } else if elements::is_orphan_tag(node)
            && let Some(symbol) =
                elements::extract_orphan_tag(&mut self.base, node, child_parent_id.as_deref())
        {
            symbols.push(symbol);
        }

        let Some(child_depth) = child_tree_depth(depth) else {
            return;
        };
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.walk_elements(child, child_parent_id.clone(), child_depth, symbols);
        }
    }

    fn extract_element_symbol(
        &mut self,
        element: tree_sitter::Node,
        parent_id: Option<&str>,
    ) -> Option<Symbol> {
        let dialect = build::BuildDialect::for_path(&self.base.file_path);
        let Some(dialect) = dialect else {
            return elements::extract_element(&mut self.base, element, parent_id);
        };
        let content = self.base.content.clone();
        if element
            .parent()
            .is_some_and(|parent| parent.kind() == "document")
            && let Some(document) =
                build::document_symbol(dialect, &content, &self.base.file_path, element)
        {
            return elements::extract_named_element(
                &mut self.base,
                element,
                parent_id,
                document.source,
                document.name,
                document.metadata,
            );
        }
        if build::is_reference_element(dialect, &content, element) {
            return None;
        }
        if let Some(name) = build::child_named_element(dialect, &content, element) {
            return elements::extract_named_element(
                &mut self.base,
                element,
                parent_id,
                "id",
                name,
                Vec::new(),
            );
        }
        elements::extract_element(&mut self.base, element, parent_id)
    }

    fn walk_references(
        &mut self,
        node: tree_sitter::Node,
        containing_symbol_id: Option<&str>,
        depth: u32,
        namespaces: &identifiers::SchemaNamespaces,
        symbols_by_start_byte: &HashMap<u32, &str>,
    ) {
        if !should_visit_tree_depth(depth) {
            return;
        }

        let own_symbol_id = symbols_by_start_byte
            .get(&(node.start_byte() as u32))
            .copied()
            .or(containing_symbol_id);
        let mut child_containing_symbol_id = containing_symbol_id;

        if node.kind() == "element" {
            child_containing_symbol_id = own_symbol_id;
            if let Some(tag) = elements::tag_node(node) {
                identifiers::extract_element_facts(
                    &mut self.base,
                    tag,
                    namespaces,
                    child_containing_symbol_id,
                );
            }
        } else if elements::is_orphan_tag(node) {
            identifiers::extract_element_facts(&mut self.base, node, namespaces, own_symbol_id);
        }

        let Some(child_depth) = child_tree_depth(depth) else {
            return;
        };
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.walk_references(
                child,
                child_containing_symbol_id,
                child_depth,
                namespaces,
                symbols_by_start_byte,
            );
        }
    }
}

fn symbol_at<'a>(symbols: &'a [Symbol], node: tree_sitter::Node) -> Option<&'a Symbol> {
    symbols.iter().find(|symbol| {
        symbol.start_byte == node.start_byte() as u32 && symbol.end_byte == node.end_byte() as u32
    })
}

fn element_symbol<'a>(
    tree: &tree_sitter::Tree,
    symbols: &'a [Symbol],
    element_id: usize,
) -> Option<&'a Symbol> {
    symbols.iter().find(|symbol| {
        tree.root_node()
            .descendant_for_byte_range(symbol.start_byte as usize, symbol.end_byte as usize)
            .is_some_and(|node| node.id() == element_id)
    })
}

fn target_symbol<'a>(symbols: &'a [Symbol], name: &str) -> Option<&'a Symbol> {
    symbols.iter().find(|symbol| {
        symbol.name == name
            && symbol
                .metadata
                .as_ref()
                .and_then(|metadata| metadata.get("tag"))
                .and_then(|tag| tag.as_str())
                == Some("Target")
    })
}
