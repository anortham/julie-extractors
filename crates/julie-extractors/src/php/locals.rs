use super::{PhpExtractor, namespaces::extract_variable_assignment, type_facts};
use crate::base::Symbol;
use tree_sitter::Node;

/// Local variables an assignment declares: `$x = ...` declares `x`, and
/// `[$a, $b] = ...` or `list($a, $b) = ...` declares each target. A property,
/// element, or static-property target declares nothing.
pub(super) fn extract_assignment(
    extractor: &mut PhpExtractor,
    node: Node,
    parent_id: Option<&str>,
) -> Vec<Symbol> {
    let Some(left) = node.child_by_field_name("left") else {
        return Vec::new();
    };
    match left.kind() {
        "variable_name" => {
            let Some(symbol) = extract_variable_assignment(extractor, node, left, parent_id) else {
                return Vec::new();
            };
            if let Some(value_node) = assignment_value_node(node) {
                type_facts::record_initializer_type(
                    &mut extractor.base,
                    &symbol.id,
                    value_node,
                    &extractor.return_types,
                );
            }
            vec![symbol]
        }
        "list_literal" => {
            let mut targets = Vec::new();
            collect_list_targets(left, &mut targets);
            targets
                .into_iter()
                .filter_map(|target| {
                    extract_variable_assignment(extractor, node, target, parent_id)
                })
                .collect()
        }
        _ => Vec::new(),
    }
}

fn collect_list_targets<'a>(list: Node<'a>, targets: &mut Vec<Node<'a>>) {
    collect_list_targets_at(list, targets, 0);
}

fn collect_list_targets_at<'a>(list: Node<'a>, targets: &mut Vec<Node<'a>>, depth: u32) {
    let Some(child_depth) = crate::tree_traversal::child_tree_depth(depth) else {
        return;
    };
    let mut cursor = list.walk();
    for child in list.named_children(&mut cursor) {
        match child.kind() {
            "variable_name" => targets.push(child),
            "list_literal" => collect_list_targets_at(child, targets, child_depth),
            "by_ref" => {
                targets.extend(child.named_child(0).filter(|n| n.kind() == "variable_name"))
            }
            _ => {}
        }
    }
}

fn assignment_value_node(node: Node<'_>) -> Option<Node<'_>> {
    node.child_by_field_name("right")
}
