//! C language symbol extractor
//!
//! Provides symbol extraction, relationship tracking, and identifier discovery for C code
//! using tree-sitter parsing. This module is organized into focused submodules:
//!
//! - `helpers` - Node finding, name extraction, and tree navigation utilities
//! - `signatures` - Signature building methods for various C constructs
//! - `types` - Type and attribute extraction from the syntax tree
//! - `declarations` - Extraction of includes, macros, functions, and variables
//! - `structs` - Extraction of structs, unions, and enums
//! - `typedefs` - Typedef extraction and post-processing
//! - `relationships` - Relationship extraction (calls, imports)
//! - `identifiers` - Identifier usage tracking (calls, member access)

use crate::base::{
    BaseExtractor, BodySpan, Identifier, NormalizedSpan, PendingRelationship, Relationship,
    StructuredPendingRelationship, Symbol, SymbolKind,
};
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};
use std::collections::HashMap;
use tree_sitter::{Node, Tree};

// Internal modules
mod declarations;
pub(crate) use declarations::{extract_include, extract_macro};
pub(crate) use relationships::include_relationship;
mod helpers;
mod identifiers;
mod parameters;
mod relationships;
mod signatures;
mod structs;
pub(crate) mod test_calls;
mod type_facts;
mod typedefs;
mod types;

/// Main C extractor struct combining all extraction functionality
pub struct CExtractor {
    pub(crate) base: BaseExtractor,
    /// Criterion test bodies the grammar parses as the statement after the test
    /// macro, keyed by block node id, mapped to the test symbol that owns them.
    detached_test_bodies: HashMap<usize, String>,
}

impl CExtractor {
    /// Create a new C extractor for the given file
    pub fn new(
        language: String,
        file_path: String,
        content: String,
        workspace_root: &std::path::Path,
    ) -> Self {
        let mut base = BaseExtractor::new(language, file_path, content, workspace_root);
        base.body_span_rule = Some(body_span);
        Self {
            base,
            detached_test_bodies: HashMap::new(),
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

    /// Access the base extractor (used by submodules)
    pub(super) fn get_base_mut(&mut self) -> &mut BaseExtractor {
        &mut self.base
    }

    /// Extract all symbols from the syntax tree
    pub fn extract_symbols(&mut self, tree: &Tree) -> Vec<Symbol> {
        let mut symbols = Vec::new();
        self.visit_node(tree.root_node(), &mut symbols, None, 0);

        typedefs::fix_struct_alignment_attributes(&mut symbols);
        test_calls::apply_call_site_test_roles(&self.base, tree.root_node(), &mut symbols);

        symbols
    }

    /// Extract all relationships (calls, imports) from the syntax tree
    pub fn extract_relationships(&mut self, tree: &Tree, symbols: &[Symbol]) -> Vec<Relationship> {
        let mut relationships = Vec::new();
        relationships::extract_relationships_from_node(
            self,
            tree.root_node(),
            symbols,
            &mut relationships,
        );
        relationships
    }

    /// Extract all identifier usages (function calls, member access, etc.)
    pub fn extract_identifiers(&mut self, tree: &Tree, symbols: &[Symbol]) -> Vec<Identifier> {
        identifiers::extract_identifiers(self, tree, symbols)
    }

    /// Infer variable types from C signatures; return types are recorded from the
    /// declaration during symbol extraction.
    pub fn infer_types(&self, symbols: &[Symbol]) -> std::collections::HashMap<String, String> {
        let mut type_map = std::collections::HashMap::new();

        for symbol in symbols {
            if let Some(ref signature) = symbol.signature
                && let Some(inferred_type) =
                    self.extract_type_from_signature(signature, &symbol.kind, &symbol.name)
            {
                type_map.insert(symbol.id.clone(), inferred_type);
            }
        }

        type_map
    }

    fn extract_type_from_signature(
        &self,
        signature: &str,
        kind: &crate::base::SymbolKind,
        name: &str,
    ) -> Option<String> {
        use crate::base::SymbolKind;

        if !matches!(kind, SymbolKind::Variable | SymbolKind::Property) {
            return None;
        }
        let type_part = signature[..signature.find(name)?].trim();
        is_type_text(type_part).then(|| type_part.to_string())
    }

    /// Recursively visit nodes in the tree, extracting symbols
    fn visit_node(
        &mut self,
        node: tree_sitter::Node,
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

        let mut symbol: Option<Symbol> = None;
        let parent_id = self.detached_test_bodies.remove(&node.id()).or(parent_id);

        match node.kind() {
            "preproc_include" => {
                symbol = declarations::extract_include(&mut self.base, node, parent_id.as_deref());
            }
            "preproc_def" | "preproc_function_def" => {
                symbol = declarations::extract_macro(&mut self.base, node, parent_id.as_deref());
            }
            "declaration" => {
                let declaration_symbols =
                    declarations::extract_declaration(self, node, parent_id.as_deref());
                symbols.extend(declaration_symbols);
            }
            "function_definition" => {
                symbol =
                    declarations::extract_function_definition(self, node, parent_id.as_deref());
            }
            "struct_specifier" | "union_specifier" => {
                symbol = self.extract_specifier(node, parent_id.as_deref());
                if let Some(owner) = symbol.as_ref().map(|s| s.id.clone()) {
                    self.extract_members(node, &owner, symbols);
                }
            }
            "enum_specifier" => {
                symbol = structs::extract_enum(self, node, parent_id.as_deref());
                let owner = symbol.as_ref().map(|s| s.id.clone()).or(parent_id.clone());
                symbols.extend(structs::extract_enum_value_symbols(
                    self,
                    node,
                    owner.as_deref(),
                ));
            }
            "type_definition" => {
                self.visit_type_definition(node, symbols, parent_id, depth);
                return;
            }
            "linkage_specification" => {
                symbol =
                    declarations::extract_linkage_specification(self, node, parent_id.as_deref());
            }
            "expression_statement" => {
                symbol =
                    typedefs::extract_from_expression_statement(self, node, parent_id.as_deref());
            }
            "call_expression" => {
                symbol =
                    test_calls::extract_c_test_call(&mut self.base, &node, parent_id.as_deref());
                if let (Some(test), Some(block)) =
                    (&symbol, crate::test_calls::detached_macro_block(&node))
                {
                    self.detached_test_bodies
                        .insert(block.id(), test.id.clone());
                }
            }
            _ => {}
        }

        let current_parent_id = if let Some(sym) = symbol {
            let symbol_id = sym.id.clone();
            let attach_parameters = node.kind() == "function_definition";
            symbols.push(sym);
            if attach_parameters {
                symbols.extend(parameters::extract_parameter_symbols(
                    self, node, &symbol_id,
                ));
            }
            Some(symbol_id)
        } else {
            parent_id
        };

        // Recursively visit children
        let Some(child_depth) = child_tree_depth(depth) else {
            return;
        };
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.visit_node(child, symbols, current_parent_id.clone(), child_depth);
        }
    }
}

impl CExtractor {
    fn extract_specifier(&mut self, node: Node, parent_id: Option<&str>) -> Option<Symbol> {
        match node.kind() {
            "struct_specifier" => structs::extract_struct(self, node, parent_id),
            "union_specifier" => structs::extract_union(self, node, parent_id),
            "enum_specifier" => structs::extract_enum(self, node, parent_id),
            _ => None,
        }
    }

    fn extract_members(&mut self, specifier: Node, owner: &str, symbols: &mut Vec<Symbol>) {
        let members = if specifier.kind() == "enum_specifier" {
            structs::extract_enum_value_symbols(self, specifier, Some(owner))
        } else {
            structs::extract_struct_field_symbols(self, specifier, owner)
        };
        symbols.extend(members);
    }

    /// A typedef whose type carries a body owns that body's members. A body with
    /// its own tag name different from every typedef name also keeps a row for
    /// the tag, so `struct tag` references resolve.
    fn visit_type_definition(
        &mut self,
        node: Node,
        symbols: &mut Vec<Symbol>,
        parent_id: Option<String>,
        depth: u32,
    ) {
        let typedef_symbols = typedefs::extract_type_definition(self, node, parent_id.as_deref());
        let Some(specifier) = node.child_by_field_name("type").filter(|t| {
            t.child_by_field_name("body").is_some()
                && matches!(
                    t.kind(),
                    "struct_specifier" | "union_specifier" | "enum_specifier"
                )
        }) else {
            symbols.extend(typedef_symbols);
            return;
        };

        let mut owner = typedef_symbols
            .iter()
            .find(|s| {
                matches!(
                    s.kind,
                    SymbolKind::Struct | SymbolKind::Union | SymbolKind::Enum
                )
            })
            .or(typedef_symbols.first())
            .map(|s| s.id.clone());
        let tag_is_new = specifier
            .child_by_field_name("name")
            .map(|name| self.base.get_node_text(&name))
            .is_some_and(|tag| typedef_symbols.iter().all(|s| s.name != tag));
        symbols.extend(typedef_symbols);
        if tag_is_new
            && let Some(tag_symbol) = self.extract_specifier(specifier, parent_id.as_deref())
        {
            owner.get_or_insert_with(|| tag_symbol.id.clone());
            symbols.push(tag_symbol);
        }

        let Some(owner) = owner else {
            return;
        };
        self.extract_members(specifier, &owner, symbols);
        let (Some(body), Some(child_depth)) = (
            specifier.child_by_field_name("body"),
            child_tree_depth(depth),
        ) else {
            return;
        };
        let mut cursor = body.walk();
        for child in body.children(&mut cursor) {
            self.visit_node(child, symbols, Some(owner.clone()), child_depth);
        }
    }
}

/// The grammar body of a C or C++ definition: the `body` field of a function,
/// record, enum, or namespace, or the record body of `typedef struct { ... } Name`.
/// Prototypes, fields, variables, and macros have no body.
pub(crate) fn body_span(node: &Node, _content: &str) -> Option<BodySpan> {
    let body = node.child_by_field_name("body").or_else(|| {
        (node.kind() == "type_definition")
            .then(|| node.child_by_field_name("type"))
            .flatten()
            .and_then(|specifier| specifier.child_by_field_name("body"))
    })?;
    Some(NormalizedSpan::from_node(&body))
}

fn is_type_text(text: &str) -> bool {
    !text.is_empty()
        && text
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | ' ' | '*'))
}
