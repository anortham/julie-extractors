use crate::base::BaseExtractor;
use tree_sitter::Node;

/// GoogleTest test-declaration macros. Each parses as a `function_definition`
/// whose declarator identifier is the macro keyword and whose two "parameters"
/// are the suite/fixture name and the test name.
pub(super) const GTEST_MACROS: &[&str] =
    &["TEST", "TEST_F", "TEST_P", "TYPED_TEST", "TYPED_TEST_P"];

/// If `func_node` (a `function_declarator`) is a GoogleTest macro invocation
/// (`TEST(Suite, Name)`, `TEST_F(Fixture, Name)`, ...), return the synthesized
/// `Suite.Name` symbol name; otherwise `None`.
pub(super) fn googletest_suite_dot_name(
    base: &BaseExtractor,
    func_node: Node,
    macro_name: &str,
) -> Option<String> {
    if !GTEST_MACROS.contains(&macro_name) {
        return None;
    }
    let params = func_node.child_by_field_name("parameters")?;
    let mut cursor = params.walk();
    let names: Vec<String> = params
        .named_children(&mut cursor)
        .filter(|c| c.kind() == "parameter_declaration")
        .filter_map(|p| p.child_by_field_name("type"))
        .filter(|t| t.kind() == "type_identifier")
        .map(|t| base.get_node_text(&t))
        .collect();
    match names.as_slice() {
        [suite, test] => Some(format!("{suite}.{test}")),
        _ => None,
    }
}

/// Descend through `pointer_declarator`/`reference_declarator` wrappers to the
/// inner `function_declarator`.
pub(super) fn unwrap_to_function_declarator(node: Node) -> Option<Node> {
    let mut current = node;
    loop {
        match current.kind() {
            "function_declarator" => return Some(current),
            "pointer_declarator" | "reference_declarator" => {
                let next = current.child_by_field_name("declarator").or_else(|| {
                    current.children(&mut current.walk()).find(|c| {
                        matches!(
                            c.kind(),
                            "function_declarator" | "pointer_declarator" | "reference_declarator"
                        )
                    })
                });
                current = next?;
            }
            _ => return None,
        }
    }
}

/// The declaration a `function_declarator` belongs to, reached through the
/// `pointer_declarator`/`reference_declarator` wrappers a pointer or reference
/// return type adds. The member's own modifiers and return type live there; the
/// enclosing class body does not, so the walk stops at the declaration.
pub(super) fn enclosing_declaration(node: Node) -> Option<Node> {
    let mut current = node;
    while let Some(parent) = current.parent() {
        match parent.kind() {
            "field_declaration" | "declaration" => return Some(parent),
            "pointer_declarator" | "reference_declarator" => current = parent,
            _ => return None,
        }
    }
    None
}
