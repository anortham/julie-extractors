/// Helper utilities for Ruby symbol extraction
/// Includes node name extraction, type inference, and context checking
use crate::base::{SymbolKind, Visibility};
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};
use tree_sitter::Node;

/// Extract a name from a node by field name
pub(super) fn extract_name_from_node(
    node: Node,
    base_get_text: impl Fn(&Node) -> String,
    field_name: &str,
) -> Option<String> {
    node.child_by_field_name(field_name)
        .map(|name_node| base_get_text(&name_node))
}

/// The name a `class` or `module` node declares. A compact declaration
/// `class Api::V1::Base` declares `Base` inside `Api::V1`.
pub(crate) fn declared_name(base: &crate::base::BaseExtractor, node: Node) -> Option<String> {
    let name_node = node.child_by_field_name("name").or_else(|| {
        let mut cursor = node.walk();
        node.children(&mut cursor)
            .find(|child| child.kind() == "constant")
    })?;
    let terminal = if name_node.kind() == "scope_resolution" {
        name_node.child_by_field_name("name")?
    } else {
        name_node
    };
    Some(base.get_node_text(&terminal))
}

/// Build a namespace-aware qualified name by walking up parent modules/classes
pub(super) fn build_qualified_name(
    node: Node,
    name: &str,
    base_get_text: impl Fn(&Node) -> String,
) -> String {
    let mut namespace_parts = Vec::new();
    let mut current = node;

    // Walk up the tree to find parent modules/classes
    while let Some(parent) = current.parent() {
        if matches!(parent.kind(), "module" | "class") {
            // Extract the name of the parent module/class
            if let Some(parent_name) = extract_name_from_node(parent, &base_get_text, "name")
                .or_else(|| extract_name_from_node(parent, &base_get_text, "constant"))
                .or_else(|| {
                    // Fallback: find first constant child
                    let mut cursor = parent.walk();
                    for child in parent.children(&mut cursor) {
                        if child.kind() == "constant" {
                            return Some(base_get_text(&child));
                        }
                    }
                    None
                })
            {
                namespace_parts.push(parent_name);
            }
        }
        current = parent;
    }

    // Reverse to get the correct order (outermost first)
    namespace_parts.reverse();

    // If we have namespace parts, join them with ::
    if namespace_parts.is_empty() {
        name.to_string()
    } else {
        format!("{}::{}", namespace_parts.join("::"), name)
    }
}

/// Infer symbol kind from assignment node (constant vs variable)
pub(super) fn infer_symbol_kind_from_assignment(
    left_node: &Node,
    base_get_text: impl Fn(&Node) -> String,
) -> SymbolKind {
    match left_node.kind() {
        "constant" => SymbolKind::Constant,
        "class_variable" | "instance_variable" => SymbolKind::Field,
        "global_variable" => SymbolKind::Variable,
        _ => {
            let text = base_get_text(left_node);
            if text.chars().all(|c| c.is_uppercase() || c == '_') {
                SymbolKind::Constant
            } else {
                SymbolKind::Variable
            }
        }
    }
}

/// Check if a node is part of an assignment
pub(super) fn is_part_of_assignment(node: &Node) -> bool {
    let mut current = *node;
    while let Some(parent) = current.parent() {
        if matches!(parent.kind(), "assignment" | "operator_assignment") {
            return true;
        }
        current = parent;
    }
    false
}

/// Check if a node is the left-hand side target of an assignment.
/// Used to skip duplicate symbol creation for constants already handled by the assignment.
pub(super) fn is_assignment_target(node: &Node) -> bool {
    node.parent().is_some_and(|p| {
        matches!(p.kind(), "assignment" | "operator_assignment")
            && p.child_by_field_name("left")
                .is_some_and(|left| left.id() == node.id())
    })
}

/// Extract the called method's name from a call node.
///
/// Reads the grammar's `method` field. Scanning for the first `identifier`
/// child instead returns the receiver of `receiver.method`, because the
/// receiver comes first in the child order.
pub(crate) fn extract_method_name_from_call(
    node: Node,
    base_get_text: impl Fn(&Node) -> String,
) -> Option<String> {
    node.child_by_field_name("method")
        .map(|method_node| base_get_text(&method_node))
}

/// Text of a call node's receiver, or `None` for a bare call.
pub(crate) fn extract_call_receiver(
    node: Node,
    base_get_text: impl Fn(&Node) -> String,
) -> Option<String> {
    node.child_by_field_name("receiver")
        .map(|receiver| base_get_text(&receiver))
}

/// Whether a call acts on the enclosing scope rather than on another object.
///
/// `include Formatting` mixes into the enclosing class; `other.include
/// Formatting` mixes into `other` and says nothing about the enclosing class.
pub(super) fn is_self_directed_call(node: Node) -> bool {
    node.child_by_field_name("receiver")
        .is_none_or(|receiver| receiver.kind() == "self")
}

/// A symbol argument that names a method the call sends or registers: a
/// Rails callback (`before_action :set_post`, `validate :check`,
/// `rescue_from E, with: :handle`, `helper_method :current_user`), a dynamic
/// send (`send(:audit)`, `method(:audit)`), or a `&:price` block argument.
pub(crate) struct MethodSymbolArgument<'tree> {
    pub(crate) node: Node<'tree>,
    pub(crate) name: String,
    /// The named method belongs to the enclosing class, so it may resolve to
    /// a same-file method.
    pub(crate) resolve_locally: bool,
}

pub(crate) fn method_symbol_arguments<'tree>(
    base: &crate::base::BaseExtractor,
    call: Node<'tree>,
) -> Vec<MethodSymbolArgument<'tree>> {
    let Some(method) = call.child_by_field_name("method") else {
        return Vec::new();
    };
    let method = base.get_node_text(&method);
    let Some(arguments) = call.child_by_field_name("arguments") else {
        return Vec::new();
    };
    let self_directed = is_self_directed_call(call);
    let symbol = |node: Node<'tree>, resolve_locally: bool| {
        (node.kind() == "simple_symbol").then(|| MethodSymbolArgument {
            node,
            name: base
                .get_node_text(&node)
                .trim_start_matches(':')
                .to_string(),
            resolve_locally,
        })
    };
    let mut cursor = arguments.walk();
    let children: Vec<Node<'tree>> = arguments.named_children(&mut cursor).collect();
    let mut found = Vec::new();
    if let Some(block_argument) = children
        .first()
        .filter(|first| first.kind() == "block_argument")
        .and_then(|first| first.named_child(0))
    {
        found.extend(symbol(block_argument, false));
        return found;
    }
    if matches!(
        method.as_str(),
        "send" | "public_send" | "__send__" | "method"
    ) {
        found.extend(
            children
                .first()
                .and_then(|first| symbol(*first, self_directed)),
        );
        return found;
    }
    if !self_directed {
        return found;
    }
    let positional = is_callback_macro(&method) || method == "helper_method";
    for child in children {
        if positional {
            found.extend(symbol(child, true));
        }
        if child.kind() == "pair"
            && let Some(key) = child.child_by_field_name("key")
            && matches!(
                base.get_node_text(&key).trim_end_matches(':'),
                "with" | "if" | "unless"
            )
            && (is_callback_macro(&method)
                || matches!(method.as_str(), "rescue_from" | "validates"))
            && let Some(value) = child.child_by_field_name("value")
        {
            found.extend(symbol(value, true));
        }
    }
    found
}

/// Rails controller and model callbacks and custom validators, whose symbol
/// arguments name methods of the same class.
fn is_callback_macro(method: &str) -> bool {
    let callback = [
        "before_",
        "after_",
        "around_",
        "skip_before_",
        "skip_after_",
        "skip_around_",
        "prepend_before_",
        "prepend_after_",
        "prepend_around_",
        "append_before_",
        "append_after_",
        "append_around_",
    ]
    .iter()
    .any(|prefix| method.starts_with(prefix));
    callback || method == "validate"
}

/// Extract target of a singleton method (e.g., 'self' or object name)
pub(super) fn extract_singleton_method_target(
    node: Node,
    base_get_text: impl Fn(&Node) -> String,
) -> String {
    // Find the target before the dot
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if (child.kind() == "identifier" || child.kind() == "self")
            && child.next_sibling().is_some_and(|s| s.kind() == ".")
        {
            return base_get_text(&child);
        }
    }
    "self".to_string()
}

/// Extract alias name from an alias node
pub(super) fn extract_alias_name(
    node: Node,
    base_get_text: impl Fn(&Node) -> String,
) -> Option<String> {
    // alias new_name old_name - extract the new_name
    let mut cursor = node.walk();
    let children: Vec<_> = node.children(&mut cursor).collect();

    if children.len() >= 2 {
        Some(base_get_text(&children[1]))
    } else {
        None
    }
}

/// Find all include/extend/prepend/using calls within a node
pub(super) fn find_includes_and_extends(
    node: &Node,
    extract_method_name: impl Fn(Node) -> Option<String> + Copy,
    base_get_text: impl Fn(&Node) -> String + Copy,
) -> Vec<String> {
    let mut includes = Vec::new();
    find_includes_and_extends_recursive(
        *node,
        &mut includes,
        extract_method_name,
        base_get_text,
        0,
    );
    includes
}

fn find_includes_and_extends_recursive(
    node: Node,
    includes: &mut Vec<String>,
    extract_method_name: impl Fn(Node) -> Option<String> + Copy,
    base_get_text: impl Fn(&Node) -> String + Copy,
    depth: u32,
) {
    if !should_visit_tree_depth(depth) {
        return;
    }

    // Check if this node itself is a call node for include/extend/prepend
    if node.kind() == "call"
        && is_self_directed_call(node)
        && let Some(method_name) = extract_method_name(node)
        && matches!(
            method_name.as_str(),
            "include" | "extend" | "prepend" | "using"
        )
    {
        includes.push(base_get_text(&node));
    }

    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if matches!(
            child.kind(),
            "class" | "module" | "singleton_class" | "method" | "singleton_method"
        ) {
            continue;
        }
        find_includes_and_extends_recursive(
            child,
            includes,
            extract_method_name,
            base_get_text,
            child_depth,
        );
    }
}

/// Parse visibility from identifier string.
/// Includes `module_function` which makes subsequent methods public as module-level functions.
pub(super) fn parse_visibility(text: &str) -> Option<Visibility> {
    match text {
        "private" => Some(Visibility::Private),
        "protected" => Some(Visibility::Protected),
        "public" | "module_function" => Some(Visibility::Public),
        _ => None,
    }
}
