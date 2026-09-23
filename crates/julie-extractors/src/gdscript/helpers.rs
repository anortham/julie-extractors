//! Shared helper functions for GDScript extraction

use crate::base::{BaseExtractor, ContainingSymbolIndex, Symbol, SymbolKind, Visibility};
use std::collections::HashMap;
use tree_sitter::Node;

pub(super) use crate::base::find_child_by_type;

/// Annotations that configure the whole script, never a single member.
const SCRIPT_ANNOTATIONS: &[&str] = &["tool", "icon", "static_unload"];

/// The annotations of a declaration, in source order: the standalone
/// annotation lines directly above it, then its inline `annotations`. The
/// standalone lines come back separately so a signature can show them. A
/// member never takes a script annotation (`@tool`, `@icon`); a script class
/// header reads through its `extends` and `class_name` lines.
pub(super) struct DeclarationAnnotations {
    pub(super) standalone: Vec<String>,
    pub(super) all: Vec<String>,
}

impl DeclarationAnnotations {
    /// The declaration text with its standalone annotation lines above it.
    pub(super) fn signature(&self, declaration: &str) -> String {
        if self.standalone.is_empty() {
            return declaration.to_string();
        }
        format!("{}\n{declaration}", self.standalone.join("\n"))
    }
}

pub(super) fn declaration_annotations(
    base: &BaseExtractor,
    node: Node,
    script_header: bool,
) -> DeclarationAnnotations {
    let mut standalone = Vec::new();
    let mut current = node.prev_named_sibling();
    while let Some(sibling) = current {
        match sibling.kind() {
            "annotation" => standalone.push(sibling),
            "comment" => {}
            "extends_statement" | "class_name_statement" if script_header => {}
            _ => break,
        }
        current = sibling.prev_named_sibling();
    }
    standalone.reverse();
    let standalone: Vec<String> = standalone
        .into_iter()
        .filter(|annotation| script_header || !is_script_annotation(base, *annotation))
        .map(|annotation| base.get_node_text(&annotation))
        .collect();

    let mut all = standalone.clone();
    let mut cursor = node.walk();
    for group in node
        .children(&mut cursor)
        .filter(|child| child.kind() == "annotations")
    {
        let mut group_cursor = group.walk();
        all.extend(
            group
                .named_children(&mut group_cursor)
                .filter(|child| child.kind() == "annotation")
                .map(|annotation| base.get_node_text(&annotation)),
        );
    }
    DeclarationAnnotations { standalone, all }
}

/// The name of an `annotation` node (`export_range` for `@export_range(0, 1)`).
pub(super) fn annotation_name(base: &BaseExtractor, annotation: Node) -> Option<String> {
    let mut cursor = annotation.walk();
    annotation
        .named_children(&mut cursor)
        .find(|child| child.kind() == "identifier")
        .map(|identifier| base.get_node_text(&identifier))
}

fn is_script_annotation(base: &BaseExtractor, annotation: Node) -> bool {
    annotation_name(base, annotation)
        .is_some_and(|name| SCRIPT_ANNOTATIONS.contains(&name.as_str()))
}

/// `_name` is private and every other name is public, as Godot's convention.
pub(super) fn member_visibility(name: &str) -> Visibility {
    if name.starts_with('_') {
        Visibility::Private
    } else {
        Visibility::Public
    }
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
