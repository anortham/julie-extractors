//! Scala Extractor
//!
//! Comprehensive Scala symbol extraction including:
//! - Classes, case classes, abstract classes, sealed classes
//! - Traits, objects, companion objects
//! - Functions/methods, vals, vars
//! - Enums (Scala 3), type aliases
//! - Given instances, extension methods
//! - Imports, packages

mod braceless;
mod declarations;
mod helpers;
mod identifiers;
mod parameters;
mod properties;
mod relationships;
pub(crate) mod test_calls;
mod type_facts;
mod types;

use crate::base::{
    BaseExtractor, Identifier, PendingRelationship, Relationship, StructuredPendingRelationship,
    Symbol,
};
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};
use std::collections::{HashMap, HashSet};
use tree_sitter::{Node, Tree};

pub struct ScalaExtractor {
    pub(crate) base: BaseExtractor,
}

impl ScalaExtractor {
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

    /// Get pending relationships that need cross-file resolution
    pub fn get_pending_relationships(&self) -> Vec<PendingRelationship> {
        self.base.get_pending_relationships()
    }

    pub fn get_type_argument_usages(&self) -> Vec<crate::base::TypeArgumentUsage> {
        self.base.get_type_argument_usages()
    }

    /// Clone captured call-argument literals.
    pub fn get_literals(&self) -> Vec<crate::base::Literal> {
        self.base.get_literals()
    }

    pub fn get_structured_pending_relationships(&self) -> Vec<StructuredPendingRelationship> {
        self.base.get_structured_pending_relationships()
    }

    /// Add a pending relationship (used during extraction)
    pub fn add_pending_relationship(&mut self, pending: PendingRelationship) {
        self.base.add_pending_relationship(pending);
    }

    pub fn add_structured_pending_relationship(&mut self, pending: StructuredPendingRelationship) {
        self.base.add_structured_pending_relationship(pending);
    }

    pub fn extract_symbols(&mut self, tree: &Tree) -> Vec<Symbol> {
        let mut symbols = Vec::new();
        self.visit_node(tree.root_node(), &mut symbols, None, 0);
        crate::test_detection::mark_scala_test_containers(&mut symbols);
        symbols
    }

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

        if !node.is_named() {
            return;
        }

        let new_parent_id = self
            .extract_node_symbols(node, symbols, parent_id.as_deref())
            .or(parent_id);

        let Some(child_depth) = child_tree_depth(depth) else {
            return;
        };
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.visit_node(child, symbols, new_parent_id.clone(), child_depth);
        }
    }

    /// Extract the symbols `node` declares, push them, and return the id its
    /// children take as parent. Kept out of `visit_node` so the recursive frame
    /// never holds a `Symbol`.
    #[inline(never)]
    fn extract_node_symbols(
        &mut self,
        node: Node,
        symbols: &mut Vec<Symbol>,
        parent_id: Option<&str>,
    ) -> Option<String> {
        let base = &mut self.base;
        let symbol = match node.kind() {
            "class_definition" => types::extract_class(base, &node, parent_id),
            "trait_definition" => types::extract_trait(base, &node, parent_id),
            "object_definition" => types::extract_object(base, &node, symbols, parent_id),
            "enum_definition" => types::extract_enum(base, &node, parent_id),
            "simple_enum_case" | "full_enum_case" => {
                types::extract_enum_case(base, &node, parent_id)
            }
            "function_definition" | "function_declaration" => {
                declarations::extract_function(base, &node, parent_id)
            }
            "val_definition" | "val_declaration" | "var_definition" | "var_declaration" => {
                let mut bindings = properties::extract_bindings(base, &node, parent_id);
                if bindings.len() != 1 {
                    symbols.append(&mut bindings);
                    return None;
                }
                bindings.pop()
            }
            "import_declaration" => declarations::extract_import(base, &node, parent_id),
            "package_clause" => declarations::extract_package(base, &node, parent_id),
            "package_object" => declarations::extract_package_object(base, &node, parent_id),
            "type_definition" => declarations::extract_type_alias(base, &node, parent_id),
            "given_definition" => declarations::extract_given(base, &node, parent_id),
            "extension_definition" => declarations::extract_extension(base, &node, parent_id),
            // ScalaTest / MUnit call-style tests. Curried call form
            // `test("n") { }` / `describe(...) { it(...) }`, and FlatSpec infix
            // form `"subject" should "behaviour" in { }`. Both return None for
            // non-test nodes, so ordinary calls/infix fall through untouched.
            "call_expression" => test_calls::extract_scala_test_call(base, &node, parent_id),
            "assignment_expression" => {
                test_calls::extract_scalacheck_property(base, &node, parent_id)
            }
            "infix_expression" => test_calls::extract_scala_flatspec_test(base, &node, parent_id),
            _ => None,
        };

        let mut symbol = symbol?;
        braceless::apply(&mut self.base, &node, &mut symbol);
        let base_types = helpers::extends_type_names(&self.base, &node);
        if !base_types.is_empty()
            && matches!(
                node.kind(),
                "class_definition" | "trait_definition" | "object_definition"
            )
        {
            symbol
                .metadata
                .get_or_insert_with(Default::default)
                .insert("base_types".to_string(), serde_json::json!(base_types));
        }

        let symbol_id = symbol.id.clone();
        symbols.push(symbol);
        if matches!(node.kind(), "function_definition" | "function_declaration") {
            symbols.extend(parameters::extract_parameter_symbols(
                &mut self.base,
                node,
                &symbol_id,
            ));
        }
        if matches!(
            node.kind(),
            "class_definition" | "enum_definition" | "full_enum_case"
        ) {
            properties::extract_constructor_fields(&mut self.base, &node, symbols, &symbol_id);
        }
        Some(symbol_id)
    }

    pub fn infer_types(&self, symbols: &[Symbol]) -> HashMap<String, String> {
        type_facts::metadata_base_types(symbols)
    }

    pub fn extract_relationships(&mut self, tree: &Tree, symbols: &[Symbol]) -> Vec<Relationship> {
        let mut relationships = Vec::new();
        self.visit_node_for_relationships(tree.root_node(), symbols, &mut relationships, 0);
        relationships::extract_call_relationships(
            self,
            tree.root_node(),
            symbols,
            &mut relationships,
            0,
        );
        dedupe_relationships(&mut relationships);
        relationships
    }

    fn visit_node_for_relationships(
        &mut self,
        node: Node,
        symbols: &[Symbol],
        relationships: &mut Vec<Relationship>,
        depth: u32,
    ) {
        if !should_visit_tree_depth(depth) {
            return;
        }

        if matches!(
            node.kind(),
            "class_definition"
                | "trait_definition"
                | "object_definition"
                | "enum_definition"
                | "simple_enum_case"
                | "full_enum_case"
                | "given_definition"
        ) {
            relationships::extract_inheritance_relationships(self, &node, symbols, relationships);
        }

        let Some(child_depth) = child_tree_depth(depth) else {
            return;
        };
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.visit_node_for_relationships(child, symbols, relationships, child_depth);
        }
    }

    pub fn extract_identifiers(&mut self, tree: &Tree, symbols: &[Symbol]) -> Vec<Identifier> {
        identifiers::extract_identifiers(&mut self.base, tree, symbols)
    }

    // ========================================================================
    // Accessors for sub-modules
    // ========================================================================

    pub(crate) fn base(&self) -> &BaseExtractor {
        &self.base
    }
}

fn dedupe_relationships(relationships: &mut Vec<Relationship>) {
    let mut seen = HashSet::new();
    relationships.retain(|relationship| seen.insert(relationship.id.clone()));
}
