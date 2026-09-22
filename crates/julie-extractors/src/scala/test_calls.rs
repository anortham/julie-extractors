//! Scala ScalaTest / MUnit call-style test extraction.
//!
//! ScalaTest and MUnit express tests as call/infix expressions, not named
//! methods, so the declaration-walking extractor misses them. Two grammar
//! shapes, both verified against tree-sitter-scala 0.25 `node-types.json` + an
//! AST probe:
//!
//! 1. Curried call form — FunSuite / MUnit `test("n") { }`, FunSpec
//!    `describe("n") { it("m") { } }`:
//!    ```text
//!    call_expression
//!      [function] call_expression          <- the f("n") clause
//!        [function] identifier  'test'      <- callee (vocab)
//!        [arguments] arguments -> string    <- description
//!      [arguments] block                    <- the { } body
//!    ```
//! 2. FlatSpec / WordSpec infix form — `"subject" should "behaviour" in { }`:
//!    ```text
//!    infix_expression  [operator] 'in'  [right] block
//!      [left] infix_expression  [operator] 'should'|'must'|'can'|'will'
//!        [left] string   <- subject
//!        [right] string  <- behaviour
//!    ```
//!
//! Only the grammar walking is Scala-local; classification + symbol construction
//! delegate to the shared `crate::test_calls` core so the captured `is_test` /
//! `test_container` metadata is byte-identical to every other call-style path.

use crate::base::{BaseExtractor, Symbol};
use crate::test_calls::{
    TestCallCategory, TestCallVocab, build_test_call_symbol, classify_call_exact,
};
use tree_sitter::Node;

/// ScalaTest / MUnit vocabulary.
/// - `test` (FunSuite, MUnit), `it` (FunSpec / WordSpec / FlatSpec result),
///   `scenario` (FeatureSpec) are test cases.
/// - `describe` / `context` (FunSpec), `feature` (FeatureSpec) are containers.
/// - Scala lifecycle hooks (`beforeEach` / `afterAll`) are METHOD OVERRIDES
///   (`def`, caught by the declaration path), not calls, so the call-style
///   lifecycle slice is empty.
pub(crate) const SCALA_VOCAB: TestCallVocab = TestCallVocab {
    test: &["test", "it", "scenario", "Scenario", "ignore"],
    container: &["describe", "context", "feature", "Feature"],
    lifecycle: &[],
};

/// WordSpec and specs2 verbs that open a group: `"subject" when { ... }`.
const WORDSPEC_GROUP_VERBS: &[&str] = &["when", "should", "must", "can"];

/// FlatSpec / WordSpec behaviour verbs introducing a test clause
/// (`"subject" should "behaviour" in { ... }`).
const FLATSPEC_VERBS: &[&str] = &["should", "must", "can", "will"];

/// Materialize a curried-call ScalaTest/MUnit DSL call (`test("n") { }`,
/// `describe("n") { }`, `it("m") { }`) as a test/container symbol. Returns
/// `None` for any `call_expression` that is not a recognized DSL test clause, so
/// the caller can invoke it for every `call_expression`.
pub(super) fn extract_scala_test_call(
    base: &mut BaseExtractor,
    node: &Node,
    parent_id: Option<&str>,
) -> Option<Symbol> {
    if node.kind() != "call_expression" {
        return None;
    }
    // The brace body is the OUTER call's `arguments` (a `block`); the callee +
    // description live in the OUTER call's `function`, which is the INNER
    // `f("name")` call_expression. A call without a block body (the inner
    // `test("name")` itself, or `assert(x)`) is not a DSL test clause.
    let body = node.child_by_field_name("arguments")?;
    if body.kind() != "block" {
        return None;
    }
    let inner = node.child_by_field_name("function")?;
    if inner.kind() != "call_expression" {
        return None;
    }
    let callee_node = inner.child_by_field_name("function")?;
    // An MUnit fixture runs its cases through `fixture.test("n") { }`.
    let callee = match callee_node.kind() {
        "field_expression" => callee_node
            .child_by_field_name("field")
            .map(|field| base.get_node_text(&field))
            .filter(|field| field == "test")
            .unwrap_or_else(|| base.get_node_text(&callee_node)),
        _ => base.get_node_text(&callee_node),
    };
    // Exact match only (#66): a curried member call (`feature.enable("x") { }`,
    // inner callee = `field_expression` "feature.enable") never equals a dotless
    // ScalaTest/MUnit clause name, so the exact-matcher rejects it without the
    // JS-only leading-segment split.
    let category = classify_call_exact(&callee, &SCALA_VOCAB)?;

    // Description = the first argument's string, also through MUnit options
    // such as `"n".ignore` and `"n".tag(Slow)`.
    let inner_args = inner.child_by_field_name("arguments")?;
    let string_node = description_string(inner_args.named_child(0)?)?;
    let name = base.decode_string_literal(&string_node)?;

    Some(build_test_call_symbol(
        base, node, &callee, name, category, parent_id,
    ))
}

/// The string a test description argument starts from.
fn description_string(node: Node) -> Option<Node> {
    match node.kind() {
        "string" => Some(node),
        "field_expression" => description_string(node.child_by_field_name("value")?),
        "call_expression" => description_string(node.child_by_field_name("function")?),
        _ => None,
    }
}

/// Materialize an infix ScalaTest / specs2 clause. Returns `None` for every
/// other infix expression; each form needs a string on the left and a block
/// on the right, which ordinary arithmetic and comparisons never have.
///
/// - FlatSpec `"subject" should "behaviour" in { }` is a case named
///   `"subject should behaviour"`.
/// - WordSpec, FreeSpec and specs2 `"name" in { }` is a case named `"name"`.
/// - WordSpec and specs2 `"subject" when { }` (also `should`, `must`, `can`)
///   is a group named `"subject when"`.
/// - FreeSpec `"subject" - { }` is a group named `"subject"`.
pub(super) fn extract_scala_flatspec_test(
    base: &mut BaseExtractor,
    node: &Node,
    parent_id: Option<&str>,
) -> Option<Symbol> {
    if node.kind() != "infix_expression" {
        return None;
    }
    let op = node.child_by_field_name("operator")?;
    let operator = base.get_node_text(&op);
    let body = node.child_by_field_name("right")?;
    if body.kind() != "block" {
        return None;
    }
    let left = node.child_by_field_name("left")?;

    if left.kind() == "string" {
        let subject = base.decode_string_literal(&left)?;
        let (callee, name, category) = match operator.as_str() {
            "in" => ("in".to_string(), subject, TestCallCategory::Test),
            "-" => ("minus".to_string(), subject, TestCallCategory::Container),
            verb if WORDSPEC_GROUP_VERBS.contains(&verb) => (
                verb.to_string(),
                format!("{subject} {verb}"),
                TestCallCategory::Container,
            ),
            _ => return None,
        };
        return Some(build_test_call_symbol(
            base, node, &callee, name, category, parent_id,
        ));
    }

    if operator != "in" || left.kind() != "infix_expression" {
        return None;
    }
    let verb_node = left.child_by_field_name("operator")?;
    let verb = base.get_node_text(&verb_node);
    if !FLATSPEC_VERBS.contains(&verb.as_str()) {
        return None;
    }
    let subject_node = left.child_by_field_name("left")?;
    let behaviour_node = left.child_by_field_name("right")?;
    let subject = base
        .decode_string_literal(&subject_node)
        .unwrap_or_else(|| base.get_node_text(&subject_node));
    let behaviour = base
        .decode_string_literal(&behaviour_node)
        .unwrap_or_else(|| base.get_node_text(&behaviour_node));
    let name = format!("{subject} {verb} {behaviour}");

    Some(build_test_call_symbol(
        base,
        node,
        &verb,
        name,
        TestCallCategory::Test,
        parent_id,
    ))
}
