/// Helper functions for common operations
///
/// Provides shared utilities:
/// - Node traversal helpers
/// - Type inference from expressions
/// - Node text extraction
use crate::base::BaseExtractor;
use tree_sitter::Node;

pub(crate) use crate::base::find_child_by_type;

/// Check if a node contains a function definition child
pub(crate) fn contains_function_definition(node: Node) -> bool {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "function_definition" {
            return true;
        }
    }
    false
}

/// Infer the data type from an expression node
///
/// Handles various expression types and returns a string representation
/// of the inferred type (e.g., "string", "number", "function", "table", "import").
pub(crate) fn infer_type_from_expression(base: &BaseExtractor, node: Node) -> String {
    match node.kind() {
        "string" => "string".to_string(),
        "number" => "number".to_string(),
        "true" | "false" => "boolean".to_string(),
        "nil" => "nil".to_string(),
        "function_definition" => "function".to_string(),
        "table_constructor" | "table" => "table".to_string(),
        "function_call" => {
            // Check if this is a require() call
            if let Some(identifier) = find_child_by_type(&node, "identifier")
                && base.get_node_text(&identifier) == "require"
            {
                return "import".to_string();
            }
            String::new()
        }
        _ => String::new(),
    }
}

/// The doc comment block directly above `node`. A blank line between comments,
/// or between the last comment and `node`, ends the block, so a file header or a
/// detached section comment never documents the next declaration.
pub(crate) fn doc_comment(base: &BaseExtractor, node: &Node) -> Option<String> {
    let mut comments = Vec::new();
    let mut next_row = node.start_position().row;
    let mut current = node.prev_named_sibling().or_else(|| {
        node.parent()
            .filter(|parent| parent.kind() == "block")
            .and_then(|block| block.prev_named_sibling())
    });
    while let Some(sibling) = current {
        if sibling.kind() != "comment" {
            break;
        }
        let end = sibling.end_position();
        let last_row = if end.column == 0 && end.row > sibling.start_position().row {
            end.row - 1
        } else {
            end.row
        };
        if last_row + 1 != next_row {
            break;
        }
        comments.push(base.get_node_text(&sibling));
        next_row = sibling.start_position().row;
        current = sibling.prev_named_sibling();
    }
    crate::base::extractor::select_doc_comment_block("lua", &comments)
}

/// LuaLS marker tags that annotate a declaration rather than type it.
const MARKER_TAGS: &[&str] = &[
    "deprecated",
    "nodiscard",
    "async",
    "private",
    "protected",
    "package",
];

/// Annotation markers from the LuaLS marker tags (`---@deprecated`,
/// `---@nodiscard`, ...) in a declaration's doc comment.
pub(crate) fn doc_annotations(doc_comment: Option<&str>) -> Vec<crate::base::AnnotationMarker> {
    let Some(doc) = doc_comment else {
        return Vec::new();
    };
    let tags: Vec<String> = doc
        .lines()
        .filter_map(|line| {
            let tag = line
                .trim_start()
                .trim_start_matches('-')
                .trim_start()
                .strip_prefix('@')?
                .split_whitespace()
                .next()?;
            MARKER_TAGS.contains(&tag).then(|| format!("@{tag}"))
        })
        .collect();
    crate::base::annotations::normalize_annotations(&tags, "lua")
}
