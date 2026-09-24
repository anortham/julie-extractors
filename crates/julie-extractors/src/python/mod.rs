pub(crate) mod assignments;
/// Python extractor for extracting symbols and relationships from Python source code
/// Implementation of Python extractor with comprehensive Python feature support
///
/// This module is organized into focused sub-modules:
/// - helpers: Shared utility functions
/// - types: Class, enum, dataclass extraction
/// - functions: Function and method extraction
/// - signatures: Parameter and type hint extraction
/// - decorators: Decorator extraction and handling
/// - imports: Import statement handling
/// - assignments: Variable and constant assignment extraction
/// - relationships: Inheritance and call relationship extraction
/// - identifiers: LSP identifier tracking for references
pub(crate) mod decorators;
pub(crate) mod functions;
pub(crate) mod helpers;
pub(crate) mod identifiers;
pub(crate) mod imports;
pub(crate) mod relationships;
pub(crate) mod signatures;
pub(crate) mod type_arguments;
pub(crate) mod type_facts;
pub(crate) mod types;

use crate::base::{BaseExtractor, Identifier, Relationship, StructuredPendingRelationship, Symbol};
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};
use std::collections::{HashMap, HashSet};
use tree_sitter::{Node, Tree};

/// Python extractor for extracting symbols and relationships from Python source code
pub struct PythonExtractor {
    pub(crate) base: BaseExtractor,
    pub(crate) same_file_class_names: HashSet<String>,
    /// Ids of `self.x` attribute symbols, so a class keeps one row per attribute.
    pub(crate) instance_attribute_ids: HashSet<String>,
    /// Declared return types of the file's functions, for assignment inference.
    pub(crate) return_types: type_facts::ReturnTypeIndex,
}

impl PythonExtractor {
    pub fn new(file_path: String, content: String, workspace_root: &std::path::Path) -> Self {
        Self {
            base: BaseExtractor::new("python".to_string(), file_path, content, workspace_root),
            same_file_class_names: HashSet::new(),
            instance_attribute_ids: HashSet::new(),
            return_types: type_facts::ReturnTypeIndex::default(),
        }
    }

    /// Extract all symbols from Python source code
    pub fn extract_symbols(&mut self, tree: &Tree) -> Vec<Symbol> {
        self.same_file_class_names = types::collect_class_names(self, tree.root_node());
        self.return_types = type_facts::ReturnTypeIndex::build(&self.base, tree.root_node());
        let mut symbols = Vec::new();
        self.traverse_tree(tree.root_node(), &mut symbols, 0);
        assignments::keep_first_attribute_declaration(&mut symbols, &self.instance_attribute_ids);
        crate::test_detection::mark_python_test_containers(&mut symbols);
        symbols
    }

    fn traverse_tree(&mut self, node: Node, symbols: &mut Vec<Symbol>, depth: u32) {
        if !should_visit_tree_depth(depth) {
            return;
        }

        match node.kind() {
            "class_definition" => {
                if let Some(symbol) = types::extract_class(self, node) {
                    symbols.push(symbol);
                }
            }
            "function_definition" => {
                if let Some(symbol) = functions::extract_function(self, node) {
                    let parameter_symbols =
                        signatures::extract_parameter_symbols(self, node, &symbol.id);
                    symbols.push(symbol);
                    symbols.extend(parameter_symbols);
                }
            }
            "async_function_definition" => {
                if let Some(symbol) = functions::extract_async_function(self, node) {
                    let parameter_symbols =
                        signatures::extract_parameter_symbols(self, node, &symbol.id);
                    symbols.push(symbol);
                    symbols.extend(parameter_symbols);
                }
            }
            "assignment" => {
                // Can produce multiple symbols for tuple unpacking (a, b = 1, 2)
                let assignment_symbols = assignments::extract_assignment(self, node);
                symbols.extend(assignment_symbols);
            }
            "import_statement" | "import_from_statement" => {
                let import_symbols = imports::extract_imports(self, node);
                symbols.extend(import_symbols);
            }
            "type_alias_statement" => {
                if let Some(symbol) = types::extract_type_alias(self, node) {
                    symbols.push(symbol);
                }
            }
            "lambda" if node.is_named() => {
                let symbol = functions::extract_lambda(self, node);
                symbols.push(symbol);
            }
            _ => {}
        }

        // Recursively traverse children
        let Some(child_depth) = child_tree_depth(depth) else {
            return;
        };
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.traverse_tree(child, symbols, child_depth);
        }
    }

    /// Extract relationships from Python code
    pub fn extract_relationships(&mut self, tree: &Tree, symbols: &[Symbol]) -> Vec<Relationship> {
        relationships::extract_relationships(self, tree, symbols)
    }

    /// Python records every type fact on the base extractor during symbol
    /// extraction, from annotation nodes only; nothing is inferred from text.
    pub fn infer_types(&self, _symbols: &[Symbol]) -> HashMap<String, String> {
        HashMap::new()
    }

    /// Extract all identifier usages (function calls, member access, etc.)
    /// Following the Rust extractor reference implementation pattern
    pub fn extract_identifiers(&mut self, tree: &Tree, symbols: &[Symbol]) -> Vec<Identifier> {
        identifiers::extract_identifiers(self, tree, symbols)
    }

    // ========================================================================
    // Accessors for sub-modules
    // ========================================================================

    pub(crate) fn base(&self) -> &BaseExtractor {
        &self.base
    }

    pub(crate) fn base_mut(&mut self) -> &mut BaseExtractor {
        &mut self.base
    }

    // ========================================================================
    // Pending Relationship Management
    // ========================================================================

    pub(crate) fn add_structured_pending_relationship(
        &mut self,
        pending: StructuredPendingRelationship,
    ) {
        self.base.add_structured_pending_relationship(pending);
    }

    /// Get all pending relationships collected during extraction
    pub fn get_pending_relationships(&self) -> Vec<crate::base::PendingRelationship> {
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
}
