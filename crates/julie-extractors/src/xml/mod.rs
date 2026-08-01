//! XML extractor — name-promoted elements as symbols, QName attribute references as identifiers.
//!
//! An element becomes a symbol only when it carries a `name` or `id` attribute; the attribute
//! value is the symbol name (`<xs:complexType name="AddPhone">` → `AddPhone`). Anonymous
//! structural elements (`<xs:sequence>`, `<item>`, `<row>`) emit nothing, so a document with
//! thousands of repeated rows still yields only the handful of named components. Named elements
//! chain to their nearest named ancestor the way YAML mapping keys chain.
//!
//! Attribute values of `type`, `ref`, `base`, and `element` become `type_usage` identifiers
//! carrying the raw QName, but only in schema context — the owning element must sit in a
//! declared XML Schema or WSDL namespace, or the attribute itself must (`xsi:type`), because
//! those four names are ordinary words that a generic document uses for its own purposes.
//! Every non-empty attribute value is captured as a literal under a `tag.attribute` carrier
//! regardless of dialect. Relationships and types are out of scope for the XML tier.
//!
//! Common use cases:
//! - XSD schemas (complexType/element/simpleType declarations and their references)
//! - WSDL service definitions (service/port/operation/message)
//! - Project and application configuration documents

mod elements;
mod identifiers;

use std::collections::HashMap;
use std::path::Path;

use crate::base::{BaseExtractor, Identifier, Relationship, Symbol};
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
        self.base.identifiers.clone()
    }

    pub fn extract_relationships(
        &mut self,
        _tree: &tree_sitter::Tree,
        _symbols: &[Symbol],
    ) -> Vec<Relationship> {
        Vec::new()
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
            if let Some(symbol) =
                elements::extract_element(&mut self.base, node, child_parent_id.as_deref())
            {
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
