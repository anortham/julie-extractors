//! C test extraction: Criterion test macros, Check `START_TEST` blocks, the
//! call sites that register CMocka, Unity, and Check tests and hooks, and the
//! Unity naming rules of a file that includes `unity.h`.
//!
//! Criterion declares tests with a macro that the C grammar parses as a *call*,
//! not a function definition:
//!
//! ```c
//! Test(math, addition) {
//!     cr_assert(2 + 2 == 4);
//! }
//! ```
//!
//! tree-sitter-c parses `Test(math, addition)` as a `call_expression` (function
//! field = identifier `Test`, arguments = an `argument_list` of two bare
//! `identifier`s — the suite and the test name) followed by a DETACHED
//! `compound_statement` block (the block is a sibling of the call, not a child).
//! Unlike the JS/Dart string-named DSLs, Criterion's name is built from the two
//! identifier arguments joined `suite.name`. Only that grammar walking is
//! C-local; classification and symbol construction delegate to the shared
//! `crate::test_calls` core so the captured `is_test` metadata is identical across
//! languages and the downstream `classify_symbols_by_role` pass treats Criterion
//! like every other call-style framework.
//!
//! Optional trailing arguments are ignored when building test names.

use crate::base::{
    BaseExtractor, NormalizedSpan, Symbol, SymbolKind, SymbolOptions, TestRole, Visibility,
};
use crate::test_calls::{
    TestCallCategory, TestCallVocab, build_test_call_symbol, build_test_call_symbol_with_block,
    classify_call_exact, detached_macro_block,
};
use crate::test_detection::{apply_test_role, c_lifecycle_role};
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};
use std::collections::HashMap;
use tree_sitter::Node;

pub(crate) const CRITERION_VOCAB: TestCallVocab = TestCallVocab {
    test: &["Test"],
    container: &["TestSuite"],
    lifecycle: &[],
};

/// Materialize a Criterion test or suite call as a test-role symbol.
/// Returns `None` for any call that is not a recognized Criterion test macro
/// (e.g. `cr_assert(...)`, `printf(...)`), so the caller can blindly invoke it for
/// every `call_expression` and only Criterion tests become symbols.
pub fn extract_c_test_call(
    base: &mut BaseExtractor,
    node: &Node,
    parent_id: Option<&str>,
) -> Option<Symbol> {
    if node.kind() != "call_expression" {
        return None;
    }

    let function_node = node.child_by_field_name("function")?;
    let full_callee = base.get_node_text(&function_node);
    let category = classify_call_exact(&full_callee, &CRITERION_VOCAB)?;

    let args_node = node.child_by_field_name("arguments")?;
    let name = match category {
        TestCallCategory::Test => {
            let mut cursor = args_node.walk();
            let identifier_args: Vec<String> = args_node
                .children(&mut cursor)
                .filter(|c| c.kind() == "identifier")
                .take(2)
                .map(|c| base.get_node_text(&c))
                .collect();
            if identifier_args.len() < 2 {
                return None;
            }
            identifier_args.join(".")
        }
        TestCallCategory::Container => first_identifier(base, args_node)?,
        TestCallCategory::Lifecycle => return None,
    };

    Some(match detached_macro_block(node) {
        Some(block) => build_test_call_symbol_with_block(
            base,
            node,
            &block,
            &full_callee,
            name,
            category,
            parent_id,
        ),
        None => build_test_call_symbol(base, node, &full_callee, name, category, parent_id),
    })
}

/// Whether a call is a Criterion test or suite macro, which declares a test
/// rather than calling a function.
pub(super) fn is_criterion_macro_call(base: &BaseExtractor, node: &Node) -> bool {
    node.kind() == "call_expression"
        && node
            .child_by_field_name("function")
            .is_some_and(|function| {
                classify_call_exact(&base.get_node_text(&function), &CRITERION_VOCAB).is_some()
            })
}

/// Give test roles to functions a call site registers: Criterion `.init`/`.fini`
/// hooks, CMocka tests and hooks named in `cmocka_unit_test*` and
/// `cmocka_run_group_tests*` arguments, Unity `RUN_TEST` tests, and Check
/// `tcase_add_*` tests and fixtures. In a file that includes `unity.h`, the
/// functions Unity's runner generator collects by name are tests too.
pub fn apply_call_site_test_roles(base: &BaseExtractor, root: Node, symbols: &mut [Symbol]) {
    let mut lifecycle_names = HashMap::new();
    collect_lifecycle_names(base, root, &mut lifecycle_names, 0);
    let unity = includes_unity(base, root, 0);
    if lifecycle_names.is_empty() && !unity {
        return;
    }

    for symbol in symbols {
        if !matches!(symbol.kind, SymbolKind::Function | SymbolKind::Method) {
            continue;
        }
        let Some(role) = lifecycle_names
            .get(&symbol.name)
            .copied()
            .or_else(|| unity.then(|| unity_role(symbol)).flatten())
        else {
            continue;
        };
        apply_test_role(symbol.metadata.get_or_insert_with(Default::default), role);
    }
}

/// Unity's `generate_test_runner.rb` collects each `void` function whose name
/// starts with `test`, `spec`, or `should` as a test, and calls `setUp`,
/// `tearDown`, `suiteSetUp`, and `suiteTearDown` around them. Its pattern
/// does not match a `static` definition, which the runner could not call.
const UNITY_TEST_PREFIXES: [&str; 3] = ["test", "spec", "should"];

fn unity_role(symbol: &Symbol) -> Option<TestRole> {
    let metadata = symbol.metadata.as_ref()?;
    let text = |key: &str| metadata.get(key).and_then(|value| value.as_str());
    if symbol.parent_id.is_some() || text("isDefinition") != Some("true") {
        return None;
    }
    c_lifecycle_role(&symbol.name).or_else(|| {
        (UNITY_TEST_PREFIXES
            .iter()
            .any(|prefix| symbol.name.starts_with(prefix))
            && text("returnType") == Some("void")
            && text("isStatic") == Some("false"))
        .then_some(TestRole::TestCase)
    })
}

fn check_test_name<'tree>(base: &BaseExtractor, node: &Node<'tree>) -> Option<Node<'tree>> {
    let declarator = node.child_by_field_name("declarator")?;
    let only_child = |parent: Node<'tree>| {
        (parent.named_child_count() == 1)
            .then(|| parent.named_child(0))
            .flatten()
    };
    match declarator.kind() {
        "parenthesized_declarator"
            if base.get_node_text(&node.child_by_field_name("type")?) == "START_TEST" =>
        {
            only_child(declarator).filter(|name| name.kind() == "identifier")
        }
        "function_declarator"
            if base.get_node_text(&declarator.child_by_field_name("declarator")?)
                == "START_TEST" =>
        {
            let parameter = only_child(declarator.child_by_field_name("parameters")?)
                .filter(|parameter| parameter.kind() == "parameter_declaration")?;
            if parameter.child_by_field_name("declarator").is_some() {
                return None;
            }
            parameter
                .child_by_field_name("type")
                .filter(|name| name.kind() == "type_identifier")
        }
        _ => None,
    }
}

/// Whether the file includes Unity's header, `unity.h` in any folder, at the
/// top level or inside a conditional block.
fn includes_unity(base: &BaseExtractor, node: Node, depth: u32) -> bool {
    if !should_visit_tree_depth(depth) {
        return false;
    }
    if node.kind() == "preproc_include" {
        return node.child_by_field_name("path").is_some_and(|path| {
            let text = base.get_node_text(&path);
            let header = text.trim_matches(|ch| matches!(ch, '"' | '<' | '>'));
            header.rsplit(['/', '\\']).next() == Some("unity.h")
        });
    }
    if !(node.kind() == "translation_unit" || node.kind().starts_with("preproc_")) {
        return false;
    }
    let Some(child_depth) = child_tree_depth(depth) else {
        return false;
    };
    let mut cursor = node.walk();
    node.children(&mut cursor)
        .any(|child| includes_unity(base, child, child_depth))
}

/// Whether a token is part of Check's test macros rather than code: the
/// `START_TEST(name)` header of a test, or an `END_TEST` that the grammar
/// read as the type of the next function.
pub(super) fn is_check_macro_token(base: &BaseExtractor, node: Node) -> bool {
    if !matches!(node.kind(), "identifier" | "type_identifier") {
        return false;
    }
    if node.kind() == "type_identifier"
        && base.get_node_text(&node) == "END_TEST"
        && node
            .next_sibling()
            .is_some_and(|next| next.kind() == "ERROR")
    {
        return true;
    }
    let mut current = node.parent();
    while let Some(ancestor) = current {
        match ancestor.kind() {
            "function_definition" => return check_test_name(base, &ancestor).is_some(),
            "compound_statement" | "translation_unit" => return false,
            _ => current = ancestor.parent(),
        }
    }
    false
}

/// Check declares a test as `START_TEST(name) { ... } END_TEST`, and the
/// macro expands to a `static` function. The grammar parses the first test of
/// a file as a `function_definition` of the type `START_TEST` whose declarator
/// is the parenthesized name. After an `END_TEST` it parses a function
/// `START_TEST` of the type `END_TEST` whose one parameter type is the name.
pub(super) fn extract_check_test(
    base: &mut BaseExtractor,
    node: &Node,
    parent_id: Option<&str>,
) -> Option<Symbol> {
    let name = base.get_node_text(&check_test_name(base, node)?);
    let mut metadata = HashMap::from([
        (
            "type".to_string(),
            serde_json::Value::String("function".to_string()),
        ),
        ("name".to_string(), serde_json::Value::String(name.clone())),
        (
            "isDefinition".to_string(),
            serde_json::Value::String("true".to_string()),
        ),
    ]);
    apply_test_role(&mut metadata, TestRole::TestCase);
    let doc_comment = base.find_doc_comment(node);
    let start = node
        .child_by_field_name("declarator")
        .filter(|declarator| declarator.kind() == "function_declarator")
        .map_or(node.start_byte(), |declarator| declarator.start_byte());
    let span = NormalizedSpan::from_content_range_with_line_starts(
        &base.content,
        base.line_starts(),
        start,
        node.end_byte(),
    )
    .unwrap_or_else(|| NormalizedSpan::from_node(node));
    Some(base.create_symbol_from_span(
        node,
        span,
        name.clone(),
        SymbolKind::Function,
        SymbolOptions {
            signature: Some(format!("START_TEST({name})")),
            visibility: Some(Visibility::Private),
            parent_id: parent_id.map(str::to_string),
            metadata: Some(metadata),
            doc_comment,
            annotations: Vec::new(),
        },
    ))
}

fn collect_lifecycle_names(
    base: &BaseExtractor,
    node: Node,
    names: &mut HashMap<String, TestRole>,
    depth: u32,
) {
    if !should_visit_tree_depth(depth) {
        return;
    }

    if node.kind() == "call_expression"
        && let Some(function) = node.child_by_field_name("function")
        && let Some(arguments) = node.child_by_field_name("arguments")
    {
        let callee = base.get_node_text(&function);
        if matches!(callee.as_str(), "Test" | "TestSuite") {
            collect_designated_lifecycle_names(base, arguments, names);
        } else if let Some((_, roles)) = REGISTRATION_CALLS
            .iter()
            .find(|(macro_name, _)| *macro_name == callee)
        {
            collect_positional_roles(base, arguments, roles, names);
        }
    }

    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_lifecycle_names(base, child, names, child_depth);
    }
}

/// Criterion names the setup hook `.init` and the teardown hook `.fini` in the
/// test macro's designated initializers, so the hook's direction is on the call
/// site, not in the hook function's own name.
const CRITERION_LIFECYCLE_MARKERS: [(&str, TestRole); 2] = [
    (".init", TestRole::FixtureSetup),
    (".fini", TestRole::FixtureTeardown),
];

/// The role a test framework gives each argument of a registration call, by
/// position: CMocka `cmocka_*`, Unity `RUN_TEST(test)` and
/// `RUN_TEST(test, line)`, and Check `tcase_add_*`.
const REGISTRATION_CALLS: [(&str, &[Option<TestRole>]); 15] = [
    ("RUN_TEST", &[Some(TestRole::TestCase)]),
    ("tcase_add_test", &[None, Some(TestRole::TestCase)]),
    ("tcase_add_loop_test", &[None, Some(TestRole::TestCase)]),
    (
        "tcase_add_test_raise_signal",
        &[None, Some(TestRole::TestCase)],
    ),
    ("tcase_add_exit_test", &[None, Some(TestRole::TestCase)]),
    (
        "tcase_add_checked_fixture",
        &[
            None,
            Some(TestRole::FixtureSetup),
            Some(TestRole::FixtureTeardown),
        ],
    ),
    (
        "tcase_add_unchecked_fixture",
        &[
            None,
            Some(TestRole::FixtureSetup),
            Some(TestRole::FixtureTeardown),
        ],
    ),
    ("cmocka_unit_test", &[Some(TestRole::TestCase)]),
    (
        "cmocka_unit_test_setup",
        &[Some(TestRole::TestCase), Some(TestRole::FixtureSetup)],
    ),
    (
        "cmocka_unit_test_teardown",
        &[Some(TestRole::TestCase), Some(TestRole::FixtureTeardown)],
    ),
    (
        "cmocka_unit_test_setup_teardown",
        &[
            Some(TestRole::TestCase),
            Some(TestRole::FixtureSetup),
            Some(TestRole::FixtureTeardown),
        ],
    ),
    ("cmocka_unit_test_prestate", &[Some(TestRole::TestCase)]),
    (
        "cmocka_unit_test_prestate_setup_teardown",
        &[
            Some(TestRole::TestCase),
            Some(TestRole::FixtureSetup),
            Some(TestRole::FixtureTeardown),
        ],
    ),
    (
        "cmocka_run_group_tests",
        &[
            None,
            Some(TestRole::FixtureSetup),
            Some(TestRole::FixtureTeardown),
        ],
    ),
    (
        "cmocka_run_group_tests_name",
        &[
            None,
            None,
            Some(TestRole::FixtureSetup),
            Some(TestRole::FixtureTeardown),
        ],
    ),
];

fn collect_positional_roles(
    base: &BaseExtractor,
    arguments: Node,
    roles: &[Option<TestRole>],
    names: &mut HashMap<String, TestRole>,
) {
    let mut cursor = arguments.walk();
    for (argument, role) in arguments.named_children(&mut cursor).zip(roles) {
        if let Some(role) = role
            && argument.kind() == "identifier"
        {
            names.insert(base.get_node_text(&argument), *role);
        }
    }
}

fn collect_designated_lifecycle_names(
    base: &BaseExtractor,
    node: Node,
    names: &mut HashMap<String, TestRole>,
) {
    let source = base.get_node_text(&node);
    for (marker, role) in CRITERION_LIFECYCLE_MARKERS {
        let mut offset = 0;
        while let Some(relative) = source[offset..].find(marker) {
            let marker_end = offset + relative + marker.len();
            let after_marker = source[marker_end..].trim_start();
            let Some(after_equals) = after_marker.strip_prefix('=') else {
                offset = marker_end;
                continue;
            };
            let hook = after_equals.trim_start();
            let hook_len = hook
                .chars()
                .take_while(|ch| ch.is_ascii_alphanumeric() || *ch == '_')
                .map(char::len_utf8)
                .sum::<usize>();
            if hook_len > 0 {
                names.insert(hook[..hook_len].to_string(), role);
            }
            offset = marker_end;
        }
    }
}

fn first_identifier(base: &BaseExtractor, node: Node) -> Option<String> {
    first_identifier_at_depth(base, node, 0)
}

fn first_identifier_at_depth(base: &BaseExtractor, node: Node, depth: u32) -> Option<String> {
    if !should_visit_tree_depth(depth) {
        return None;
    }
    if node.kind() == "identifier" {
        return Some(base.get_node_text(&node));
    }

    let child_depth = child_tree_depth(depth)?;
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if let Some(identifier) = first_identifier_at_depth(base, child, child_depth) {
            return Some(identifier);
        }
    }
    None
}
