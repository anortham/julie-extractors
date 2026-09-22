//! Shared helper functions for GDScript extraction

use crate::base::{BaseExtractor, ContainingSymbolIndex, Symbol, SymbolKind};
use std::collections::HashMap;
use tree_sitter::Node;

pub(super) use crate::base::find_child_by_type;

/// Helper to find multiple annotations preceding a node at the source level
pub(super) fn extract_variable_annotations(
    base: &mut BaseExtractor,
    parent_node: Node,
    signature: &str,
) -> (Vec<String>, String) {
    let mut annotations = Vec::new();
    let mut full_signature = signature.to_string();

    // Check for annotations as children
    for i in 0..parent_node.child_count() {
        if let Some(child) = parent_node.child(i as u32)
            && child.kind() == "annotations"
        {
            for j in 0..child.child_count() {
                if let Some(annotation_child) = child.child(j as u32)
                    && annotation_child.kind() == "annotation"
                {
                    let annotation_text = base.get_node_text(&annotation_child);
                    annotations.push(annotation_text);
                }
            }
        }
    }

    // Also look for sibling annotations at source level
    if let Some(grandparent) = parent_node.parent() {
        // Find parent node index
        let mut node_index = None;
        for i in 0..grandparent.child_count() {
            if let Some(child) = grandparent.child(i as u32)
                && child.id() == parent_node.id()
            {
                node_index = Some(i);
                break;
            }
        }

        if let Some(idx) = node_index {
            let mut annotation_texts = Vec::new();

            // Look backwards for annotations
            for i in (0..idx).rev() {
                if let Some(child) = grandparent.child(i as u32) {
                    if child.kind() == "annotations" {
                        for j in 0..child.child_count() {
                            if let Some(annotation_child) = child.child(j as u32)
                                && annotation_child.kind() == "annotation"
                            {
                                let annotation_text = base.get_node_text(&annotation_child);
                                annotations.push(annotation_text.clone());
                                annotation_texts.insert(0, annotation_text);
                            }
                        }
                    } else if child.kind() == "annotation" {
                        let annotation_text = base.get_node_text(&child);
                        annotations.push(annotation_text.clone());
                        annotation_texts.insert(0, annotation_text);
                    } else if matches!(
                        child.kind(),
                        "variable_statement" | "constant_statement" | "var" | "const"
                    ) {
                        break;
                    }
                }
            }

            // Build full signature with annotations
            if !annotation_texts.is_empty() {
                full_signature = format!("{}\n{}", annotation_texts.join("\n"), signature);
            }
        }
    }

    (annotations, full_signature)
}

/// The `##` block directly above `node`, read through any annotation lines
/// between them. A blank line or a plain `#` comment ends the block, so a
/// container's doc never reaches the next member.
pub(super) fn doc_comment(base: &BaseExtractor, node: Node) -> Option<String> {
    let mut lines = Vec::new();
    let mut next_row = node.start_position().row;
    let mut current = node.prev_named_sibling();
    while let Some(sibling) = current {
        if sibling.end_position().row + 1 != next_row {
            break;
        }
        match sibling.kind() {
            "annotation" => {}
            "comment" => {
                let text = base.get_node_text(&sibling);
                if !text.starts_with("##") {
                    break;
                }
                lines.push(text);
            }
            _ => break,
        }
        next_row = sibling.start_position().row;
        current = sibling.prev_named_sibling();
    }
    if lines.is_empty() {
        return None;
    }
    lines.reverse();
    Some(lines.join("\n"))
}

/// A `##` block that starts on the line after `node` and runs over contiguous
/// comment lines: the Godot place for a script's class documentation.
pub(super) fn doc_comment_after(base: &BaseExtractor, node: Node) -> Option<String> {
    let mut lines = Vec::new();
    let mut next_row = node.end_position().row + 1;
    let mut current = node.next_named_sibling();
    while let Some(sibling) = current {
        if sibling.kind() != "comment" || sibling.start_position().row != next_row {
            break;
        }
        let text = base.get_node_text(&sibling);
        if !text.starts_with("##") {
            break;
        }
        lines.push(text);
        next_row = sibling.end_position().row + 1;
        current = sibling.next_named_sibling();
    }
    (!lines.is_empty()).then(|| lines.join("\n"))
}

/// Node kinds whose body runs as callable code, so a `var` inside them is a local.
pub(super) fn is_callable_scope(kind: &str) -> bool {
    matches!(
        kind,
        "function_definition" | "constructor_definition" | "lambda" | "set_body" | "get_body"
    )
}

pub(super) fn inside_callable(node: Node) -> bool {
    let mut current = node.parent();
    while let Some(parent) = current {
        if is_callable_scope(parent.kind()) {
            return true;
        }
        current = parent.parent();
    }
    false
}

const DECLARATION_KINDS: &[&str] = &[
    "function_definition",
    "constructor_definition",
    "lambda",
    "variable_statement",
    "export_variable_statement",
    "onready_variable_statement",
    "const_statement",
    "signal_statement",
    "enum_definition",
    "class_definition",
];

/// Finds the declaration that owns a node by walking its syntax ancestors, so
/// a field initializer or property accessor belongs to its field and an inner
/// class member belongs to that class. Locals never own code. Nodes outside
/// every declaration fall back to the span-based index.
pub(super) struct DeclarationIndex<'a> {
    by_range: HashMap<(u32, u32), &'a Symbol>,
    fallback: ContainingSymbolIndex<'a>,
}

impl<'a> DeclarationIndex<'a> {
    pub(super) fn new(base: &BaseExtractor, symbols: &'a [Symbol]) -> Self {
        let by_range = symbols
            .iter()
            .filter(|symbol| symbol.kind != SymbolKind::Variable)
            .map(|symbol| ((symbol.start_byte, symbol.end_byte), symbol))
            .collect();
        Self {
            by_range,
            fallback: base.containing_symbol_index(symbols),
        }
    }

    pub(super) fn find(&self, node: Node) -> Option<&'a Symbol> {
        let mut current = node.parent();
        while let Some(ancestor) = current {
            if DECLARATION_KINDS.contains(&ancestor.kind())
                && let Some(symbol) = self
                    .by_range
                    .get(&(ancestor.start_byte() as u32, ancestor.end_byte() as u32))
            {
                return Some(symbol);
            }
            current = ancestor.parent();
        }
        self.fallback.find(node)
    }
}
