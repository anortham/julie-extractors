//! Helper utilities for Bash node traversal and extraction
//!
//! Provides common functions for finding specific node types and working with
//! tree-sitter nodes in Bash code.

use tree_sitter::Node;

use crate::base::body::BodySpan;
use crate::base::{BaseExtractor, NormalizedSpan};
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};

impl super::BashExtractor {
    /// Find the name node for a function definition
    pub(super) fn find_name_node<'a>(&self, node: Node<'a>) -> Option<Node<'a>> {
        // Look for function name nodes
        if let Some(name_field) = node.child_by_field_name("name") {
            return Some(name_field);
        }

        // Fallback: look for 'word' or 'identifier' children
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if matches!(child.kind(), "word" | "identifier") {
                return Some(child);
            }
        }
        None
    }

    /// Find variable name node in variable assignments
    #[allow(clippy::manual_find)] // Manual loops required for borrow checker
    pub(super) fn find_variable_name_node<'a>(&self, node: Node<'a>) -> Option<Node<'a>> {
        // Look for variable name in assignments
        if let Some(name_field) = node.child_by_field_name("name") {
            return Some(name_field);
        }

        // Look for variable_name child
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "variable_name" {
                return Some(child);
            }
        }

        // Fallback: look for word child (first one usually)
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "word" {
                return Some(child);
            }
        }
        None
    }

    /// Find command name node in command invocations
    #[allow(clippy::manual_find)] // Manual loops required for borrow checker
    pub(super) fn find_command_name_node<'a>(&self, node: Node<'a>) -> Option<Node<'a>> {
        // Look for command name field
        if let Some(name_field) = node.child_by_field_name("name") {
            return Some(name_field);
        }

        // Look for command_name child
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "command_name" {
                return Some(child);
            }
        }

        // Fallback: first word child
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "word" {
                return Some(child);
            }
        }
        None
    }

    /// Positional parameter reads (`$1`, `${2}`, `${1:-x}`) in a function,
    /// with their numbers. `$0` names the script, and a nested function owns
    /// its own parameters.
    pub(super) fn collect_parameter_nodes<'a>(
        &self,
        function: Node<'a>,
    ) -> Vec<(Node<'a>, String)> {
        let mut params = Vec::new();
        self.collect_parameter_nodes_at_depth(function, &mut params, 0);
        params
    }

    fn collect_parameter_nodes_at_depth<'a>(
        &self,
        node: Node<'a>,
        params: &mut Vec<(Node<'a>, String)>,
        depth: u32,
    ) {
        if !should_visit_tree_depth(depth) {
            return;
        }
        if depth > 0 && node.kind() == "function_definition" {
            return;
        }
        if matches!(node.kind(), "simple_expansion" | "expansion")
            && let Some(number) = positional_number(&self.base, node)
        {
            params.push((node, number));
        }

        let Some(child_depth) = child_tree_depth(depth) else {
            return;
        };
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.collect_parameter_nodes_at_depth(child, params, child_depth);
        }
    }
}

fn positional_number(base: &BaseExtractor, expansion: Node<'_>) -> Option<String> {
    let mut cursor = expansion.walk();
    let name = expansion
        .named_children(&mut cursor)
        .find(|child| child.kind() == "variable_name")?;
    let number = base.get_node_text(&name);
    (number.bytes().all(|b| b.is_ascii_digit()) && !number.trim_start_matches('0').is_empty())
        .then_some(number)
}

/// Bash body spans come from the grammar: a function's compound body and an
/// assignment's value. Nothing else has a body.
pub(super) fn body_span(node: &Node, _content: &str) -> Option<BodySpan> {
    let field = match node.kind() {
        "function_definition" => "body",
        "variable_assignment" => "value",
        _ => return None,
    };
    node.child_by_field_name(field)
        .map(|body| NormalizedSpan::from_node(&body))
}

/// The `#` comment block directly above `node`. A blank line ends the block,
/// a comment that trails code on its line documents nothing, and the `#!`
/// interpreter line is never documentation.
pub(crate) fn doc_comment(base: &BaseExtractor, node: &Node) -> Option<String> {
    let mut comments = Vec::new();
    let mut next_row = node.start_position().row;
    let mut current = node.prev_named_sibling();
    while let Some(sibling) = current {
        if sibling.kind() != "comment" || sibling.end_position().row + 1 != next_row {
            break;
        }
        let text = base.get_node_text(&sibling);
        let line_start = base.content[..sibling.start_byte()]
            .rfind('\n')
            .map_or(0, |index| index + 1);
        let owns_line = base.content[line_start..sibling.start_byte()]
            .trim()
            .is_empty();
        if !owns_line || (sibling.start_byte() == 0 && text.starts_with("#!")) {
            break;
        }
        comments.push(text);
        next_row = sibling.start_position().row;
        current = sibling.prev_named_sibling();
    }
    crate::base::extractor::select_doc_comment_block("bash", &comments)
}
