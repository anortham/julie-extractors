//! Visibility extraction for JavaScript
//!
//! Handles extraction of symbol visibility based on naming conventions,
//! since JavaScript doesn't have explicit visibility modifiers like TypeScript.

use crate::base::{BaseExtractor, Symbol, SymbolKind, Visibility};
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};
use std::collections::HashSet;
use tree_sitter::Node;

impl super::JavaScriptExtractor {
    /// Extract visibility - direct Implementation of extractVisibility
    pub(super) fn extract_visibility(&self, node: &Node) -> Visibility {
        // JavaScript doesn't have explicit visibility modifiers like TypeScript
        // But we can infer from naming conventions (reference logic)
        let name_node = node
            .child_by_field_name("name")
            .or_else(|| node.child_by_field_name("property"));

        if let Some(name) = name_node {
            let name_text = self.base.get_node_text(&name);
            if name_text.starts_with('#') {
                return Visibility::Private;
            }
            if name_text.starts_with('_') {
                return Visibility::Protected; // Convention
            }
        }

        Visibility::Public
    }
}

/// Module-level visibility. In a module (a file with `import`, `export`,
/// `require`, or CommonJS export assignments) a top-level declaration is
/// public when the module exports it by any form and private otherwise. In a
/// script every top-level declaration is a global and stays public. Locals
/// of a callable carry no visibility.
pub(super) fn apply_module_visibility(base: &BaseExtractor, root: Node, symbols: &mut [Symbol]) {
    let mut scan = ModuleScan::default();
    scan_module_syntax(base, root, &mut scan, 0);
    for symbol in symbols.iter() {
        match symbol.kind {
            SymbolKind::Import | SymbolKind::Export => scan.is_module = true,
            _ => {}
        }
        if symbol.kind == SymbolKind::Export
            && let Some(metadata) = &symbol.metadata
            && !metadata.contains_key("source")
            && let Some(local) = metadata.get("localName").and_then(|value| value.as_str())
        {
            scan.exported.insert(local.to_string());
        }
    }

    let callable_ids: HashSet<String> = symbols
        .iter()
        .filter(|symbol| {
            matches!(
                symbol.kind,
                SymbolKind::Function | SymbolKind::Method | SymbolKind::Constructor
            )
        })
        .map(|symbol| symbol.id.clone())
        .collect();
    for symbol in symbols.iter_mut() {
        if symbol.visibility.is_none() {
            continue;
        }
        let declares_binding = matches!(
            symbol.kind,
            SymbolKind::Class
                | SymbolKind::Function
                | SymbolKind::Variable
                | SymbolKind::Constant
                | SymbolKind::Import
        );
        match symbol.parent_id.as_deref() {
            Some(parent) if declares_binding && callable_ids.contains(parent) => {
                symbol.visibility = None;
            }
            None if declares_binding && symbol.kind != SymbolKind::Import => {
                let exported = scan.exported.contains(&symbol.name)
                    || symbol
                        .metadata
                        .as_ref()
                        .is_some_and(|metadata| metadata.contains_key("isCommonJSExport"));
                symbol.visibility = Some(if exported || !scan.is_module {
                    Visibility::Public
                } else {
                    Visibility::Private
                });
            }
            _ => {}
        }
    }
}

#[derive(Default)]
struct ModuleScan {
    is_module: bool,
    exported: HashSet<String>,
}

fn scan_module_syntax(base: &BaseExtractor, node: Node, scan: &mut ModuleScan, depth: u32) {
    if !should_visit_tree_depth(depth) {
        return;
    }
    match node.kind() {
        "call_expression"
            if node
                .child_by_field_name("function")
                .is_some_and(|function| base.get_node_text(&function) == "require") =>
        {
            scan.is_module = true;
        }
        "assignment_expression" => record_commonjs_export(base, node, scan),
        _ => {}
    }
    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        scan_module_syntax(base, child, scan, child_depth);
    }
}

/// `module.exports = X`, `module.exports = { X, y: Y }`,
/// `module.exports.x = X`, and `exports.x = X` export the local `X`.
fn record_commonjs_export(base: &BaseExtractor, assignment: Node, scan: &mut ModuleScan) {
    let (Some(left), Some(right)) = (
        assignment.child_by_field_name("left"),
        assignment.child_by_field_name("right"),
    ) else {
        return;
    };
    let left_text = base.get_node_text(&left);
    let whole_module = left_text == "module.exports";
    if !whole_module
        && !left_text.starts_with("module.exports.")
        && !left_text.starts_with("exports.")
    {
        return;
    }
    scan.is_module = true;
    match right.kind() {
        "identifier" => {
            scan.exported.insert(base.get_node_text(&right));
        }
        "class" | "function_expression" | "generator_function" => {
            if let Some(name) = right.child_by_field_name("name") {
                scan.exported.insert(base.get_node_text(&name));
            }
        }
        "object" if whole_module => {
            let mut cursor = right.walk();
            for member in right.named_children(&mut cursor) {
                let local = match member.kind() {
                    "shorthand_property_identifier" => Some(member),
                    "pair" => member
                        .child_by_field_name("value")
                        .filter(|value| value.kind() == "identifier"),
                    _ => None,
                };
                if let Some(local) = local {
                    scan.exported.insert(base.get_node_text(&local));
                }
            }
        }
        _ => {}
    }
}
