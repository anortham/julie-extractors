use crate::base::BaseExtractor;
use tree_sitter::Node;

use super::{function_declarators, helpers};

/// Extract function modifiers (virtual, static, explicit, inline, etc.)
pub(super) fn extract_function_modifiers(base: &mut BaseExtractor, node: Node) -> Vec<String> {
    let mut modifiers = Vec::new();
    let modifier_types = ["virtual", "static", "explicit", "friend", "inline"];

    helpers::collect_modifiers_recursive(base, node, &mut modifiers, &modifier_types);

    modifiers
}

/// Extract method modifiers (checks multiple tree levels)
pub(super) fn extract_method_modifiers(
    base: &mut BaseExtractor,
    declaration_node: Node,
    func_node: Node,
) -> Vec<String> {
    let mut modifiers = Vec::new();
    let modifier_types = [
        "virtual", "static", "explicit", "friend", "inline", "override", "final",
    ];

    let mut nodes_to_check = vec![declaration_node, func_node];
    nodes_to_check.extend(function_declarators::enclosing_declaration(
        declaration_node,
    ));

    for node in nodes_to_check {
        if node.kind() == "field_declaration" || node.kind() == "declaration" {
            for child in node.children(&mut node.walk()) {
                if modifier_types.contains(&child.kind()) {
                    let modifier = base.get_node_text(&child);
                    if !modifiers.contains(&modifier) {
                        modifiers.push(modifier);
                    }
                } else if child.kind() == "storage_class_specifier" {
                    let text = base.get_node_text(&child);
                    if modifier_types.contains(&text.as_str()) && !modifiers.contains(&text) {
                        modifiers.push(text);
                    }
                }
            }
        }

        helpers::collect_modifiers_recursive(base, node, &mut modifiers, &modifier_types);
    }

    modifiers
}

/// The node kinds a declared return type is written as.
const RETURN_TYPE_KINDS: &[&str] = &[
    "primitive_type",
    "type_identifier",
    "qualified_identifier",
    "scoped_type_identifier",
    "sized_type_specifier",
    "template_type",
    "dependent_type",
    "decltype",
    "struct_specifier",
    "enum_specifier",
    "union_specifier",
    "auto",
    "placeholder_type_specifier",
];

/// The whole declared return type, ready to prepend to the name: the qualifiers
/// written before the type, the type text, and one `*`, `&` or `&&` per
/// declarator wrapper. Qt style attaches the mark to the name, so the result
/// ends in the mark (`QQuickItem *`) or in a space (`void `), and is empty when
/// nothing is declared.
pub(super) fn declared_return_type(base: &mut BaseExtractor, node: Node) -> String {
    let (declaration, marks) = return_type_declaration(node);
    let declaration = declaration.unwrap_or(node);
    let mut qualifiers = Vec::new();
    let mut type_node = None;
    for child in declaration.children(&mut declaration.walk()) {
        if RETURN_TYPE_KINDS.contains(&child.kind()) {
            type_node = Some(child);
            break;
        }
        if child.kind() == "type_qualifier" {
            qualifiers.push(base.get_node_text(&child));
        }
    }
    let Some(type_node) = type_node else {
        return String::new();
    };

    let mut text = String::new();
    for qualifier in qualifiers {
        text.push_str(&qualifier);
        text.push(' ');
    }
    text.push_str(&base.get_node_text(&type_node));
    text.push(' ');
    text.push_str(&marks);
    text
}

/// The declaration that carries the return type, reached from the
/// `function_declarator`, and the pointer and reference marks written between
/// the two.
fn return_type_declaration(node: Node) -> (Option<Node>, String) {
    let declarator = if node.kind() == "function_declarator" {
        Some(node)
    } else {
        node.child_by_field_name("declarator")
            .and_then(function_declarators::unwrap_to_function_declarator)
    };
    let Some(declarator) = declarator else {
        return (None, String::new());
    };

    let mut marks = String::new();
    let mut current = declarator;
    while let Some(parent) = current.parent() {
        match parent.kind() {
            "pointer_declarator" | "reference_declarator" => {
                marks.insert_str(0, declarator_mark(parent));
                current = parent;
            }
            "field_declaration" | "declaration" | "function_definition" => {
                return (Some(parent), marks);
            }
            _ => return (None, marks),
        }
    }
    (None, marks)
}

fn declarator_mark(node: Node) -> &'static str {
    node.children(&mut node.walk())
        .find_map(|child| match child.kind() {
            "*" => Some("*"),
            "&" => Some("&"),
            "&&" => Some("&&"),
            _ => None,
        })
        .unwrap_or_default()
}

/// Extract trailing return type (for auto return type deduction)
pub(super) fn extract_trailing_return_type(base: &mut BaseExtractor, node: Node) -> String {
    let func_declarator = node
        .children(&mut node.walk())
        .find(|c| c.kind() == "function_declarator");

    if let Some(declarator) = func_declarator {
        let children: Vec<Node> = declarator.children(&mut declarator.walk()).collect();

        for (i, child) in children.iter().enumerate() {
            if child.kind() == "->" && i + 1 < children.len() {
                return base.get_node_text(&children[i + 1]);
            } else if child.kind() == "trailing_return_type" {
                return child
                    .children(&mut child.walk())
                    .find(|c| {
                        matches!(
                            c.kind(),
                            "primitive_type" | "type_identifier" | "qualified_identifier"
                        )
                    })
                    .map(|type_node| base.get_node_text(&type_node))
                    .unwrap_or_else(|| base.get_node_text(child));
            }
        }
    }

    String::new()
}

/// Extract function parameters as string
pub(super) fn extract_function_parameters(base: &mut BaseExtractor, func_node: Node) -> String {
    if let Some(param_list) = func_node
        .children(&mut func_node.walk())
        .find(|c| c.kind() == "parameter_list")
    {
        base.get_node_text(&param_list)
    } else {
        "()".to_string()
    }
}

/// Check if function has const qualifier
pub(super) fn extract_const_qualifier(func_node: Node) -> bool {
    func_node
        .children(&mut func_node.walk())
        .any(|c| c.kind() == "type_qualifier")
}

/// Extract noexcept specifier
pub(super) fn extract_noexcept_specifier(base: &mut BaseExtractor, func_node: Node) -> String {
    for child in func_node.children(&mut func_node.walk()) {
        if child.kind() == "noexcept" {
            return base.get_node_text(&child);
        }
    }
    String::new()
}
