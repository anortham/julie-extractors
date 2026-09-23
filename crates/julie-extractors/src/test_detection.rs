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
        // `is_test = 1` means a test case or a hook in the schema contract, so
        // consumers that count cases must not see step definitions there, even
        // when a path or name rule flagged the method first.
        TestRole::StepDefinition => {
            metadata.remove("is_test");
            metadata.remove("test_lifecycle");
            metadata.remove("test_container");
        }
    }
    metadata.insert(
        "test_role".to_string(),
        serde_json::Value::String(role.as_str().to_string()),
    );
}

pub(crate) fn clear_test_role(metadata: &mut HashMap<String, serde_json::Value>) {
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
/// stay production directories. Bare `integration/` stays production too: Nest
/// and DDD codebases use `src/integration/` for production modules, and the
/// Cypress legacy layout is already matched by its `cypress/` parent.
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
    "_test.sh",
    "_spec.sh",
    ".bats",
];

const PATH_SEPARATORS: [char; 2] = ['/', '\\'];

/// Check whether `file_path` looks like it lives in a test directory or is a test file.
///
/// Language-agnostic: works for Rust, Python, Java, C#, Go, JS/TS, Ruby, Swift, etc.
/// Accepts both `/` and `\` separators so Windows-spelled paths read the same.
pub(crate) fn is_test_path(file_path: &str) -> bool {
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
    if file_name == "conftest.py" || file_name == "tests.py" {
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
        "csharp" | "vbnet" | "razor" | "fsharp" => detect_csharp(annotation_keys),
        "go" => detect_go(name, file_path),
        "javascript" | "typescript" => detect_js_ts(name, file_path),
        "php" => detect_php(name, file_path, annotation_keys, doc_comment),
        "bash" => detect_bash(name, file_path),
        "powershell" => detect_powershell(name, file_path),
        "ruby" => detect_ruby(name, file_path),
        "swift" => detect_swift(name, file_path, annotation_keys),
        "dart" => detect_dart(name, file_path, annotation_keys),
        "gdscript" => detect_gdscript(name, file_path),
        "qml" => detect_qml(name, file_path),
        "lua" => detect_lua(name, file_path),
        "r" => detect_r(name, file_path),
        "c" => {
            detect_generic(name, file_path)
                || (c_test_lifecycle_direction(name).is_lifecycle() && is_test_path(file_path))
        }
        "zig" => false,
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
        .any(|annotation| is_python_fixture_annotation(annotation))
    {
        return TestLifecycleDirection::Setup;
    }
    match name {
        "setUp" | "setUpClass" | "setUpTestData" | "setUpModule" | "asyncSetUp"
        | "setup_method" | "setup_class" | "setup_function" | "setup_module" => {
            TestLifecycleDirection::Setup
        }
        "tearDown" | "tearDownClass" | "tearDownModule" | "asyncTearDown" | "teardown_method"
        | "teardown_class" | "teardown_function" | "teardown_module" => {
            TestLifecycleDirection::Teardown
        }
        _ => TestLifecycleDirection::None,
    }
}

/// `@pytest.fixture` and the pytest-asyncio `@pytest_asyncio.fixture`.
fn is_python_fixture_annotation(annotation: &str) -> bool {
    matches!(annotation, "pytest.fixture" | "pytest_asyncio.fixture")
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
    if annotation_keys
        .iter()
        .any(|annotation| is_python_fixture_annotation(annotation))
    {
        return true;
    }
    // Bare hook names are ordinary Python elsewhere: a production
    // `ConnectionPool.setUp` in `src/client.py` earns no role.
    if python_test_lifecycle_direction(name, annotation_keys).is_lifecycle()
        && is_test_path(file_path)
    {
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

/// GoogleTest macro keys that declare a parameterized case. The C++ extractor
/// synthesizes these keys from the macro keyword, because the macros carry no
/// source attribute.
const GOOGLETEST_PARAMETERIZED_MACRO_KEYS: [&str; 2] = ["test_p", "typed_test_p"];

fn cpp_test_case_role(annotation_keys: &[String]) -> Option<TestRole> {
    annotation_keys
        .iter()
        .any(|key| GOOGLETEST_PARAMETERIZED_MACRO_KEYS.contains(&key.as_str()))
        .then_some(TestRole::ParameterizedTest)
}

/// GoogleTest fixture hooks, matched on the method name after any `Class::`
/// qualifier so an out-of-class definition classifies like its in-class twin.
/// Unity runs `setUp`/`tearDown` around every test and `suiteSetUp`/
/// `suiteTearDown` around the run; the caller checks the file is a test file.
fn c_test_lifecycle_direction(name: &str) -> TestLifecycleDirection {
    match name {
        "setUp" | "suiteSetUp" => TestLifecycleDirection::Setup,
        "tearDown" | "suiteTearDown" => TestLifecycleDirection::Teardown,
        _ => TestLifecycleDirection::None,
    }
}

fn cpp_test_lifecycle_direction(name: &str) -> TestLifecycleDirection {
    match name.rsplit("::").next().unwrap_or(name) {
        "SetUp" | "SetUpTestSuite" | "SetUpTestCase" => TestLifecycleDirection::Setup,
        "TearDown" | "TearDownTestSuite" | "TearDownTestCase" => TestLifecycleDirection::Teardown,
        _ => TestLifecycleDirection::None,
    }
}

/// The role of a GoogleTest fixture hook the C++ extractor recognized from the
/// enclosing fixture's base type. That structural gate already proved the method
/// is a hook, so an unrecognized spelling still records the setup half rather
/// than dropping the role.
pub(crate) fn cpp_fixture_lifecycle_role(name: &str) -> TestRole {
    cpp_test_lifecycle_direction(name)
        .fixture_role()
        .unwrap_or(TestRole::FixtureSetup)
}

/// The role of a GoogleTest case macro, from the synthetic macro-keyword key.
pub(crate) fn cpp_googletest_case_role(annotation_keys: &[String]) -> TestRole {
    cpp_test_case_role(annotation_keys).unwrap_or(TestRole::TestCase)
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
    matches!(annotation, "nested" | "suite") || is_testng_class_case_annotation(annotation)
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
        "setup"
        | "onetimesetup"
        | "testinitialize"
        | "classinitialize"
        | "assemblyinitialize"
        | "before"
        | "beforeevery"
        | "beforescenario"
        | "beforefeature"
        | "beforetestrun"
        | "beforestep"
        | "beforescenarioblock" => TestLifecycleDirection::Setup,
        "teardown" | "onetimeteardown" | "testcleanup" | "classcleanup" | "assemblycleanup"
        | "after" | "afterevery" | "afterscenario" | "afterfeature" | "aftertestrun"
        | "afterstep" | "afterscenarioblock" => TestLifecycleDirection::Teardown,
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
        "theory"
            | "datatestmethod"
            | "testcase"
            | "testcasesource"
            | "arguments"
            | "methoddatasource"
            | "classdatasource"
            | "matrixdatasource"
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
        "testfixture"
            | "testclass"
            | "collectiondefinition"
            | "setupfixture"
            | "testfixturesource"
            | "binding"
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
        "csharp" | "vbnet" | "razor" | "fsharp" => {
            first_annotation_direction(annotation_keys, dotnet_test_lifecycle_direction)
        }
        "c" => c_test_lifecycle_direction(name),
        "cpp" => cpp_test_lifecycle_direction(name),
        "php" => php_test_lifecycle_direction(name, annotation_keys),
        "python" => python_test_lifecycle_direction(name, annotation_keys),
        "rust" => rust_test_lifecycle_direction(annotation_keys),
        "go" => go_test_lifecycle_direction(name),
        "ruby" => ruby_test_lifecycle_direction(name),
        "bash" => bash_test_lifecycle_direction(name),
        "lua" => lua_test_lifecycle_direction(name),
        "r" => r_test_lifecycle_direction(name),
        "gdscript" => gdscript_test_lifecycle_direction(name),
        "qml" => qml_test_lifecycle_direction(name),
        "scala" => scala_test_lifecycle_direction(name),
        "swift" => swift_test_lifecycle_direction(name),
        _ => TestLifecycleDirection::None,
    }
}

/// The non-lifecycle role of a test callable, for languages that mark a
/// parameterized case with an annotation.
fn annotated_test_case_role(language: &str, annotation_keys: &[String]) -> Option<TestRole> {
    match language {
        "cpp" => cpp_test_case_role(annotation_keys),
        "java" | "kotlin" => java_test_case_role(annotation_keys),
        "php" => php_test_case_role(annotation_keys),
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
        .filter(|symbol| {
            (symbol.kind == SymbolKind::Class
                && metadata_string_list_contains(symbol, "base_types", base_type))
                || is_qml_object_of_type(symbol, base_type)
        })
        .map(|symbol| symbol.id.clone())
        .collect();

    for symbol in symbols
        .iter_mut()
        .filter(|symbol| test_container_ids.contains(&symbol.id))
    {
        mark_class_test_container(symbol);
    }

    if base_type == "TestCase" {
        normalize_scoped_test_roles(symbols, &test_container_ids);
        apply_qml_test_roles(symbols, &test_container_ids);
    }
}

/// A nested QML object (`Item { TestCase { ... } }`) is a `field` row whose
/// `object_type` metadata names its type, bare or module-qualified.
fn is_qml_object_of_type(symbol: &Symbol, base_type: &str) -> bool {
    symbol.language == "qml"
        && symbol.kind == SymbolKind::Field
        && symbol
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.get("object_type"))
            .and_then(|value| value.as_str())
            .is_some_and(|object_type| {
                object_type == base_type
                    || object_type
                        .strip_suffix(base_type)
                        .is_some_and(|qualifier| qualifier.ends_with('.'))
            })
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

/// Machine.Specifications contexts: a class whose fields are `It`,
/// `Establish`, `Because`, or `Cleanup` delegates is a test container, and
/// the lambda each field holds is a case (`It`) or a fixture hook.
/// `field_type` returns the declared type of a field symbol.
pub(crate) fn mark_mspec_contexts(
    symbols: &mut [Symbol],
    field_type: impl Fn(&Symbol) -> Option<String>,
) {
    let delegate_roles: HashMap<String, (String, TestRole)> = symbols
        .iter()
        .filter(|symbol| symbol.kind == SymbolKind::Field)
        .filter_map(|field| {
            let role = match field_type(field)?.as_str() {
                "It" => TestRole::TestCase,
                "Establish" | "Because" => TestRole::FixtureSetup,
                "Cleanup" => TestRole::FixtureTeardown,
                _ => return None,
            };
            Some((field.id.clone(), (field.parent_id.clone()?, role)))
        })
        .collect();
    let contexts: HashSet<String> = delegate_roles
        .values()
        .filter(|(_, role)| *role != TestRole::FixtureTeardown)
        .map(|(context, _)| context.clone())
        .collect();
    for symbol in symbols.iter_mut() {
        if contexts.contains(&symbol.id) {
            mark_class_test_container(symbol);
            continue;
        }
        let Some((context, role)) = symbol
            .parent_id
            .as_ref()
            .and_then(|parent_id| delegate_roles.get(parent_id))
        else {
            continue;
        };
        if is_callable(&symbol.kind) && contexts.contains(context) {
            apply_test_role(symbol.metadata.get_or_insert_with(Default::default), *role);
        }
    }
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
    let mut binding_ids: HashSet<String> = HashSet::new();
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
        if has_dotnet_annotation(symbol, |key| key == "binding") {
            binding_ids.insert(symbol.id.clone());
        }
    }

    apply_dotnet_member_test_roles(symbols, &test_container_ids, &binding_ids);
}

/// SpecFlow and Reqnroll step attributes. `[Given]` and its peers are common
/// words, so they bind a step only on a method of a `[Binding]` class.
fn is_dotnet_step_annotation(annotation: &str) -> bool {
    matches!(annotation, "given" | "when" | "then" | "stepdefinition")
}

/// Upgrade data-driven cases and classify the xUnit lifecycle members that
/// carry no attribute of their own.
///
/// xUnit has no setup or teardown attribute: the constructor and the
/// `IAsyncLifetime`/`IDisposable` members are the fixture hooks. Those names are
/// ordinary C# elsewhere, so they only earn a role inside a type the attribute
/// or member pass already marked as a test container.
fn apply_dotnet_member_test_roles(
    symbols: &mut [Symbol],
    test_container_ids: &HashSet<String>,
    binding_ids: &HashSet<String>,
) {
    for symbol in symbols
        .iter_mut()
        .filter(|symbol| is_callable(&symbol.kind))
    {
        let parent_in = |ids: &HashSet<String>| {
            symbol
                .parent_id
                .as_ref()
                .is_some_and(|parent_id| ids.contains(parent_id))
        };
        let inside_container = parent_in(test_container_ids);
        let inside_binding = parent_in(binding_ids);
        let Some(role) = dotnet_member_test_role(symbol, inside_container, inside_binding) else {
            continue;
        };
        apply_test_role(symbol.metadata.get_or_insert_with(Default::default), role);
    }
}

fn dotnet_member_test_role(
    symbol: &Symbol,
    inside_container: bool,
    inside_binding: bool,
) -> Option<TestRole> {
    if has_dotnet_annotation(symbol, is_dotnet_parameterized_test_annotation) {
        return Some(TestRole::ParameterizedTest);
    }
    if inside_binding && has_dotnet_annotation(symbol, is_dotnet_step_annotation) {
        return Some(TestRole::StepDefinition);
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
        .filter(|symbol| matches!(symbol.kind, SymbolKind::Class | SymbolKind::Interface))
    {
        if containers_with_test_members.contains(&symbol.id) {
            mark_class_test_container(symbol);
        }
        if symbol.kind != SymbolKind::Class {
            continue;
        }
        let extends_testcase = metadata_string_list_contains(symbol, "base_types", "TestCase");
        if has_java_annotation(symbol, is_java_container_annotation) || extends_testcase {
            mark_class_test_container(symbol);
        }
        if has_java_annotation(symbol, is_testng_class_case_annotation) {
            testng_class_ids.insert(symbol.id.clone());
        }
    }
    mark_test_interface_implementers(symbols);

    mark_ancestor_test_containers(symbols);

    let test_container_ids = marked_test_container_ids(symbols);
    normalize_scoped_test_roles(symbols, &test_container_ids);
    apply_java_member_test_roles(symbols, &testng_class_ids);
}

/// JUnit 5 runs the `@Test` default methods of an interface on every class that
/// implements it, so a class implementing a same-file test interface is a test
/// container even when it declares no test of its own.
fn mark_test_interface_implementers(symbols: &mut [Symbol]) {
    let test_interfaces: Vec<String> = symbols
        .iter()
        .filter(|symbol| {
            symbol.kind == SymbolKind::Interface && metadata_flag(symbol, "test_container")
        })
        .map(|symbol| symbol.name.clone())
        .collect();
    if test_interfaces.is_empty() {
        return;
    }
    for symbol in symbols
        .iter_mut()
        .filter(|symbol| symbol.kind == SymbolKind::Class)
    {
        let implements_test_interface = symbol
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.get("base_types"))
            .and_then(|value| value.as_array())
            .into_iter()
            .flatten()
            .filter_map(|value| value.as_str())
            .map(|base| base.split('<').next().unwrap_or(base).trim())
            .any(|base| {
                test_interfaces
                    .iter()
                    .any(|name| base == name || base.ends_with(&format!(".{name}")))
            });
        if implements_test_interface {
            mark_class_test_container(symbol);
        }
    }
}

/// Kotest and Spek spec base classes. A class extending one of them is a spec
/// even when its body is empty, because the base class is what the engine runs.
pub(crate) const KOTLIN_SPEC_BASE_TYPES: &[&str] = &[
    "AnnotationSpec",
    "BehaviorSpec",
    "DescribeSpec",
    "ExpectSpec",
    "FeatureSpec",
    "FreeSpec",
    "FunSpec",
    "ShouldSpec",
    "StringSpec",
    "WordSpec",
    "Spek",
];

/// Mark a Kotest or Spek spec scope as a test container.
///
/// A spec carries no annotation, so the pass takes two proofs and either one is
/// enough:
///
/// - the class extends a named Kotest or Spek spec base type, or
/// - the declaration's body is a spec lambda: a call-DSL step already earned a
///   role and named this declaration as its parent. This catches a project's own
///   spec base class, which the name list cannot know, and a Kotest test factory
///   (`val factory = funSpec { … }`), which is a property rather than a class.
///
/// The second proof is limited to a class or a property so that a nested step
/// never turns the case above it into a container.
///
/// It must run before [`mark_java_test_containers`], whose scoping pass reads
/// the marked containers.
pub(crate) fn mark_kotlin_test_containers(symbols: &mut [Symbol]) {
    let spec_scopes_with_dsl_steps: HashSet<String> = symbols
        .iter()
        .filter(|symbol| symbol.kind == SymbolKind::Function)
        .filter(|symbol| {
            symbol
                .metadata
                .as_ref()
                .is_some_and(|metadata| metadata.contains_key("test_role"))
        })
        .filter_map(|symbol| symbol.parent_id.clone())
        .collect();

    for symbol in symbols
        .iter_mut()
        .filter(|symbol| matches!(symbol.kind, SymbolKind::Class | SymbolKind::Property))
    {
        let extends_spec_base = symbol.kind == SymbolKind::Class
            && KOTLIN_SPEC_BASE_TYPES
                .iter()
                .any(|base_type| metadata_string_list_contains(symbol, "base_types", base_type));
        if extends_spec_base || spec_scopes_with_dsl_steps.contains(&symbol.id) {
            mark_class_test_container(symbol);
        }
    }
}

/// Suite base types of ScalaTest, MUnit, specs2, ZIO Test, weaver, utest and
/// ScalaCheck. A class or object extending one is a test container.
const SCALA_SUITE_BASE_TYPES: &[&str] = &[
    "AnyFunSuite",
    "AnyFlatSpec",
    "AnyWordSpec",
    "AnyFreeSpec",
    "AnyFunSpec",
    "AnyFeatureSpec",
    "AnyPropSpec",
    "AsyncFunSuite",
    "AsyncFlatSpec",
    "AsyncWordSpec",
    "AsyncFreeSpec",
    "AsyncFunSpec",
    "AsyncFeatureSpec",
    "FunSuite",
    "FlatSpec",
    "WordSpec",
    "FreeSpec",
    "FunSpec",
    "FeatureSpec",
    "PropSpec",
    "CatsEffectSuite",
    "ScalaCheckSuite",
    "Specification",
    "SpecificationWithJUnit",
    "ZIOSpecDefault",
    "ZIOSpec",
    "SimpleIOSuite",
    "IOSuite",
    "TestSuite",
    "Properties",
];

/// Mark Scala test containers, then clear the name-based roles (`test*`,
/// `beforeAll`, `it("…")`) of every callable outside one.
///
/// A class or object is a container when it extends a known suite base type,
/// holds a JUnit-annotated test method, or sits in a test path and holds a
/// ScalaTest / MUnit DSL step. The test-path guard keeps a production object
/// that happens to call `it("…") { }` out.
pub(crate) fn mark_scala_test_containers(symbols: &mut [Symbol]) {
    let scopes_with_tests: HashSet<String> = symbols
        .iter()
        .filter(|symbol| {
            (symbol.kind == SymbolKind::Function
                && metadata_has_test_role(symbol)
                && is_test_path(&symbol.file_path))
                || (symbol.kind == SymbolKind::Method
                    && has_java_annotation(symbol, is_java_member_test_annotation))
        })
        .filter_map(|symbol| symbol.parent_id.clone())
        .collect();

    for symbol in symbols
        .iter_mut()
        .filter(|symbol| matches!(symbol.kind, SymbolKind::Class | SymbolKind::Trait))
    {
        let extends_suite = SCALA_SUITE_BASE_TYPES
            .iter()
            .any(|base_type| metadata_string_list_contains(symbol, "base_types", base_type));
        if extends_suite || scopes_with_tests.contains(&symbol.id) {
            mark_class_test_container(symbol);
        }
    }

    mark_ancestor_test_containers(symbols);
    let container_ids = marked_test_container_ids(symbols);
    normalize_scoped_test_roles(symbols, &container_ids);
}

fn metadata_has_test_role(symbol: &Symbol) -> bool {
    symbol
        .metadata
        .as_ref()
        .is_some_and(|metadata| metadata.contains_key("test_role"))
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

    let mut testcase_ids = HashSet::new();
    for symbol in symbols
        .iter_mut()
        .filter(|symbol| symbol.kind == SymbolKind::Class)
    {
        let extends_testcase = extends_python_testcase(symbol);
        if extends_testcase {
            testcase_ids.insert(symbol.id.clone());
        }
        if extends_testcase || containers_with_test_members.contains(&symbol.id) {
            mark_class_test_container(symbol);
        }
    }

    apply_python_testcase_member_roles(symbols, &testcase_ids);
}

/// unittest loads every `TestCase` subclass wherever it lives. These are the
/// standard library, Django, and DRF `TestCase` subclasses.
const PYTHON_TESTCASE_BASES: &[&str] = &[
    "TestCase",
    "IsolatedAsyncioTestCase",
    "SimpleTestCase",
    "TransactionTestCase",
    "LiveServerTestCase",
    "StaticLiveServerTestCase",
    "APITestCase",
    "APISimpleTestCase",
    "APITransactionTestCase",
    "APILiveServerTestCase",
];

fn extends_python_testcase(symbol: &Symbol) -> bool {
    symbol
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get("superclasses"))
        .and_then(|value| value.as_array())
        .is_some_and(|bases| {
            bases.iter().filter_map(|base| base.as_str()).any(|base| {
                base.rsplit('.')
                    .next()
                    .is_some_and(|name| PYTHON_TESTCASE_BASES.contains(&name))
            })
        })
}

/// A `test*` method or a unittest lifecycle hook inside a `TestCase` subclass
/// is collected whatever the file path, so it gets its role here even when the
/// path guard in `detect_python` withheld it.
fn apply_python_testcase_member_roles(symbols: &mut [Symbol], testcase_ids: &HashSet<String>) {
    for symbol in symbols.iter_mut().filter(|symbol| {
        matches!(
            symbol.kind,
            SymbolKind::Method | SymbolKind::Constructor | SymbolKind::Function
        ) && symbol
            .parent_id
            .as_ref()
            .is_some_and(|parent| testcase_ids.contains(parent))
            && !metadata_flag(symbol, "is_test")
    }) {
        let role = python_test_lifecycle_direction(&symbol.name, &[])
            .fixture_role()
            .or_else(|| {
                symbol
                    .name
                    .starts_with("test")
                    .then_some(TestRole::TestCase)
            });
        if let Some(role) = role {
            apply_test_role(symbol.metadata.get_or_insert_with(Default::default), role);
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
    let file_name = file_path
        .rsplit(PATH_SEPARATORS)
        .next()
        .unwrap_or(file_path);
    let in_test_file =
        file_name.contains(".test.") || file_name.contains(".spec.") || is_test_path(file_path);
    is_test_fn && in_test_file
}

/// PHPUnit hook attributes, keyed on the lower-cased rightmost namespace
/// segment of the attribute name. The PHP extractor spells a PHPDoc `@before`
/// tag as the same key, so a docblock hook classifies like its attribute.
fn php_attribute_lifecycle_direction(annotation: &str) -> TestLifecycleDirection {
    match annotation {
        "before" | "beforeclass" => TestLifecycleDirection::Setup,
        "after" | "afterclass" => TestLifecycleDirection::Teardown,
        _ => TestLifecycleDirection::None,
    }
}

/// PHPUnit's fixture method names. `setUp` and `tearDown` run around each test
/// of a `TestCase` subclass; `setUpBeforeClass` and `tearDownAfterClass` run
/// once around the whole class.
fn php_name_lifecycle_direction(name: &str) -> TestLifecycleDirection {
    match name {
        "setUp" | "setUpBeforeClass" => TestLifecycleDirection::Setup,
        "tearDown" | "tearDownAfterClass" => TestLifecycleDirection::Teardown,
        _ => TestLifecycleDirection::None,
    }
}

fn php_test_lifecycle_direction(name: &str, annotation_keys: &[String]) -> TestLifecycleDirection {
    let attribute_direction =
        first_annotation_direction(annotation_keys, php_attribute_lifecycle_direction);
    if attribute_direction.is_lifecycle() {
        return attribute_direction;
    }
    php_name_lifecycle_direction(name)
}

/// `#[DataProvider]` binds one method to a data set, so PHPUnit reports one
/// result per row instead of one result per method.
fn php_test_case_role(annotation_keys: &[String]) -> Option<TestRole> {
    annotation_keys
        .iter()
        .any(|annotation| annotation == "dataprovider")
        .then_some(TestRole::ParameterizedTest)
}

/// The role a PHPUnit member earns from its own name or attributes.
///
/// A `#[DataProvider]`-referenced method supplies rows and runs no assertion,
/// so it earns nothing here: its name is not `test`-prefixed and it carries no
/// hook attribute.
fn php_member_test_role(name: &str, annotation_keys: &[String]) -> Option<TestRole> {
    if let Some(role) = php_test_lifecycle_direction(name, annotation_keys).fixture_role() {
        return Some(role);
    }
    name.starts_with("test")
        .then(|| php_test_case_role(annotation_keys).unwrap_or(TestRole::TestCase))
}

/// Whether a PHPDoc block carries `@tag` as a whole tag: `@test` matches
/// `@test` but not `@tested-by`, `@testdox`, or `qa@testing.example.com`.
pub(crate) fn has_phpdoc_tag(doc_comment: &str, tag: &str) -> bool {
    doc_comment.match_indices('@').any(|(at, _)| {
        let starts_tag = doc_comment[..at]
            .chars()
            .next_back()
            .is_none_or(|before| before.is_whitespace() || before == '*');
        starts_tag
            && doc_comment[at + 1..].strip_prefix(tag).is_some_and(|rest| {
                rest.chars()
                    .next()
                    .is_none_or(|ch| !ch.is_alphanumeric() && ch != '-' && ch != '_')
            })
    })
}

/// An attribute or a `@test` docblock names a case wherever the file sits,
/// because neither spelling occurs in ordinary PHP. The `test` name prefix is
/// ordinary PHP — `testConnection()` on a service class — so it stays gated on
/// a test path, and so do the fixture method names.
fn detect_php(
    name: &str,
    file_path: &str,
    annotation_keys: &[String],
    doc_comment: Option<&str>,
) -> bool {
    if annotation_keys
        .iter()
        .any(|annotation| annotation == "test")
        || first_annotation_direction(annotation_keys, php_attribute_lifecycle_direction)
            .is_lifecycle()
    {
        return true;
    }
    if doc_comment.is_some_and(|doc| has_phpdoc_tag(doc, "test")) {
        return true;
    }
    is_test_path(file_path)
        && (name.starts_with("test") || php_name_lifecycle_direction(name).is_lifecycle())
}

/// Mark PHP test containers, classify the members they hold, then scope the
/// call-style roles in a production file.
///
/// A class is a container when it extends PHPUnit's `TestCase` or when one of
/// its members already carries a test role, which covers a `#[Test]`-holding
/// class that extends nothing. The member pass then classifies the
/// name-convention members of a container — PHPUnit collects `testXxx` and runs
/// `setUp`/`tearDown` on the name alone — which the declaration path leaves
/// alone outside a test path.
///
/// Pest declares its cases as top-level `test()` and `it()` calls, so a
/// production file that calls a function of that name would otherwise publish a
/// case. Outside a test path a role therefore survives only inside a container.
pub(crate) fn mark_php_test_containers(symbols: &mut [Symbol], file_path: &str) {
    let containers_with_test_members: HashSet<String> = symbols
        .iter()
        .filter(|symbol| is_callable(&symbol.kind))
        .filter(|symbol| metadata_flag(symbol, "is_test"))
        .filter_map(|symbol| symbol.parent_id.clone())
        .collect();

    let behat_imports = php_behat_imports(symbols);
    let mut test_container_ids: HashSet<String> = HashSet::new();
    let mut framework_containers: HashMap<String, PhpSuiteFramework> = HashMap::new();
    for symbol in symbols
        .iter_mut()
        .filter(|symbol| symbol.kind == SymbolKind::Class)
    {
        let framework = php_suite_framework(symbol, file_path, &behat_imports);
        if framework.is_some()
            || extends_php_test_case(symbol)
            || containers_with_test_members.contains(&symbol.id)
        {
            mark_class_test_container(symbol);
            test_container_ids.insert(symbol.id.clone());
        }
        if let Some(framework) = framework {
            framework_containers.insert(symbol.id.clone(), framework);
        }
    }

    apply_php_framework_member_roles(symbols, &framework_containers);
    let phpunit_container_ids: HashSet<String> = test_container_ids
        .iter()
        .filter(|id| {
            !matches!(
                framework_containers.get(*id),
                Some(PhpSuiteFramework::Behat)
            )
        })
        .cloned()
        .collect();
    apply_php_member_test_roles(symbols, &phpunit_container_ids);

    if !is_test_path(file_path) {
        normalize_scoped_test_roles(symbols, &test_container_ids);
    }
}

/// A PHP suite collected by a runner other than PHPUnit.
#[derive(Clone, Copy)]
enum PhpSuiteFramework {
    /// Codeception: a `*Cest` class in a `*Cest.php` file. Each public method
    /// that takes an actor (`AcceptanceTester $I`) is a case; `_before` and
    /// `_after` are the hooks.
    Codeception,
    /// PHPSpec: an `ObjectBehavior` subclass. `it_*` and `its_*` methods are
    /// examples; `let` and `letGo` are the hooks.
    PhpSpec,
    /// Behat: a class that implements a Behat `Context` interface. Its step
    /// methods bind to `.feature` steps; its hook methods wrap suites,
    /// features, scenarios, and steps.
    Behat,
}

fn php_suite_framework(
    class: &Symbol,
    file_path: &str,
    behat_imports: &HashMap<String, String>,
) -> Option<PhpSuiteFramework> {
    let file_name = file_path.rsplit(['/', '\\']).next().unwrap_or(file_path);
    if class.name.ends_with("Cest") && file_name.ends_with("Cest.php") {
        return Some(PhpSuiteFramework::Codeception);
    }
    if php_base_types(class).any(|base_type| base_type == "ObjectBehavior") {
        return Some(PhpSuiteFramework::PhpSpec);
    }
    implements_behat_context(class, behat_imports).then_some(PhpSuiteFramework::Behat)
}

/// Behat's context interfaces, by their last name segment.
const BEHAT_CONTEXT_INTERFACES: &[&str] = &[
    "Context",
    "SnippetAcceptingContext",
    "CustomSnippetAcceptingContext",
    "TranslatableContext",
];

/// `Context` is a common class name, so a base type counts only when it
/// resolves into the `Behat\` namespace: written qualified, or bound by a
/// `use Behat\...` import.
fn implements_behat_context(class: &Symbol, behat_imports: &HashMap<String, String>) -> bool {
    class
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get("base_types"))
        .and_then(|value| value.as_array())
        .into_iter()
        .flatten()
        .filter_map(|value| value.as_str())
        .filter_map(|base_type| {
            let written = base_type.trim_start_matches('\\');
            if written.contains('\\') {
                Some(written)
            } else {
                behat_imports.get(written).map(String::as_str)
            }
        })
        .any(|qualified| {
            qualified.starts_with("Behat\\")
                && qualified
                    .rsplit('\\')
                    .next()
                    .is_some_and(|name| BEHAT_CONTEXT_INTERFACES.contains(&name))
        })
}

/// The local names that `use Behat\...` imports bind, mapped to the
/// qualified name.
fn php_behat_imports(symbols: &[Symbol]) -> HashMap<String, String> {
    symbols
        .iter()
        .filter(|symbol| symbol.kind == SymbolKind::Import)
        .filter_map(|symbol| {
            let qualified = symbol.name.trim_start_matches('\\');
            if !qualified.starts_with("Behat\\") {
                return None;
            }
            let local = symbol
                .metadata
                .as_ref()
                .and_then(|metadata| metadata.get("alias"))
                .and_then(|value| value.as_str())
                .or_else(|| qualified.rsplit('\\').next())?;
            Some((local.to_string(), qualified.to_string()))
        })
        .collect()
}

fn php_base_types(symbol: &Symbol) -> impl Iterator<Item = &str> {
    symbol
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get("base_types"))
        .and_then(|value| value.as_array())
        .into_iter()
        .flatten()
        .filter_map(|value| value.as_str())
        .map(|base_type| base_type.rsplit('\\').next().unwrap_or(base_type))
}

fn apply_php_framework_member_roles(
    symbols: &mut [Symbol],
    framework_containers: &HashMap<String, PhpSuiteFramework>,
) {
    for symbol in symbols
        .iter_mut()
        .filter(|symbol| is_callable(&symbol.kind))
    {
        let Some(framework) = symbol
            .parent_id
            .as_ref()
            .and_then(|parent_id| framework_containers.get(parent_id))
        else {
            continue;
        };
        let role = match framework {
            PhpSuiteFramework::Codeception => match symbol.name.as_str() {
                "_before" => Some(TestRole::FixtureSetup),
                "_after" => Some(TestRole::FixtureTeardown),
                name if !name.starts_with('_')
                    && symbol.visibility == Some(crate::base::Visibility::Public)
                    && takes_codeception_actor(symbol) =>
                {
                    Some(TestRole::TestCase)
                }
                _ => None,
            },
            PhpSuiteFramework::PhpSpec => match symbol.name.as_str() {
                "let" => Some(TestRole::FixtureSetup),
                "letGo" => Some(TestRole::FixtureTeardown),
                name if name.starts_with("it_") || name.starts_with("its_") => {
                    Some(TestRole::TestCase)
                }
                _ => None,
            },
            PhpSuiteFramework::Behat => behat_member_role(symbol),
        };
        if let Some(role) = role {
            apply_test_role(symbol.metadata.get_or_insert_with(Default::default), role);
        }
    }
}

/// A Behat context member's role from its attributes (`#[Given]`,
/// `#[BeforeScenario]`) or its docblock tags (`@Given`, `@BeforeScenario`).
fn behat_member_role(method: &Symbol) -> Option<TestRole> {
    let carries = |tag: &str| {
        let key = tag.to_ascii_lowercase();
        method
            .annotations
            .iter()
            .any(|annotation| annotation.annotation_key == key)
            || method
                .doc_comment
                .as_deref()
                .is_some_and(|doc| has_phpdoc_tag(doc, tag))
    };
    if ["Given", "When", "Then"].into_iter().any(carries) {
        return Some(TestRole::StepDefinition);
    }
    if [
        "BeforeSuite",
        "BeforeFeature",
        "BeforeScenario",
        "BeforeStep",
    ]
    .into_iter()
    .any(carries)
    {
        return Some(TestRole::FixtureSetup);
    }
    ["AfterSuite", "AfterFeature", "AfterScenario", "AfterStep"]
        .into_iter()
        .any(carries)
        .then_some(TestRole::FixtureTeardown)
}

/// Codeception actors are generated `*Tester` classes (`AcceptanceTester`,
/// `FunctionalTester`, `UnitTester`, `ApiTester`).
fn takes_codeception_actor(method: &Symbol) -> bool {
    method
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get("parameters"))
        .and_then(|value| value.as_str())
        .is_some_and(|parameters| {
            parameters
                .trim_matches(['(', ')'])
                .split(',')
                .filter_map(|parameter| parameter.split('$').next())
                .filter_map(|declared| declared.split_whitespace().last())
                .any(|declared| {
                    let type_name = declared.rsplit('\\').next().unwrap_or(declared);
                    type_name.ends_with("Tester")
                })
        })
}

/// PHP separates namespace segments with `\`, and a `use` statement lets a
/// class name PHPUnit's base class by its short name, so the rule compares the
/// last segment of each declared base type.
fn extends_php_test_case(symbol: &Symbol) -> bool {
    symbol
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get("base_types"))
        .and_then(|value| value.as_array())
        .is_some_and(|base_types| {
            base_types
                .iter()
                .filter_map(|value| value.as_str())
                .any(|base_type| base_type.rsplit('\\').next().unwrap_or(base_type) == "TestCase")
        })
}

fn apply_php_member_test_roles(symbols: &mut [Symbol], test_container_ids: &HashSet<String>) {
    for symbol in symbols
        .iter_mut()
        .filter(|symbol| is_callable(&symbol.kind))
    {
        let inside_container = symbol
            .parent_id
            .as_ref()
            .is_some_and(|parent_id| test_container_ids.contains(parent_id));
        if !inside_container || metadata_flag(symbol, "is_test") {
            continue;
        }
        let annotation_keys: Vec<String> = symbol
            .annotations
            .iter()
            .map(|annotation| annotation.annotation_key.clone())
            .collect();
        let Some(role) = php_member_test_role(&symbol.name, &annotation_keys) else {
            continue;
        };
        apply_test_role(symbol.metadata.get_or_insert_with(Default::default), role);
    }
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

/// bats, ShellSpec, shunit2, and bashunit. shunit2 and bashunit run every
/// function whose name starts with `test` (`testAdds`, `test_adds`).
fn detect_bash(name: &str, file_path: &str) -> bool {
    matches_script_test_name(
        name,
        file_path,
        true,
        &[
            "describe", "context", "it", "specify", "example", "feature", "scenario",
        ],
    ) || (is_test_path(file_path)
        && ((name.starts_with("test") && name.len() > 4)
            || bash_test_lifecycle_direction(name) != TestLifecycleDirection::None))
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

/// Minitest, Test::Unit, and ActiveSupport::TestCase all name their per-case
/// hooks `setup` and `teardown`. RSpec's `before`/`after`/`around` reach this
/// function through the block symbols the Ruby call extractor names after the
/// hook itself.
///
/// `around` wraps the example on both sides, so it reports
/// [`TestLifecycleDirection::Ambiguous`].
fn ruby_test_lifecycle_direction(name: &str) -> TestLifecycleDirection {
    match name {
        "setup" | "before" => TestLifecycleDirection::Setup,
        "teardown" | "after" => TestLifecycleDirection::Teardown,
        "around" => TestLifecycleDirection::Ambiguous,
        _ => TestLifecycleDirection::None,
    }
}

/// The role a bare Ruby block call plays, for the RSpec, Rails, and
/// ActiveSupport vocabularies the Ruby call extractor recognises.
pub(crate) fn ruby_block_test_role(method_name: &str) -> Option<TestRole> {
    ruby_test_lifecycle_direction(method_name).fixture_role()
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
            "setup",
            "teardown",
            "xdescribe",
            "xcontext",
            "xit",
            "fdescribe",
            "fit",
        ],
    )
}

/// Base classes whose subclasses a Ruby test runner collects on sight.
///
/// Minitest and Test::Unit collect `test_`-prefixed methods from a subclass;
/// Rails layers `ActiveSupport::TestCase`, `ActionDispatch::IntegrationTest`,
/// and the per-component test cases on top of Minitest and adds the
/// `test "name" do` macro.
const RUBY_TEST_BASE_TYPES: &[&str] = &[
    "Minitest::Test",
    "Test::Unit::TestCase",
    "ActiveSupport::TestCase",
    "ActionDispatch::IntegrationTest",
    "ActionDispatch::SystemTestCase",
    "ActionController::TestCase",
    "ActionMailer::TestCase",
    "ActionMailbox::TestCase",
    "ActionView::TestCase",
    "ActiveJob::TestCase",
    "ActionCable::TestCase",
    "ActionCable::Connection::TestCase",
    "ActionCable::Channel::TestCase",
    "Rails::Generators::TestCase",
];

/// An application test base such as `ApplicationSystemTestCase` lives in the
/// test tree and subclasses a Rails test case in another file, so a base named
/// `*TestCase` or `*Test` counts inside a test path.
fn has_ruby_application_test_base(symbol: &Symbol) -> bool {
    symbol.kind == SymbolKind::Class
        && is_test_path(&symbol.file_path)
        && symbol
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.get("base_types"))
            .and_then(|value| value.as_array())
            .is_some_and(|bases| {
                bases.iter().filter_map(|base| base.as_str()).any(|base| {
                    let name = base.rsplit("::").next().unwrap_or(base);
                    name.ends_with("TestCase") || name.ends_with("Test")
                })
            })
}

/// Mark Ruby test containers, then strip every role that sits outside one.
///
/// Two container families exist. RSpec `describe`/`context`/`shared_examples`
/// blocks are marked as containers by the Ruby call extractor, which reads
/// their block syntax. Minitest-family suites are ordinary classes, so they are
/// found through the `base_types` metadata the Ruby class extractor emits.
///
/// The scoping pass matters because Ruby's test vocabulary — `setup`,
/// `teardown`, `test_`-prefixed methods — is ordinary Ruby elsewhere. A
/// `def setup` in a spec-directory support class earns no role.
pub(crate) fn mark_ruby_test_containers(symbols: &mut [Symbol]) {
    for base_type in RUBY_TEST_BASE_TYPES {
        mark_base_type_test_containers(symbols, base_type);
    }
    for symbol in symbols
        .iter_mut()
        .filter(|symbol| has_ruby_application_test_base(symbol))
    {
        mark_class_test_container(symbol);
    }

    let test_container_ids: HashSet<String> = symbols
        .iter()
        .filter(|symbol| metadata_flag(symbol, "test_container"))
        .map(|symbol| symbol.id.clone())
        .collect();

    normalize_scoped_test_roles(symbols, &test_container_ids);
}

/// XCTest's per-test hooks, plus Quick's wrapping hook.
///
/// `aroundEach` runs its closure on both sides of an example, so it reports
/// [`TestLifecycleDirection::Ambiguous`]. Quick spells that hook as a call, and
/// the shared call-role rule reaches the same `fixture_setup` from the name.
///
/// Swift Testing's `init`/`deinit` are absent on purpose: those names carry no
/// test meaning outside a suite, so the Swift container pass assigns their
/// roles the way the xUnit constructor rule does.
fn swift_test_lifecycle_direction(name: &str) -> TestLifecycleDirection {
    match name {
        "setUp" | "setUpWithError" => TestLifecycleDirection::Setup,
        "tearDown" | "tearDownWithError" => TestLifecycleDirection::Teardown,
        "aroundEach" => TestLifecycleDirection::Ambiguous,
        _ => TestLifecycleDirection::None,
    }
}

/// XCTest collects a method whose name starts with `test`; Swift Testing
/// collects whatever the `@Test` macro names.
///
/// The macro is definitive, so it carries no path guard and a `@Test` function
/// in `Sources/` is a real case. The name convention is ordinary Swift
/// everywhere else, so it keeps the path guard and the Swift container pass
/// scopes it further.
fn detect_swift(name: &str, file_path: &str, annotation_keys: &[String]) -> bool {
    if annotation_keys
        .iter()
        .any(|key| key == SWIFT_TEST_MACRO_KEY)
    {
        return true;
    }
    is_test_path(file_path)
        && (name.starts_with("test") || swift_test_lifecycle_direction(name).is_lifecycle())
}

/// The normalized annotation key for Swift Testing's `@Test` macro. The Swift
/// annotation normalizer lower-cases the macro name and drops its argument
/// list, so `@Test` and `@Test(arguments:)` share this key.
pub(crate) const SWIFT_TEST_MACRO_KEY: &str = "test";

/// The normalized annotation key for Swift Testing's `@Suite` macro.
pub(crate) const SWIFT_SUITE_MACRO_KEY: &str = "suite";

/// ExUnit discovers tests only through the `test`/`property` macros, which the
/// Elixir extractor marks itself. A `def test_*` function is never a test.
fn detect_elixir(name: &str) -> bool {
    name.starts_with("test ")
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
fn common_test_lifecycle_direction(name: &str) -> TestLifecycleDirection {
    match name {
        "init_per_suite" | "init_per_testcase" | "init_per_group" => TestLifecycleDirection::Setup,
        "end_per_suite" | "end_per_testcase" | "end_per_group" => TestLifecycleDirection::Teardown,
        _ => TestLifecycleDirection::None,
    }
}

/// Common Test callbacks that describe a suite instead of exercising it.
const COMMON_TEST_CONFIG_NAMES: [&str; 4] = ["all", "groups", "group", "suite"];

/// Common Test runs every test case as `Case(Config)`.
const COMMON_TEST_CASE_ARITY: u32 = 1;

/// Whether an Erlang module hosts EUnit tests, Common Test cases, PropEr
/// properties, or several of them. The frameworks are independent — a
/// `*_SUITE` module may also include `eunit.hrl` — so the classification
/// carries a flag per framework.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct ErlangTestModule {
    eunit: bool,
    common_test: bool,
    proper: bool,
}

impl ErlangTestModule {
    pub(crate) fn classify(
        module_name: &str,
        includes_eunit_header: bool,
        includes_proper_header: bool,
    ) -> Self {
        Self {
            eunit: includes_eunit_header || module_name.ends_with("_tests"),
            common_test: module_name.ends_with("_SUITE"),
            proper: includes_proper_header || module_name.starts_with("prop_"),
        }
    }

    pub(crate) fn is_test_container(&self) -> bool {
        self.eunit || self.common_test || self.proper
    }

    pub(crate) fn is_common_test(&self) -> bool {
        self.common_test
    }
}

/// Classify an Erlang function against EUnit, Common Test, and PropEr.
///
/// Common Test dispatches on exact callback names inside a `*_SUITE` module and
/// runs the `Case(Config)` functions its `all/0` and `groups/0` list. When the
/// suite lists them literally, `listed_cases` holds those names and nothing else
/// is a case; otherwise every other exported `Case(Config)` counts. EUnit
/// matches the name suffix on any zero-arity function, in any module, because
/// EUnit test modules are not required to be named or located in a particular
/// way. PropEr runs the zero-arity `prop_*` functions of a PropEr module.
pub(crate) fn erlang_test_role(
    module: ErlangTestModule,
    name: &str,
    arity: u32,
    exported: bool,
    listed_cases: Option<&std::collections::HashSet<String>>,
) -> Option<TestRole> {
    if module.common_test {
        if let Some(role) = common_test_lifecycle_direction(name).fixture_role() {
            return Some(role);
        }
        let is_case = match listed_cases {
            Some(cases) => cases.contains(name),
            None => exported && !COMMON_TEST_CONFIG_NAMES.contains(&name),
        };
        if is_case && arity == COMMON_TEST_CASE_ARITY {
            return Some(TestRole::TestCase);
        }
    }

    let is_property = module.proper && name.starts_with("prop_");
    (arity == 0 && (is_property || detect_erlang(name))).then_some(TestRole::TestCase)
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

/// bats `setup`/`setup_file`/`setup_suite`, shunit2 `setUp`/`oneTimeSetUp`,
/// bashunit `set_up`/`set_up_before_script`, and their teardown halves.
fn bash_test_lifecycle_direction(name: &str) -> TestLifecycleDirection {
    match name.to_ascii_lowercase().as_str() {
        "setup"
        | "setup_file"
        | "setup_suite"
        | "onetimesetup"
        | "set_up"
        | "set_up_before_script" => TestLifecycleDirection::Setup,
        "teardown"
        | "teardown_file"
        | "teardown_suite"
        | "onetimeteardown"
        | "tear_down"
        | "tear_down_after_script" => TestLifecycleDirection::Teardown,
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

/// Lua luaunit: test functions/methods are `testXxx` (camelCase) or `test_xxx`,
/// and `setUp`/`tearDown` (plus the suite and class hooks) are fixtures.
/// busted (`describe`/`it`) is call-style and handled in `test_calls`, not here.
fn detect_lua(name: &str, file_path: &str) -> bool {
    is_test_path(file_path)
        && (name.starts_with("test") || lua_test_lifecycle_direction(name).is_lifecycle())
}

fn lua_test_lifecycle_direction(name: &str) -> TestLifecycleDirection {
    match name {
        "setUp" | "setupSuite" | "setupClass" => TestLifecycleDirection::Setup,
        "tearDown" | "teardownSuite" | "teardownClass" => TestLifecycleDirection::Teardown,
        _ => TestLifecycleDirection::None,
    }
}

/// R RUnit: in a `runit*.R` file (RUnit's default `testFileRegexp`), functions
/// matching `^test.+` are tests and `.setUp` / `.tearDown` are fixtures.
/// testthat (`test_that("...")`) is call-style and handled in `test_calls`, so a
/// plain helper function in a testthat file is not a test.
fn detect_r(name: &str, file_path: &str) -> bool {
    is_runit_file(file_path)
        && ((name.len() > 4 && name.starts_with("test"))
            || r_test_lifecycle_direction(name).is_lifecycle())
}

fn is_runit_file(file_path: &str) -> bool {
    file_path
        .rsplit(PATH_SEPARATORS)
        .next()
        .is_some_and(|file_name| file_name.starts_with("runit"))
}

fn r_test_lifecycle_direction(name: &str) -> TestLifecycleDirection {
    match name {
        ".setUp" => TestLifecycleDirection::Setup,
        ".tearDown" => TestLifecycleDirection::Teardown,
        _ => TestLifecycleDirection::None,
    }
}

// ---------------------------------------------------------------------------
// Generic fallback — for the ~20 languages without specific frameworks
// ---------------------------------------------------------------------------

fn detect_generic(name: &str, file_path: &str) -> bool {
    let has_test_name = name.starts_with("test_") || name.starts_with("Test");
    has_test_name && is_test_path(file_path)
}
