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
    ginkgo_alias: Option<String>,
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
            ginkgo_alias: None,
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

    /// Clone captured call-argument literals.
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
        self.ginkgo_alias = test_calls::ginkgo_package_alias(&self.base, tree.root_node());
        self.test_role_ids.clear();

        let mut symbols = Vec::new();
        self.walk_tree(tree.root_node(), &mut symbols, None, 0);
        self.recover_function_symbols_from_source(tree.root_node(), &mut symbols);

        // Prioritize functions over fields with the same name (reference logic)
        let mut symbols = self.prioritize_functions_over_fields(symbols);
        self.scope_ginkgo_test_roles(&mut symbols);
        mark_go_test_containers(&mut symbols);
        let suites = test_calls::gocheck_suite_names(&self.base, tree.root_node());
        for symbol in symbols
            .iter_mut()
            .filter(|symbol| symbol.kind == SymbolKind::Struct && suites.contains(&symbol.name))
        {
            crate::test_detection::apply_test_role(
                symbol.metadata.get_or_insert_with(Default::default),
                crate::base::TestRole::TestContainer,
            );
        }
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

        let call = test_calls::extract_ginkgo_test_call(
            &mut self.base,
            node,
            parent_id,
            self.ginkgo_alias.as_deref(),
        )?;
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
        let scope = relationships::RelationshipScope {
            symbols,
            symbol_map: self.build_symbol_map(symbols),
            containers: self.base.containing_symbol_index(symbols),
        };
        self.walk_tree_for_relationships(tree.root_node(), &scope, &mut relationships, 0);

        relationships
    }

    pub fn infer_types(&self, symbols: &[Symbol]) -> HashMap<String, String> {
        let mut types = HashMap::new();

        for symbol in symbols {
            if let Some(signature) = &symbol.signature {
                // Extract type information from signatures
                match symbol.kind {
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
                self.walk_var_declaration(node, symbols, parent_id, depth);
                return;
            }
            "short_var_declaration" => {
                let local_symbols = self.extract_short_var_symbols(node, parent_id.as_deref());
                symbols.extend(local_symbols);
            }
            "range_clause" | "type_switch_statement" => {
                let bindings = self.extract_binding_symbols(node, parent_id.as_deref());
                symbols.extend(bindings);
            }
            "const_declaration" => {
                let const_symbols = self.extract_const_symbols(node, parent_id.as_deref());
                symbols.extend(const_symbols);
            }
            "type_declaration" => {
                self.walk_type_declaration(node, symbols, parent_id, depth);
                return;
            }
            "field_declaration" => {
                if !is_anonymous_struct_in_body(node) {
                    let field_symbols = self.extract_field(node, parent_id.as_deref());
                    symbols.extend(field_symbols);
                }
                return;
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

    /// A variable declaration parents what its initializer declares (the
    /// locals of a `func` literal assigned to it), like a function does.
    #[inline(never)]
    fn walk_var_declaration(
        &mut self,
        node: Node,
        symbols: &mut Vec<Symbol>,
        parent_id: Option<String>,
        depth: u32,
    ) {
        let first_new = symbols.len();
        symbols.extend(self.extract_var_symbols(node, parent_id.as_deref()));
        let declared: Vec<(u32, u32, String)> = symbols[first_new..]
            .iter()
            .filter(|symbol| symbol.name != "_")
            .map(|symbol| (symbol.start_byte, symbol.end_byte, symbol.id.clone()))
            .collect();
        let Some(child_depth) = child_tree_depth(depth) else {
            return;
        };
        let mut children = Vec::new();
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "var_spec_list" {
                children.extend(child.children(&mut child.walk()));
            } else {
                children.push(child);
            }
        }
        for child in children {
            let start = child.start_byte() as u32;
            let end = child.end_byte() as u32;
            let owner = declared
                .iter()
                .find(|(symbol_start, symbol_end, _)| *symbol_start <= start && end <= *symbol_end)
                .map(|(_, _, id)| id.clone());
            self.walk_tree(
                child,
                symbols,
                owner.or_else(|| parent_id.clone()),
                child_depth,
            );
        }
    }

    /// Every `type_spec` / `type_alias` in a (possibly grouped) type
    /// declaration becomes its own symbol and parents its own members.
    // Kept out of line: `walk_tree` recurses to the traversal depth budget, so its
    // stack frame must stay small.
    #[inline(never)]
    fn walk_type_declaration(
        &mut self,
        node: Node,
        symbols: &mut Vec<Symbol>,
        parent_id: Option<String>,
        depth: u32,
    ) {
        let Some(child_depth) = child_tree_depth(depth) else {
            return;
        };
        let mut cursor = node.walk();
        for spec in node.children(&mut cursor) {
            let symbol = match spec.kind() {
                "type_spec" => self.extract_type_spec(spec, parent_id.as_deref()),
                "type_alias" => self.extract_type_alias(spec, parent_id.as_deref()),
                _ => None,
            };
            let spec_parent = symbol.as_ref().map(|symbol| symbol.id.clone());
            symbols.extend(symbol);
            let mut spec_cursor = spec.walk();
            for child in spec.children(&mut spec_cursor) {
                self.walk_tree(
                    child,
                    symbols,
                    spec_parent.clone().or_else(|| parent_id.clone()),
                    child_depth,
                );
            }
        }
    }

    /// Extract symbol from node (port from extractSymbol method)
    fn extract_symbol(&mut self, node: Node, parent_id: Option<&str>) -> Option<Symbol> {
        match node.kind() {
            "package_clause" => self.extract_package(node, parent_id),
            "method_elem" => self.extract_method_elem(node, parent_id),
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

/// A field of an anonymous struct type written inside a function body (a
/// table-driven test's `[]struct{ name string }`) declares no named member.
fn is_anonymous_struct_in_body(field: Node) -> bool {
    let mut current = field.parent();
    while let Some(node) = current {
        match node.kind() {
            "type_spec" | "type_alias" | "source_file" => return false,
            "block" => return true,
            _ => current = node.parent(),
        }
    }
    false
}
