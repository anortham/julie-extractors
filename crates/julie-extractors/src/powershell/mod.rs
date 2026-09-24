//! PowerShell language extractor - Implementation of PowerShell extractor
//! Handles PowerShell-specific constructs for Windows/Azure DevOps
//!
//! Provides symbol extraction for:
//! - Functions (simple and advanced with `[CmdletBinding()]`)
//! - Variables declared by plain assignments (any scope, including `$env:`)
//! - Classes, methods, properties, and enums (PowerShell 5.0+)
//! - Azure PowerShell cmdlets and Windows management commands
//! - Module imports, exports, and using statements
//! - Parameter definitions with attributes and validation
//! - Cross-platform DevOps tool calls (docker, kubectl, az CLI)
//!
//! Special focus on Windows/Azure DevOps tracing to complement Bash for complete
//! cross-platform deployment automation coverage.

pub mod classes;
pub mod commands;
pub mod documentation;
pub mod functions;
pub mod helpers;
pub mod identifiers;
pub mod imports;
pub mod manifest;
pub mod relationships;
pub mod structural;
pub mod test_calls;
pub mod type_facts;
pub mod types;
pub mod variables;

use crate::base::{
    BaseExtractor, Identifier, PendingRelationship, Relationship, StructuredPendingRelationship,
    Symbol,
};
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};
use std::collections::HashSet;
use tree_sitter::Tree;

fn is_parameter(symbol: &Symbol) -> bool {
    symbol
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get("role"))
        .and_then(|role| role.as_str())
        == Some("parameter")
}

/// PowerShell language extractor that handles PowerShell-specific constructs for Windows/Azure DevOps
pub struct PowerShellExtractor {
    pub base: BaseExtractor,
    /// Variables already declared, keyed by syntax parent and
    /// [`helpers::variable_key`], so a reassignment adds no second symbol.
    declared_variables: HashSet<(Option<String>, String)>,
    /// The file's classes and declared return types, for assignment inference.
    return_types: type_facts::ReturnTypeIndex,
}

impl PowerShellExtractor {
    pub fn new(
        language: String,
        file_path: String,
        content: String,
        workspace_root: &std::path::Path,
    ) -> Self {
        Self {
            base: BaseExtractor::new(language, file_path, content, workspace_root),
            declared_variables: HashSet::new(),
            return_types: type_facts::ReturnTypeIndex::default(),
        }
    }

    /// Extract all symbols from the PowerShell AST
    pub fn extract_symbols(&mut self, tree: &Tree) -> Vec<Symbol> {
        let mut symbols = Vec::new();
        self.return_types = type_facts::ReturnTypeIndex::build(&self.base, tree.root_node());
        self.walk_tree_for_symbols(tree.root_node(), &mut symbols, None, 0);
        if manifest::is_data_file_path(&self.base.file_path) {
            symbols.extend(manifest::extract_manifest_symbols(
                &mut self.base,
                tree.root_node(),
            ));
        }
        imports::narrow_to_exported_functions(&self.base.file_path, &mut symbols);
        symbols
    }

    /// Walk the tree and extract symbols recursively
    fn walk_tree_for_symbols(
        &mut self,
        node: tree_sitter::Node,
        symbols: &mut Vec<Symbol>,
        parent_id: Option<String>,
        depth: u32,
    ) {
        if !should_visit_tree_depth(depth) {
            return;
        }

        let current_parent_id = self.record_node_symbols(node, symbols, parent_id);

        let Some(child_depth) = child_tree_depth(depth) else {
            return;
        };
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.walk_tree_for_symbols(child, symbols, current_parent_id.clone(), child_depth);
        }
    }

    /// Push the symbols `node` declares and return the parent for its
    /// children. Kept out of the recursive walker so the walker's frame holds
    /// no `Symbol` while it recurses to the depth budget.
    #[inline(never)]
    fn record_node_symbols(
        &mut self,
        node: tree_sitter::Node,
        symbols: &mut Vec<Symbol>,
        parent_id: Option<String>,
    ) -> Option<String> {
        let mut current_parent_id = parent_id;

        let variable_key = (node.kind() == "assignment_expression")
            .then(|| variables::assignment_target_key(&self.base, node))
            .flatten()
            .map(|key| (current_parent_id.clone(), key));
        let reassigns_declared_variable = variable_key
            .as_ref()
            .is_some_and(|key| self.declared_variables.contains(key));

        if !reassigns_declared_variable {
            for symbol in self.extract_symbols_from_node(node, current_parent_id.as_deref()) {
                if matches!(
                    symbol.kind,
                    crate::base::SymbolKind::Function
                        | crate::base::SymbolKind::Method
                        | crate::base::SymbolKind::Constructor
                ) {
                    let parameters = functions::extract_function_parameters(
                        &mut self.base,
                        node,
                        Some(&symbol.id),
                    );
                    self.declare_parameters(&parameters);
                    symbols.extend(parameters);
                }
                if is_parameter(&symbol) {
                    self.declare_parameters(std::slice::from_ref(&symbol));
                } else {
                    current_parent_id = Some(symbol.id.clone());
                }
                if let Some(key) = variable_key.clone() {
                    self.declared_variables.insert(key);
                }
                symbols.push(symbol);
            }
        }
        current_parent_id
    }

    fn declare_parameters(&mut self, parameters: &[Symbol]) {
        for parameter in parameters {
            self.declared_variables.insert((
                parameter.parent_id.clone(),
                helpers::variable_key(&parameter.name),
            ));
        }
    }

    /// Extract the symbols a single node declares, based on its kind.
    fn extract_symbols_from_node(
        &mut self,
        node: tree_sitter::Node,
        parent_id: Option<&str>,
    ) -> Vec<Symbol> {
        match node.kind() {
            "param_block" => {
                match functions::extract_advanced_function(&mut self.base, node, parent_id) {
                    Some(function) => vec![function],
                    None if node
                        .parent()
                        .is_some_and(|parent| parent.kind() == "program") =>
                    {
                        functions::extract_function_parameters(&mut self.base, node, parent_id)
                    }
                    None => Vec::new(),
                }
            }
            "comment" => imports::extract_requires_modules(&mut self.base, node, parent_id),
            "command" => {
                let command_name = node
                    .child_by_field_name("command_name")
                    .filter(|name| name.kind() == "command_name")
                    .map(|name| self.base.get_node_text(&name));
                match command_name {
                    Some(name) if imports::is_module_command(&name) => {
                        imports::extract_import_command(&mut self.base, node, &name, parent_id)
                    }
                    _ => self
                        .extract_symbol_from_node(node, parent_id)
                        .into_iter()
                        .collect(),
                }
            }
            _ => self
                .extract_symbol_from_node(node, parent_id)
                .into_iter()
                .collect(),
        }
    }

    /// Extract a symbol from a single node based on its kind
    fn extract_symbol_from_node(
        &mut self,
        node: tree_sitter::Node,
        parent_id: Option<&str>,
    ) -> Option<Symbol> {
        match node.kind() {
            "function_statement" => functions::extract_function(&mut self.base, node, parent_id),
            "ERROR" => self.extract_error_node(node, parent_id),
            "assignment_expression" => {
                variables::extract_variable(&mut self.base, node, parent_id, &self.return_types)
            }
            "class_statement" => classes::extract_class(&mut self.base, node, parent_id),
            "class_method_definition" => classes::extract_method(&mut self.base, node, parent_id),
            "class_property_definition" => {
                classes::extract_property(&mut self.base, node, parent_id)
            }
            "enum_statement" => classes::extract_enum(&mut self.base, node, parent_id),
            "enum_member" => classes::extract_enum_member(&mut self.base, node, parent_id),
            "command" => {
                if helpers::is_dot_sourcing(&self.base, node) {
                    imports::extract_dot_sourcing(&mut self.base, node, parent_id)
                } else if helpers::find_command_name_node(node)
                    .is_some_and(|cn| self.base.get_node_text(&cn) == "Configuration")
                {
                    commands::extract_dsc_configuration(&mut self.base, node, parent_id)
                } else if let Some(sym) =
                    test_calls::extract_pester_test_call(&mut self.base, node, parent_id)
                {
                    Some(sym)
                } else {
                    commands::extract_build_task(&mut self.base, node, parent_id)
                }
            }
            _ => None,
        }
    }

    /// Extract configuration/function from ERROR nodes
    fn extract_error_node(
        &mut self,
        node: tree_sitter::Node,
        parent_id: Option<&str>,
    ) -> Option<Symbol> {
        let node_text = self.base.get_node_text(&node);

        // Check if this ERROR node contains a DSC configuration
        if node_text.contains("Configuration ")
            && let Some((name, signature)) =
                commands::extract_configuration_from_error(&self.base, &node_text)
        {
            return Some(self.base.create_symbol(
                &node,
                name,
                crate::base::SymbolKind::Function,
                crate::base::SymbolOptions {
                    signature: Some(signature),
                    visibility: Some(crate::base::Visibility::Public),
                    parent_id: parent_id.map(|s| s.to_string()),
                    metadata: None,
                    doc_comment: None,
                    annotations: Vec::new(),
                },
            ));
        }

        // Also check for function definitions that might be in ERROR nodes
        if node_text.contains("function ")
            && let Some((name, signature)) =
                commands::extract_function_from_error(&self.base, &node_text)
        {
            return Some(self.base.create_symbol(
                &node,
                name,
                crate::base::SymbolKind::Function,
                crate::base::SymbolOptions {
                    signature: Some(signature),
                    visibility: Some(crate::base::Visibility::Public),
                    parent_id: parent_id.map(|s| s.to_string()),
                    metadata: None,
                    doc_comment: None,
                    annotations: Vec::new(),
                },
            ));
        }

        None
    }

    /// Extract relationships between symbols
    pub fn extract_relationships(&mut self, tree: &Tree, symbols: &[Symbol]) -> Vec<Relationship> {
        let mut relationships = Vec::new();
        relationships::walk_tree_for_relationships(
            self,
            tree.root_node(),
            symbols,
            &mut relationships,
            0,
        );
        relationships
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

    pub(crate) fn add_structured_pending_relationship(
        &mut self,
        pending: StructuredPendingRelationship,
    ) {
        self.base.add_structured_pending_relationship(pending);
    }

    /// Infer types for symbols
    pub fn infer_types(&self, symbols: &[Symbol]) -> std::collections::HashMap<String, String> {
        types::infer_types(symbols, &self.base.type_info)
    }

    /// Extract identifiers (function calls, member access, etc.)
    pub fn extract_identifiers(&mut self, tree: &Tree, symbols: &[Symbol]) -> Vec<Identifier> {
        identifiers::extract_identifiers(&mut self.base, tree, symbols)
    }
}
