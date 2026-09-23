//! EUnit fixture roles. A `*_test_` generator returns fixture tuples such as
//! `{setup, fun start/0, fun stop/1, [fun put_then_get/0]}`; the funs they
//! name run as the fixture's setup, its cleanup, and its tests.

use std::collections::HashMap;

use tree_sitter::Node;

use super::ErlangExtractor;
use super::definition_forms;
use super::helpers::{NameArity, named_children, unquote_atom};
use crate::base::TestRole;
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};

/// Roles for the funs named by the fixture tuples of every `*_test_`
/// generator in the file.
pub(super) fn eunit_fixture_roles(
    extractor: &ErlangExtractor,
    declarations: &[Node],
) -> HashMap<NameArity, TestRole> {
    let mut roles = HashMap::new();
    for declaration in declarations {
        let is_generator = declaration.kind() == "fun_decl"
            && definition_forms::function_clause(extractor, declaration).is_some_and(|clause| {
                clause.identity.1 == 0 && clause.identity.0.ends_with("_test_")
            });
        if is_generator {
            collect(extractor, *declaration, &mut roles, 0);
        }
    }
    roles
}

fn collect(
    extractor: &ErlangExtractor,
    node: Node,
    roles: &mut HashMap<NameArity, TestRole>,
    depth: u32,
) {
    if !should_visit_tree_depth(depth) {
        return;
    }
    if node.kind() == "tuple" {
        classify_fixture(extractor, &node, roles);
    }
    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };
    for child in named_children(&node) {
        collect(extractor, child, roles, child_depth);
    }
}

/// `{setup | foreach, [Where,] Setup, [Cleanup,] Tests}`: `Where` is an atom
/// (`spawn`, `local`) and the funs in the last element are tests.
fn classify_fixture(
    extractor: &ErlangExtractor,
    tuple: &Node,
    roles: &mut HashMap<NameArity, TestRole>,
) {
    let elements = named_children(tuple);
    let Some(tag) = elements.first().filter(|tag| tag.kind() == "atom") else {
        return;
    };
    if !matches!(
        unquote_atom(&extractor.base.get_node_text(tag)).as_str(),
        "setup" | "foreach"
    ) {
        return;
    }
    let mut rest = &elements[1..];
    if rest.len() >= 3 && rest[0].kind() == "atom" {
        rest = &rest[1..];
    }
    let Some((tests, hooks)) = rest.split_last() else {
        return;
    };
    let hook_roles = [TestRole::FixtureSetup, TestRole::FixtureTeardown];
    for (hook, role) in hooks.iter().zip(hook_roles) {
        if let Some(identity) = fun_reference(extractor, hook) {
            roles.entry(identity).or_insert(role);
        }
    }
    let test_funs = if tests.kind() == "list" {
        named_children(tests)
    } else {
        vec![*tests]
    };
    for test in test_funs {
        if let Some(identity) = fun_reference(extractor, &test) {
            roles.entry(identity).or_insert(TestRole::TestCase);
        }
    }
}

/// The `name/arity` of `fun name/arity`.
fn fun_reference(extractor: &ErlangExtractor, node: &Node) -> Option<NameArity> {
    if node.kind() != "internal_fun" {
        return None;
    }
    let name = node
        .child_by_field_name("fun")
        .filter(|fun| fun.kind() == "atom")?;
    let arity = node
        .child_by_field_name("arity")?
        .child_by_field_name("value")?;
    Some((
        unquote_atom(&extractor.base.get_node_text(&name)),
        extractor.base.get_node_text(&arity).trim().parse().ok()?,
    ))
}
