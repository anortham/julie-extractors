/// Ruby language extractor with support for:
/// - Modules, classes, singleton classes
/// - Methods, singleton methods, initialize/constructor
/// - Variables, constants, aliases
/// - Assignments, parallel assignments, rest assignments
/// - Special calls: require, attr_accessor, define_method, def_delegator
/// - Relationships: inheritance, module inclusion
/// - Identifier extraction for LSP-quality find_references
///
/// Implementation of comprehensive Ruby extractor
use crate::base::{
    BaseExtractor, Identifier, Relationship, StructuredPendingRelationship, Symbol, Visibility,
};
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};
use std::collections::HashMap;
use tree_sitter::{Node, Tree};

// Private modules - encapsulate implementation details
mod assignments;
mod calls;
pub(crate) mod doc_comments;
pub(crate) mod helpers;
mod identifiers;
mod locals;
mod parameters;
mod relationships;
mod signatures;
mod symbols;
mod type_facts;

/// Ruby extractor that handles Ruby-specific constructs
pub struct RubyExtractor {
    pub(crate) base: BaseExtractor,
    current_visibility: Visibility,
    /// Inside a `class << self` body, where every `def` is a class method.
    in_singleton_class: bool,
    same_file_class_names: std::collections::HashSet<String>,
    recorded_fields: std::collections::HashSet<(Option<String>, String)>,
    /// Locals already declared per scope: a later assignment or `+=` to the
    /// same name writes the existing local and declares nothing.
    recorded_locals: std::collections::HashSet<(Option<String>, String)>,
    /// `private :name` style calls, applied once every symbol exists.
    visibility_calls: Vec<calls::VisibilityCall>,
    /// Literal types read from each assignment's right-hand node.
    literal_types: HashMap<String, String>,
    symbol_map: std::collections::HashMap<String, Symbol>,
}

impl RubyExtractor {
    pub fn new(file_path: String, content: String, workspace_root: &std::path::Path) -> Self {
        Self {
            base: BaseExtractor::new("ruby".to_string(), file_path, content, workspace_root),
            current_visibility: Visibility::Public,
            in_singleton_class: false,
            same_file_class_names: std::collections::HashSet::new(),
            recorded_fields: std::collections::HashSet::new(),
            recorded_locals: std::collections::HashSet::new(),
            visibility_calls: Vec::new(),
            literal_types: HashMap::new(),
            symbol_map: std::collections::HashMap::new(),
        }
    }

    /// Extract all symbols from Ruby source code
    pub fn extract_symbols(&mut self, tree: &Tree) -> Vec<Symbol> {
        let mut symbols = Vec::new();
        self.current_visibility = Visibility::Public; // Reset for each file

        // Clear any previous symbols from symbol_map
        self.symbol_map.clear();
        self.same_file_class_names =
            type_facts::collect_same_file_class_names(&self.base, tree.root_node());
        self.recorded_fields.clear();
        self.recorded_locals.clear();
        self.visibility_calls.clear();
        self.literal_types.clear();
        self.in_singleton_class = false;

        self.traverse_tree(tree.root_node(), &mut symbols);

        // Include additional symbols from symbol_map (parallel assignments, etc.)
        // BUT: Only add symbols that weren't already added during traversal
        // (create_symbol automatically adds to symbol_map, causing duplication)
        let existing_ids: std::collections::HashSet<_> =
            symbols.iter().map(|s| s.id.clone()).collect();

        for (id, symbol) in self.symbol_map.iter() {
            if !existing_ids.contains(id) {
                symbols.push(symbol.clone());
            }
        }

        calls::apply_visibility_calls(&mut symbols, &self.visibility_calls);
        crate::test_detection::mark_ruby_test_containers(&mut symbols);

        symbols
    }

    /// Extract relationships between symbols (inheritance, module inclusion, etc.)
    pub fn extract_relationships(&mut self, tree: &Tree, symbols: &[Symbol]) -> Vec<Relationship> {
        relationships::extract_relationships(self, tree, symbols)
    }

    /// Extract identifier usages for LSP-quality references
    pub fn extract_identifiers(&mut self, tree: &Tree, symbols: &[Symbol]) -> Vec<Identifier> {
        identifiers::extract_identifiers(&mut self.base, tree, symbols)
    }

    /// Literal types of constants, variables, and fields, read from the node
    /// kind of each assignment's right-hand side: `%w[a b]` is an Array,
    /// `%r{x}` a Regexp. A method call on a literal (`"a,b".split`) states no
    /// type. Parameters record nothing.
    pub fn infer_types(&self, _symbols: &[Symbol]) -> HashMap<String, String> {
        self.literal_types.clone()
    }

    // ========================================================================
    // Symbol Extraction - Tree Traversal
    // ========================================================================

    fn traverse_tree(&mut self, node: Node, symbols: &mut Vec<Symbol>) {
        self.traverse_tree_with_parent(node, symbols, None, 0);
    }

    fn traverse_tree_with_parent(
        &mut self,
        node: Node,
        symbols: &mut Vec<Symbol>,
        parent_id: Option<String>,
        depth: u32,
    ) {
        if !should_visit_tree_depth(depth) {
            return;
        }

        let mut symbol_opt: Option<Symbol> = None;
        let mut extra_symbols = Vec::new();
        let scope = symbols::MemberScope {
            visibility: self.current_visibility.clone(),
            class_method: self.in_singleton_class,
        };

        match node.kind() {
            "module" => {
                symbol_opt = symbols::extract_module(&mut self.base, node, parent_id.clone());
            }
            "class" => {
                symbol_opt = symbols::extract_class(&mut self.base, node, parent_id.clone());
            }
            "method" => {
                symbol_opt =
                    symbols::extract_method(&mut self.base, node, parent_id.clone(), &scope);
                if let Some(symbol) = &symbol_opt {
                    extra_symbols =
                        parameters::extract_parameter_symbols(&mut self.base, node, &symbol.id);
                }
            }
            "singleton_method" => {
                symbol_opt =
                    symbols::extract_singleton_method(&mut self.base, node, parent_id.clone());
                if let Some(symbol) = &symbol_opt {
                    extra_symbols =
                        parameters::extract_parameter_symbols(&mut self.base, node, &symbol.id);
                }
            }
            "call" => {
                if let Some(visibility_call) =
                    calls::visibility_call(&self.base, node, parent_id.clone(), &scope)
                {
                    self.visibility_calls.push(visibility_call);
                }
                let call_symbols =
                    calls::extract_call(&mut self.base, node, parent_id.clone(), &scope);
                if call_symbols.len() == 1 {
                    symbol_opt = call_symbols.into_iter().next();
                } else {
                    for s in &call_symbols {
                        self.symbol_map.insert(s.id.clone(), s.clone());
                    }
                    symbols.extend(call_symbols);
                }
            }
            "assignment" | "operator_assignment" => {
                // `Name = Struct.new(...)`, `Class.new(Base)`, and `Data.define(...)`
                // declare a class; its block methods are parented to it.
                if let Some((struct_class, field_props)) =
                    calls::try_extract_struct_new(&mut self.base, node, parent_id.clone())
                {
                    for s in &field_props {
                        self.symbol_map.insert(s.id.clone(), s.clone());
                    }
                    symbols.extend(field_props);
                    symbol_opt = Some(struct_class);
                } else if let Some(symbol) = assignments::extract_assignment(
                    &mut self.base,
                    node,
                    parent_id.clone(),
                    assignments::AssignmentContext {
                        same_file_class_names: &self.same_file_class_names,
                        recorded_fields: &mut self.recorded_fields,
                        recorded_locals: &mut self.recorded_locals,
                        literal_types: &mut self.literal_types,
                        symbol_map: &mut self.symbol_map,
                    },
                ) {
                    self.symbol_map.insert(symbol.id.clone(), symbol.clone());
                    symbols.push(symbol);
                }
            }
            "alias" => {
                symbol_opt =
                    symbols::extract_alias(&mut self.base, node, parent_id.clone(), &scope);
            }
            "identifier" => {
                // Handle visibility modifiers
                let text = self.base.get_node_text(&node);
                if let Some(new_visibility) = helpers::parse_visibility(&text) {
                    self.current_visibility = new_visibility;
                }
            }
            _ => {}
        }

        for parameter in &extra_symbols {
            self.recorded_locals
                .insert((parameter.parent_id.clone(), parameter.name.clone()));
        }

        // Add symbol to collection and update parent_id for children
        let current_parent_id = if let Some(symbol) = symbol_opt {
            let symbol_id = symbol.id.clone();
            self.symbol_map.insert(symbol_id.clone(), symbol.clone());
            symbols.push(symbol);
            for s in &extra_symbols {
                self.symbol_map.insert(s.id.clone(), s.clone());
            }
            symbols.extend(std::mem::take(&mut extra_symbols));
            Some(symbol_id)
        } else {
            parent_id
        };

        // Each class, module, and `class << self` body starts public.
        let old_visibility = self.current_visibility.clone();
        let old_singleton = self.in_singleton_class;
        if matches!(node.kind(), "class" | "module" | "singleton_class") {
            self.current_visibility = Visibility::Public;
            self.in_singleton_class = node.kind() == "singleton_class";
        }
        if let Some(child_depth) = child_tree_depth(depth) {
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                // Check if child is a visibility modifier that affects subsequent siblings
                if child.kind() == "identifier" {
                    let text = self.base.get_node_text(&child);
                    if let Some(new_visibility) = helpers::parse_visibility(&text) {
                        self.current_visibility = new_visibility;
                    }
                }
                self.traverse_tree_with_parent(
                    child,
                    symbols,
                    current_parent_id.clone(),
                    child_depth,
                );
            }
        }
        self.current_visibility = old_visibility;
        self.in_singleton_class = old_singleton;
    }

    // ========================================================================
    // Accessors for sub-modules
    // ========================================================================

    pub(crate) fn base(&self) -> &BaseExtractor {
        &self.base
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
