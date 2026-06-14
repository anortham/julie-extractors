use crate::base::TypeArgument;
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};
use tree_sitter::Node;

use super::get_node_text_from_content;

/// Recursively extract ordered, nested type arguments from a TypeScript
/// `type_arguments` node, reading type names from the Vue script section text.
pub(super) fn extract_vue_type_arguments<'a>(
    arg_list_node: Node<'a>,
    script_content: &str,
) -> Vec<TypeArgument> {
    extract_vue_type_arguments_at_depth(arg_list_node, script_content, 0)
}

fn extract_vue_type_arguments_at_depth<'a>(
    arg_list_node: Node<'a>,
    script_content: &str,
    depth: u32,
) -> Vec<TypeArgument> {
    if !should_visit_tree_depth(depth) {
        return Vec::new();
    }

    let mut arguments = Vec::new();
    let mut ordinal: u32 = 0;
    let child_depth = child_tree_depth(depth);
    let children: Vec<Node<'a>> = arg_list_node.children(&mut arg_list_node.walk()).collect();
    for child in children {
        if !child.is_named() {
            continue;
        }
        match child.kind() {
            "generic_type" => {
                let name = child
                    .child_by_field_name("name")
                    .map(|n| get_node_text_from_content(&n, script_content))
                    .unwrap_or_else(|| get_node_text_from_content(&child, script_content));
                let nested_children: Vec<Node<'a>> = child.children(&mut child.walk()).collect();
                let nested_arg_list = nested_children
                    .iter()
                    .find(|c| c.kind() == "type_arguments")
                    .copied();
                let children = nested_arg_list
                    .and_then(|nested| {
                        child_depth.map(|depth| {
                            extract_vue_type_arguments_at_depth(nested, script_content, depth)
                        })
                    })
                    .unwrap_or_default();
                arguments.push(TypeArgument {
                    ordinal,
                    type_name: name,
                    children,
                });
                ordinal += 1;
            }
            _ => {
                arguments.push(TypeArgument {
                    ordinal,
                    type_name: get_node_text_from_content(&child, script_content),
                    children: Vec::new(),
                });
                ordinal += 1;
            }
        }
    }
    arguments
}
