//! Core symbol extraction logic
//!
//! This module handles the main tree traversal and symbol type routing.
//! It delegates to specialized modules for specific symbol kinds.

use super::{classes, functions, imports_exports, interfaces};
use crate::base::{Symbol, SymbolKind};
use crate::javascript::test_symbols;
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};
use crate::typescript::TypeScriptExtractor;
use tree_sitter::{Node, Tree};

/// Extract all symbols from the syntax tree
pub(super) fn extract_symbols(extractor: &mut TypeScriptExtractor, tree: &Tree) -> Vec<Symbol> {
    let mut symbols = Vec::new();
    extractor.test_dsl_active =
        test_symbols::test_dsl_is_active(extractor.base(), tree.root_node());
    visit_node(extractor, tree.root_node(), &mut symbols, None, 0);
    symbols
}

/// Check if a node is a direct child of an interface_body
fn is_inside_interface(node: &Node) -> bool {
    node.parent()
        .map(|p| p.kind() == "interface_body")
        .unwrap_or(false)
}

/// Recursively visit nodes and extract symbols based on node kind
fn visit_node(
    extractor: &mut TypeScriptExtractor,
    node: Node,
    symbols: &mut Vec<Symbol>,
    parent_id: Option<String>,
    depth: u32,
) {
    if !should_visit_tree_depth(depth) {
        return;
    }

    let mut symbol: Option<Symbol> = None;
    let mut next_parent_id = parent_id.clone();

    // Route node types to appropriate extraction modules
    match node.kind() {
        // Class extraction
        "class_declaration" => {
            symbol = classes::extract_class(extractor, node, parent_id.as_deref());
        }

        // Function extraction
        "function_declaration" | "function" => {
            symbol = functions::extract_function(extractor, node, parent_id.as_deref());
        }

        // Method extraction (inside classes, not interfaces — interface methods
        // are extracted by extract_interface to get correct parent_id)
        "method_definition" => {
            symbol = functions::extract_method(extractor, node, parent_id.as_deref());
        }
        "method_signature" if !is_inside_interface(&node) => {
            symbol = functions::extract_method(extractor, node, parent_id.as_deref());
        }

        // Variable/arrow function assignment
        "variable_declarator" => {
            symbol = functions::extract_variable(extractor, node, parent_id.as_deref());
        }

        // Interface extraction (with members)
        "interface_declaration" => {
            let interface_symbols =
                interfaces::extract_interface(extractor, node, parent_id.as_deref());
            next_parent_id = interface_symbols
                .first()
                .map(|symbol| symbol.id.clone())
                .or_else(|| parent_id.clone());
            symbols.extend(interface_symbols);
        }

        // Type aliases
        "type_alias_declaration" => {
            symbol = interfaces::extract_type_alias(extractor, node, parent_id.as_deref());
        }

        // Enums
        "enum_declaration" => {
            let enum_symbols = interfaces::extract_enum(extractor, node, parent_id.as_deref());
            next_parent_id = enum_symbols
                .first()
                .map(|symbol| symbol.id.clone())
                .or_else(|| parent_id.clone());
            symbols.extend(enum_symbols);
        }

        // Import/export statements
        "import_statement" | "import_declaration" => {
            let import_symbols = imports_exports::extract_import(extractor, node);
            symbols.extend(import_symbols);
        }
        "export_statement" => {
            let export_symbols = imports_exports::extract_export(extractor, node);
            symbols.extend(export_symbols);
        }

        // Namespaces/modules
        "namespace_declaration" | "module_declaration" | "internal_module" => {
            symbol = interfaces::extract_namespace(extractor, node, parent_id.as_deref());
        }

        // Properties and fields (skip interface members — already handled by extract_interface)
        "property_signature" if !is_inside_interface(&node) => {
            symbol = interfaces::extract_property(extractor, node, parent_id.as_deref());
        }
        "public_field_definition" | "property_definition" => {
            symbol = interfaces::extract_property(extractor, node, parent_id.as_deref());
        }

        // Test call expression extraction (describe/it/test/beforeEach/etc.)
        "call_expression"
            if extractor.test_dsl_active
                && test_symbols::is_test_dsl_call(extractor.base(), node) =>
        {
            // Find parent describe for nesting
            let parent = symbols
                .iter()
                .rev()
                .find(|s| {
                    s.metadata
                        .as_ref()
                        .and_then(|m| m.get("test_container"))
                        .and_then(|v| v.as_bool())
                        == Some(true)
                        && s.start_byte <= node.start_byte() as u32
                        && s.end_byte >= node.end_byte() as u32
                })
                .map(|s| s.id.as_str());
            // Need to extract parent_id before mutable borrow of extractor
            let parent_id_owned = parent.map(|s| s.to_string());
            symbol = test_symbols::extract_test_call(
                extractor.base_mut(),
                node,
                parent_id_owned.as_deref(),
            );
        }

        _ => {}
    }

    if let Some(sym) = symbol {
        if is_parent_scope_kind(&sym.kind) {
            next_parent_id = Some(sym.id.clone());
        }
        symbols.push(sym);
    }

    // Recursively visit children
    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        visit_node(
            extractor,
            child,
            symbols,
            next_parent_id.clone(),
            child_depth,
        );
    }
}

fn is_parent_scope_kind(kind: &SymbolKind) -> bool {
    matches!(
        kind,
        SymbolKind::Class
            | SymbolKind::Interface
            | SymbolKind::Namespace
            | SymbolKind::Module
            | SymbolKind::Enum
            | SymbolKind::Function
            | SymbolKind::Method
            | SymbolKind::Constructor
    )
}
