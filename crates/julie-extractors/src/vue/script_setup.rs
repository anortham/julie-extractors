//! `<script setup>` compiler-macro annotations on the component symbol and
//! on the bindings `defineExpose` publishes.

use super::parsing::ParsedVueSfc;
use crate::base::{AnnotationMarker, Symbol, SymbolKind, normalize_annotations};
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};
use tree_sitter::Node;

const COMPONENT_MACROS: [&str; 3] = ["defineOptions", "defineProps", "defineEmits"];

/// Attach script-setup macro metadata to the component symbol and defineExpose targets.
pub(super) fn apply_script_setup_annotations(symbols: &mut [Symbol], parsed_sfc: &ParsedVueSfc) {
    for (idx, section) in parsed_sfc.sections.iter().enumerate() {
        if section.section_type != "script" || !section.is_setup {
            continue;
        }
        let Some(tree) = parsed_sfc.script_tree(idx) else {
            continue;
        };

        let component_macros = collect_component_macros(tree.root_node(), &section.content);
        if !component_macros.is_empty()
            && let Some(component) = symbols
                .iter_mut()
                .find(|symbol| is_vue_component_symbol(symbol))
        {
            merge_annotations(component, vue_macro_annotations(&component_macros));
        }

        let exposed_names = collect_define_expose_names(tree.root_node(), &section.content);
        for name in exposed_names {
            if let Some(symbol) = symbols.iter_mut().find(|symbol| {
                symbol.name == name
                    && matches!(
                        symbol.kind,
                        SymbolKind::Function | SymbolKind::Variable | SymbolKind::Method
                    )
            }) {
                merge_annotations(symbol, vue_macro_annotations(&["defineExpose".to_string()]));
            }
        }
    }
}

fn is_vue_component_symbol(symbol: &Symbol) -> bool {
    symbol.kind == SymbolKind::Class
        && symbol.metadata.as_ref().is_some_and(|metadata| {
            metadata.get("type").and_then(|value| value.as_str()) == Some("vue-sfc")
        })
}

fn merge_annotations(symbol: &mut Symbol, annotations: Vec<AnnotationMarker>) {
    for annotation in annotations {
        if symbol
            .annotations
            .iter()
            .any(|existing| existing.annotation_key == annotation.annotation_key)
        {
            continue;
        }
        symbol.annotations.push(annotation);
    }
}

fn vue_macro_annotations(macro_names: &[String]) -> Vec<AnnotationMarker> {
    let raw_texts: Vec<String> = macro_names.iter().map(|name| format!("{name}()")).collect();
    normalize_annotations(&raw_texts, "javascript")
}

fn collect_component_macros(node: Node, content: &str) -> Vec<String> {
    let mut macros = Vec::new();
    collect_component_macros_recursive(node, content, &mut macros, 0);
    macros.sort();
    macros.dedup();
    macros
}

fn collect_component_macros_recursive(
    node: Node,
    content: &str,
    macros: &mut Vec<String>,
    depth: u32,
) {
    if !should_visit_tree_depth(depth) {
        return;
    }

    if node.kind() == "call_expression"
        && let Some(callee) = node.child_by_field_name("function")
    {
        let callee_name = get_node_text(&callee, content);
        if COMPONENT_MACROS.contains(&callee_name.as_str()) {
            macros.push(callee_name);
        }
    }

    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_component_macros_recursive(child, content, macros, child_depth);
    }
}

fn collect_define_expose_names(node: Node, content: &str) -> Vec<String> {
    let mut names = Vec::new();
    collect_define_expose_names_recursive(node, content, &mut names, 0);
    names.sort();
    names.dedup();
    names
}

fn collect_define_expose_names_recursive(
    node: Node,
    content: &str,
    names: &mut Vec<String>,
    depth: u32,
) {
    if !should_visit_tree_depth(depth) {
        return;
    }

    if node.kind() == "call_expression"
        && let Some(callee) = node.child_by_field_name("function")
        && get_node_text(&callee, content) == "defineExpose"
        && let Some(arguments) = node.child_by_field_name("arguments")
    {
        let Some(child_depth) = child_tree_depth(depth) else {
            return;
        };
        collect_exposed_object_names(arguments, content, names, child_depth);
    }

    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_define_expose_names_recursive(child, content, names, child_depth);
    }
}

fn collect_exposed_object_names(node: Node, content: &str, names: &mut Vec<String>, depth: u32) {
    if !should_visit_tree_depth(depth) {
        return;
    }

    match node.kind() {
        "object" | "object_pattern" => {
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                collect_exposed_property_name(child, content, names);
            }
        }
        _ => {
            let Some(child_depth) = child_tree_depth(depth) else {
                return;
            };
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                collect_exposed_object_names(child, content, names, child_depth);
            }
        }
    }
}

fn collect_exposed_property_name(node: Node, content: &str, names: &mut Vec<String>) {
    match node.kind() {
        "pair" => {
            if let Some(value) = node.child_by_field_name("value")
                && value.kind() == "identifier"
            {
                names.push(get_node_text(&value, content));
            }
        }
        "shorthand_property_identifier" | "shorthand_property_identifier_pattern" => {
            names.push(get_node_text(&node, content));
        }
        _ => {}
    }
}

/// Get text content from a tree-sitter node using the section content
fn get_node_text(node: &Node, content: &str) -> String {
    let start = node.start_byte();
    let end = node.end_byte();
    if end <= content.len() {
        content[start..end].to_string()
    } else {
        String::new()
    }
}
