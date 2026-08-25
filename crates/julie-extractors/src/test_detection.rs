//! Test symbol detection for all 34 supported languages.
//!
//! Provides [`is_test_symbol`] — a pure, data-driven function that determines whether
//! a symbol is a test based on its language, name, file path, kind, annotation keys,
//! and doc comment. No tree-sitter, no file I/O.

use crate::base::{Symbol, SymbolKind, TestRole, Visibility};
use std::collections::{HashMap, HashSet};

/// Which side of a fixture a test lifecycle hook runs on.
///
/// `Ambiguous` covers a hook that wraps a test case on both sides, such as an
/// RSpec `around` block or Go's `TestMain`. It resolves to
/// [`TestRole::FixtureSetup`] because a wrapping hook always runs its setup
/// half first.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TestLifecycleDirection {
    Setup,
    Teardown,
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
pub(crate) fn apply_test_role(metadata: &mut HashMap<String, serde_json::Value>, role: TestRole) {
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

/// Directory names that mean "the code below me is test code".
///
/// Matching is exact and case-sensitive, so `integrations/` and `androidTestUtils/`
/// stay production directories.
const TEST_DIRECTORY_SEGMENTS: &[&str] = &[
    "test",
    "tests",
    "Test",
    "Tests",
    "spec",
    "Spec",
    "__tests__",
    "autotests",
    "e2e",
    "cypress",
    "integration",
    "integrationTest",
    "testFixtures",
    "androidTest",
    "functionalTest",
];

/// File-name endings that mean "this file is a test file".
const TEST_FILE_NAME_SUFFIXES: &[&str] = &[
    "_test.go",
    "_test.rb",
    "_spec.rb",
    "_test.py",
    "Test.php",
    "Cest.php",
    "Spec.php",
    "Tests.swift",
];

const PATH_SEPARATORS: [char; 2] = ['/', '\\'];

/// Check whether `file_path` looks like it lives in a test directory or is a test file.
///
/// Language-agnostic: works for Rust, Python, Java, C#, Go, JS/TS, Ruby, Swift, etc.
/// Accepts both `/` and `\` separators so Windows-spelled paths read the same.
fn is_test_path(file_path: &str) -> bool {
    for segment in file_path.split(PATH_SEPARATORS) {
        if TEST_DIRECTORY_SEGMENTS.contains(&segment) {
            return true;
        }
        // C# `MyProject.Test/`, C# `MyProject.Tests/`, Xcode `MyAppTests/`
        if segment.ends_with(".Test") || segment.ends_with("Tests") {
            return true;
        }
    }

    let file_name = file_path
        .rsplit(PATH_SEPARATORS)
        .next()
        .unwrap_or(file_path);
    if file_name == "conftest.py" {
        return true;
    }
    if TEST_FILE_NAME_SUFFIXES
        .iter()
        .any(|suffix| file_name.ends_with(suffix))
    {
        return true;
    }

    file_name.contains(".test.")
        || file_name.contains(".spec.")
        || file_name.contains(".cy.")
        || file_name.starts_with("test_")
        || file_name.starts_with("tst_")
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

/// Attribute macros that make a Rust function a test case on their own.
///
/// Matching runs on the last `::` segment of the normalized key, so
/// `rstest::rstest` reads the same as `rstest`. The `test` entry is what makes
/// the whole `tokio::test`, `actix_web::test`, `actix_rt::test`, `sqlx::test`,
/// `async_std::test`, `googletest::test`, and `test_log::test` family classify
/// without naming each crate: any qualified attribute whose last segment is
/// exactly `test` is a test attribute. The segment must match whole, so
/// `latest`, `contest`, and `test_util` stay production attributes.
const RUST_TEST_CASE_ATTRIBUTES: [&str; 8] = [
    "test",
    "test_case",
    "wasm_bindgen_test",
    "quickcheck",
    "proptest",
    "gtest",
    "traced_test",
    "rstest",
];

fn rust_attribute_segment(annotation_key: &str) -> &str {
    annotation_key
        .rsplit("::")
        .next()
        .unwrap_or(annotation_key)
        .trim()
}

/// An rstest per-case attribute. Each one adds one more run of the same
/// function. Matched on the leading segment, because rstest names a case by
/// suffixing the attribute: `#[case::six_times_seven(6, 7)]` is one case.
fn is_rstest_case_attribute(annotation_key: &str) -> bool {
    annotation_key
        .split("::")
        .next()
        .unwrap_or(annotation_key)
        .trim()
        == "case"
}

fn has_rust_attribute(annotation_keys: &[String], segment: &str) -> bool {
    annotation_keys
        .iter()
        .any(|key| rust_attribute_segment(key) == segment)
}

fn is_rust_test_case_attribute(annotation_key: &str) -> bool {
    RUST_TEST_CASE_ATTRIBUTES.contains(&rust_attribute_segment(annotation_key))
}

/// rstest's `#[fixture]` builds a value a test case asks for by name. It only
/// ever runs inside a test session, so it is a lifecycle hook, not a case. A
/// fixture that returns a guard also tears down, but the setup half always
/// runs, so the contract publishes the single honest direction: setup.
fn rust_test_lifecycle_direction(annotation_keys: &[String]) -> TestLifecycleDirection {
    if has_rust_attribute(annotation_keys, "fixture") {
        return TestLifecycleDirection::Setup;
    }
    TestLifecycleDirection::None
}

/// `#[test_case(..)]` and an `#[rstest]` carrying `#[case]` attributes both make
/// the runner report one result per data row instead of one per function.
///
/// rstest also builds a case matrix from `#[values(..)]`, but that attribute
/// sits on a parameter rather than on the function, so it never reaches these
/// keys and such a function reports `test_case`.
fn rust_test_case_role(annotation_keys: &[String]) -> Option<TestRole> {
    let rstest_has_cases = has_rust_attribute(annotation_keys, "rstest")
        && annotation_keys
            .iter()
            .any(|key| is_rstest_case_attribute(key));
    (has_rust_attribute(annotation_keys, "test_case") || rstest_has_cases)
        .then_some(TestRole::ParameterizedTest)
}

/// Annotation-only. A Rust function earns a role from an attribute macro and
/// never from its name or its path: `fn test_parser` with no attribute is
/// ordinary code, and `#[test]` in `src/lib.rs` is a real case.
fn detect_rust(annotation_keys: &[String]) -> bool {
    annotation_keys
        .iter()
        .any(|key| is_rust_test_case_attribute(key))
        || rust_test_lifecycle_direction(annotation_keys).is_lifecycle()
}

/// unittest fixtures, pytest xunit hooks, and `@pytest.fixture` factories.
///
/// A `@pytest.fixture` may also tear down after a `yield`, but its setup half
/// always runs, so a fixture factory reports as setup.
fn python_test_lifecycle_direction(
    name: &str,
    annotation_keys: &[String],
) -> TestLifecycleDirection {
    if annotation_keys
        .iter()
        .any(|annotation| annotation == "pytest.fixture")
    {
        return TestLifecycleDirection::Setup;
    }
    match name {
        "setUp" | "setUpClass" | "setUpModule" | "asyncSetUp" | "setup_method" | "setup_class"
        | "setup_function" | "setup_module" => TestLifecycleDirection::Setup,
        "tearDown" | "tearDownClass" | "tearDownModule" | "asyncTearDown" | "teardown_method"
        | "teardown_class" | "teardown_function" | "teardown_module" => {
            TestLifecycleDirection::Teardown
        }
        _ => TestLifecycleDirection::None,
    }
}

/// `@pytest.mark.parametrize` runs one case per argument set.
fn python_test_case_role(annotation_keys: &[String]) -> Option<TestRole> {
    annotation_keys
        .iter()
        .any(|annotation| annotation == "pytest.mark.parametrize")
        .then_some(TestRole::ParameterizedTest)
}

/// Both collectors take a bare `test` prefix: pytest matches `python_functions
/// = test*` and unittest matches `TestLoader.testMethodPrefix = "test"`, so
/// `testAddition` is a real case. The prefix rule stays path-guarded because
/// production code shares the vocabulary.
///
/// Annotation keys arrive lower-cased, so the `unittest` decorators are spelled
/// lower-case here.
fn detect_python(name: &str, file_path: &str, annotation_keys: &[String]) -> bool {
    if annotation_keys.iter().any(|annotation| {
        annotation.starts_with("pytest.mark.")
            || matches!(
                annotation.as_str(),
                "unittest.skip"
                    | "unittest.skipif"
                    | "unittest.skipunless"
                    | "unittest.expectedfailure"
            )
    }) {
        return true;
    }
    if python_test_lifecycle_direction(name, annotation_keys).is_lifecycle() {
        return true;
    }
    name.starts_with("test") && is_test_path(file_path)
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
    matches!(annotation, "test" | "testfactory" | "testtemplate")
        || is_java_parameterized_test_annotation(annotation)
}

/// Annotations that run one method once per case, so the runner reports one
/// result per case instead of one result per method.
fn is_java_parameterized_test_annotation(annotation: &str) -> bool {
    matches!(annotation, "parameterizedtest" | "repeatedtest")
}

/// TestNG's class-level `@Test`, which runs every public method of the class as
/// a case.
fn is_testng_class_case_annotation(annotation: &str) -> bool {
    annotation == "test"
}

/// Annotations that declare a type to be a test container on their own.
fn is_java_container_annotation(annotation: &str) -> bool {
    annotation == "nested" || is_testng_class_case_annotation(annotation)
}

/// JUnit 4/5, TestNG, and kotlin.test hook annotations, keyed on the lower-cased
/// last segment of the annotation name.
fn java_test_lifecycle_direction(annotation: &str) -> TestLifecycleDirection {
    match annotation {
        "beforeeach" | "beforeall" | "before" | "beforeclass" | "beforemethod" | "beforesuite"
        | "beforetest" | "beforegroups" => TestLifecycleDirection::Setup,
        "aftereach" | "afterall" | "after" | "afterclass" | "aftermethod" | "aftersuite"
        | "aftertest" | "aftergroups" => TestLifecycleDirection::Teardown,
        _ => TestLifecycleDirection::None,
    }
}

fn java_test_case_role(annotation_keys: &[String]) -> Option<TestRole> {
    annotation_keys
        .iter()
        .any(|annotation| is_java_parameterized_test_annotation(annotation))
        .then_some(TestRole::ParameterizedTest)
}

fn detect_java_kotlin(annotation_keys: &[String]) -> bool {
    annotation_keys.iter().any(|annotation| {
        is_java_test_case_annotation(annotation)
            || java_test_lifecycle_direction(annotation).is_lifecycle()
    })
}

fn dotnet_test_lifecycle_direction(annotation: &str) -> TestLifecycleDirection {
    match annotation {
        "setup" | "onetimesetup" | "testinitialize" | "classinitialize" | "assemblyinitialize" => {
            TestLifecycleDirection::Setup
        }
        "teardown" | "onetimeteardown" | "testcleanup" | "classcleanup" | "assemblycleanup" => {
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
    matches!(annotation, "test" | "testmethod" | "fact")
        || is_dotnet_parameterized_test_annotation(annotation)
}

/// Attributes that bind one method to a data set, so the runner reports one
/// result per row instead of one result per method.
fn is_dotnet_parameterized_test_annotation(annotation: &str) -> bool {
    matches!(
        annotation,
        "theory" | "datatestmethod" | "testcase" | "testcasesource"
    )
}

/// Attributes that declare a type to be a test container on their own.
///
/// `testfixturesource` is NUnit's class-level parameterized-fixture attribute:
/// it supplies constructor arguments to the fixture, so it names a container,
/// not a case.
fn is_dotnet_container_annotation(annotation: &str) -> bool {
    matches!(
        annotation,
        "testfixture" | "testclass" | "collectiondefinition" | "setupfixture" | "testfixturesource"
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
        "python" => python_test_lifecycle_direction(name, annotation_keys),
        "rust" => rust_test_lifecycle_direction(annotation_keys),
        "go" => go_test_lifecycle_direction(name),
        "bash" => bash_test_lifecycle_direction(name),
        "gdscript" => gdscript_test_lifecycle_direction(name),
        "qml" => qml_test_lifecycle_direction(name),
        "scala" => scala_test_lifecycle_direction(name),
        _ => TestLifecycleDirection::None,
    }
}

/// The non-lifecycle role of a test callable, for languages that mark a
/// parameterized case with an annotation.
fn annotated_test_case_role(language: &str, annotation_keys: &[String]) -> Option<TestRole> {
    match language {
        "java" | "kotlin" => java_test_case_role(annotation_keys),
        "python" => python_test_case_role(annotation_keys),
        "rust" => rust_test_case_role(annotation_keys),
        _ => None,
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
        .or_else(|| annotated_test_case_role(language, annotation_keys))
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
        normalize_scoped_test_roles(symbols, &test_container_ids);
        apply_qml_test_roles(symbols, &test_container_ids);
    }
}

fn parent_index(symbols: &[Symbol]) -> HashMap<String, Option<String>> {
    symbols
        .iter()
        .map(|symbol| (symbol.id.clone(), symbol.parent_id.clone()))
        .collect()
}

/// Strip the test role from every callable that no symbol in `test_container_ids`
/// contains.
///
/// Language-neutral: the caller decides which symbols are test containers and
/// passes their ids. Use it wherever a name-based rule can fire on production
/// code that happens to share a test framework's vocabulary.
pub(crate) fn normalize_scoped_test_roles(
    symbols: &mut [Symbol],
    test_container_ids: &HashSet<String>,
) {
    let parent_by_id = parent_index(symbols);

    for symbol in symbols.iter_mut().filter(|symbol| {
        is_callable(&symbol.kind)
            && !has_test_container_ancestor(symbol, test_container_ids, &parent_by_id)
    }) {
        let Some(mut metadata) = symbol.metadata.take() else {
            continue;
        };
        clear_test_role(&mut metadata);
        symbol.metadata = (!metadata.is_empty()).then_some(metadata);
    }
}

fn apply_qml_test_roles(symbols: &mut [Symbol], test_container_ids: &HashSet<String>) {
    let parent_by_id = parent_index(symbols);

    for symbol in symbols.iter_mut().filter(|symbol| {
        symbol.language == "qml"
            && is_callable(&symbol.kind)
            && has_test_container_ancestor(symbol, test_container_ids, &parent_by_id)
    }) {
        let mut metadata = symbol.metadata.take().unwrap_or_default();
        clear_test_role(&mut metadata);
        if let Some(role) = qml_test_role(&symbol.name) {
            apply_test_role(&mut metadata, role);
        }
        symbol.metadata = (!metadata.is_empty()).then_some(metadata);
    }
}

fn has_test_container_ancestor(
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

    let mut test_container_ids: HashSet<String> = HashSet::new();
    for symbol in symbols
        .iter_mut()
        .filter(|symbol| matches!(symbol.kind, SymbolKind::Class | SymbolKind::Struct))
    {
        let has_container_attribute = symbol
            .annotations
            .iter()
            .any(|annotation| is_dotnet_container_annotation(&annotation.annotation_key));
        if has_container_attribute || containers_with_test_members.contains(&symbol.id) {
            mark_class_test_container(symbol);
            test_container_ids.insert(symbol.id.clone());
        }
    }

    apply_dotnet_member_test_roles(symbols, &test_container_ids);
}

/// Upgrade data-driven cases and classify the xUnit lifecycle members that
/// carry no attribute of their own.
///
/// xUnit has no setup or teardown attribute: the constructor and the
/// `IAsyncLifetime`/`IDisposable` members are the fixture hooks. Those names are
/// ordinary C# elsewhere, so they only earn a role inside a type the attribute
/// or member pass already marked as a test container.
fn apply_dotnet_member_test_roles(symbols: &mut [Symbol], test_container_ids: &HashSet<String>) {
    for symbol in symbols
        .iter_mut()
        .filter(|symbol| is_callable(&symbol.kind))
    {
        let inside_container = symbol
            .parent_id
            .as_ref()
            .is_some_and(|parent_id| test_container_ids.contains(parent_id));
        let Some(role) = dotnet_member_test_role(symbol, inside_container) else {
            continue;
        };
        apply_test_role(symbol.metadata.get_or_insert_with(Default::default), role);
    }
}

fn dotnet_member_test_role(symbol: &Symbol, inside_container: bool) -> Option<TestRole> {
    if has_dotnet_annotation(symbol, is_dotnet_parameterized_test_annotation) {
        return Some(TestRole::ParameterizedTest);
    }
    let carries_own_role = has_dotnet_annotation(symbol, is_dotnet_test_case_annotation)
        || has_dotnet_annotation(symbol, |key| {
            dotnet_test_lifecycle_direction(key).is_lifecycle()
        });
    if carries_own_role || !inside_container {
        return None;
    }
    xunit_lifecycle_direction(symbol).fixture_role()
}

fn has_dotnet_annotation(symbol: &Symbol, matches_key: impl Fn(&str) -> bool) -> bool {
    symbol
        .annotations
        .iter()
        .any(|annotation| matches_key(&annotation.annotation_key))
}

fn xunit_lifecycle_direction(symbol: &Symbol) -> TestLifecycleDirection {
    if symbol.kind == SymbolKind::Constructor {
        return TestLifecycleDirection::Setup;
    }
    match symbol.name.as_str() {
        "InitializeAsync" => TestLifecycleDirection::Setup,
        "Dispose" | "DisposeAsync" => TestLifecycleDirection::Teardown,
        _ => TestLifecycleDirection::None,
    }
}

pub(crate) fn mark_java_test_containers(symbols: &mut [Symbol]) {
    let containers_with_test_members: HashSet<String> = symbols
        .iter()
        .filter(|symbol| symbol.kind == SymbolKind::Method)
        .filter(|symbol| has_java_annotation(symbol, is_java_member_test_annotation))
        .filter_map(|symbol| symbol.parent_id.clone())
        .collect();

    let mut testng_class_ids: HashSet<String> = HashSet::new();
    for symbol in symbols
        .iter_mut()
        .filter(|symbol| symbol.kind == SymbolKind::Class)
    {
        let extends_testcase = metadata_string_list_contains(symbol, "base_types", "TestCase");
        if has_java_annotation(symbol, is_java_container_annotation)
            || extends_testcase
            || containers_with_test_members.contains(&symbol.id)
        {
            mark_class_test_container(symbol);
        }
        if has_java_annotation(symbol, is_testng_class_case_annotation) {
            testng_class_ids.insert(symbol.id.clone());
        }
    }

    mark_ancestor_test_containers(symbols);

    if scopes_name_convention_roles(symbols) {
        let test_container_ids = marked_test_container_ids(symbols);
        normalize_scoped_test_roles(symbols, &test_container_ids);
    }
    apply_java_member_test_roles(symbols, &testng_class_ids);
}

/// Whether the JUnit 3 `testXxx` name convention is the only rule here that can
/// fire outside a test container.
///
/// Kotlin shares this pass but also earns roles from the Kotest and Spek call
/// DSLs, whose spec classes carry no container marker yet, so scoping a Kotlin
/// file would strip real roles. Java has no call-style test DSL.
fn scopes_name_convention_roles(symbols: &[Symbol]) -> bool {
    symbols.iter().all(|symbol| symbol.language == "java")
}

/// Annotations that make a method test infrastructure, so its enclosing class is
/// a test container. A class holding only hooks — a shared JUnit base class —
/// still counts.
fn is_java_member_test_annotation(annotation: &str) -> bool {
    is_java_test_case_annotation(annotation)
        || java_test_lifecycle_direction(annotation).is_lifecycle()
}

fn has_java_annotation(symbol: &Symbol, matches_key: impl Fn(&str) -> bool) -> bool {
    symbol
        .annotations
        .iter()
        .any(|annotation| matches_key(&annotation.annotation_key))
}

fn marked_test_container_ids(symbols: &[Symbol]) -> HashSet<String> {
    symbols
        .iter()
        .filter(|symbol| metadata_flag(symbol, "test_container"))
        .map(|symbol| symbol.id.clone())
        .collect()
}

/// Restore the annotation-driven roles the scoping pass cleared, and classify
/// the members a TestNG class-level `@Test` covers.
///
/// Scoping is a name-convention guard, but it cannot see where a role came
/// from: it also strips an annotated Kotlin top-level test function, which has
/// no enclosing class at all. Re-deriving from annotations alone puts those
/// roles back without reviving the name convention.
fn apply_java_member_test_roles(symbols: &mut [Symbol], testng_class_ids: &HashSet<String>) {
    for symbol in symbols
        .iter_mut()
        .filter(|symbol| is_callable(&symbol.kind))
    {
        let inside_testng_class = symbol
            .parent_id
            .as_ref()
            .is_some_and(|parent_id| testng_class_ids.contains(parent_id));
        let Some(role) = java_member_test_role(symbol, inside_testng_class) else {
            continue;
        };
        apply_test_role(symbol.metadata.get_or_insert_with(Default::default), role);
    }
}

/// TestNG runs every public method of a `@Test`-annotated class as a case, so
/// those methods carry no annotation of their own. A hook annotation on such a
/// method wins, because TestNG runs it around the cases instead.
fn java_member_test_role(symbol: &Symbol, inside_testng_class: bool) -> Option<TestRole> {
    let annotation_keys: Vec<String> = symbol
        .annotations
        .iter()
        .map(|annotation| annotation.annotation_key.clone())
        .collect();
    if let Some(role) =
        first_annotation_direction(&annotation_keys, java_test_lifecycle_direction).fixture_role()
    {
        return Some(role);
    }
    if annotation_keys
        .iter()
        .any(|annotation| is_java_test_case_annotation(annotation))
    {
        return Some(java_test_case_role(&annotation_keys).unwrap_or(TestRole::TestCase));
    }
    let runs_as_testng_case = inside_testng_class
        && symbol.kind == SymbolKind::Method
        && symbol.visibility == Some(Visibility::Public);
    runs_as_testng_case.then_some(TestRole::TestCase)
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

/// `go test` compiles only the files whose name ends in `_test.go`, so every Go
/// test role is gated on that suffix. Both separators are accepted so a
/// Windows-spelled path reads the same.
pub(crate) fn is_go_test_file(file_path: &str) -> bool {
    file_path
        .rsplit(PATH_SEPARATORS)
        .next()
        .unwrap_or(file_path)
        .ends_with("_test.go")
}

/// Name prefixes that `go test` reports as their own result.
///
/// `Benchmark` is included because `go test -list` lists benchmarks beside
/// tests, fuzz targets, and examples, so a benchmark-only file must not be
/// invisible to a test-aware consumer.
const GO_TEST_CASE_PREFIXES: [&str; 4] = ["Test", "Benchmark", "Fuzz", "Example"];

/// `go` requires the character after the prefix to not be a lower-case letter,
/// so `Testable` stays production code while `Test_adds` is a case.
fn matches_go_test_prefix(name: &str, prefix: &str) -> bool {
    name.strip_prefix(prefix)
        .is_some_and(|suffix| !suffix.starts_with(char::is_lowercase))
}

fn is_go_test_case_name(name: &str) -> bool {
    GO_TEST_CASE_PREFIXES
        .iter()
        .any(|prefix| matches_go_test_prefix(name, prefix))
}

/// `TestMain` wraps the whole package run around `m.Run()`, so it is an
/// around-style hook, not a case. testify spells its hooks `SetupXxx`,
/// `TearDownXxx`, `BeforeTest`, and `AfterTest`; gocheck spells them `SetUpXxx`
/// and `TearDownXxx`.
fn go_test_lifecycle_direction(name: &str) -> TestLifecycleDirection {
    match name {
        "TestMain" => TestLifecycleDirection::Ambiguous,
        "SetupSuite" | "SetupTest" | "SetupSubTest" | "BeforeTest" | "SetUpSuite" | "SetUpTest" => {
            TestLifecycleDirection::Setup
        }
        "TearDownSuite" | "TearDownTest" | "TearDownSubTest" | "AfterTest" => {
            TestLifecycleDirection::Teardown
        }
        _ => TestLifecycleDirection::None,
    }
}

fn detect_go(name: &str, file_path: &str) -> bool {
    is_go_test_file(file_path)
        && (is_go_test_case_name(name) || go_test_lifecycle_direction(name).is_lifecycle())
}

/// Mark a testify suite struct as a test container.
///
/// testify runs a suite by embedding `suite.Suite` in a struct declared in a
/// `_test.go` file, and the suite's methods attach through their receiver type.
/// An aliased import spells the embedded type with a different qualifier, so
/// the rule keys on a qualified embedded type whose final segment is `Suite`.
pub(crate) fn mark_go_test_containers(symbols: &mut [Symbol]) {
    let suite_struct_ids: HashSet<String> = symbols
        .iter()
        .filter(|symbol| symbol.kind == SymbolKind::Field)
        .filter(|symbol| is_go_test_file(&symbol.file_path))
        .filter(|symbol| metadata_flag(symbol, "go_embedded"))
        .filter(|symbol| embeds_go_test_suite(symbol))
        .filter_map(|symbol| symbol.parent_id.clone())
        .collect();

    for symbol in symbols
        .iter_mut()
        .filter(|symbol| symbol.kind == SymbolKind::Struct)
        .filter(|symbol| suite_struct_ids.contains(&symbol.id))
    {
        mark_class_test_container(symbol);
    }
}

fn embeds_go_test_suite(symbol: &Symbol) -> bool {
    symbol
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get("embedded_type"))
        .and_then(|value| value.as_str())
        .map(|embedded| embedded.trim_start_matches('*'))
        .is_some_and(|embedded| matches!(embedded.rsplit_once('.'), Some((_, "Suite"))))
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
