//! Test-symbol detection for the JavaScript family (js, jsx, ts, tsx).
//!
//! Two entry points, both shared by [`crate::javascript::JavaScriptExtractor`]
//! (js/jsx) and [`crate::typescript::TypeScriptExtractor`] (ts/tsx):
//!
//! - [`extract_test_call`] classifies a `call_expression` written in a test DSL
//!   (`describe(...)`, `test.beforeEach(...)`, `it.each([...])(...)`).
//! - [`apply_declared_test_metadata`] classifies a declared function or method,
//!   including decorator frameworks such as testdeck.
//!
//! Every role write goes through [`apply_test_role`] or
//! [`apply_callable_test_metadata`], so the boolean flags and the `test_role`
//! string can never disagree.
//!
//! ## Why detection is gated
//!
//! The DSL vocabulary overlaps ordinary production names — `setup`, `teardown`,
//! `before`, `after`, `context`, `suite`. A bare call to one of those only means
//! a test when the file is a test file, so [`test_dsl_is_active`] requires either
//! a test file path or an import of a known test framework.

use crate::base::{BaseExtractor, Symbol, SymbolKind, TestRole};
use crate::test_calls::{TestCallCategory, build_test_call_symbol};
use crate::test_detection::{apply_callable_test_metadata, apply_test_role, is_test_symbol};
use std::collections::HashMap;
use tree_sitter::Node;

/// Callee segments that change how a test runs, never what it is.
///
/// Dropping them collapses `describe.only`, `it.skip`, and
/// `test.describe.serial` onto the DSL word underneath.
const MODIFIER_SEGMENTS: &[&str] = &[
    "only",
    "skip",
    "todo",
    "failing",
    "fails",
    "concurrent",
    "sequential",
    "serial",
    "parallel",
    "skipIf",
    "runIf",
];

/// Receivers a framework hangs its DSL off: Playwright `test.*`, QUnit
/// `QUnit.*`, node:test subtests `t.*`, and Jest/Vitest chained modifiers.
const NAMESPACE_ROOTS: &[&str] = &["test", "it", "describe", "suite", "QUnit", "t"];

/// DSL words that declare a test case.
const TEST_WORDS: &[&str] = &["it", "test", "specify", "bench", "xit", "fit", "xtest"];

/// DSL words that declare a grouping container.
const CONTAINER_WORDS: &[&str] = &[
    "describe",
    "context",
    "suite",
    "xdescribe",
    "fdescribe",
    "xcontext",
];

/// DSL words that declare a setup hook. Mocha's BDD interface spells them
/// `before*`; its TDD interface uses `setup`/`suiteSetup`.
const SETUP_WORDS: &[&str] = &["beforeEach", "beforeAll", "before", "setup", "suiteSetup"];

/// DSL words that declare a teardown hook.
const TEARDOWN_WORDS: &[&str] = &[
    "afterEach",
    "afterAll",
    "after",
    "teardown",
    "suiteTeardown",
];

/// QUnit spells its container `QUnit.module`. A bare `module(...)` call is
/// CommonJS-adjacent production code, so the word counts only behind a namespace
/// root.
const NAMESPACED_CONTAINER_WORDS: &[&str] = &["module"];

/// The segment that turns a DSL word into a table-driven run: `test.each(table)(name, fn)`.
const TABLE_SEGMENT: &str = "each";

/// Module specifiers only a test file imports.
const TEST_FRAMEWORK_MODULES: &[&str] = &[
    "vitest",
    "jest",
    "mocha",
    "chai",
    "qunit",
    "jasmine",
    "ava",
    "tape",
    "uvu",
    "bun:test",
    "node:test",
    "playwright/test",
    "testdeck",
];

/// Scoped-package and submodule prefixes of the same frameworks.
const TEST_FRAMEWORK_MODULE_PREFIXES: &[&str] = &[
    "@jest/",
    "@playwright/",
    "@vitest/",
    "@testing-library/",
    "@testdeck/",
    "node:test/",
    "uvu/",
];

/// Decorator keys that mark a declared method as a test case (testdeck `@test`,
/// including its `@test.only` / `@test.skip` chained spellings).
const DECORATOR_TEST_CASE_KEYS: &[&str] = &["test"];

/// Decorator keys that mark a declared method as a parameterized test
/// (testdeck `@params`).
const DECORATOR_PARAMETERIZED_KEYS: &[&str] = &["params"];

/// A `call_expression` resolved to a test-DSL declaration.
struct TestCall {
    /// Callee text as written, used for the symbol signature.
    callee: String,
    /// The DSL word the chain resolves to, used to name a lifecycle hook.
    word: String,
    /// Shape of the emitted symbol: which arguments carry the name.
    category: TestCallCategory,
    role: TestRole,
}

/// File extensions that make this extractor the whole story for the file.
const SOURCE_FILE_EXTENSIONS: &[&str] =
    &[".js", ".jsx", ".mjs", ".cjs", ".ts", ".tsx", ".mts", ".cts"];

/// How deep an import can sit and still count as a module-level import.
///
/// `program > lexical_declaration > variable_declarator > call_expression`
/// reaches a top-level `require(...)` in three hops; the fourth is headroom.
const IMPORT_SCAN_DEPTH: u32 = 4;

/// Whether this file may contain test-DSL calls at all.
///
/// Call once per file and cache the verdict.
///
/// A host document that embeds script bodies — an HTML page, for one — reaches
/// this extractor with its own path and loads its framework by a route the
/// script text never shows. That host decides which roles survive, so the guard
/// applies only to standalone JavaScript-family source files.
pub(crate) fn test_dsl_is_active(base: &BaseExtractor, root: Node) -> bool {
    !is_source_file(&base.file_path)
        || is_test_file_path(&base.file_path)
        || imports_test_framework(base, root)
}

fn is_source_file(file_path: &str) -> bool {
    let lowercased = file_path.to_ascii_lowercase();
    SOURCE_FILE_EXTENSIONS
        .iter()
        .any(|extension| lowercased.ends_with(extension))
}

/// Whether the file path alone marks this as a test file.
///
/// The shared path rule lives behind [`is_test_symbol`], which for a JS/TS DSL
/// name reduces to exactly that rule. Probing it here keeps one definition of
/// "test file" instead of a second copy that could drift.
fn is_test_file_path(file_path: &str) -> bool {
    is_test_symbol(
        "javascript",
        "describe",
        file_path,
        &SymbolKind::Function,
        &[],
        None,
    )
}

fn imports_test_framework(base: &BaseExtractor, root: Node) -> bool {
    let mut pending = vec![(root, 0)];
    while let Some((node, depth)) = pending.pop() {
        if let Some(specifier) = import_specifier(base, node)
            && is_test_framework_module(&specifier)
        {
            return true;
        }
        if depth == IMPORT_SCAN_DEPTH {
            continue;
        }
        let mut cursor = node.walk();
        pending.extend(node.children(&mut cursor).map(|child| (child, depth + 1)));
    }
    false
}

fn import_specifier(base: &BaseExtractor, node: Node) -> Option<String> {
    let source = match node.kind() {
        "import_statement" => node.child_by_field_name("source")?,
        "call_expression" => {
            let function = node.child_by_field_name("function")?;
            let callee = base.get_node_text(&function);
            if callee != "require" && callee != "import" {
                return None;
            }
            let arguments = node.child_by_field_name("arguments")?;
            let mut cursor = arguments.walk();
            arguments
                .children(&mut cursor)
                .find(|child| child.kind() == "string")?
        }
        _ => return None,
    };
    Some(unquote(&base.get_node_text(&source)))
}

fn is_test_framework_module(specifier: &str) -> bool {
    TEST_FRAMEWORK_MODULES.contains(&specifier)
        || TEST_FRAMEWORK_MODULE_PREFIXES
            .iter()
            .any(|prefix| specifier.starts_with(prefix))
}

fn unquote(raw: &str) -> String {
    raw.trim()
        .trim_matches(|character| character == '"' || character == '\'' || character == '`')
        .to_string()
}

/// Resolve a dotted callee chain to the DSL word it declares.
///
/// `describe.only` drops its modifier to `describe`; `test.beforeEach` reads the
/// property behind a known namespace root; `ordinary.test` resolves to nothing
/// because `ordinary` is not a namespace root.
fn classify_callee(callee: &str) -> Option<(TestCallCategory, TestRole, &str)> {
    let segments: Vec<&str> = callee
        .split('.')
        .map(str::trim)
        .filter(|segment| !segment.is_empty() && !MODIFIER_SEGMENTS.contains(segment))
        .collect();

    let (word, namespaced) = match segments.as_slice() {
        [word] => (*word, false),
        [root, .., word] if NAMESPACE_ROOTS.contains(root) => (*word, true),
        _ => return None,
    };

    classify_word(word, namespaced).map(|(category, role)| (category, role, word))
}

/// The category drives the emitted symbol's shape; the role is read from the
/// word itself, because a namespaced hook such as `test.afterAll` carries its
/// direction in the trailing segment.
fn classify_word(word: &str, namespaced: bool) -> Option<(TestCallCategory, TestRole)> {
    if TEST_WORDS.contains(&word) {
        Some((TestCallCategory::Test, TestRole::TestCase))
    } else if CONTAINER_WORDS.contains(&word)
        || (namespaced && NAMESPACED_CONTAINER_WORDS.contains(&word))
    {
        Some((TestCallCategory::Container, TestRole::TestContainer))
    } else if SETUP_WORDS.contains(&word) {
        Some((TestCallCategory::Lifecycle, TestRole::FixtureSetup))
    } else if TEARDOWN_WORDS.contains(&word) {
        Some((TestCallCategory::Lifecycle, TestRole::FixtureTeardown))
    } else {
        None
    }
}

/// Resolve the callee of a `call_expression`, unwrapping a `.each` table call.
///
/// `test.each(table)("name", fn)` parses as a call whose *callee is itself a
/// call*, and a tagged-template table (`` it.each`...`("name", fn) ``) parses the
/// same way. Both resolve through the inner callee with the `each` segment
/// stripped.
fn resolve(base: &BaseExtractor, node: Node) -> Option<TestCall> {
    if node.kind() != "call_expression" {
        return None;
    }
    let function_node = node.child_by_field_name("function")?;

    let (callee, tabled) = match function_node.kind() {
        "call_expression" => {
            let inner = function_node.child_by_field_name("function")?;
            let chain = base.get_node_text(&inner);
            let head = chain
                .rsplit_once('.')
                .filter(|(_, last)| last.trim() == TABLE_SEGMENT)?
                .0
                .to_string();
            (head, true)
        }
        _ => (base.get_node_text(&function_node), false),
    };

    let (category, role, word) = classify_callee(&callee)?;
    let word = word.to_string();

    // Jest and Vitest run `describe.each` as a suite factory: the table
    // multiplies groups, and only `test.each`/`it.each` declares cases.
    let (category, role, callee) = if tabled {
        let role = match category {
            TestCallCategory::Test => TestRole::ParameterizedTest,
            _ => role,
        };
        (category, role, format!("{callee}.{TABLE_SEGMENT}"))
    } else {
        (category, role, callee)
    };

    Some(TestCall {
        callee,
        word,
        category,
        role,
    })
}

/// Whether this node is a test-DSL call, without building a symbol for it.
pub(crate) fn is_test_dsl_call(base: &BaseExtractor, node: Node) -> bool {
    resolve(base, node).is_some()
}

/// The DSL word a test-DSL call resolves to, used to look the emitted symbol
/// back up by name when attributing relationships.
pub(crate) fn dsl_word_of_call(base: &BaseExtractor, node: Node) -> Option<String> {
    resolve(base, node).map(|call| call.word)
}

/// Build the `Function` symbol for a test-DSL call.
///
/// Returns `None` when the node is not a recognized test call, or when a
/// test/container block carries no string description.
pub(crate) fn extract_test_call(
    base: &mut BaseExtractor,
    node: Node,
    parent_id: Option<&str>,
) -> Option<Symbol> {
    let call = resolve(base, node)?;

    let name = match call.category {
        TestCallCategory::Lifecycle => call.word.clone(),
        _ => call_description(base, &node)?,
    };

    let mut symbol =
        build_test_call_symbol(base, &node, &call.callee, name, call.category, parent_id);
    apply_test_role(
        symbol.metadata.get_or_insert_with(Default::default),
        call.role,
    );
    Some(symbol)
}

fn call_description(base: &BaseExtractor, node: &Node) -> Option<String> {
    let arguments = node.child_by_field_name("arguments")?;
    let mut cursor = arguments.walk();
    let first_string = arguments
        .children(&mut cursor)
        .find(|child| child.kind() == "string" || child.kind() == "template_string")?;
    Some(unquote(&base.get_node_text(&first_string)))
}

/// Write the test role of a declared function or method.
///
/// Decorator frameworks win over the name-and-path rule: testdeck marks its
/// cases with `@test` and `@params`, the way JUnit marks Java methods, so the
/// decorator is decisive regardless of where the file lives.
pub(crate) fn apply_declared_test_metadata(
    language: &str,
    name: &str,
    file_path: &str,
    kind: &SymbolKind,
    annotation_keys: &[String],
    doc_comment: Option<&str>,
    metadata: &mut HashMap<String, serde_json::Value>,
) {
    if let Some(role) = decorator_test_role(kind, annotation_keys) {
        apply_test_role(metadata, role);
        return;
    }
    apply_callable_test_metadata(
        language,
        name,
        file_path,
        kind,
        annotation_keys,
        doc_comment,
        metadata,
    );
}

fn decorator_test_role(kind: &SymbolKind, annotation_keys: &[String]) -> Option<TestRole> {
    if !matches!(
        kind,
        SymbolKind::Function | SymbolKind::Method | SymbolKind::Constructor
    ) {
        return None;
    }

    let mut role = None;
    for key in annotation_keys {
        let base = key.split('.').next().unwrap_or(key);
        if DECORATOR_PARAMETERIZED_KEYS.contains(&base) {
            return Some(TestRole::ParameterizedTest);
        }
        if DECORATOR_TEST_CASE_KEYS.contains(&base) {
            role = Some(TestRole::TestCase);
        }
    }
    role
}
