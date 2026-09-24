/// Rust language extractor with support for:
/// - Structs, enums, traits, unions
/// - Functions, methods, impl blocks
/// - Modules, macros, type aliases
/// - Constants, statics
/// - Two-phase processing: extract symbols → process impl blocks
///
/// Implementation of comprehensive Rust extractor
use crate::base::{
    BaseExtractor, Identifier, PendingRelationship, Relationship, StructuredPendingRelationship,
    Symbol, SymbolKind,
};
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};
use regex::Regex;
use std::sync::LazyLock;
use tree_sitter::{Node, Tree};

/// Matches type annotations: `: Type`
static VAR_TYPE_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r":\s*([^=\s{]+)").unwrap());

// Private modules
mod functions;
mod helpers;
mod identifiers;
mod item_macros;
mod locals;
mod relationships;
mod signatures;
mod type_facts;
mod types;

// Re-export types
pub use self::helpers::ImplBlockInfo;

/// Rust extractor that handles Rust-specific constructs
pub struct RustExtractor {
    pub(crate) base: BaseExtractor,
    impl_blocks: Vec<ImplBlockInfo>,
    is_processing_impl_blocks: bool,
    /// Phase 1 depth inside impl blocks: their items wait for phase 2.
    impl_nesting: u32,
    /// The implemented type while phase 2 walks an impl block.
    current_impl_type: Option<String>,
    /// Declared return types of the file's functions, for `let` inference.
    return_types: type_facts::ReturnTypeIndex,
    /// Item-macro bodies re-parsed as Rust items (`lazy_static!`, `cfg_if!`).
    macro_trees: Vec<Tree>,
    /// The re-parsed macro tree the walk is in, if any.
    current_macro_tree: Option<usize>,
}

impl RustExtractor {
    pub fn new(
        language: String,
        file_path: String,
        content: String,
        workspace_root: &std::path::Path,
    ) -> Self {
        Self {
            base: BaseExtractor::new(language, file_path, content, workspace_root),
            impl_blocks: Vec::new(),
            is_processing_impl_blocks: false,
            impl_nesting: 0,
            current_impl_type: None,
            return_types: type_facts::ReturnTypeIndex::default(),
            macro_trees: Vec::new(),
            current_macro_tree: None,
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
    pub fn add_structured_pending_relationship(&mut self, pending: StructuredPendingRelationship) {
        self.base.add_structured_pending_relationship(pending);
    }

    pub fn add_pending_relationship(&mut self, pending: PendingRelationship) {
        self.base.add_pending_relationship(pending);
    }

    /// Extract symbols using two-phase approach
    /// Phase 1: Extract all symbols except methods in impl blocks
    /// Phase 2: Process impl blocks and link methods to parent structs/traits
    pub fn extract_symbols(&mut self, tree: &Tree) -> Vec<Symbol> {
        let mut symbols = Vec::new();

        // Phase 1: Extract symbols (skip impl block methods)
        self.impl_blocks.clear();
        self.is_processing_impl_blocks = false;
        self.return_types = type_facts::ReturnTypeIndex::build(&self.base, tree.root_node());
        self.walk_tree(tree.root_node(), &mut symbols, None, 0);

        // Phase 2: Process impl blocks after all symbols are extracted
        // SAFETY FIX: Pass tree reference so we can reconstruct nodes from byte ranges
        self.is_processing_impl_blocks = true;
        self.process_impl_blocks(tree, &mut symbols);

        symbols
    }

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

        let enters_impl = node.kind() == "impl_item";
        let scope_id = match node.kind() {
            "impl_item" if self.is_processing_impl_blocks => return,
            "impl_item" => {
                functions::extract_impl(self, node, parent_id.clone());
                parent_id
            }
            _ if self.impl_nesting > 0 => parent_id,
            "use_declaration" | "extern_crate_declaration" => {
                self.extract_use_symbols(node, symbols, parent_id);
                return;
            }
            "macro_invocation" if self.extract_item_macro(node, symbols, &parent_id, depth) => {
                return;
            }
            _ => self
                .push_symbol(node, parent_id.clone(), symbols)
                .or(parent_id),
        };

        if enters_impl {
            self.impl_nesting += 1;
        }
        if let Some(child_depth) = child_tree_depth(depth) {
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                self.walk_tree(child, symbols, scope_id.clone(), child_depth);
            }
        }
        if enters_impl {
            self.impl_nesting -= 1;
        }
    }

    /// Phase 2 entry for one item of an impl block's declaration list.
    pub(super) fn walk_impl_item(
        &mut self,
        node: Node,
        symbols: &mut Vec<Symbol>,
        parent_id: Option<String>,
    ) {
        self.walk_tree(node, symbols, parent_id, 0);
    }

    // Kept out of line: the extracted `Symbol` is large, and `walk_tree` recurses
    // to the traversal depth budget, so the value must not live in its frame.
    #[inline(never)]
    fn push_symbol(
        &mut self,
        node: Node,
        parent_id: Option<String>,
        symbols: &mut Vec<Symbol>,
    ) -> Option<String> {
        let mut symbol = self.extract_symbol(node, parent_id)?;
        // The shared fallback reads `//!` inner docs as outer docs; rustdoc
        // attachment is decided by the Rust rules alone.
        symbol.doc_comment = helpers::find_doc_comment(&self.base, node);
        let symbol_id = symbol.id.clone();
        symbols.push(symbol);
        Some(symbol_id)
    }

    /// Item-position macros that define items: re-parse or token-scan their
    /// body. Returns false for any other macro, which defines no symbol.
    #[inline(never)]
    fn extract_item_macro(
        &mut self,
        node: Node,
        symbols: &mut Vec<Symbol>,
        parent_id: &Option<String>,
        depth: u32,
    ) -> bool {
        let parent_id = parent_id.clone();
        match item_macros::classify(&self.base, node) {
            Some(item_macros::ItemMacro::Reparse(ranges)) => {
                let Some(tree) = item_macros::reparse(&self.base.content, &ranges) else {
                    return false;
                };
                let index = self.macro_trees.len();
                self.macro_trees.push(tree.clone());
                let previous = self.current_macro_tree.replace(index);
                let root = tree.root_node();
                for item in root.children(&mut root.walk()) {
                    self.walk_tree(item, symbols, parent_id.clone(), depth);
                }
                self.current_macro_tree = previous;
                true
            }
            Some(item_macros::ItemMacro::Bitflags(body)) => {
                symbols.extend(item_macros::bitflags_symbols(
                    &mut self.base,
                    body,
                    parent_id,
                ));
                true
            }
            Some(item_macros::ItemMacro::Proptest(body)) => {
                symbols.extend(item_macros::proptest_symbols(
                    &mut self.base,
                    body,
                    parent_id,
                ));
                true
            }
            None => false,
        }
    }

    // Kept out of line: `walk_tree` recurses to the traversal depth budget, so its
    // stack frame must stay small.
    #[inline(never)]
    fn extract_use_symbols(
        &mut self,
        node: Node,
        symbols: &mut Vec<Symbol>,
        parent_id: Option<String>,
    ) {
        symbols.extend(signatures::extract_use_symbols(self, node, parent_id));
    }

    fn extract_symbol(&mut self, node: Node, parent_id: Option<String>) -> Option<Symbol> {
        match node.kind() {
            "struct_item" => types::extract_struct(self, node, parent_id),
            "enum_item" => types::extract_enum(self, node, parent_id),
            "trait_item" => types::extract_trait(self, node, parent_id),
            "function_item" => functions::extract_function(self, node, parent_id),
            "function_signature_item" => {
                signatures::extract_function_signature(self, node, parent_id)
            }
            "parameter" if locals::is_callable_parameter(node) => {
                locals::extract_parameter(self, node, parent_id)
            }
            "self_parameter" if locals::is_callable_parameter(node) => {
                let impl_type_name = self.current_impl_type.clone();
                locals::extract_self_parameter(self, node, parent_id, impl_type_name.as_deref())
            }
            "let_declaration" => locals::extract_let_local(self, node, parent_id),
            "associated_type" => signatures::extract_associated_type(self, node, parent_id),
            "field_declaration" => types::extract_field(self, node, parent_id),
            "enum_variant" => types::extract_enum_variant(self, node, parent_id),
            "union_item" => types::extract_union(self, node, parent_id),
            "mod_item" => types::extract_module(self, node, parent_id),
            "const_item" => types::extract_const(self, node, parent_id),
            "static_item" => types::extract_static(self, node, parent_id),
            "macro_definition" => types::extract_macro(self, node, parent_id),
            "type_item" => types::extract_type_alias(self, node, parent_id),
            _ => None,
        }
    }

    fn process_impl_blocks(&mut self, tree: &Tree, symbols: &mut Vec<Symbol>) {
        functions::process_impl_blocks(self, tree, symbols);
    }

    pub fn extract_relationships(&mut self, tree: &Tree, symbols: &[Symbol]) -> Vec<Relationship> {
        relationships::extract_relationships(self, tree, symbols)
    }

    pub fn extract_identifiers(&mut self, tree: &Tree, symbols: &[Symbol]) -> Vec<Identifier> {
        identifiers::extract_identifiers(self, tree, symbols)
    }

    /// Infer types from Rust signatures (function return types, variable types, field types)
    pub fn infer_types(&self, symbols: &[Symbol]) -> std::collections::HashMap<String, String> {
        let mut type_map = std::collections::HashMap::new();

        for symbol in symbols {
            // For variables, properties, fields - extract type annotation
            if matches!(
                symbol.kind,
                SymbolKind::Variable | SymbolKind::Property | SymbolKind::Field
            ) && let Some(ref signature) = symbol.signature
            {
                // Extract type from annotations: "name: Type" or "name: Type ="
                if let Some(captures) = VAR_TYPE_RE.captures(signature) {
                    let type_str = captures[1].trim().to_string();
                    if !type_str.is_empty() {
                        type_map.insert(symbol.id.clone(), type_str);
                    }
                }
            }
        }

        type_map
    }

    // Accessors for use by submodules and tests
    pub(crate) fn get_base_mut(&mut self) -> &mut BaseExtractor {
        &mut self.base
    }

    pub(super) fn get_impl_blocks(&self) -> &[ImplBlockInfo] {
        &self.impl_blocks
    }

    pub(super) fn add_impl_block(&mut self, block: ImplBlockInfo) {
        self.impl_blocks.push(block);
    }
}
