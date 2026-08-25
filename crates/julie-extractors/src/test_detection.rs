//! Test symbol detection for all 34 supported languages.
//!
//! Provides [`is_test_symbol`] — a pure, data-driven function that determines whether
//! a symbol is a test based on its language, name, file path, kind, annotation keys,
//! and doc comment. No tree-sitter, no file I/O.

use crate::base::{Symbol, SymbolKind, TestRole};
use std::collections::{HashMap, HashSet};

/// Which side of a fixture a test lifecycle hook runs on.
///
/// `Ambiguous` covers a hook that wraps a test case on both sides, such as an
/// RSpec `around` block. It resolves to [`TestRole::FixtureSetup`] because a
/// wrapping hook always runs its setup half first.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TestLifecycleDirection {
    Setup,
    Teardown,
    #[allow(
        dead_code,
        reason = "no supported language reports an around-style hook yet"
    )]
    Ambiguous,
    None,
}

impl TestLifecycleDirection {
    fn is_lifecycle(self) -> bool {
        !matches!(self, TestLifecycleDirection::None)
    }

    fn fixture_role(self) -> Option<TestRole> {
        match self {
            TestLifecycleDirection::Setup | TestLifecycleDirection::Ambiguous => {
                Some(TestRole::FixtureSetup)
            }
            TestLifecycleDirection::Teardown => Some(TestRole::FixtureTeardown),
            TestLifecycleDirection::None => None,
        }
    }
}

/// Write the boolean role flags and the `test_role` string for one role.
///
/// Every test-role write in this module goes through here, so the booleans a
/// consumer reads and the `test_role` string can never disagree.
fn apply_test_role(metadata: &mut HashMap<String, serde_json::Value>, role: TestRole) {
    match role {
        TestRole::TestContainer => {
            metadata.insert("test_container".to_string(), serde_json::Value::Bool(true));
        }
        TestRole::FixtureSetup | TestRole::FixtureTeardown => {
            metadata.insert("is_test".to_string(), serde_json::Value::Bool(true));
            metadata.insert("test_lifecycle".to_string(), serde_json::Value::Bool(true));
        }
        TestRole::TestCase | TestRole::ParameterizedTest => {
            metadata.insert("is_test".to_string(), serde_json::Value::Bool(true));
        }
    }
    metadata.insert(
        "test_role".to_string(),
        serde_json::Value::String(role.as_str().to_string()),
    );
}

fn clear_test_role(metadata: &mut HashMap<String, serde_json::Value>) {
    metadata.remove("is_test");
    metadata.remove("test_lifecycle");
    metadata.remove("test_container");
    metadata.remove("test_role");
}

/// Callable symbol kinds — only these can be actual test functions/methods.
fn is_callable(kind: &SymbolKind) -> bool {
    matches!(
        kind,
        SymbolKind::Function | SymbolKind::Method | SymbolKind::Constructor
    )
}

/// Check whether `file_path` looks like it lives in a test directory or is a test file.
///
/// Language-agnostic: works for Rust, Python, Java, C#, Go, JS/TS, Ruby, Swift, etc.
fn is_test_path(file_path: &str) -> bool {
    // Segment-level checks (directory names)
    for segment in file_path.split('/') {
        match segment {
            "test" | "tests" | "Test" | "Tests" | "spec" | "Spec" | "__tests__" | "autotests" => {
                return true;
            }
            _ => {}
        }
        // C# convention: MyProject.Tests/
        if segment.ends_with(".Tests") || segment.ends_with(".Test") {
            return true;
        }
    }

    // File-name patterns
    let file_name = file_path.rsplit('/').next().unwrap_or(file_path);
    if file_name.ends_with("_test.go")
        || file_name.contains(".test.")
        || file_name.contains(".spec.")
        || file_name.starts_with("test_")
        || file_name.starts_with("tst_")
    {
        return true;
    }

    false
}

/// Determine if a symbol is a test symbol.
///
/// Two-tier approach:
/// 1. **Language-specific**: check normalized annotation keys, doc comments, and
///    language-idiomatic naming conventions.
/// 2. **Generic fallback**: for the ~20 languages without specific test framework conventions,
///    check if the function name starts with `test_` or `Test` AND the file is in a test path.
///
/// Only callable symbols (Function, Method, Constructor) can be tests. Classes, structs,
/// interfaces, etc. return `false` — they are containers, not tests.
///
/// `doc_comment` is currently only used for PHP's `@test` annotation pattern.
pub fn is_test_symbol(
    language: &str,
    name: &str,
    file_path: &str,
    kind: &SymbolKind,
    annotation_keys: &[String],
    doc_comment: Option<&str>,
) -> bool {
    // Gate: only callable symbols can be tests
    if !is_callable(kind) {
        return false;
    }

    match language {
        "rust" => detect_rust(annotation_keys),
        "python" => detect_python(name, file_path, annotation_keys),
        "java" | "kotlin" => {
            // JUnit 4/5 annotations, OR JUnit 3 `testXxx` methods (no annotation,
            // inside a `TestCase` subclass) — path-guarded like swift/php so a
            // production method named `testConnection` isn't mis-flagged.
            detect_java_kotlin(annotation_keys)
                || (name.starts_with("test") && is_test_path(file_path))
        }
        "scala" => detect_scala(name, annotation_keys),
        "elixir" => detect_elixir(name),
        "erlang" => detect_erlang(name),
        "csharp" | "vbnet" | "razor" => detect_csharp(annotation_keys),
        "go" => detect_go(name, file_path),
        "javascript" | "typescript" => detect_js_ts(name, file_path),
        "php" => detect_php(name, file_path, annotation_keys, doc_comment),
        "bash" => detect_bash(name, file_path),
        "powershell" => detect_powershell(name, file_path),
        "ruby" => detect_ruby(name, file_path),
        "swift" => detect_swift(name, file_path),
        "dart" => detect_dart(name, file_path, annotation_keys),
        "gdscript" => detect_gdscript(name, file_path),
        "qml" => detect_qml(name, file_path),
        "lua" => detect_lua(name, file_path),
        "r" => detect_r(name, file_path),
        _ => detect_generic(name, file_path),
    }
}

// ---------------------------------------------------------------------------
// Language-specific detectors
// ---------------------------------------------------------------------------

fn detect_rust(annotation_keys: &[String]) -> bool {
    annotation_keys
        .iter()
        .any(|a| a == "test" || a == "tokio::test" || a == "rstest")
}

fn python_test_lifecycle_direction(name: &str) -> TestLifecycleDirection {
    match name {
        "setUp" | "setUpClass" => TestLifecycleDirection::Setup,
        "tearDown" | "tearDownClass" => TestLifecycleDirection::Teardown,
        _ => TestLifecycleDirection::None,
    }
}

fn detect_python(name: &str, file_path: &str, annotation_keys: &[String]) -> bool {
    if annotation_keys.iter().any(|annotation| {
        annotation.starts_with("pytest.mark.")
            || matches!(
                annotation.as_str(),
                "unittest.skip"
                    | "unittest.skipIf"
                    | "unittest.skipUnless"
                    | "unittest.expectedFailure"
            )
    }) {
        return true;
    }
    if python_test_lifecycle_direction(name).is_lifecycle() {
        return true;
    }
    name.starts_with("test_") && is_test_path(file_path)
}

fn detect_scala(name: &str, annotation_keys: &[String]) -> bool {
    if detect_java_kotlin(annotation_keys) {
        return true;
    }
    if scala_test_lifecycle_direction(name).is_lifecycle() {
        return true;
    }
    name.starts_with("test")
}

fn is_java_test_case_annotation(annotation: &str) -> bool {
    matches!(annotation, "test" | "parameterizedtest" | "repeatedtest")
}

fn java_test_lifecycle_direction(annotation: &str) -> TestLifecycleDirection {
    match annotation {
        "beforeeach" | "beforeall" | "before" | "beforeclass" => TestLifecycleDirection::Setup,
        "aftereach" | "afterall" | "after" | "afterclass" => TestLifecycleDirection::Teardown,
        _ => TestLifecycleDirection::None,
    }
}

fn detect_java_kotlin(annotation_keys: &[String]) -> bool {
    annotation_keys.iter().any(|annotation| {
        is_java_test_case_annotation(annotation)
            || java_test_lifecycle_direction(annotation).is_lifecycle()
    })
}

fn dotnet_test_lifecycle_direction(annotation: &str) -> TestLifecycleDirection {
    match annotation {
        "setup" | "onetimesetup" | "testinitialize" | "classinitialize" => {
            TestLifecycleDirection::Setup
        }
        "teardown" | "onetimeteardown" | "testcleanup" | "classcleanup" => {
            TestLifecycleDirection::Teardown
        }
        _ => TestLifecycleDirection::None,
    }
}

fn detect_csharp(annotation_keys: &[String]) -> bool {
    annotation_keys.iter().any(|annotation| {
        is_dotnet_test_case_annotation(annotation)
            || dotnet_test_lifecycle_direction(annotation).is_lifecycle()
    })
}

fn is_dotnet_test_case_annotation(annotation: &str) -> bool {
    matches!(
        annotation,
        "test" | "testcase" | "testmethod" | "fact" | "theory"
    )
}

fn first_annotation_direction(
    annotation_keys: &[String],
    direction_of: fn(&str) -> TestLifecycleDirection,
) -> TestLifecycleDirection {
    annotation_keys
        .iter()
        .map(|annotation| direction_of(annotation))
        .find(|direction| direction.is_lifecycle())
        .unwrap_or(TestLifecycleDirection::None)
}

fn is_test_lifecycle(
    language: &str,
    name: &str,
    annotation_keys: &[String],
) -> TestLifecycleDirection {
    match language {
        "java" | "kotlin" => {
            first_annotation_direction(annotation_keys, java_test_lifecycle_direction)
        }
        "csharp" | "vbnet" | "razor" => {
            first_annotation_direction(annotation_keys, dotnet_test_lifecycle_direction)
        }
        "python" => python_test_lifecycle_direction(name),
        "bash" => bash_test_lifecycle_direction(name),
        "gdscript" => gdscript_test_lifecycle_direction(name),
        "qml" => qml_test_lifecycle_direction(name),
        "scala" => scala_test_lifecycle_direction(name),
        _ => TestLifecycleDirection::None,
    }
}

/// Set `is_test`, `test_role`, and — for a lifecycle hook — `test_lifecycle` on
/// callable metadata.
pub(crate) fn apply_callable_test_metadata(
    language: &str,
    name: &str,
    file_path: &str,
    kind: &SymbolKind,
    annotation_keys: &[String],
    doc_comment: Option<&str>,
    metadata: &mut HashMap<String, serde_json::Value>,
) {
    if !is_test_symbol(
        language,
        name,
        file_path,
        kind,
        annotation_keys,
        doc_comment,
    ) {
        return;
    }
    let role = is_test_lifecycle(language, name, annotation_keys)
        .fixture_role()
        .unwrap_or(TestRole::TestCase);
    apply_test_role(metadata, role);
}

pub(crate) fn mark_base_type_test_containers(symbols: &mut [Symbol], base_type: &str) {
    let test_container_ids: HashSet<String> = symbols
        .iter()
        .filter(|symbol| symbol.kind == SymbolKind::Class)
        .filter(|symbol| metadata_string_list_contains(symbol, "base_types", base_type))
        .map(|symbol| symbol.id.clone())
        .collect();

    for symbol in symbols.iter_mut().filter(|symbol| {
        symbol.kind == SymbolKind::Class && test_container_ids.contains(&symbol.id)
    }) {
        mark_class_test_container(symbol);
    }

    if base_type == "TestCase" {
        normalize_qml_test_roles(symbols, &test_container_ids);
    }
}

fn normalize_qml_test_roles(symbols: &mut [Symbol], test_container_ids: &HashSet<String>) {
    let parent_by_id: HashMap<String, Option<String>> = symbols
        .iter()
        .map(|symbol| (symbol.id.clone(), symbol.parent_id.clone()))
        .collect();

    for symbol in symbols
        .iter_mut()
        .filter(|symbol| symbol.language == "qml" && is_callable(&symbol.kind))
    {
        let in_test_case = has_testcase_ancestor(symbol, test_container_ids, &parent_by_id);
        let role = in_test_case.then(|| qml_test_role(&symbol.name)).flatten();
        let mut metadata = symbol.metadata.take().unwrap_or_default();
        clear_test_role(&mut metadata);

        if let Some(role) = role {
            apply_test_role(&mut metadata, role);
        }
        symbol.metadata = (!metadata.is_empty()).then_some(metadata);
    }
}

fn has_testcase_ancestor(
    symbol: &Symbol,
    test_container_ids: &HashSet<String>,
    parent_by_id: &HashMap<String, Option<String>>,
) -> bool {
    let mut current = symbol.parent_id.clone();
    let mut visited = HashSet::new();
    while let Some(parent_id) = current {
        if !visited.insert(parent_id.clone()) {
            return false;
        }
        if test_container_ids.contains(&parent_id) {
            return true;
        }
        current = parent_by_id.get(&parent_id).cloned().flatten();
    }
    false
}

fn qml_test_role(name: &str) -> Option<TestRole> {
    if let Some(role) = qml_test_lifecycle_direction(name).fixture_role() {
        return Some(role);
    }
    if name == "init_data" || (name.starts_with("test_") && name.ends_with("_data")) {
        return None;
    }
    if name.starts_with("test_")
        || name.starts_with("benchmark_")
        || name.starts_with("benchmark_once_")
    {
        return Some(TestRole::TestCase);
    }
    None
}

fn mark_class_test_container(symbol: &mut Symbol) {
    apply_test_role(
        symbol.metadata.get_or_insert_with(Default::default),
        TestRole::TestContainer,
    );
}

fn metadata_string_list_contains(symbol: &Symbol, key: &str, needle: &str) -> bool {
    symbol
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get(key))
        .and_then(|value| value.as_array())
        .map(|values| {
            values
                .iter()
                .filter_map(|value| value.as_str())
                .any(|name| name == needle || name.ends_with(&format!(".{needle}")))
        })
        .unwrap_or(false)
}

fn metadata_flag(symbol: &Symbol, key: &str) -> bool {
    symbol
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get(key))
        .and_then(|value| value.as_bool())
        == Some(true)
}

pub(crate) fn mark_dotnet_test_containers(symbols: &mut [Symbol]) {
    let containers_with_test_members: HashSet<String> = symbols
        .iter()
        .filter(|symbol| symbol.kind == SymbolKind::Method)
        .filter(|symbol| {
            symbol
                .annotations
                .iter()
                .any(|annotation| is_dotnet_test_case_annotation(&annotation.annotation_key))
        })
        .filter_map(|symbol| symbol.parent_id.clone())
        .collect();

    for symbol in symbols
        .iter_mut()
        .filter(|symbol| symbol.kind == SymbolKind::Class)
    {
        let has_container_attribute = symbol.annotations.iter().any(|annotation| {
            matches!(
                annotation.annotation_key.as_str(),
                "testfixture" | "testclass"
            )
        });
        if has_container_attribute || containers_with_test_members.contains(&symbol.id) {
            mark_class_test_container(symbol);
        }
    }
}

pub(crate) fn mark_java_test_containers(symbols: &mut [Symbol]) {
    let containers_with_test_members: HashSet<String> = symbols
        .iter()
        .filter(|symbol| symbol.kind == SymbolKind::Method)
        .filter(|symbol| {
            symbol
                .annotations
                .iter()
                .any(|annotation| is_java_test_case_annotation(&annotation.annotation_key))
        })
        .filter_map(|symbol| symbol.parent_id.clone())
        .collect();

    for symbol in symbols
        .iter_mut()
        .filter(|symbol| symbol.kind == SymbolKind::Class)
    {
        let has_nested_attribute = symbol
            .annotations
            .iter()
            .any(|annotation| annotation.annotation_key == "nested");
        let extends_testcase = metadata_string_list_contains(symbol, "base_types", "TestCase");
        if has_nested_attribute
            || extends_testcase
            || containers_with_test_members.contains(&symbol.id)
        {
            mark_class_test_container(symbol);
        }
    }

    mark_ancestor_test_containers(symbols);
}

/// Mark every `Class` ancestor of an already-marked test-container class.
///
/// JUnit executes an outer class whose only test content is a `@Nested` inner
/// class, so the enclosing class is itself a test container even without direct
/// test members.
fn mark_ancestor_test_containers(symbols: &mut [Symbol]) {
    let index_by_id: HashMap<&str, usize> = symbols
        .iter()
        .enumerate()
        .map(|(index, symbol)| (symbol.id.as_str(), index))
        .collect();

    let mut ancestors_to_mark: HashSet<usize> = HashSet::new();
    for symbol in symbols.iter().filter(|symbol| {
        symbol.kind == SymbolKind::Class && metadata_flag(symbol, "test_container")
    }) {
        let mut parent = symbol.parent_id.as_deref();
        while let Some(parent_id) = parent {
            let Some(&index) = index_by_id.get(parent_id) else {
                break;
            };
            let ancestor = &symbols[index];
            if ancestor.kind == SymbolKind::Class {
                ancestors_to_mark.insert(index);
            }
            parent = ancestor.parent_id.as_deref();
        }
    }

    for index in ancestors_to_mark {
        mark_class_test_container(&mut symbols[index]);
    }
}

pub(crate) fn mark_python_test_containers(symbols: &mut [Symbol]) {
    let containers_with_test_members: HashSet<String> = symbols
        .iter()
        .filter(|symbol| matches!(symbol.kind, SymbolKind::Method | SymbolKind::Function))
        .filter(|symbol| {
            metadata_flag(symbol, "is_test") && !metadata_flag(symbol, "test_lifecycle")
        })
        .filter_map(|symbol| symbol.parent_id.clone())
        .collect();

    for symbol in symbols
        .iter_mut()
        .filter(|symbol| symbol.kind == SymbolKind::Class)
    {
        let extends_testcase = metadata_string_list_contains(symbol, "superclasses", "TestCase");
        if extends_testcase || containers_with_test_members.contains(&symbol.id) {
            mark_class_test_container(symbol);
        }
    }
}

fn detect_go(name: &str, file_path: &str) -> bool {
    // Go tests require BOTH: recognized prefix AND _test.go file suffix
    let file_name = file_path.rsplit('/').next().unwrap_or(file_path);
    (name.starts_with("Test") || name.starts_with("Fuzz") || name.starts_with("Example"))
        && file_name.ends_with("_test.go")
}

/// Known limitation: in Jest/Mocha, `test()`/`describe()` are call expressions, not named
/// function definitions. Symbol-level detection will mostly catch path-based heuristics.
/// The name check is a secondary signal.
fn detect_js_ts(name: &str, file_path: &str) -> bool {
    // Must be a test runner function AND in a test/spec file
    let is_test_fn = matches!(name, "describe" | "it" | "test");
    let file_name = file_path.rsplit('/').next().unwrap_or(file_path);
    let in_test_file =
        file_name.contains(".test.") || file_name.contains(".spec.") || is_test_path(file_path);
    is_test_fn && in_test_file
}

fn detect_php(
    name: &str,
    file_path: &str,
    annotation_keys: &[String],
    doc_comment: Option<&str>,
) -> bool {
    if annotation_keys.iter().any(|a| a == "test") {
        return true;
    }
    // @test annotation in doc comment — genuine test marker regardless of path
    if let Some(doc) = doc_comment
        && doc.contains("@test")
    {
        return true;
    }
    // Name prefix — requires test path to avoid false positives on production code
    // (e.g. testConnection() in a service class)
    name.starts_with("test") && is_test_path(file_path)
}

fn matches_script_test_name(
    name: &str,
    file_path: &str,
    allow_test_prefix: bool,
    keywords: &[&str],
) -> bool {
    let normalized = name.to_ascii_lowercase();
    if allow_test_prefix && normalized.starts_with("test_") && is_test_path(file_path) {
        return true;
    }

    is_test_path(file_path) && keywords.contains(&normalized.as_str())
}

fn detect_bash(name: &str, file_path: &str) -> bool {
    matches_script_test_name(
        name,
        file_path,
        true,
        &[
            "describe", "context", "it", "specify", "example", "feature", "scenario", "setup",
            "teardown",
        ],
    )
}

fn detect_powershell(name: &str, file_path: &str) -> bool {
    matches_script_test_name(
        name,
        file_path,
        false,
        &[
            "describe",
            "context",
            "it",
            "beforeall",
            "afterall",
            "beforeeach",
            "aftereach",
        ],
    )
}

fn detect_ruby(name: &str, file_path: &str) -> bool {
    matches_script_test_name(
        name,
        file_path,
        true,
        &[
            "describe",
            "context",
            "it",
            "specify",
            "example",
            "feature",
            "scenario",
            "before",
            "after",
            "around",
            "xdescribe",
            "xcontext",
            "xit",
            "fdescribe",
            "fit",
        ],
    )
}

fn detect_swift(name: &str, file_path: &str) -> bool {
    // XCTest convention: test* prefix + lifecycle methods — all require test path
    // to avoid false positives on production code with similarly-named methods
    is_test_path(file_path)
        && (name.starts_with("test")
            || matches!(
                name,
                "setUp" | "tearDown" | "setUpWithError" | "tearDownWithError"
            ))
}

fn detect_elixir(name: &str) -> bool {
    name.starts_with("test_") || name.starts_with("test ")
}

/// EUnit discovers a test from its name alone: `sum_test/0` is a test case and
/// `sum_test_/0` is a test generator. Detection is deliberately path-independent
/// because EUnit tests live beside the code they exercise, and the
/// `test_`/`Test` prefix the generic fallback looks for never matches the
/// `_test` suffix convention.
///
/// EUnit also requires arity zero, which this entry point does not receive.
/// The Erlang extractor applies the arity gate through [`erlang_test_role`].
fn detect_erlang(name: &str) -> bool {
    name.ends_with("_test") || name.ends_with("_test_")
}

/// Common Test callbacks that set up or tear down a suite, group, or case.
const COMMON_TEST_LIFECYCLE_NAMES: [&str; 6] = [
    "init_per_suite",
    "end_per_suite",
    "init_per_testcase",
    "end_per_testcase",
    "init_per_group",
    "end_per_group",
];

/// Common Test callbacks that describe a suite instead of exercising it.
const COMMON_TEST_CONFIG_NAMES: [&str; 3] = ["all", "groups", "suite"];

/// Common Test runs every test case as `Case(Config)`.
const COMMON_TEST_CASE_ARITY: u32 = 1;

/// Whether an Erlang module hosts EUnit tests, Common Test cases, or both.
/// The two frameworks are independent — a `*_SUITE` module may also include
/// `eunit.hrl` — so the classification carries a flag per framework.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct ErlangTestModule {
    eunit: bool,
    common_test: bool,
}

impl ErlangTestModule {
    pub(crate) fn classify(module_name: &str, includes_eunit_header: bool) -> Self {
        Self {
            eunit: includes_eunit_header || module_name.ends_with("_tests"),
            common_test: module_name.ends_with("_SUITE"),
        }
    }

    pub(crate) fn is_test_container(&self) -> bool {
        self.eunit || self.common_test
    }
}

/// The role an Erlang function plays in its module's test framework.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ErlangTestRole {
    Case,
    Lifecycle,
}

/// Classify an Erlang function against EUnit and Common Test.
///
/// Common Test dispatches on exact callback names inside a `*_SUITE` module and
/// runs every other exported `Case(Config)` as a test case. EUnit matches the
/// name suffix on any zero-arity function, in any module, because EUnit test
/// modules are not required to be named or located in a particular way.
pub(crate) fn erlang_test_role(
    module: ErlangTestModule,
    name: &str,
    arity: u32,
    exported: bool,
) -> Option<ErlangTestRole> {
    if module.common_test {
        if COMMON_TEST_LIFECYCLE_NAMES.contains(&name) {
            return Some(ErlangTestRole::Lifecycle);
        }
        if exported && arity == COMMON_TEST_CASE_ARITY && !COMMON_TEST_CONFIG_NAMES.contains(&name)
        {
            return Some(ErlangTestRole::Case);
        }
    }

    (arity == 0 && detect_erlang(name)).then_some(ErlangTestRole::Case)
}

fn detect_dart(name: &str, file_path: &str, annotation_keys: &[String]) -> bool {
    // isTest annotation key is definitive, no path guard needed.
    if annotation_keys.iter().any(|d| d == "istest") {
        return true;
    }
    // Name prefix — requires test path to avoid false positives on production Dart functions
    name.starts_with("test") && is_test_path(file_path)
}

/// GDScript GUT (Godot Unit Test): test methods run by GUT are any `test`-prefixed
/// method (`func test_foo` / `func testFoo`). The enclosing `extends GutTest` class
/// is represented independently through `base_types` metadata. Path-guarded so a
/// production method like `testConnection` isn't mis-flagged. Broader than the
/// generic fallback, which only catches `test_`/`Test`.
fn gdscript_test_lifecycle_direction(name: &str) -> TestLifecycleDirection {
    match name {
        "before_each" | "before_all" => TestLifecycleDirection::Setup,
        "after_each" | "after_all" => TestLifecycleDirection::Teardown,
        _ => TestLifecycleDirection::None,
    }
}

fn detect_gdscript(name: &str, file_path: &str) -> bool {
    is_test_path(file_path)
        && (name.starts_with("test") || gdscript_test_lifecycle_direction(name).is_lifecycle())
}

fn qml_test_lifecycle_direction(name: &str) -> TestLifecycleDirection {
    match name {
        "initTestCase" | "init" => TestLifecycleDirection::Setup,
        "cleanupTestCase" | "cleanup" => TestLifecycleDirection::Teardown,
        _ => TestLifecycleDirection::None,
    }
}

fn detect_qml(name: &str, file_path: &str) -> bool {
    is_test_path(file_path) && qml_test_role(name).is_some()
}

fn bash_test_lifecycle_direction(name: &str) -> TestLifecycleDirection {
    match name.to_ascii_lowercase().as_str() {
        "setup" => TestLifecycleDirection::Setup,
        "teardown" => TestLifecycleDirection::Teardown,
        _ => TestLifecycleDirection::None,
    }
}

fn scala_test_lifecycle_direction(name: &str) -> TestLifecycleDirection {
    match name {
        "beforeEach" | "beforeAll" => TestLifecycleDirection::Setup,
        "afterEach" | "afterAll" => TestLifecycleDirection::Teardown,
        _ => TestLifecycleDirection::None,
    }
}

/// Lua luaunit: test functions/methods are `testXxx` (camelCase) or `test_xxx`.
/// busted (`describe`/`it`) is call-style and handled in `test_calls`, not here.
fn detect_lua(name: &str, file_path: &str) -> bool {
    is_test_path(file_path) && name.starts_with("test")
}

/// R RUnit: test functions are named `test.foo` (dot convention) or `test_foo`.
/// testthat (`test_that("...")`) is call-style and handled in `test_calls`, not here.
fn detect_r(name: &str, file_path: &str) -> bool {
    is_test_path(file_path) && (name.starts_with("test.") || name.starts_with("test_"))
}

// ---------------------------------------------------------------------------
// Generic fallback — for the ~20 languages without specific frameworks
// ---------------------------------------------------------------------------

fn detect_generic(name: &str, file_path: &str) -> bool {
    let has_test_name = name.starts_with("test_") || name.starts_with("Test");
    has_test_name && is_test_path(file_path)
}
