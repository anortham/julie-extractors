mod functions;
mod helpers;
mod identifiers;
mod parameters;
mod relationships;
mod signatures;
mod specs;
pub(crate) mod test_calls;
mod type_facts;
mod types;

use crate::base::{
    BaseExtractor, Identifier, PendingRelationship, Relationship, StructuredPendingRelationship,
    Symbol, SymbolKind,
};
use crate::test_calls::TestCallCategory;
use crate::test_detection::{mark_go_test_containers, normalize_scoped_test_roles};
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};
use std::collections::{HashMap, HashSet};
use tree_sitter::{Node, Tree};

/// Go language extractor that handles Go-specific constructs including:
/// - Structs, interfaces, and type aliases
/// - Functions and methods with receivers
/// - Packages and imports
/// - Constants and variables
/// - Goroutines and channels
/// - Interface implementations and embedding
pub struct GoExtractor {
    pub(crate) base: BaseExtractor,
    ginkgo_enabled: bool,
    ginkgo_node_ids: HashSet<String>,
    ginkgo_scoped_ids: HashSet<String>,
    test_role_ids: HashSet<String>,
}

impl GoExtractor {
    pub fn new(
        language: String,
        file_path: String,
        content: String,
        workspace_root: &std::path::Path,
    ) -> Self {
        Self {
            base: BaseExtractor::new(language, file_path, content, workspace_root),
            ginkgo_enabled: false,
            ginkgo_node_ids: HashSet::new(),
            ginkgo_scoped_ids: HashSet::new(),
            test_role_ids: HashSet::new(),
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

    /// Add a pending relationship (used during extraction)
    pub fn add_pending_relationship(&mut self, pending: PendingRelationship) {
        self.base.add_pending_relationship(pending);
    }

    pub fn add_structured_pending_relationship(&mut self, pending: StructuredPendingRelationship) {
        self.base.add_structured_pending_relationship(pending);
    }

    pub fn get_structured_pending_relationships(&self) -> Vec<StructuredPendingRelationship> {
        self.base.get_structured_pending_relationships()
    }

    /// Extract symbols from Go source code - direct port from reference logic
    pub fn extract_symbols(&mut self, tree: &Tree) -> Vec<Symbol> {
        self.ginkgo_enabled = test_calls::file_enables_ginkgo(&self.base, tree.root_node());
        self.test_role_ids.clear();

        let mut symbols = Vec::new();
        self.walk_tree(tree.root_node(), &mut symbols, None, 0);
        self.recover_function_symbols_from_source(&mut symbols);

        // Prioritize functions over fields with the same name (reference logic)
        let mut symbols = self.prioritize_functions_over_fields(symbols);
        self.scope_ginkgo_test_roles(&mut symbols);
        mark_go_test_containers(&mut symbols);
        symbols
    }

    /// Keep a nested Ginkgo spec or hook only when another Ginkgo node encloses
    /// it.
    ///
    /// Ginkgo builds its spec tree at file scope, and the suite itself is the
    /// implicit root, so a top-level `It` or `BeforeSuite` is a real node and
    /// keeps its role. A `It` written inside an ordinary function body is not:
    /// Ginkgo builds the tree before any test runs, so a spec declared from a
    /// plain helper never joins a suite. Scoping touches the captured Ginkgo
    /// calls alone; a `TestXxx` function or a testify suite method is a root of
    /// its own and keeps the role its name earned.
    fn scope_ginkgo_test_roles(&self, symbols: &mut [Symbol]) {
        if self.ginkgo_scoped_ids.is_empty() {
            return;
        }

        let slots: Vec<usize> = symbols
            .iter()
            .enumerate()
            .filter(|(_, symbol)| self.ginkgo_scoped_ids.contains(&symbol.id))
            .map(|(slot, _)| slot)
            .collect();
        let mut scoped: Vec<Symbol> = slots.iter().map(|slot| symbols[*slot].clone()).collect();

        normalize_scoped_test_roles(&mut scoped, &self.ginkgo_node_ids);

        for (slot, symbol) in slots.into_iter().zip(scoped) {
            symbols[slot] = symbol;
        }
    }

    fn extract_ginkgo_call(&mut self, node: Node, parent_id: Option<&str>) -> Option<Symbol> {
        if !self.ginkgo_enabled {
            return None;
        }

        let call = test_calls::extract_ginkgo_test_call(&mut self.base, node, parent_id)?;
        self.ginkgo_node_ids.insert(call.symbol.id.clone());
        let nested_leaf = parent_id.is_some()
            && matches!(
                call.category,
                TestCallCategory::Test | TestCallCategory::Lifecycle
            );
        if nested_leaf {
            self.ginkgo_scoped_ids.insert(call.symbol.id.clone());
        }
        Some(call.symbol)
    }

    fn extract_call(&mut self, node: Node, parent_id: Option<&str>) -> Option<Symbol> {
        let enclosing_test = parent_id.is_some_and(|id| self.test_role_ids.contains(id));
        if let Some(symbol) = test_calls::extract_standard_subtest_call(
            &mut self.base,
            node,
            parent_id,
            enclosing_test,
        ) {
            return Some(symbol);
        }

        self.extract_ginkgo_call(node, parent_id)
    }

    pub fn extract_relationships(&mut self, tree: &Tree, symbols: &[Symbol]) -> Vec<Relationship> {
        let mut relationships = Vec::new();
        let symbol_map = self.build_symbol_map(symbols);

        // Extract relationships from the AST
        self.walk_tree_for_relationships(tree.root_node(), &symbol_map, &mut relationships, 0);

        relationships
    }

    pub fn infer_types(&self, symbols: &[Symbol]) -> HashMap<String, String> {
        let mut types = HashMap::new();

        for symbol in symbols {
            if let Some(signature) = &symbol.signature {
                // Extract type information from signatures
                match symbol.kind {
                    SymbolKind::Function | SymbolKind::Method => {
                        if let Some(return_type) =
                            self.extract_return_type_from_signature(signature)
                        {
                            types.insert(symbol.id.clone(), return_type);
                        }
                    }
                    SymbolKind::Variable | SymbolKind::Constant => {
                        if let Some(var_type) = self.extract_variable_type_from_signature(signature)
                        {
                            types.insert(symbol.id.clone(), var_type);
                        }
                    }
                    _ => {}
                }
            }
        }

        types
    }

    /// Extract all identifier usages (function calls, member access, etc.)
    /// Following the Rust extractor reference implementation pattern
    pub fn extract_identifiers(&mut self, tree: &Tree, symbols: &[Symbol]) -> Vec<Identifier> {
        let containing_symbols = self.base.containing_symbol_index(symbols);
        self.walk_tree_for_identifiers(tree.root_node(), &containing_symbols, 0);
        self.base.identifiers.clone()
    }

    /// Prioritize functions over fields with the same name (reference implementation)
    fn prioritize_functions_over_fields(&self, symbols: Vec<Symbol>) -> Vec<Symbol> {
        let mut symbol_map: HashMap<String, Vec<Symbol>> = HashMap::new();

        // Group symbols by name
        for symbol in symbols {
            symbol_map
                .entry(symbol.name.clone())
                .or_default()
                .push(symbol);
        }

        let mut result = Vec::new();

        // For each name group, add functions first, then other types
        for (_name, symbol_group) in symbol_map {
            let functions: Vec<Symbol> = symbol_group
                .iter()
                .filter(|s| s.kind == SymbolKind::Function || s.kind == SymbolKind::Method)
                .cloned()
                .collect();
            let others: Vec<Symbol> = symbol_group
                .iter()
                .filter(|s| s.kind != SymbolKind::Function && s.kind != SymbolKind::Method)
                .cloned()
                .collect();

            result.extend(functions);
            result.extend(others);
        }

        result
    }

    /// Walk the tree and extract symbols (port from walkTree method)
    fn walk_tree(
        &mut self,
        node: Node,
        symbols: &mut Vec<Symbol>,
        parent_id: Option<String>,
        depth: u32,
    ) {
        if !should_visit_tree_depth(depth) {
            return;
        }

        // Handle declarations that can produce multiple symbols
        match node.kind() {
            "import_declaration" => {
                let import_symbols = self.extract_import_symbols(node, parent_id.as_deref());
                symbols.extend(import_symbols);
            }
            "var_declaration" => {
                let var_symbols = self.extract_var_symbols(node, parent_id.as_deref());
                symbols.extend(var_symbols);
            }
            "short_var_declaration" => {
                let local_symbols = self.extract_short_var_symbols(node, parent_id.as_deref());
                symbols.extend(local_symbols);
            }
            "const_declaration" => {
                let const_symbols = self.extract_const_symbols(node, parent_id.as_deref());
                symbols.extend(const_symbols);
            }
            "field_declaration" => {
                // Fields can have multiple names on same line (X, Y float64)
                let field_symbols = self.extract_field(node, parent_id.as_deref());
                symbols.extend(field_symbols);
                return; // Don't walk children - fields are leaf nodes
            }
            _ => {
                if let Some(symbol) = self.extract_symbol(node, parent_id.as_deref()) {
                    let symbol_id = symbol.id.clone();
                    if symbol
                        .metadata
                        .as_ref()
                        .and_then(|metadata| metadata.get("is_test"))
                        .and_then(serde_json::Value::as_bool)
                        == Some(true)
                    {
                        self.test_role_ids.insert(symbol_id.clone());
                    }
                    symbols.push(symbol);
                    if matches!(node.kind(), "function_declaration" | "method_declaration") {
                        let parameter_symbols =
                            parameters::extract_parameter_symbols(&mut self.base, node, &symbol_id);
                        symbols.extend(parameter_symbols);
                    }

                    // Recursively walk children with the new parent_id
                    let Some(child_depth) = child_tree_depth(depth) else {
                        return;
                    };
                    let mut cursor = node.walk();
                    for child in node.children(&mut cursor) {
                        self.walk_tree(child, symbols, Some(symbol_id.clone()), child_depth);
                    }
                    return;
                }
            }
        }

        // If no symbol was created, continue walking children with same parent_id
        let Some(child_depth) = child_tree_depth(depth) else {
            return;
        };
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.walk_tree(child, symbols, parent_id.clone(), child_depth);
        }
    }

    /// Extract symbol from node (port from extractSymbol method)
    fn extract_symbol(&mut self, node: Node, parent_id: Option<&str>) -> Option<Symbol> {
        match node.kind() {
            "package_clause" => self.extract_package(node, parent_id),
            "type_declaration" => self.extract_type_declaration(node, parent_id),
            "function_declaration" => self.extract_function(node, parent_id),
            "method_declaration" => self.extract_method(node, parent_id),
            // "field_declaration" handled in walk_tree (can produce multiple symbols)
            "call_expression" => self.extract_call(node, parent_id),
            "ERROR" => self.extract_from_error_node(node, parent_id),
            _ => None,
        }
    }

    fn build_symbol_map<'a>(&self, symbols: &'a [Symbol]) -> HashMap<String, &'a Symbol> {
        let mut by_name: HashMap<&str, Vec<&'a Symbol>> = HashMap::new();
        for symbol in symbols {
            by_name.entry(&symbol.name).or_default().push(symbol);
        }

        by_name
            .into_iter()
            .filter_map(|(name, candidates)| match candidates.as_slice() {
                [symbol] => Some((name.to_string(), *symbol)),
                _ => single_callable_with_module_duplicate(&candidates)
                    .map(|symbol| (name.to_string(), symbol)),
            })
            .collect()
    }
}

fn single_callable_with_module_duplicate<'a>(symbols: &[&'a Symbol]) -> Option<&'a Symbol> {
    let callable_symbols: Vec<_> = symbols
        .iter()
        .copied()
        .filter(|symbol| matches!(symbol.kind, SymbolKind::Function | SymbolKind::Method))
        .collect();

    match callable_symbols.as_slice() {
        [callable]
            if symbols.iter().all(|symbol| {
                matches!(
                    symbol.kind,
                    SymbolKind::Function | SymbolKind::Method | SymbolKind::Module
                )
            }) =>
        {
            Some(*callable)
        }
        _ => None,
    }
}
