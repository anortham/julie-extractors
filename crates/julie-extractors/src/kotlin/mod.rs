//! Kotlin Extractor
//!
//! Implementation of Kotlin extractor to idiomatic Rust.
//!
//! This extractor handles comprehensive Kotlin symbol extraction including:
//! - Classes, data classes, sealed classes, enums
//! - Objects, companion objects
//! - Functions, extension functions, operators
//! - Interfaces, type aliases, annotations
//! - Generics with variance
//! - Property delegation
//! - Constructor parameters

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

pub struct KotlinExtractor {
    pub(crate) base: BaseExtractor,
    /// Ids of the symbols the Kotest / Spek adapter materialized.
    ///
    /// Such a symbol is named by a description string — `"shouldBeZero" { }`
    /// declares a case, not a function called `shouldBeZero` — so it must never
    /// answer a call-target lookup. Kotest's own suite has cases named after the
    /// matcher they exercise, and without this set every such call resolved to
    /// the case that contains it.
    dsl_call_symbol_ids: HashSet<String>,
    same_file_type_names: HashSet<String>,
}

impl KotlinExtractor {
    pub fn new(
        language: String,
        file_path: String,
        content: String,
        workspace_root: &std::path::Path,
    ) -> Self {
        Self {
            base: BaseExtractor::new(language, file_path, content, workspace_root),
            dsl_call_symbol_ids: HashSet::new(),
            same_file_type_names: HashSet::new(),
        }
    }

    /// Get pending relationships that need cross-file resolution
    pub fn get_pending_relationships(&self) -> Vec<PendingRelationship> {
        self.base.get_pending_relationships()
    }

    pub fn get_type_argument_usages(&self) -> Vec<crate::base::TypeArgumentUsage> {
        self.base.get_type_argument_usages()
    }

    /// Clone captured call-argument literals (Miller bridge Phase 3).
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
        self.same_file_type_names = type_facts::collect_type_names(&self.base, tree.root_node());
        let mut symbols = Vec::new();
        self.visit_node(tree.root_node(), &mut symbols, None, 0);
        crate::test_detection::mark_kotlin_test_containers(&mut symbols);
        crate::test_detection::mark_java_test_containers(&mut symbols);
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
            return; // Skip unnamed nodes
        }

        let mut symbol: Option<Symbol> = None;
        let mut new_parent_id = parent_id.clone();

        match node.kind() {
            "class_declaration" | "enum_declaration" => {
                symbol = types::extract_class(&mut self.base, &node, parent_id.as_deref());
            }
            "interface_declaration" => {
                symbol = types::extract_interface(&mut self.base, &node, parent_id.as_deref());
            }
            "object_declaration" => {
                symbol = types::extract_object(&mut self.base, &node, parent_id.as_deref());
            }
            "companion_object" => {
                symbol = Some(types::extract_companion_object(
                    &mut self.base,
                    &node,
                    parent_id.as_deref(),
                ));
            }
            "function_declaration" => {
                symbol =
                    declarations::extract_function(&mut self.base, &node, parent_id.as_deref());
            }
            "property_declaration" | "property_signature" => {
                let parent_kind = parent_id
                    .as_deref()
                    .and_then(|pid| symbols.iter().find(|s| s.id == pid).map(|s| s.kind.clone()));
                let type_names = self.same_file_type_names.clone();
                symbol = properties::extract_property(
                    &mut self.base,
                    &node,
                    parent_id.as_deref(),
                    parent_kind,
                    &type_names,
                );
            }
            "enum_class_body" => {
                types::extract_enum_members(&mut self.base, &node, symbols, parent_id.as_deref());
            }
            "primary_constructor" => {
                properties::extract_constructor_parameters(
                    &mut self.base,
                    &node,
                    symbols,
                    parent_id.as_deref(),
                );
            }
            "secondary_constructor" => {
                // Look up the parent class name from already-extracted symbols
                let class_name = parent_id
                    .as_deref()
                    .and_then(|pid| symbols.iter().find(|s| s.id == pid))
                    .map(|s| s.name.clone())
                    .unwrap_or_else(|| "constructor".to_string());
                symbol = declarations::extract_secondary_constructor(
                    &mut self.base,
                    &node,
                    parent_id.as_deref(),
                    &class_name,
                );
            }
            "package_header" => {
                symbol = declarations::extract_package(&mut self.base, &node, parent_id.as_deref());
            }
            "import" => {
                symbol = declarations::extract_import(&mut self.base, &node, parent_id.as_deref());
            }
            "type_alias" => {
                symbol =
                    declarations::extract_type_alias(&mut self.base, &node, parent_id.as_deref());
            }
            // Kotest / Spek call-style tests (Miller bridge Wave-3).
            // `describe("name") { it("name") { } }`, `test("n") { }`,
            // `beforeEach { }`, etc. Returns None for non-DSL calls (no vocab
            // match or no trailing lambda body), so ordinary call_expressions
            // fall through untouched.
            "call_expression" => {
                symbol = test_calls::extract_kotlin_test_call(
                    &mut self.base,
                    &node,
                    parent_id.as_deref(),
                );
                self.record_dsl_call_symbol(symbol.as_ref());
            }
            // Kotest WordSpec (`"subject" should { }`) and FreeSpec
            // (`"subject" - { }`) open a group with an infix or operator call
            // instead of a named one. Returns None for every other infix and
            // binary expression.
            "infix_expression" => {
                symbol = test_calls::extract_kotlin_wordspec_group(
                    &mut self.base,
                    &node,
                    parent_id.as_deref(),
                );
                self.record_dsl_call_symbol(symbol.as_ref());
            }
            "binary_expression" => {
                symbol = test_calls::extract_kotlin_freespec_group(
                    &mut self.base,
                    &node,
                    parent_id.as_deref(),
                );
                self.record_dsl_call_symbol(symbol.as_ref());
            }
            // ERROR recovery: when tree-sitter can't fully parse a class declaration
            // (e.g., due to unsupported syntax like `class Foo\nprivate constructor(...)`),
            // it wraps the entire class in an ERROR node. The ERROR node's children still
            // contain the class structure (modifiers, "class" keyword, identifier, body),
            // so we can pass it to the same extraction functions.
            "ERROR" => {
                let has_class_keyword = node
                    .children(&mut node.walk())
                    .any(|n| !n.is_named() && self.base.get_node_text(&n) == "class");
                let has_interface_keyword = node
                    .children(&mut node.walk())
                    .any(|n| !n.is_named() && self.base.get_node_text(&n) == "interface");
                let has_identifier = node
                    .children(&mut node.walk())
                    .any(|n| n.kind() == "identifier");

                if has_class_keyword && has_identifier {
                    symbol = types::extract_class(&mut self.base, &node, parent_id.as_deref());
                } else if has_interface_keyword && has_identifier {
                    symbol = types::extract_interface(&mut self.base, &node, parent_id.as_deref());
                }
            }
            _ => {}
        }

        if let Some(sym) = &symbol {
            symbols.push(sym.clone());
            new_parent_id = Some(sym.id.clone());
            if matches!(
                node.kind(),
                "function_declaration" | "secondary_constructor"
            ) {
                symbols.extend(parameters::extract_parameter_symbols(
                    &mut self.base,
                    node,
                    &sym.id,
                ));
            }
        }

        // Recursively visit children
        let Some(child_depth) = child_tree_depth(depth) else {
            return;
        };
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.visit_node(child, symbols, new_parent_id.clone(), child_depth);
        }
    }

    pub fn infer_types(&self, symbols: &[Symbol]) -> HashMap<String, String> {
        type_facts::metadata_base_types(symbols)
    }

    pub fn extract_relationships(&mut self, tree: &Tree, symbols: &[Symbol]) -> Vec<Relationship> {
        let mut relationships = Vec::new();
        self.visit_node_for_relationships(tree.root_node(), symbols, &mut relationships, 0);
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

        match node.kind() {
            "class_declaration"
            | "enum_declaration"
            | "object_declaration"
            | "interface_declaration" => {
                relationships::extract_inheritance_relationships(
                    self,
                    &node,
                    symbols,
                    relationships,
                );
                // Also extract method calls from within this type
                relationships::extract_call_relationships(
                    self,
                    node,
                    symbols,
                    relationships,
                    depth,
                );
            }
            "function_declaration" => {
                // Extract function calls from within this function
                relationships::extract_call_relationships(
                    self,
                    node,
                    symbols,
                    relationships,
                    depth,
                );
            }
            _ => {}
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

    fn record_dsl_call_symbol(&mut self, symbol: Option<&Symbol>) {
        if let Some(symbol) = symbol {
            self.dsl_call_symbol_ids.insert(symbol.id.clone());
        }
    }

    pub(crate) fn is_dsl_call_symbol(&self, symbol_id: &str) -> bool {
        self.dsl_call_symbol_ids.contains(symbol_id)
    }
}

fn dedupe_relationships(relationships: &mut Vec<Relationship>) {
    let mut seen = HashSet::new();
    relationships.retain(|relationship| seen.insert(relationship.id.clone()));
}
