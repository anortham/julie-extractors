//! XML extractor — name-promoted elements as symbols, QName attribute references as identifiers.
//!
//! An element becomes a symbol only when it carries a `name` or `id` attribute; the attribute
//! value is the symbol name (`<xs:complexType name="AddPhone">` → `AddPhone`). Anonymous
//! structural elements (`<xs:sequence>`, `<item>`, `<row>`) emit nothing, so a document with
//! thousands of repeated rows still yields only the handful of named components. Named elements
//! chain to their nearest named ancestor the way YAML mapping keys chain.
//!
//! The kind comes from what the element declares when its vocabulary is known (XML Schema,
//! WSDL, XSLT, XAML, MSBuild, Ant, Spring, MyBatis, TestNG, Android, `.resx`): an
//! `xs:complexType` is a class, a WSDL operation a method, a build target a function. A
//! document in an unknown vocabulary keeps the structural rule: an element with child
//! elements is a module, a leaf is a variable.
//!
//! References (see [`references`]) become identifiers, same-file edges, and structured pending
//! rows from one shared list of sites. Every non-empty attribute value is captured as a literal
//! under a `tag.attribute` carrier regardless of dialect.
//!
//! Build manifests (see [`build`]) add a root symbol (MSBuild project, NuGet package, Maven
//! artifact), target `Calls` edges and identifiers, structured pending `Imports` rows for
//! referenced project, import, and module files, and dependency and property facts. Document
//! links (see [`links`]) add pending `Imports` rows for XInclude, stylesheet, schema-location,
//! DTD, and XSLT import targets.
//!
//! Common use cases:
//! - XSD schemas (complexType/element/simpleType declarations and their references)
//! - WSDL service definitions (service/port/operation/message)
//! - Project and application configuration documents

pub(crate) mod build;
pub(crate) mod context;
mod elements;
pub(crate) mod facts;
pub(crate) mod links;
mod literals;
mod references;

use std::collections::HashMap;
use std::path::Path;

pub(crate) use elements::comment_documents_following_element;

use crate::base::{
    BaseExtractor, Identifier, Relationship, RelationshipKind, StructuredPendingRelationship,
    Symbol, UnresolvedTarget,
};
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};
use context::XmlContext;

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

    fn context(&self, tree: &tree_sitter::Tree) -> XmlContext {
        XmlContext::detect(tree, &self.base.file_path, &self.base.content)
    }

    pub fn extract_symbols(&mut self, tree: &tree_sitter::Tree) -> Vec<Symbol> {
        let context = self.context(tree);
        let mut symbols = Vec::new();
        self.walk_elements(&context, tree.root_node(), None, 0, &mut symbols);
        symbols
    }

    pub fn extract_identifiers(
        &mut self,
        tree: &tree_sitter::Tree,
        symbols: &[Symbol],
    ) -> Vec<Identifier> {
        let context = self.context(tree);
        let symbols_by_start_byte: HashMap<u32, &str> = symbols
            .iter()
            .map(|symbol| (symbol.start_byte, symbol.id.as_str()))
            .collect();
        self.walk_literals(tree.root_node(), None, 0, &symbols_by_start_byte);
        let sites = references::references(&self.base, &context, tree);
        let resolver = references::Resolver::new(&self.base.content, tree, symbols);
        references::emit_identifiers(&mut self.base, &sites, &resolver, symbols);
        self.base.identifiers.clone()
    }

    /// Same-file edges and structured pending rows for every resolvable
    /// reference site, plus pending `Imports` rows for referenced files.
    pub fn extract_relationships(
        &mut self,
        tree: &tree_sitter::Tree,
        symbols: &[Symbol],
    ) -> Vec<Relationship> {
        let context = self.context(tree);
        let mut relationships = Vec::new();
        let sites = references::references(&self.base, &context, tree);
        let resolver = references::Resolver::new(&self.base.content, tree, symbols);
        references::emit_relationships(
            &mut self.base,
            &sites,
            &resolver,
            symbols,
            &mut relationships,
        );
        let content = self.base.content.clone();
        let mut files: Vec<(tree_sitter::Node, String)> = Vec::new();
        if let (Some(dialect), Some(root)) = (context.build, build::root_element(tree)) {
            files.extend(
                build::file_references(dialect, &content, root)
                    .into_iter()
                    .map(|reference| (reference.node, reference.path)),
            );
        }
        files.extend(
            links::document_links(&content, tree)
                .into_iter()
                .filter(|link| link.imports_file())
                .map(|link| (link.node, link.href.replace('\\', "/"))),
        );
        for (node, path) in files {
            let Some(from) =
                references::containing_symbol(symbols, node.start_byte(), node.end_byte())
            else {
                continue;
            };
            let terminal_name = path.rsplit('/').next().unwrap_or(&path).to_string();
            let pending = StructuredPendingRelationship::new(
                from.id.clone(),
                UnresolvedTarget {
                    display_name: path.clone(),
                    terminal_name,
                    receiver: None,
                    namespace_path: Vec::new(),
                    import_context: Some(path.clone()),
                },
                Some(from.id.clone()),
                RelationshipKind::Imports,
                self.base.file_path.clone(),
                node.start_position().row as u32 + 1,
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
        context: &XmlContext,
        node: tree_sitter::Node,
        parent_id: Option<String>,
        depth: u32,
        symbols: &mut Vec<Symbol>,
    ) {
        if !should_visit_tree_depth(depth) {
            return;
        }

        let mut child_parent_id = parent_id;
        match node.kind() {
            "element" => {
                if let Some(symbol) =
                    self.extract_element_symbol(context, node, child_parent_id.as_deref())
                {
                    child_parent_id = Some(symbol.id.clone());
                    symbols.push(symbol);
                }
            }
            "elementdecl" | "GEDecl" => {
                if let Some(symbol) = elements::extract_dtd_declaration(
                    &mut self.base,
                    node,
                    child_parent_id.as_deref(),
                ) {
                    symbols.push(symbol);
                }
            }
            _ if elements::is_orphan_tag(node) => {
                if let Some(symbol) = elements::extract_orphan_tag(
                    &mut self.base,
                    context,
                    node,
                    child_parent_id.as_deref(),
                ) {
                    symbols.push(symbol);
                }
            }
            _ => {}
        }

        let Some(child_depth) = child_tree_depth(depth) else {
            return;
        };
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.walk_elements(
                context,
                child,
                child_parent_id.clone(),
                child_depth,
                symbols,
            );
        }
    }

    fn extract_element_symbol(
        &mut self,
        context: &XmlContext,
        element: tree_sitter::Node,
        parent_id: Option<&str>,
    ) -> Option<Symbol> {
        let Some(dialect) = context.build else {
            return elements::extract_element(&mut self.base, context, element, parent_id);
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
                context,
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
                context,
                element,
                parent_id,
                "id",
                name,
                Vec::new(),
            );
        }
        elements::extract_element(&mut self.base, context, element, parent_id)
    }

    fn walk_literals(
        &mut self,
        node: tree_sitter::Node,
        containing_symbol_id: Option<&str>,
        depth: u32,
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
                literals::record_attribute_literals(
                    &mut self.base,
                    tag,
                    child_containing_symbol_id,
                );
            }
        } else if elements::is_orphan_tag(node) {
            literals::record_attribute_literals(&mut self.base, node, own_symbol_id);
        }

        let Some(child_depth) = child_tree_depth(depth) else {
            return;
        };
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.walk_literals(
                child,
                child_containing_symbol_id,
                child_depth,
                symbols_by_start_byte,
            );
        }
    }
}

/// `sql` for the body of a MyBatis mapper statement (`select`, `insert`,
/// `update`, `delete`, `sql`): the `content` node between its tags.
pub(crate) fn embedded_sql_language(
    _file_path: &str,
    content: &str,
    node: tree_sitter::Node<'_>,
) -> Option<&'static str> {
    if node.kind() != "content" {
        return None;
    }
    let statement = node.parent().filter(|parent| parent.kind() == "element")?;
    if !matches!(
        build::local_tag(content, statement),
        Some("select" | "insert" | "update" | "delete" | "sql")
    ) {
        return None;
    }
    let root =
        std::iter::successors(Some(statement), |node| build::parent_element(*node)).last()?;
    let is_mapper = build::local_tag(content, root) == Some("mapper")
        && build::attribute(content, root, "namespace").is_some()
        && build::parent_element(statement).is_some_and(|parent| parent.id() == root.id());
    is_mapper.then_some("sql")
}
