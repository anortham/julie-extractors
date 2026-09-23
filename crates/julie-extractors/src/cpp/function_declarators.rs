use crate::base::BaseExtractor;
use tree_sitter::Node;

/// GoogleTest test-declaration macros. Each parses as a `function_definition`
/// whose declarator identifier is the macro keyword and whose two "parameters"
/// are the suite/fixture name and the test name.
pub(super) const GTEST_MACROS: &[&str] =
    &["TEST", "TEST_F", "TEST_P", "TYPED_TEST", "TYPED_TEST_P"];

/// Boost.Test case macros. Each parses as a `function_definition` whose
/// declarator identifier is the macro keyword and whose first "parameter" is the
/// test name; `BOOST_FIXTURE_TEST_CASE` names its fixture second.
pub(super) const BOOST_TEST_CASE_MACROS: &[&str] =
    &["BOOST_AUTO_TEST_CASE", "BOOST_FIXTURE_TEST_CASE"];

/// The test name of a Boost.Test case macro invocation.
pub(super) fn boost_test_case_name(
    base: &BaseExtractor,
    func_node: Node,
    macro_name: &str,
) -> Option<String> {
    BOOST_TEST_CASE_MACROS
        .contains(&macro_name)
        .then(|| first_parameter_type(func_node))
        .flatten()
        .map(|name| base.get_node_text(&name))
}

/// The test name a Boost.Test case macro writes where the grammar expects a
/// parameter type: a name, never a type use.
pub(super) fn is_test_macro_name(base: &BaseExtractor, node: Node) -> bool {
    let Some(func_node) = node
        .parent()
        .and_then(|parameter| parameter.parent())
        .and_then(|parameters| parameters.parent())
        .filter(|func_node| func_node.kind() == "function_declarator")
    else {
        return false;
    };
    func_node
        .child_by_field_name("declarator")
        .is_some_and(|name| BOOST_TEST_CASE_MACROS.contains(&base.get_node_text(&name).as_str()))
        && first_parameter_type(func_node).is_some_and(|name| name.id() == node.id())
}

fn first_parameter_type(func_node: Node) -> Option<Node> {
    let parameters = func_node.child_by_field_name("parameters")?;
    let mut cursor = parameters.walk();
    let first = parameters
        .named_children(&mut cursor)
        .find(|child| child.kind() == "parameter_declaration")?;
    first
        .child_by_field_name("type")
        .filter(|name| name.kind() == "type_identifier")
}

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
