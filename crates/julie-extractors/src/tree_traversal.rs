use tree_sitter::Node;

use crate::base::{NormalizedSpan, ParseDiagnostic, ParseDiagnosticKind};

pub(crate) const TREE_TRAVERSAL_DEPTH_LIMIT: u32 = 1024;

pub(crate) fn should_visit_tree_depth(depth: u32) -> bool {
    should_visit_bounded_depth(depth, TREE_TRAVERSAL_DEPTH_LIMIT)
}

/// For walkers whose own grammar bound is tighter than the crate-wide budget.
pub(crate) fn should_visit_bounded_depth(depth: u32, limit: u32) -> bool {
    depth <= limit
}

pub(crate) fn child_tree_depth(depth: u32) -> Option<u32> {
    if depth >= TREE_TRAVERSAL_DEPTH_LIMIT {
        return None;
    }

    depth.checked_add(1)
}

/// Every fact-emitting walker starts at the tree root and stops descending at
/// `TREE_TRAVERSAL_DEPTH_LIMIT`, so a tree with any node below that depth is
/// extracted with facts missing. Report it once per file instead of truncating
/// silently.
pub(crate) fn depth_truncation_diagnostic(root: Node<'_>) -> Option<ParseDiagnostic> {
    let node = node_below_depth_limit(root)?;
    let span = NormalizedSpan::from_node(&node);

    Some(ParseDiagnostic {
        kind: ParseDiagnosticKind::DepthTruncated,
        message: Some(format!(
            "tree traversal depth limit {TREE_TRAVERSAL_DEPTH_LIMIT} reached; \
             facts below this depth were not extracted"
        )),
        start_line: span.start_line,
        start_column: span.start_column,
        end_line: span.end_line,
        end_column: span.end_column,
        start_byte: span.start_byte,
        end_byte: span.end_byte,
    })
}

/// Some node below the budget, or `None` when the whole tree fits. Measured
/// over every child, named or not, because that is the deepest measure any
/// walker uses. Iterative on purpose: the probe for unbounded depth cannot
/// itself be bounded by the stack it is protecting.
fn node_below_depth_limit<'tree>(root: Node<'tree>) -> Option<Node<'tree>> {
    let mut stack = vec![(root, 0u32)];

    while let Some((node, depth)) = stack.pop() {
        if depth > TREE_TRAVERSAL_DEPTH_LIMIT {
            return Some(node);
        }

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            stack.push((child, depth + 1));
        }
    }

    None
}
