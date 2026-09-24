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

mod annotation_repair;
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
    /// Same-file names and return types for property initializer inference.
    initializer_index: type_facts::InitializerIndex,
    annotation_repair: Option<annotation_repair::AnnotationRepair>,
    /// Whether the file may hold Kotest / Spek DSL calls at all.
    test_dsl_active: bool,
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
            initializer_index: type_facts::InitializerIndex::default(),
            annotation_repair: None,
            test_dsl_active: false,
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
        self.annotation_repair = annotation_repair::repair(&self.base.content, tree);
        let tree = self.working_tree(tree);
        self.initializer_index = type_facts::InitializerIndex::build(&self.base, tree.root_node());
        self.test_dsl_active = test_calls::test_dsl_is_active(&self.base);
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
            return;
        }

        let new_parent_id = self
            .extract_node_symbol(node, symbols, parent_id.as_deref())
            .or(parent_id);

        let Some(child_depth) = child_tree_depth(depth) else {
            return;
        };
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.visit_node(child, symbols, new_parent_id.clone(), child_depth);
        }
    }

    /// Extract the symbol `node` declares, push it with its parameters, and
    /// return the id its children take as parent. Kept out of `visit_node` so
    /// the recursive frame never holds a `Symbol`.
    #[inline(never)]
    fn extract_node_symbol(
        &mut self,
        node: Node,
        symbols: &mut Vec<Symbol>,
        parent_id: Option<&str>,
    ) -> Option<String> {
        let parent_kind = || {
            parent_id.and_then(|pid| symbols.iter().find(|s| s.id == pid).map(|s| s.kind.clone()))
        };
        let symbol: Option<Symbol> = match node.kind() {
            "class_declaration" | "enum_declaration" => {
                types::extract_class(&mut self.base, &node, parent_id)
            }
            "interface_declaration" => types::extract_interface(&mut self.base, &node, parent_id),
            "object_declaration" => types::extract_object(&mut self.base, &node, parent_id),
            "companion_object" => Some(types::extract_companion_object(
                &mut self.base,
                &node,
                parent_id,
            )),
            "function_declaration" => {
                let parent_kind = parent_kind();
                declarations::extract_function(&mut self.base, &node, parent_id, parent_kind)
            }
            "property_declaration" | "property_signature" => {
                let parent_kind = parent_kind();
                properties::extract_property(
                    &mut self.base,
                    &node,
                    parent_id,
                    parent_kind,
                    &self.initializer_index,
                )
            }
            // An accessor is named `get`/`set`, never a call target.
            "getter" | "setter" => {
                let symbol = properties::extract_accessor(&mut self.base, &node, parent_id);
                self.record_dsl_call_symbol(symbol.as_ref());
                symbol
            }
            "enum_class_body" => {
                types::extract_enum_members(&mut self.base, &node, symbols, parent_id);
                None
            }
            // An enum entry with a body (`ADD { override fun apply() … }`)
            // owns the members declared in that body.
            "enum_entry" => {
                return symbols
                    .iter()
                    .find(|symbol| {
                        symbol.start_byte == node.start_byte() as u32
                            && symbol.kind == crate::base::SymbolKind::EnumMember
                    })
                    .map(|symbol| symbol.id.clone());
            }
            "primary_constructor" => {
                properties::extract_constructor_parameters(
                    &mut self.base,
                    &node,
                    symbols,
                    parent_id,
                );
                None
            }
            "secondary_constructor" => {
                let class_name = parent_id
                    .and_then(|pid| symbols.iter().find(|s| s.id == pid))
                    .map(|s| s.name.clone())
                    .unwrap_or_else(|| "constructor".to_string());
                declarations::extract_secondary_constructor(
                    &mut self.base,
                    &node,
                    parent_id,
                    &class_name,
                )
            }
            "package_header" => declarations::extract_package(&mut self.base, &node, parent_id),
            "import" => declarations::extract_import(&mut self.base, &node, parent_id),
            "type_alias" => declarations::extract_type_alias(&mut self.base, &node, parent_id),
            // Kotest / Spek call-style tests: `describe("name") { it("name") { } }`,
            // `test("n") { }`, `beforeEach { }`. Non-DSL calls return None.
            "call_expression" if self.test_dsl_active => {
                let symbol = test_calls::extract_kotlin_test_call(&mut self.base, &node, parent_id);
                self.record_dsl_call_symbol(symbol.as_ref());
                symbol
            }
            // Kotest WordSpec (`"subject" should { }`) and FreeSpec
            // (`"subject" - { }`) open a group with an infix or operator call.
            "infix_expression" if self.test_dsl_active => {
                let symbol =
                    test_calls::extract_kotlin_wordspec_group(&mut self.base, &node, parent_id);
                self.record_dsl_call_symbol(symbol.as_ref());
                symbol
            }
            "binary_expression" if self.test_dsl_active => {
                let symbol =
                    test_calls::extract_kotlin_freespec_group(&mut self.base, &node, parent_id);
                self.record_dsl_call_symbol(symbol.as_ref());
                symbol
            }
            // ERROR recovery: tree-sitter wraps a class it cannot fully parse
            // (`class Foo\nprivate constructor(...)`) in an ERROR node whose
            // children still hold the class structure.
            "ERROR" => {
                let has_keyword = |keyword: &str| {
                    node.children(&mut node.walk())
                        .any(|n| !n.is_named() && self.base.get_node_text(&n) == keyword)
                };
                let has_identifier = node
                    .children(&mut node.walk())
                    .any(|n| n.kind() == "identifier");
                if has_keyword("class") && has_identifier {
                    types::extract_class(&mut self.base, &node, parent_id)
                } else if has_keyword("interface") && has_identifier {
                    types::extract_interface(&mut self.base, &node, parent_id)
                } else {
                    None
                }
            }
            _ => None,
        };

        let mut symbol = symbol?;
        declarations::apply_declaration_body_span(&self.base, &node, &mut symbol);
        if let Some(repair) = &self.annotation_repair {
            repair.attach(&mut self.base, &node, &mut symbol);
        }
        let symbol_id = symbol.id.clone();
        symbols.push(symbol);
        if matches!(
            node.kind(),
            "function_declaration" | "secondary_constructor"
        ) {
            symbols.extend(parameters::extract_parameter_symbols(
                &mut self.base,
                node,
                &symbol_id,
            ));
        }
        Some(symbol_id)
    }

    pub fn infer_types(&self, symbols: &[Symbol]) -> HashMap<String, String> {
        type_facts::metadata_base_types(symbols)
    }

    pub fn extract_relationships(&mut self, tree: &Tree, symbols: &[Symbol]) -> Vec<Relationship> {
        let tree = self.working_tree(tree);
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
            "property_declaration"
                if node
                    .parent()
                    .is_some_and(|parent| parent.kind() == "source_file") =>
            {
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
        let annotation_nodes = self
            .annotation_repair
            .as_ref()
            .map(|repair| repair.original_annotation_nodes(tree))
            .unwrap_or_default();
        let working = self.working_tree(tree);
        identifiers::extract_identifiers(&mut self.base, &working, &annotation_nodes, symbols)
    }

    /// The tree extraction walks: the annotation-repaired reparse when the
    /// grammar detached top-level annotations, else the parsed tree.
    fn working_tree(&self, tree: &Tree) -> Tree {
        self.annotation_repair
            .as_ref()
            .map_or_else(|| tree.clone(), |repair| repair.tree.clone())
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
