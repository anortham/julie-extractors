//! The names C++ scopes bind, so an unqualified call can be traced to the
//! declaration it reaches: a local, a parameter, a capture, a class member, or
//! a namespace member.

use super::declarators::{declarator_target, declared_names};
use crate::base::BaseExtractor;
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};
use tree_sitter::Node;

/// Names a declaration-like node introduces into its scope: objects, types,
/// aliases, using-declarations, and enumerators. Function names are included
/// only when `functions` is set.
pub(super) fn declaration_names(
    base: &BaseExtractor,
    node: Node,
    functions: bool,
    names: &mut Vec<String>,
) {
    declaration_names_at(base, node, functions, names, 0);
}

fn declaration_names_at(
    base: &BaseExtractor,
    node: Node,
    functions: bool,
    names: &mut Vec<String>,
    depth: u32,
) {
    if !should_visit_tree_depth(depth) {
        return;
    }
    match node.kind() {
        "declaration" | "field_declaration" | "type_definition" | "function_definition" => {
            if let Some(type_node) = node.child_by_field_name("type") {
                specifier_names(base, type_node, names);
            }
            let keeps_functions = functions || node.kind() == "type_definition";
            let mut cursor = node.walk();
            for declarator in node.children_by_field_name("declarator", &mut cursor) {
                let is_function =
                    declarator_target(declarator).is_some_and(|target| target.function.is_some());
                if keeps_functions || !is_function {
                    names.extend(bound_names(base, declarator));
                }
            }
        }
        "alias_declaration" => {
            names.extend(
                node.child_by_field_name("name")
                    .map(|name| base.get_node_text(&name)),
            );
        }
        "using_declaration" => names.extend(using_declaration_name(base, node)),
        "class_specifier" | "struct_specifier" | "union_specifier" | "enum_specifier" => {
            specifier_names(base, node, names);
        }
        "template_declaration" => {
            let Some(child_depth) = child_tree_depth(depth) else {
                return;
            };
            let mut cursor = node.walk();
            for child in node.named_children(&mut cursor) {
                declaration_names_at(base, child, functions, names, child_depth);
            }
        }
        _ => {}
    }
}

/// The tag name of a class, struct, union, or enum specifier, and the
/// enumerators an unscoped enum puts in the enclosing scope.
fn specifier_names(base: &BaseExtractor, node: Node, names: &mut Vec<String>) {
    if !matches!(
        node.kind(),
        "class_specifier" | "struct_specifier" | "union_specifier" | "enum_specifier"
    ) {
        return;
    }
    names.extend(
        node.child_by_field_name("name")
            .map(|name| base.get_node_text(&name)),
    );
    let scoped = node
        .children(&mut node.walk())
        .any(|child| matches!(child.kind(), "class" | "struct"));
    if node.kind() != "enum_specifier" || scoped {
        return;
    }
    let Some(body) = node.child_by_field_name("body") else {
        return;
    };
    let mut cursor = body.walk();
    names.extend(
        body.named_children(&mut cursor)
            .filter(|child| child.kind() == "enumerator")
            .filter_map(|enumerator| enumerator.child_by_field_name("name"))
            .map(|name| base.get_node_text(&name)),
    );
}

/// The name `using ns::name;` brings into scope. A using-directive
/// (`using namespace ns;`) names no single entity.
fn using_declaration_name(base: &BaseExtractor, node: Node) -> Option<String> {
    if node
        .children(&mut node.walk())
        .any(|child| child.kind() == "namespace")
    {
        return None;
    }
    let mut name = node.named_child(0)?;
    while name.kind() == "qualified_identifier" {
        name = name.child_by_field_name("name")?;
    }
    Some(base.get_node_text(&name))
}

fn bound_names(base: &BaseExtractor, declarator: Node) -> Vec<String> {
    let declarator = if declarator.kind() == "variadic_declarator" {
        match declarator.named_child(0) {
            Some(inner) => inner,
            None => return Vec::new(),
        }
    } else {
        declarator
    };
    declared_names(declarator)
        .iter()
        .map(|name| base.get_node_text(name))
        .collect()
}

/// Whether a function, lambda, or block around `node` binds `name` before
/// namespace scope: a parameter, a lambda capture, a range-for variable, a
/// condition variable, or a local declaration. Every declaration in an
/// enclosing block counts, even one after `node`.
pub(super) fn binds_locally(base: &BaseExtractor, node: Node, name: &str) -> bool {
    let mut current = node.parent();
    while let Some(scope) = current {
        let binds = match scope.kind() {
            "translation_unit" | "declaration_list" => return false,
            "function_definition" => scope
                .child_by_field_name("declarator")
                .and_then(|declarator| declarator_target(declarator)?.function)
                .is_some_and(|function| parameters_bind(base, function, name)),
            "lambda_expression" => {
                captures_bind(base, scope, name)
                    || scope
                        .child_by_field_name("declarator")
                        .is_some_and(|declarator| parameters_bind(base, declarator, name))
            }
            "catch_clause" => parameters_bind(base, scope, name),
            "for_range_loop" => {
                scope
                    .child_by_field_name("declarator")
                    .is_some_and(|declarator| {
                        bound_names(base, declarator).iter().any(|n| n == name)
                    })
                    || statements_bind(base, scope, name, 0)
            }
            "compound_statement" | "if_statement" | "while_statement" | "for_statement"
            | "switch_statement" | "case_statement" | "condition_clause" | "init_statement" => {
                statements_bind(base, scope, name, 0)
            }
            _ => false,
        };
        if binds {
            return true;
        }
        current = scope.parent();
    }
    false
}

fn statements_bind(base: &BaseExtractor, scope: Node, name: &str, depth: u32) -> bool {
    if !should_visit_tree_depth(depth) {
        return false;
    }
    let mut cursor = scope.walk();
    scope
        .named_children(&mut cursor)
        .any(|child| match child.kind() {
            "condition_clause" | "init_statement" => child_tree_depth(depth)
                .is_some_and(|child_depth| statements_bind(base, child, name, child_depth)),
            // A block cannot define a function or a template. The parser puts
            // them there only when a macro such as `NS_BEGIN namespace x {`
            // turns the namespace into a function body.
            "function_definition" | "template_declaration" => false,
            _ => {
                let mut names = Vec::new();
                declaration_names(base, child, true, &mut names);
                names.iter().any(|bound| bound == name)
            }
        })
}

fn parameters_bind(base: &BaseExtractor, holder: Node, name: &str) -> bool {
    let Some(parameters) = holder.child_by_field_name("parameters") else {
        return false;
    };
    let mut cursor = parameters.walk();
    parameters.named_children(&mut cursor).any(|parameter| {
        parameter
            .child_by_field_name("declarator")
            .is_some_and(|declarator| bound_names(base, declarator).iter().any(|n| n == name))
    })
}

fn captures_bind(base: &BaseExtractor, lambda: Node, name: &str) -> bool {
    let Some(captures) = lambda.child_by_field_name("captures") else {
        return false;
    };
    let mut cursor = captures.walk();
    captures
        .named_children(&mut cursor)
        .filter_map(|capture| match capture.kind() {
            "identifier" => Some(capture),
            "lambda_capture_initializer" => capture.child_by_field_name("left"),
            _ => None,
        })
        .any(|capture| base.get_node_text(&capture) == name)
}

/// The namespace path (`a::b`, or empty for the global namespace) whose
/// members an unqualified name at `node` sees first. Anonymous and inline
/// namespaces add no segment: their members are visible in the enclosing one.
pub(super) fn namespace_path(base: &BaseExtractor, node: Node) -> String {
    let mut segments = Vec::new();
    let mut current = node.parent();
    while let Some(scope) = current {
        let is_inline = scope
            .children(&mut scope.walk())
            .any(|child| child.kind() == "inline");
        if scope.kind() == "namespace_definition"
            && !is_inline
            && let Some(name) = scope.child_by_field_name("name")
        {
            let text: String = base
                .get_node_text(&name)
                .chars()
                .filter(|c| !c.is_whitespace())
                .collect();
            segments.push(text);
        }
        current = scope.parent();
    }
    segments.reverse();
    segments.join("::")
}

/// A namespace path and each namespace that encloses it, innermost first,
/// ending with the global namespace.
pub(super) fn enclosing_namespaces(namespace: &str) -> Vec<&str> {
    let mut chain = vec![namespace];
    let mut rest = namespace;
    while let Some((outer, _)) = rest.rsplit_once("::") {
        chain.push(outer);
        rest = outer;
    }
    if !namespace.is_empty() {
        chain.push("");
    }
    chain
}
