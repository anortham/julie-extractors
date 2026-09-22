//! GDScript Extractor for Julie
//!
//! Extracts symbols from GDScript files (Godot's scripting language).
//! Supports:
//! - Classes (both explicit class_name and inner class definitions)
//! - Functions and methods
//! - Variables with @export and @onready annotations
//! - Constants
//! - Enums and enum members
//! - Signals
//! - Constructors (_init)

mod classes;
mod enums;
mod functions;
mod helpers;
mod identifiers;
mod parameters;
mod relationships;
mod signals;
mod test_roles;
mod type_facts;
mod types;
mod variables;

use crate::base::{
    BaseExtractor, Identifier, PendingRelationship, Relationship, StructuredPendingRelationship,
    Symbol, SymbolKind,
};
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};
use regex::Regex;
use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;
use tree_sitter::{Node, Tree};

// Static regexes compiled once for performance
static FUNC_RETURN_TYPE_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"->\s*(\w+)").unwrap());
static VAR_CONST_TYPE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?:var|const)\s+\w+\s*:\s*(\w+)").unwrap());

/// The symbol that owns the declarations being visited, and whether that
/// owner is a class (so a `func` there is a method).
struct Scope {
    parent_id: Option<String>,
    in_class: bool,
}

pub struct GDScriptExtractor {
    pub(crate) base: BaseExtractor,
    same_file_class_names: HashSet<String>,
}

impl GDScriptExtractor {
    pub fn new(
        language: String,
        file_path: String,
        content: String,
        workspace_root: &std::path::Path,
    ) -> Self {
        Self {
            base: BaseExtractor::new(language, file_path, content, workspace_root),
            same_file_class_names: HashSet::new(),
        }
    }

    pub fn extract_symbols(&mut self, tree: &Tree) -> Vec<Symbol> {
        let mut symbols = Vec::new();
        let root_node = tree.root_node();
        self.same_file_class_names = type_facts::collect_class_names(&self.base, root_node);

        let script_classes = classes::extract_script_classes(&mut self.base, root_node);
        let class_starts: Vec<(u32, String)> = script_classes
            .iter()
            .map(|class| (class.start_byte, class.id.clone()))
            .collect();
        symbols.extend(script_classes);

        let mut cursor = root_node.walk();
        let children: Vec<Node> = root_node.children(&mut cursor).collect();
        for child in children {
            let owner = class_starts
                .iter()
                .rev()
                .find(|(start, _)| *start <= child.start_byte() as u32)
                .or(class_starts.first())
                .map(|(_, id)| id.clone());
            let scope = Scope {
                in_class: owner.is_some(),
                parent_id: owner,
            };
            self.traverse_node(child, &scope, &mut symbols, 1);
        }
        test_roles::apply_gdscript_test_roles(&mut symbols);

        symbols
    }

    pub fn extract_relationships(&mut self, tree: &Tree, symbols: &[Symbol]) -> Vec<Relationship> {
        relationships::extract_relationships(self, tree, symbols)
    }

    /// Extract all identifier usages (function calls, member access, etc.)
    pub fn extract_identifiers(&mut self, tree: &Tree, symbols: &[Symbol]) -> Vec<Identifier> {
        identifiers::extract_identifiers(&mut self.base, tree, symbols)
    }

    /// Read-only access to the underlying `BaseExtractor` (captured literals, etc.).
    #[cfg(test)]
    pub(crate) fn base(&self) -> &BaseExtractor {
        &self.base
    }

    /// Infer types from GDScript type annotations in signatures.
    ///
    /// GDScript supports explicit types: `func foo() -> String:`, `var x: int = 0`
    pub fn infer_types(&self, symbols: &[Symbol]) -> HashMap<String, String> {
        let mut type_map = HashMap::new();

        for symbol in symbols {
            if let Some(ref signature) = symbol.signature
                && let Some(inferred) = Self::infer_type_from_signature(signature, &symbol.kind)
            {
                type_map.insert(symbol.id.clone(), inferred);
            }
        }

        type_map
    }

    fn infer_type_from_signature(signature: &str, kind: &SymbolKind) -> Option<String> {
        // GDScript signatures may be multiline (include body). Use the first
        // non-annotation line for functions, last annotation-bearing line for vars.
        match kind {
            SymbolKind::Function | SymbolKind::Method | SymbolKind::Constructor => {
                // Signature: `func name(...) -> ReturnType:\n    body`
                // Return type is on the `func` line
                let func_line = signature
                    .lines()
                    .find(|l| l.trim_start().contains("func "))?;
                FUNC_RETURN_TYPE_RE
                    .captures(func_line)
                    .map(|c| c[1].to_string())
            }
            SymbolKind::Variable
            | SymbolKind::Property
            | SymbolKind::Field
            | SymbolKind::Constant => {
                // Signature: `@export var name: Type = value` or `const NAME: Type = value`
                VAR_CONST_TYPE_RE
                    .captures(signature)
                    .map(|c| c[1].to_string())
            }
            _ => None,
        }
    }

    fn traverse_children(
        &mut self,
        node: Node,
        scope: &Scope,
        symbols: &mut Vec<Symbol>,
        depth: u32,
    ) {
        let Some(child_depth) = child_tree_depth(depth) else {
            return;
        };
        let mut cursor = node.walk();
        let children: Vec<Node> = node.children(&mut cursor).collect();
        for child in children {
            self.traverse_node(child, scope, symbols, child_depth);
        }
    }

    fn traverse_node(&mut self, node: Node, scope: &Scope, symbols: &mut Vec<Symbol>, depth: u32) {
        if !should_visit_tree_depth(depth) {
            return;
        }
        let parent_id = scope.parent_id.as_ref();

        let symbol = match node.kind() {
            "class_definition" => classes::extract_inner_class(&mut self.base, node, parent_id),
            "function_definition" => functions::extract_function_definition(
                &mut self.base,
                node,
                parent_id,
                scope.in_class,
            ),
            "lambda" if node.child_by_field_name("name").is_some() => {
                functions::extract_function_definition(&mut self.base, node, parent_id, false)
            }
            "constructor_definition" => {
                functions::extract_constructor_definition(&mut self.base, node, parent_id)
            }
            "variable_statement"
            | "export_variable_statement"
            | "onready_variable_statement"
            | "const_statement" => variables::extract_variable(
                &mut self.base,
                node,
                parent_id,
                &self.same_file_class_names,
            ),
            "enum_definition" => {
                symbols.extend(enums::extract_enum(&mut self.base, node, parent_id));
                return;
            }
            "signal_statement" => {
                signals::extract_signal_statement(&mut self.base, node, parent_id)
            }
            "ERROR" => functions::try_recover_function_from_error(
                &mut self.base,
                node,
                parent_id,
                scope.in_class,
            ),
            _ => None,
        };

        let Some(symbol) = symbol else {
            self.traverse_children(node, scope, symbols, depth);
            return;
        };
        let symbol_id = symbol.id.clone();
        let is_class = symbol.kind == SymbolKind::Class;
        symbols.push(symbol);
        if matches!(
            node.kind(),
            "function_definition" | "constructor_definition" | "lambda"
        ) {
            symbols.extend(parameters::extract_parameter_symbols(
                &mut self.base,
                node,
                &symbol_id,
            ));
        }
        let child_scope = Scope {
            parent_id: Some(symbol_id),
            in_class: is_class,
        };
        self.traverse_children(node, &child_scope, symbols, depth);
    }

    // ========================================================================
    // Pending Relationships Management
    // ========================================================================

    pub(crate) fn add_structured_pending_relationship(
        &mut self,
        pending: StructuredPendingRelationship,
    ) {
        self.base.add_structured_pending_relationship(pending);
    }

    /// Get all pending relationships collected during extraction
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
}
