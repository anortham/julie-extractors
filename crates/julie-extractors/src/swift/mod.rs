// Swift language extractor - comprehensive symbol and relationship extraction
// Organized into focused modules for maintainability and clarity

pub(super) mod callables;
pub(super) mod enum_cases;
pub(super) mod extensions;
pub(super) mod external_symbols;
pub(super) mod identifiers;
pub(super) mod parameters;
pub(super) mod properties;
pub(super) mod protocol;
pub(super) mod relationships;
pub(super) mod signatures;
pub(crate) mod test_calls;
pub(super) mod test_roles;
pub(super) mod type_facts;
pub(super) mod types;

use crate::base::{
    BaseExtractor, NormalizedSpan, PendingRelationship, StructuredPendingRelationship, Symbol,
    SymbolKind, Visibility,
};
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};
use std::collections::HashSet;
use tree_sitter::{Node, Tree};

/// Swift extractor for extracting symbols and relationships from Swift source code
/// Implementation of comprehensive Swift extractor with full Swift language support
pub struct SwiftExtractor {
    pub(crate) base: BaseExtractor,
    pub(crate) same_file_type_names: HashSet<String>,
    /// Declared return types of the file's functions, for binding inference.
    return_types: type_facts::ReturnTypeIndex,
}

impl SwiftExtractor {
    pub fn new(
        language: String,
        file_path: String,
        content: String,
        workspace_root: &std::path::Path,
    ) -> Self {
        Self {
            base: BaseExtractor::new(language, file_path, content, workspace_root),
            same_file_type_names: HashSet::new(),
            return_types: type_facts::ReturnTypeIndex::default(),
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

    /// Extract all symbols from Swift source code
    /// Implementation of extractSymbols method with comprehensive Swift support
    pub fn extract_symbols(&mut self, tree: &Tree) -> Vec<Symbol> {
        self.same_file_type_names = type_facts::collect_type_names(&self.base, tree.root_node());
        self.return_types = type_facts::ReturnTypeIndex::build(&self.base, tree.root_node());
        let mut symbols = Vec::new();
        self.visit_node(tree.root_node(), &mut symbols, None, 0);
        test_roles::apply_swift_test_roles(&mut symbols);
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

        let mut extracted = self.extract_node_symbols(node, parent_id.as_deref());

        let parent = parent_id
            .as_deref()
            .and_then(|id| symbols.iter().rev().find(|symbol| symbol.id == id));
        let explicit = signatures::explicit_access_level(&self.extract_modifiers(node));
        for symbol in &mut extracted {
            symbol.visibility = access_level(symbol, explicit.clone(), parent);
        }

        let mut current_parent_id = parent_id.clone();
        if node.kind() != "enum_entry"
            && let Some(first) = extracted.first()
        {
            current_parent_id = Some(first.id.clone());
            if matches!(node.kind(), "function_declaration" | "init_declaration") {
                let parameters = parameters::extract_parameter_symbols(self, node, &first.id);
                extracted.extend(parameters);
            }
        }
        symbols.extend(extracted);

        // Recursively visit children
        let Some(child_depth) = child_tree_depth(depth) else {
            return;
        };
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.visit_node(child, symbols, current_parent_id.clone(), child_depth);
        }
    }

    /// The symbols one node declares. Kept out of line so the recursive
    /// walker's frame never holds a `Symbol`.
    #[inline(never)]
    fn extract_node_symbols(&mut self, node: Node, parent_id: Option<&str>) -> Vec<Symbol> {
        let mut extracted: Vec<Symbol> = Vec::new();
        let mut symbol: Option<Symbol> = None;

        match node.kind() {
            "class_declaration" => {
                symbol = self.extract_class(node, parent_id);
            }
            "struct_declaration" => {
                symbol = self.extract_struct(node, parent_id);
            }
            "protocol_declaration" => {
                symbol = self.extract_protocol(node, parent_id);
            }
            "enum_declaration" => {
                symbol = self.extract_enum(node, parent_id);
            }
            "enum_case_declaration" => {
                self.extract_enum_cases(node, &mut extracted, parent_id);
            }
            "enum_entry" => {
                extracted = self.extract_enum_case(node, parent_id);
            }
            "function_declaration" => {
                symbol = self.extract_function(node, parent_id);
            }
            "macro_declaration" => {
                symbol = self.extract_macro(node, parent_id);
            }
            "operator_declaration" => {
                symbol = self.extract_operator_declaration(node, parent_id);
            }
            "protocol_function_declaration" => {
                symbol = self.extract_protocol_function(node, parent_id);
            }
            "protocol_property_declaration" => {
                symbol = self.extract_protocol_property(node, parent_id);
            }
            "associatedtype_declaration" => {
                symbol = self.extract_associated_type(node, parent_id);
            }
            "subscript_declaration" => {
                symbol = Some(self.extract_subscript(node, parent_id));
            }
            "init_declaration" => {
                symbol = Some(self.extract_initializer(node, parent_id));
            }
            "deinit_declaration" => {
                symbol = Some(self.extract_deinitializer(node, parent_id));
            }
            "property_declaration" => {
                extracted = self.extract_property(node, parent_id);
            }
            "extension_declaration" => {
                symbol = self.extract_extension(node, parent_id);
            }
            "import_declaration" => {
                symbol = self.extract_import(node, parent_id);
            }
            "typealias_declaration" => {
                symbol = self.extract_type_alias(node, parent_id);
            }
            "call_expression" => {
                symbol = test_calls::extract_quick_test_call(&mut self.base, node, parent_id);
            }
            _ => {}
        }
        extracted.extend(symbol);
        extracted
    }
}

/// Swift access control: an explicit modifier wins. Otherwise protocol
/// requirements and enum cases share their parent's level, extension members
/// take the extension's level (`private` there means file-private), members of
/// a private type are file-private, and everything else is `internal`. Locals
/// and declarations inside a function body have no access level.
fn access_level(
    symbol: &Symbol,
    explicit: Option<Visibility>,
    parent: Option<&Symbol>,
) -> Option<Visibility> {
    symbol.visibility.as_ref()?;
    if symbol.kind == SymbolKind::Import {
        return symbol.visibility.clone();
    }
    let parent_kind = parent.map(|parent| &parent.kind);
    if symbol.kind == SymbolKind::Variable
        || matches!(
            parent_kind,
            Some(
                SymbolKind::Function
                    | SymbolKind::Method
                    | SymbolKind::Constructor
                    | SymbolKind::Destructor
            )
        )
    {
        return None;
    }
    if explicit.is_some() {
        return explicit;
    }
    let Some(parent) = parent else {
        return Some(Visibility::Internal);
    };
    let parent_level = parent.visibility.clone().unwrap_or(Visibility::Internal);
    Some(match parent.kind {
        SymbolKind::Interface => parent_level,
        SymbolKind::Enum if symbol.kind == SymbolKind::EnumMember => parent_level,
        SymbolKind::Module => match parent_level {
            Visibility::Private | Visibility::FilePrivate => Visibility::FilePrivate,
            other => other,
        },
        _ => match parent_level {
            Visibility::Private | Visibility::FilePrivate => Visibility::FilePrivate,
            _ => Visibility::Internal,
        },
    })
}

/// Replace a symbol's inferred body with `body`, or clear it.
pub(super) fn set_body(base: &BaseExtractor, symbol: &mut Symbol, body: Option<Node>) {
    symbol.body_span = body.map(|body| NormalizedSpan::from_node(&body));
    symbol.body_hash = symbol
        .body_span
        .and_then(|span| crate::base::body::body_hash(&base.content, span, "swift"));
}

/// Declarations without code (enum cases, protocol requirements) have no body.
pub(super) fn clear_body(symbol: &mut Symbol) {
    symbol.body_span = None;
    symbol.body_hash = None;
}
