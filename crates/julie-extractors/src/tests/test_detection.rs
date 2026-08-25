// Tests for test_detection module — is_test_symbol() function
//
// Covers all 34 languages via language-specific rules + generic fallback.

use crate::base::SymbolKind;
use crate::test_detection::{apply_callable_test_metadata, is_test_symbol};
use std::collections::HashMap;

// ---------------------------------------------------------------------------
// Helper: shorthand for common no-annotations/no-doc calls
// ---------------------------------------------------------------------------

fn check(
    language: &str,
    name: &str,
    file_path: &str,
    kind: &SymbolKind,
    annotation_keys: &[String],
    doc_comment: Option<&str>,
) -> bool {
    is_test_symbol(
        language,
        name,
        file_path,
        kind,
        annotation_keys,
        doc_comment,
    )
}

fn s(val: &str) -> String {
    val.to_string()
}

// ===========================================================================
// Rust
// ===========================================================================

#[test]
fn rust_test_attribute() {
    assert!(check(
        "rust",
        "test_add",
        "src/tests/math.rs",
        &SymbolKind::Function,
        &[s("test")],
        None,
    ));
}

#[test]
fn rust_tokio_test_attribute() {
    assert!(check(
        "rust",
        "test_async_fetch",
        "src/lib.rs",
        &SymbolKind::Function,
        &[s("tokio::test")],
        None,
    ));
}

#[test]
fn rust_rstest_attribute() {
    assert!(check(
        "rust",
        "my_parameterized",
        "src/lib.rs",
        &SymbolKind::Function,
        &[s("rstest")],
        None,
    ));
}

#[test]
fn rust_no_test_attr() {
    assert!(!check(
        "rust",
        "process_data",
        "src/lib.rs",
        &SymbolKind::Function,
        &[s("inline")],
        None,
    ));
}

// ===========================================================================
// Python
// ===========================================================================

#[test]
fn python_pytest_decorator() {
    assert!(check(
        "python",
        "test_payment",
        "tests/test_payment.py",
        &SymbolKind::Function,
        &[s("pytest.mark.parametrize")],
        None,
    ));
}

#[test]
fn python_mock_decorator_is_not_test_evidence() {
    assert!(!check(
        "python",
        "patch_client",
        "tests/test_helpers.py",
        &SymbolKind::Function,
        &[s("unittest.mock.patch")],
        None,
    ));
}

#[test]
fn python_fixture_and_mock_decorators_preserve_independent_test_evidence() {
    assert!(check(
        "python",
        "test_fixture",
        "tests/test_helpers.py",
        &SymbolKind::Function,
        &[s("pytest.fixture")],
        None,
    ));
    assert!(check(
        "python",
        "test_patch",
        "tests/test_helpers.py",
        &SymbolKind::Function,
        &[s("unittest.mock.patch")],
        None,
    ));
}

#[test]
fn python_unittest_test_control_decorators_are_test_evidence() {
    for annotation in [
        "unittest.skip",
        "unittest.skipif",
        "unittest.skipunless",
        "unittest.expectedfailure",
    ] {
        assert!(check(
            "python",
            "ordinary_helper",
            "src/helpers.py",
            &SymbolKind::Function,
            &[s(annotation)],
            None,
        ));
    }
}

#[test]
fn python_unittest_decorator() {
    assert!(check(
        "python",
        "test_thing",
        "tests/test_thing.py",
        &SymbolKind::Method,
        &[s("unittest.skip")],
        None,
    ));
}

#[test]
fn scala_test_path_is_not_callable_evidence() {
    assert!(!check(
        "scala",
        "helper",
        "src/test/scala/Helpers.scala",
        &SymbolKind::Function,
        &[],
        None,
    ));
}

#[test]
fn elixir_test_path_is_not_callable_evidence() {
    assert!(!check(
        "elixir",
        "helper",
        "test/helpers.exs",
        &SymbolKind::Function,
        &[],
        None,
    ));
}

#[test]
fn python_test_prefix_function() {
    assert!(check(
        "python",
        "test_login",
        "tests/test_auth.py",
        &SymbolKind::Function,
        &[],
        None,
    ));
}

#[test]
fn python_test_class_returns_false() {
    // Classes are containers, not tests themselves
    assert!(!check(
        "python",
        "TestPaymentProcessor",
        "tests/test_payment.py",
        &SymbolKind::Class,
        &[],
        None,
    ));
}

#[test]
fn python_test_prefix_in_source_not_test_path() {
    // test_result_histories in a source file should NOT be marked as a test
    assert!(!check(
        "python",
        "test_result_histories",
        "python/eros/store/sqlite.py",
        &SymbolKind::Function,
        &[],
        None,
    ));
}

#[test]
fn python_test_prefix_in_test_path_still_detected() {
    assert!(check(
        "python",
        "test_result_histories",
        "tests/store/test_sqlite.py",
        &SymbolKind::Function,
        &[],
        None,
    ));
}

#[test]
fn python_pytest_decorator_in_source_still_detected() {
    // Annotation-driven detection stays path-independent
    assert!(check(
        "python",
        "test_something",
        "src/mypackage/helpers.py",
        &SymbolKind::Function,
        &[s("pytest.mark.parametrize")],
        None,
    ));
}

#[test]
fn python_regular_function() {
    assert!(!check(
        "python",
        "process_payment",
        "src/payment.py",
        &SymbolKind::Function,
        &[],
        None,
    ));
}

// ===========================================================================
// Java / Kotlin
// ===========================================================================

#[test]
fn java_test_annotation() {
    assert!(check(
        "java",
        "shouldProcessPayment",
        "src/test/java/PaymentTest.java",
        &SymbolKind::Method,
        &[s("test")],
        None,
    ));
}

#[test]
fn java_parameterized_test() {
    assert!(check(
        "java",
        "testWithParams",
        "src/test/java/PaymentTest.java",
        &SymbolKind::Method,
        &[s("parameterizedtest")],
        None,
    ));
}

#[test]
fn java_repeated_test() {
    assert!(check(
        "java",
        "testRepeated",
        "src/test/java/PaymentTest.java",
        &SymbolKind::Method,
        &[s("repeatedtest")],
        None,
    ));
}

#[test]
fn java_regular_method() {
    assert!(!check(
        "java",
        "processPayment",
        "src/main/java/Payment.java",
        &SymbolKind::Method,
        &[s("override")],
        None,
    ));
}

#[test]
fn kotlin_test_annotation() {
    assert!(check(
        "kotlin",
        "shouldReturnUser",
        "src/test/kotlin/UserTest.kt",
        &SymbolKind::Method,
        &[s("test")],
        None,
    ));
}

// ===========================================================================
// C#
// ===========================================================================

#[test]
fn csharp_fact_attribute() {
    assert!(check(
        "csharp",
        "ShouldProcessOrder",
        "MyProject.Tests/OrderTests.cs",
        &SymbolKind::Method,
        &[s("fact")],
        None,
    ));
}

#[test]
fn csharp_theory_attribute() {
    assert!(check(
        "csharp",
        "ShouldCalculateTotal",
        "MyProject.Tests/OrderTests.cs",
        &SymbolKind::Method,
        &[s("theory")],
        None,
    ));
}

#[test]
fn csharp_test_attribute() {
    assert!(check(
        "csharp",
        "TestOrder",
        "MyProject.Tests/OrderTests.cs",
        &SymbolKind::Method,
        &[s("test")],
        None,
    ));
}

#[test]
fn csharp_test_method_attribute() {
    assert!(check(
        "csharp",
        "TestMethod1",
        "MyProject.Tests/OrderTests.cs",
        &SymbolKind::Method,
        &[s("testmethod")],
        None,
    ));
}

#[test]
fn csharp_normalized_fact_attribute() {
    // C# extractor markers are normalized before test detection.
    assert!(check(
        "csharp",
        "ShouldValidateInput",
        "MyProject.Tests/ValidationTests.cs",
        &SymbolKind::Method,
        &[s("fact")],
        None,
    ));
}

#[test]
fn csharp_normalized_theory_attribute() {
    assert!(check(
        "csharp",
        "ShouldCalculateDiscount",
        "MyProject.Tests/PricingTests.cs",
        &SymbolKind::Method,
        &[s("theory")],
        None,
    ));
}

// ===========================================================================
// Razor (routes to C# detection)
// ===========================================================================

#[test]
fn razor_routes_to_csharp_fact_attribute() {
    // Razor files with C# annotation keys should route through detect_csharp.
    assert!(check(
        "razor",
        "ShouldRenderComponent",
        "MyProject.Tests/Components/ButtonTests.cshtml",
        &SymbolKind::Method,
        &[s("fact")],
        None,
    ));
}

#[test]
fn razor_routes_to_csharp_test_attribute() {
    assert!(check(
        "razor",
        "TestRender",
        "MyProject.Tests/Views/IndexTests.cshtml",
        &SymbolKind::Method,
        &[s("test")],
        None,
    ));
}

#[test]
fn razor_no_test_attribute_returns_false() {
    // Razor method without test annotation keys should not be flagged.
    assert!(!check(
        "razor",
        "OnGet",
        "MyProject/Pages/Index.cshtml",
        &SymbolKind::Method,
        &[s("httpget")],
        None,
    ));
}

// ===========================================================================
// Go
// ===========================================================================

#[test]
fn go_test_function_in_test_file() {
    assert!(check(
        "go",
        "TestProcessPayment",
        "payment/payment_test.go",
        &SymbolKind::Function,
        &[],
        None,
    ));
}

#[test]
fn go_test_name_not_in_test_file() {
    // Go requires BOTH the Test prefix AND the _test.go file
    assert!(!check(
        "go",
        "TestHelper",
        "payment/helpers.go",
        &SymbolKind::Function,
        &[],
        None,
    ));
}

#[test]
fn go_benchmark_not_test() {
    // BenchmarkX isn't a test function for our purposes
    assert!(!check(
        "go",
        "BenchmarkProcess",
        "payment/payment_test.go",
        &SymbolKind::Function,
        &[],
        None,
    ));
}

#[test]
fn go_fuzz_function_in_test_file() {
    assert!(check(
        "go",
        "FuzzParseInput",
        "parser/parser_test.go",
        &SymbolKind::Function,
        &[],
        None,
    ));
}

#[test]
fn go_example_function_in_test_file() {
    assert!(check(
        "go",
        "ExampleProcessPayment",
        "payment/payment_test.go",
        &SymbolKind::Function,
        &[],
        None,
    ));
}

// ===========================================================================
// JavaScript / TypeScript
// ===========================================================================

#[test]
fn js_test_in_test_file() {
    assert!(check(
        "javascript",
        "test",
        "src/__tests__/payment.test.js",
        &SymbolKind::Function,
        &[],
        None,
    ));
}

#[test]
fn ts_describe_in_spec_file() {
    assert!(check(
        "typescript",
        "describe",
        "src/payment.spec.ts",
        &SymbolKind::Function,
        &[],
        None,
    ));
}

#[test]
fn js_it_in_test_file() {
    assert!(check(
        "javascript",
        "it",
        "tests/payment.test.js",
        &SymbolKind::Function,
        &[],
        None,
    ));
}

#[test]
fn ts_test_function_not_in_test_file() {
    // "test" function in production code is NOT a test
    assert!(!check(
        "typescript",
        "test",
        "src/utils.ts",
        &SymbolKind::Function,
        &[],
        None,
    ));
}

// ===========================================================================
// PHP
// ===========================================================================

#[test]
fn php_test_annotation_key() {
    assert!(check(
        "php",
        "itShouldProcess",
        "tests/PaymentTest.php",
        &SymbolKind::Method,
        &[s("test")],
        None,
    ));
}

#[test]
fn php_doc_comment_test_fallback_still_works() {
    assert!(check(
        "php",
        "itShouldProcess",
        "tests/PaymentTest.php",
        &SymbolKind::Method,
        &[],
        Some("/** @test */"),
    ));
}

#[test]
fn php_test_prefix() {
    assert!(check(
        "php",
        "testProcessPayment",
        "tests/PaymentTest.php",
        &SymbolKind::Method,
        &[],
        None,
    ));
}

// ===========================================================================
// Ruby
// ===========================================================================

#[test]
fn ruby_test_prefix_in_spec_dir() {
    assert!(check(
        "ruby",
        "test_process_payment",
        "spec/payment_spec.rb",
        &SymbolKind::Method,
        &[],
        None,
    ));
}

#[test]
fn ruby_test_prefix_in_test_dir() {
    assert!(check(
        "ruby",
        "test_login",
        "test/auth_test.rb",
        &SymbolKind::Method,
        &[],
        None,
    ));
}

#[test]
fn test_bash_powershell_and_ruby_test_framework_detection_covers_common_frameworks() {
    // test_ prefix in non-test path is NOT a test (path guard added)
    assert!(!check(
        "bash",
        "test_helper",
        "scripts/build.sh",
        &SymbolKind::Function,
        &[],
        None,
    ));
    // test_ prefix in test path IS a test
    assert!(check(
        "bash",
        "test_helper",
        "tests/build.sh",
        &SymbolKind::Function,
        &[],
        None,
    ));

    assert!(check(
        "bash",
        "describe",
        "spec/build.sh",
        &SymbolKind::Function,
        &[],
        None,
    ));

    assert!(!check(
        "bash",
        "setup",
        "scripts/build.sh",
        &SymbolKind::Function,
        &[],
        None,
    ));

    assert!(check(
        "powershell",
        "Describe",
        "tests/build.ps1",
        &SymbolKind::Function,
        &[],
        None,
    ));

    assert!(!check(
        "powershell",
        "Test-Connection",
        "tests/network.ps1",
        &SymbolKind::Function,
        &[],
        None,
    ));

    // test_ prefix in non-test path is NOT a test (path guard added)
    assert!(!check(
        "ruby",
        "test_login",
        "lib/payment.rb",
        &SymbolKind::Method,
        &[],
        None,
    ));
    // test_ prefix in test path IS a test
    assert!(check(
        "ruby",
        "test_login",
        "test/payment_test.rb",
        &SymbolKind::Method,
        &[],
        None,
    ));

    assert!(check(
        "ruby",
        "it",
        "spec/payment_spec.rb",
        &SymbolKind::Method,
        &[],
        None,
    ));

    assert!(!check(
        "ruby",
        "it",
        "lib/payment.rb",
        &SymbolKind::Method,
        &[],
        None,
    ));
}

// ===========================================================================
// Swift
// ===========================================================================

#[test]
fn swift_test_prefix_method() {
    assert!(check(
        "swift",
        "testLogin",
        "Tests/AuthTests.swift",
        &SymbolKind::Method,
        &[],
        None,
    ));
}

#[test]
fn swift_test_prefix_function() {
    assert!(check(
        "swift",
        "testNetworkCall",
        "Tests/NetworkTests.swift",
        &SymbolKind::Function,
        &[],
        None,
    ));
}

#[test]
fn swift_class_with_test_prefix_returns_false() {
    // Classes aren't callable
    assert!(!check(
        "swift",
        "TestHelper",
        "Tests/Helpers.swift",
        &SymbolKind::Class,
        &[],
        None,
    ));
}

// ===========================================================================
// Dart
// ===========================================================================

#[test]
fn dart_is_test_decorator() {
    assert!(check(
        "dart",
        "myTest",
        "test/widget_test.dart",
        &SymbolKind::Function,
        &[s("istest")],
        None,
    ));
}

#[test]
fn dart_test_prefix() {
    assert!(check(
        "dart",
        "testWidgetRendering",
        "test/widget_test.dart",
        &SymbolKind::Function,
        &[],
        None,
    ));
}

#[test]
fn dart_test_prefix_in_production_code_returns_false() {
    // testWidgetRendering in lib/widgets.dart is NOT a test — path guard prevents false positive
    assert!(!check(
        "dart",
        "testWidgetRendering",
        "lib/widgets.dart",
        &SymbolKind::Function,
        &[],
        None,
    ));
}

// ===========================================================================
// Explicit framework arms beyond the generic fallback
// (gdscript GUT, lua luaunit, r RUnit, java JUnit3)
// ===========================================================================

#[test]
fn gdscript_gut_camelcase_method_in_test_path() {
    // GUT runs any `test`-prefixed method, including camelCase `testFoo` (no
    // underscore) — which the generic fallback (test_/Test only) misses.
    assert!(check(
        "gdscript",
        "testPlayerHealth",
        "test/player_test.gd",
        &SymbolKind::Method,
        &[],
        None,
    ));
}

#[test]
fn gdscript_non_test_method_in_test_path_returns_false() {
    assert!(!check(
        "gdscript",
        "helperFunction",
        "test/player_test.gd",
        &SymbolKind::Method,
        &[],
        None,
    ));
}

#[test]
fn gdscript_test_prefix_in_production_path_returns_false() {
    // Path guard: a `test`-prefixed method outside a test path is not a test.
    assert!(!check(
        "gdscript",
        "testConnection",
        "src/network.gd",
        &SymbolKind::Method,
        &[],
        None,
    ));
}

#[test]
fn lua_luaunit_camelcase_function_in_test_path() {
    // luaunit runs `testXxx` (camelCase) functions; generic fallback misses these.
    assert!(check(
        "lua",
        "testAddition",
        "tests/math_spec.lua",
        &SymbolKind::Function,
        &[],
        None,
    ));
}

#[test]
fn lua_test_underscore_prefix_still_detected() {
    assert!(check(
        "lua",
        "test_subtraction",
        "tests/math_spec.lua",
        &SymbolKind::Function,
        &[],
        None,
    ));
}

#[test]
fn lua_non_test_function_in_test_path_returns_false() {
    assert!(!check(
        "lua",
        "make_fixture",
        "tests/math_spec.lua",
        &SymbolKind::Function,
        &[],
        None,
    ));
}

#[test]
fn r_runit_dot_prefix_in_test_path() {
    // RUnit names tests `test.foo` (dot) — the generic fallback only checks
    // `test_`/`Test`, so detect_r adds the dot convention.
    assert!(check(
        "r",
        "test.addition",
        "tests/runit_math.R",
        &SymbolKind::Function,
        &[],
        None,
    ));
}

#[test]
fn r_test_underscore_prefix_in_test_path() {
    assert!(check(
        "r",
        "test_addition",
        "tests/test_math.R",
        &SymbolKind::Function,
        &[],
        None,
    ));
}

#[test]
fn r_non_test_function_in_test_path_returns_false() {
    assert!(!check(
        "r",
        "build_data",
        "tests/runit_math.R",
        &SymbolKind::Function,
        &[],
        None,
    ));
}

#[test]
fn java_junit3_testxxx_method_without_annotation_in_test_path() {
    // JUnit3: `public void testLegacy()` in a TestCase subclass, no @Test
    // annotation. The annotation-only detector misses it; the path-guarded name
    // fallback catches it.
    assert!(check(
        "java",
        "testLegacyBehavior",
        "src/test/java/LegacyTest.java",
        &SymbolKind::Method,
        &[],
        None,
    ));
}

#[test]
fn java_junit3_non_test_method_in_test_path_returns_false() {
    assert!(!check(
        "java",
        "helperMethod",
        "src/test/java/LegacyTest.java",
        &SymbolKind::Method,
        &[],
        None,
    ));
}

#[test]
fn java_test_prefix_in_production_path_returns_false() {
    // Path guard: a `test`-prefixed method in production code with no annotation
    // is not a test (mirrors the swift/php convention).
    assert!(!check(
        "java",
        "testConnection",
        "src/main/java/Database.java",
        &SymbolKind::Method,
        &[],
        None,
    ));
}

// ===========================================================================
// Generic fallback (covers remaining ~20 languages)
// ===========================================================================

#[test]
fn generic_test_underscore_prefix_in_test_path() {
    assert!(check(
        "lua",
        "test_something",
        "tests/test_util.lua",
        &SymbolKind::Function,
        &[],
        None,
    ));
}

#[test]
fn generic_test_capital_prefix_in_test_path() {
    assert!(check(
        "zig",
        "TestAllocator",
        "tests/allocator_test.zig",
        &SymbolKind::Function,
        &[],
        None,
    ));
}

// ===========================================================================
// Test lifecycle methods (setUp, tearDown, etc.)
// ===========================================================================

#[test]
fn csharp_setup_is_test() {
    assert!(check(
        "csharp",
        "SetUp",
        "Tests/MyTests.cs",
        &SymbolKind::Method,
        &[s("setup")],
        None
    ));
}

#[test]
fn csharp_teardown_is_test() {
    assert!(check(
        "csharp",
        "TearDown",
        "Tests/MyTests.cs",
        &SymbolKind::Method,
        &[s("teardown")],
        None
    ));
}

#[test]
fn csharp_onetime_setup_is_test() {
    assert!(check(
        "csharp",
        "Initialize",
        "Tests/MyTests.cs",
        &SymbolKind::Method,
        &[s("onetimesetup")],
        None
    ));
}

#[test]
fn java_before_each_is_test() {
    assert!(check(
        "java",
        "setup",
        "src/test/MyTest.java",
        &SymbolKind::Method,
        &[s("beforeeach")],
        None
    ));
}

#[test]
fn python_setup_is_test() {
    assert!(check(
        "python",
        "setUp",
        "tests/test_foo.py",
        &SymbolKind::Method,
        &[],
        None
    ));
}

#[test]
fn swift_setup_is_test() {
    assert!(check(
        "swift",
        "setUp",
        "Tests/MyTests.swift",
        &SymbolKind::Method,
        &[],
        None
    ));
}

// ===========================================================================
// PHP — path guard for name-prefix detection
// ===========================================================================

#[test]
fn php_test_prefix_in_production_code_returns_false() {
    // testConnection() in a production PHP service should NOT be flagged as a test
    assert!(!check(
        "php",
        "testConnection",
        "src/services/database.php",
        &SymbolKind::Method,
        &[],
        None,
    ));
}

#[test]
fn php_test_annotation_in_production_code_returns_true() {
    // A normalized @test annotation key is a genuine test marker regardless of file path.
    assert!(check(
        "php",
        "someMethod",
        "src/services/database.php",
        &SymbolKind::Method,
        &[s("test")],
        None,
    ));
}

// ===========================================================================
// Swift — path guard for name-prefix detection
// ===========================================================================

#[test]
fn swift_test_prefix_in_production_code_returns_false() {
    // testConnection() in a production Swift file should NOT be flagged as a test
    assert!(!check(
        "swift",
        "testConnection",
        "Sources/App/Database.swift",
        &SymbolKind::Method,
        &[],
        None,
    ));
}

#[test]
fn swift_setup_in_production_code_returns_false() {
    // setUp() outside a test directory is NOT a test lifecycle method
    assert!(!check(
        "swift",
        "setUp",
        "Sources/App/Database.swift",
        &SymbolKind::Method,
        &[],
        None,
    ));
}

// ===========================================================================
// False positive prevention
// ===========================================================================

#[test]
fn false_positive_production_function_with_test_in_name() {
    // A production utility function that happens to have "test" in its name
    // should NOT be flagged as a test if it's not in a test path
    assert!(!check(
        "rust",
        "test_connection_pool",
        "src/database/pool.rs",
        &SymbolKind::Function,
        &[],
        None,
    ));
}

#[test]
fn false_positive_test_helper_in_prod_code() {
    assert!(!check(
        "python",
        "create_test_user",
        "src/factories.py",
        &SymbolKind::Function,
        &[],
        None,
    ));
}

// ===========================================================================
// Non-callable symbol kind filter
// ===========================================================================

#[test]
fn struct_named_test_fixture_returns_false() {
    assert!(!check(
        "rust",
        "TestFixture",
        "src/tests/fixtures.rs",
        &SymbolKind::Struct,
        &[s("test")],
        None,
    ));
}

#[test]
fn enum_named_test_variant_returns_false() {
    assert!(!check(
        "java",
        "TestStatus",
        "src/test/java/Status.java",
        &SymbolKind::Enum,
        &[s("test")],
        None,
    ));
}

#[test]
fn interface_returns_false() {
    assert!(!check(
        "csharp",
        "ITestService",
        "MyProject.Tests/ITestService.cs",
        &SymbolKind::Interface,
        &[s("fact")],
        None,
    ));
}

#[test]
fn variable_returns_false() {
    assert!(!check(
        "javascript",
        "test",
        "src/__tests__/payment.test.js",
        &SymbolKind::Variable,
        &[],
        None,
    ));
}

#[test]
fn constant_returns_false() {
    assert!(!check(
        "typescript",
        "TEST_TIMEOUT",
        "src/payment.spec.ts",
        &SymbolKind::Constant,
        &[],
        None,
    ));
}

// ===========================================================================
// Constructor edge case — constructors ARE callable
// ===========================================================================

#[test]
fn constructor_with_test_attr_returns_true() {
    // Constructors are callable, so if they have test annotation keys, they count.
    assert!(check(
        "csharp",
        "TestSetup",
        "MyProject.Tests/Setup.cs",
        &SymbolKind::Constructor,
        &[s("testmethod")],
        None,
    ));
}

// ===========================================================================
// Integration tests — run actual extractors and verify is_test metadata
// ===========================================================================

/// Helper: extract symbols from code using the specified language extractor
fn extract_symbols_for(language: &str, file_path: &str, code: &str) -> Vec<crate::base::Symbol> {
    let workspace_root = std::path::PathBuf::from("/tmp/test");
    let tree = super::helpers::init_parser(code, language);
    let results = crate::factory::extract_symbols_and_relationships(
        &tree,
        file_path,
        code,
        language,
        &workspace_root,
    )
    .expect("Extraction should succeed");
    results.symbols
}

/// Helper: check if a symbol has is_test=true in its metadata
fn has_is_test(symbol: &crate::base::Symbol) -> bool {
    symbol
        .metadata
        .as_ref()
        .and_then(|m| m.get("is_test"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
}

#[test]
fn integration_rust_test_function_detected() {
    let code = r#"
#[test]
fn test_addition() {
    assert_eq!(2 + 2, 4);
}

fn regular_function() {
    println!("hello");
}
"#;
    let symbols = extract_symbols_for("rust", "src/tests/math.rs", code);

    let test_fn = symbols.iter().find(|s| s.name == "test_addition");
    assert!(test_fn.is_some(), "Should extract test_addition");
    assert!(
        has_is_test(test_fn.unwrap()),
        "test_addition should have is_test=true"
    );

    let regular_fn = symbols.iter().find(|s| s.name == "regular_function");
    assert!(regular_fn.is_some(), "Should extract regular_function");
    assert!(
        !has_is_test(regular_fn.unwrap()),
        "regular_function should NOT have is_test"
    );
}

#[test]
fn integration_python_test_function_detected() {
    let code = r#"
def test_payment_processing():
    assert process_payment() == True

def helper_function():
    return 42
"#;
    let symbols = extract_symbols_for("python", "tests/test_payment.py", code);

    let test_fn = symbols.iter().find(|s| s.name == "test_payment_processing");
    assert!(test_fn.is_some(), "Should extract test_payment_processing");
    assert!(
        has_is_test(test_fn.unwrap()),
        "test_payment_processing should have is_test=true"
    );

    let helper_fn = symbols.iter().find(|s| s.name == "helper_function");
    assert!(helper_fn.is_some(), "Should extract helper_function");
    assert!(
        !has_is_test(helper_fn.unwrap()),
        "helper_function should NOT have is_test"
    );
}

#[test]
fn integration_go_test_function_detected() {
    let code = r#"
package payment

import "testing"

func TestProcessPayment(t *testing.T) {
    if true {
        t.Fatal("failed")
    }
}

func processPayment() bool {
    return true
}
"#;
    let symbols = extract_symbols_for("go", "payment/payment_test.go", code);

    let test_fn = symbols.iter().find(|s| s.name == "TestProcessPayment");
    assert!(test_fn.is_some(), "Should extract TestProcessPayment");
    assert!(
        has_is_test(test_fn.unwrap()),
        "TestProcessPayment should have is_test=true"
    );

    let regular_fn = symbols.iter().find(|s| s.name == "processPayment");
    assert!(regular_fn.is_some(), "Should extract processPayment");
    assert!(
        !has_is_test(regular_fn.unwrap()),
        "processPayment should NOT have is_test"
    );
}

#[test]
fn integration_regular_function_no_test_metadata() {
    // A regular Rust function outside test context should not get is_test
    let code = r#"
pub fn calculate_sum(a: i32, b: i32) -> i32 {
    a + b
}
"#;
    let symbols = extract_symbols_for("rust", "src/math.rs", code);

    let sum_fn = symbols.iter().find(|s| s.name == "calculate_sum");
    assert!(sum_fn.is_some(), "Should extract calculate_sum");
    assert!(
        !has_is_test(sum_fn.unwrap()),
        "calculate_sum should NOT have is_test"
    );
}

// ===========================================================================
// Comprehensive dispatch test — exercises every `match language` arm in
// `is_test_symbol` plus the `is_callable` gate and generic fallback.
// ===========================================================================

#[test]
fn test_is_test_symbol_dispatch_across_languages() {
    // Each tuple: (language, name, file_path, kind, annotation_keys, doc_comment, expected)
    type Case = (
        &'static str,
        &'static str,
        &'static str,
        SymbolKind,
        Vec<String>,
        Option<&'static str>,
        bool,
    );

    let cases: Vec<Case> = vec![
        // --- Rust: annotation-key-driven only ---
        (
            "rust",
            "test_add",
            "src/tests/math.rs",
            SymbolKind::Function,
            vec![s("test")],
            None,
            true,
        ),
        (
            "rust",
            "test_async",
            "src/lib.rs",
            SymbolKind::Function,
            vec![s("tokio::test")],
            None,
            true,
        ),
        // Rust: test_ prefix without #[test] marker returns false.
        (
            "rust",
            "test_something",
            "src/tests/foo.rs",
            SymbolKind::Function,
            vec![],
            None,
            false,
        ),
        // Rust: regular function
        (
            "rust",
            "process_data",
            "src/lib.rs",
            SymbolKind::Function,
            vec![],
            None,
            false,
        ),
        // --- Python: annotation key or name prefix (no path guard) ---
        (
            "python",
            "test_login",
            "tests/test_auth.py",
            SymbolKind::Function,
            vec![],
            None,
            true,
        ),
        // Python: test_ prefix in non-test path is NOT a test (path guard added)
        (
            "python",
            "test_login",
            "src/auth.py",
            SymbolKind::Function,
            vec![],
            None,
            false,
        ),
        // Python: pytest annotation key
        (
            "python",
            "some_check",
            "tests/test_auth.py",
            SymbolKind::Function,
            vec![s("pytest.mark.parametrize")],
            None,
            true,
        ),
        // Python: setUp lifecycle method
        (
            "python",
            "setUp",
            "tests/test_foo.py",
            SymbolKind::Method,
            vec![],
            None,
            true,
        ),
        // Python: regular function
        (
            "python",
            "login",
            "src/auth.py",
            SymbolKind::Function,
            vec![],
            None,
            false,
        ),
        // --- Java: @Test annotation key ---
        (
            "java",
            "shouldReturnTrue",
            "src/test/java/FooTest.java",
            SymbolKind::Method,
            vec![s("test")],
            None,
            true,
        ),
        // Java: @BeforeEach lifecycle
        (
            "java",
            "init",
            "src/test/java/FooTest.java",
            SymbolKind::Method,
            vec![s("beforeeach")],
            None,
            true,
        ),
        // Java: no annotation → false
        (
            "java",
            "processOrder",
            "src/main/java/Order.java",
            SymbolKind::Method,
            vec![],
            None,
            false,
        ),
        // --- Kotlin: shares java/kotlin detector ---
        (
            "kotlin",
            "shouldReturnUser",
            "src/test/kotlin/UserTest.kt",
            SymbolKind::Method,
            vec![s("test")],
            None,
            true,
        ),
        (
            "kotlin",
            "fetchUser",
            "src/main/kotlin/User.kt",
            SymbolKind::Method,
            vec![],
            None,
            false,
        ),
        (
            "scala",
            "shouldCompute",
            "src/test/scala/MathSpec.scala",
            SymbolKind::Method,
            vec![s("test")],
            None,
            true,
        ),
        (
            "scala",
            "compute",
            "src/test/scala/MathSpec.scala",
            SymbolKind::Method,
            vec![],
            None,
            false,
        ),
        (
            "scala",
            "testComputation",
            "src/main/scala/Math.scala",
            SymbolKind::Function,
            vec![],
            None,
            true,
        ),
        (
            "scala",
            "compute",
            "src/main/scala/Math.scala",
            SymbolKind::Function,
            vec![],
            None,
            false,
        ),
        (
            "elixir",
            "test_addition",
            "test/math_test.exs",
            SymbolKind::Function,
            vec![],
            None,
            true,
        ),
        (
            "elixir",
            "setup_context",
            "test/support/helpers.exs",
            SymbolKind::Function,
            vec![],
            None,
            false,
        ),
        (
            "elixir",
            "test greets the world",
            "lib/my_app.ex",
            SymbolKind::Function,
            vec![],
            None,
            true,
        ),
        (
            "elixir",
            "add",
            "lib/math.ex",
            SymbolKind::Function,
            vec![],
            None,
            false,
        ),
        // --- C#: annotation-key-driven ---
        (
            "csharp",
            "ShouldWork",
            "Tests/MyTest.cs",
            SymbolKind::Method,
            vec![s("fact")],
            None,
            true,
        ),
        (
            "csharp",
            "ShouldAlsoWork",
            "Tests/MyTest.cs",
            SymbolKind::Method,
            vec![s("theory")],
            None,
            true,
        ),
        // C#: normalized attribute key
        (
            "csharp",
            "ShouldValidate",
            "Tests/MyTest.cs",
            SymbolKind::Method,
            vec![s("fact")],
            None,
            true,
        ),
        // C#: no test annotation key returns false.
        (
            "csharp",
            "ProcessOrder",
            "MyProject/OrderService.cs",
            SymbolKind::Method,
            vec![],
            None,
            false,
        ),
        // --- Razor: routes to C# detector ---
        (
            "razor",
            "ShouldRender",
            "MyProject.Tests/ButtonTests.cshtml",
            SymbolKind::Method,
            vec![s("fact")],
            None,
            true,
        ),
        (
            "razor",
            "OnGet",
            "MyProject/Pages/Index.cshtml",
            SymbolKind::Method,
            vec![],
            None,
            false,
        ),
        // --- Go: Test/Fuzz/Example prefix AND _test.go file ---
        (
            "go",
            "TestParseInput",
            "parser/parser_test.go",
            SymbolKind::Function,
            vec![],
            None,
            true,
        ),
        (
            "go",
            "FuzzParse",
            "parser/parser_test.go",
            SymbolKind::Function,
            vec![],
            None,
            true,
        ),
        (
            "go",
            "ExampleParse",
            "parser/parser_test.go",
            SymbolKind::Function,
            vec![],
            None,
            true,
        ),
        // Go: Test prefix but NOT _test.go → false
        (
            "go",
            "TestParseInput",
            "parser/parser.go",
            SymbolKind::Function,
            vec![],
            None,
            false,
        ),
        // Go: _test.go but no recognized prefix → false
        (
            "go",
            "helperSetup",
            "parser/parser_test.go",
            SymbolKind::Function,
            vec![],
            None,
            false,
        ),
        // --- JavaScript: test/it/describe in test file ---
        (
            "javascript",
            "test",
            "src/__tests__/auth.test.js",
            SymbolKind::Function,
            vec![],
            None,
            true,
        ),
        (
            "javascript",
            "it",
            "tests/payment.test.js",
            SymbolKind::Function,
            vec![],
            None,
            true,
        ),
        (
            "javascript",
            "describe",
            "tests/payment.test.js",
            SymbolKind::Function,
            vec![],
            None,
            true,
        ),
        // JS: test function NOT in test file → false
        (
            "javascript",
            "test",
            "src/utils.js",
            SymbolKind::Function,
            vec![],
            None,
            false,
        ),
        // --- TypeScript: same rules as JS ---
        (
            "typescript",
            "describe",
            "src/payment.spec.ts",
            SymbolKind::Function,
            vec![],
            None,
            true,
        ),
        (
            "typescript",
            "test",
            "src/utils.ts",
            SymbolKind::Function,
            vec![],
            None,
            false,
        ),
        // --- PHP: annotation key or test prefix in test path ---
        (
            "php",
            "itShouldProcess",
            "tests/PaymentTest.php",
            SymbolKind::Method,
            vec![s("test")],
            None,
            true,
        ),
        (
            "php",
            "testProcessPayment",
            "tests/PaymentTest.php",
            SymbolKind::Method,
            vec![],
            None,
            true,
        ),
        // PHP: normalized @test key works even outside test path.
        (
            "php",
            "someMethod",
            "src/Service.php",
            SymbolKind::Method,
            vec![s("test")],
            None,
            true,
        ),
        // PHP: test prefix in prod code → false (path guard)
        (
            "php",
            "testConnection",
            "src/database.php",
            SymbolKind::Method,
            vec![],
            None,
            false,
        ),
        // --- Ruby: test_ prefix AND test/spec path ---
        (
            "ruby",
            "test_process_payment",
            "spec/payment_spec.rb",
            SymbolKind::Method,
            vec![],
            None,
            true,
        ),
        (
            "ruby",
            "test_login",
            "test/auth_test.rb",
            SymbolKind::Method,
            vec![],
            None,
            true,
        ),
        // Ruby: test_ prefix but NOT in test path → false
        (
            "ruby",
            "test_connection",
            "lib/database.rb",
            SymbolKind::Method,
            vec![],
            None,
            false,
        ),
        // --- Swift: test prefix + test path ---
        (
            "swift",
            "testLogin",
            "Tests/AuthTests.swift",
            SymbolKind::Method,
            vec![],
            None,
            true,
        ),
        // Swift: setUp lifecycle in test path
        (
            "swift",
            "setUp",
            "Tests/AuthTests.swift",
            SymbolKind::Method,
            vec![],
            None,
            true,
        ),
        // Swift: test prefix NOT in test path → false
        (
            "swift",
            "testConnection",
            "Sources/App/DB.swift",
            SymbolKind::Method,
            vec![],
            None,
            false,
        ),
        // --- Dart: @isTest annotation key or test prefix in test path ---
        (
            "dart",
            "myTest",
            "test/widget_test.dart",
            SymbolKind::Function,
            vec![s("istest")],
            None,
            true,
        ),
        (
            "dart",
            "testWidgetRendering",
            "test/widget_test.dart",
            SymbolKind::Function,
            vec![],
            None,
            true,
        ),
        // Dart: test prefix in prod code → false (path guard)
        (
            "dart",
            "testWidgetRendering",
            "lib/widgets.dart",
            SymbolKind::Function,
            vec![],
            None,
            false,
        ),
        // --- Generic fallback: test_/Test prefix + test path ---
        (
            "lua",
            "test_something",
            "tests/test_util.lua",
            SymbolKind::Function,
            vec![],
            None,
            true,
        ),
        (
            "zig",
            "TestAllocator",
            "tests/allocator_test.zig",
            SymbolKind::Function,
            vec![],
            None,
            true,
        ),
        // Generic: test_ prefix but NOT in test path → false
        (
            "lua",
            "test_helper",
            "src/utils.lua",
            SymbolKind::Function,
            vec![],
            None,
            false,
        ),
        // Unknown language falls through to generic
        (
            "brainfuck",
            "test_something",
            "tests/test.bf",
            SymbolKind::Function,
            vec![],
            None,
            true,
        ),
        (
            "brainfuck",
            "run_program",
            "src/main.bf",
            SymbolKind::Function,
            vec![],
            None,
            false,
        ),
        // --- is_callable gate: non-callable kinds always return false ---
        (
            "rust",
            "TestFixture",
            "src/tests/foo.rs",
            SymbolKind::Struct,
            vec![s("test")],
            None,
            false,
        ),
        (
            "python",
            "TestPaymentProcessor",
            "tests/test_payment.py",
            SymbolKind::Class,
            vec![],
            None,
            false,
        ),
        (
            "java",
            "TestStatus",
            "src/test/java/Status.java",
            SymbolKind::Enum,
            vec![s("test")],
            None,
            false,
        ),
        (
            "csharp",
            "ITestService",
            "Tests/ITestService.cs",
            SymbolKind::Interface,
            vec![s("fact")],
            None,
            false,
        ),
        (
            "javascript",
            "test",
            "src/__tests__/payment.test.js",
            SymbolKind::Variable,
            vec![],
            None,
            false,
        ),
        // Constructor IS callable
        (
            "csharp",
            "TestSetup",
            "MyProject.Tests/Setup.cs",
            SymbolKind::Constructor,
            vec![s("testmethod")],
            None,
            true,
        ),
    ];

    for (i, (lang, name, path, kind, annotation_keys, doc, expected)) in cases.iter().enumerate() {
        let result = is_test_symbol(lang, name, path, kind, annotation_keys, *doc);
        assert_eq!(
            result, *expected,
            "Case {} FAILED: is_test_symbol({:?}, {:?}, {:?}, {:?}) = {} but expected {}",
            i, lang, name, path, kind, result, expected,
        );
    }
}

#[test]
fn integration_zig_test_block_detected() {
    let code = r#"
const std = @import("std");

test "basic addition" {
    try std.testing.expectEqual(@as(u32, 4), 2 + 2);
}

pub fn add(a: u32, b: u32) u32 {
    return a + b;
}
"#;
    let symbols = extract_symbols_for("zig", "tests/math_test.zig", code);

    let test_block = symbols.iter().find(|s| s.name == "basic addition");
    assert!(
        test_block.is_some(),
        "Should extract test block 'basic addition'"
    );
    assert!(
        has_is_test(test_block.unwrap()),
        "Zig test block should have is_test=true"
    );

    let add_fn = symbols.iter().find(|s| s.name == "add");
    assert!(add_fn.is_some(), "Should extract add function");
    assert!(
        !has_is_test(add_fn.unwrap()),
        "add function should NOT have is_test"
    );
}

// ===========================================================================
// test_role contract — booleans and role string are written together
// ===========================================================================

fn callable_test_metadata(
    language: &str,
    name: &str,
    file_path: &str,
    annotation_keys: &[String],
) -> HashMap<String, serde_json::Value> {
    let mut metadata = HashMap::new();
    apply_callable_test_metadata(
        language,
        name,
        file_path,
        &SymbolKind::Method,
        annotation_keys,
        None,
        &mut metadata,
    );
    metadata
}

fn role(metadata: &HashMap<String, serde_json::Value>) -> Option<&str> {
    metadata.get("test_role").and_then(|value| value.as_str())
}

fn flag(metadata: &HashMap<String, serde_json::Value>, key: &str) -> bool {
    metadata.get(key).and_then(|value| value.as_bool()) == Some(true)
}

fn symbol_role(symbol: &crate::base::Symbol) -> Option<&str> {
    symbol
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get("test_role"))
        .and_then(|value| value.as_str())
}

#[test]
fn java_test_annotation_carries_the_test_case_role() {
    let metadata = callable_test_metadata(
        "java",
        "addsTwoNumbers",
        "src/test/java/CalcTest.java",
        &[s("test")],
    );
    assert_eq!(role(&metadata), Some("test_case"));
    assert!(flag(&metadata, "is_test"));
    assert!(!metadata.contains_key("test_lifecycle"));
}

#[test]
fn java_parameterized_test_annotation_still_carries_the_test_case_role() {
    let metadata = callable_test_metadata(
        "java",
        "addsPairs",
        "src/test/java/CalcTest.java",
        &[s("parameterizedtest")],
    );
    assert_eq!(role(&metadata), Some("test_case"));
}

#[test]
fn java_before_each_carries_the_fixture_setup_role() {
    let metadata = callable_test_metadata(
        "java",
        "prepare",
        "src/test/java/CalcTest.java",
        &[s("beforeeach")],
    );
    assert_eq!(role(&metadata), Some("fixture_setup"));
    assert!(flag(&metadata, "is_test"));
    assert!(flag(&metadata, "test_lifecycle"));
}

#[test]
fn java_after_all_carries_the_fixture_teardown_role() {
    let metadata = callable_test_metadata(
        "java",
        "shutdown",
        "src/test/java/CalcTest.java",
        &[s("afterall")],
    );
    assert_eq!(role(&metadata), Some("fixture_teardown"));
    assert!(flag(&metadata, "test_lifecycle"));
}

#[test]
fn dotnet_test_initialize_carries_the_fixture_setup_role() {
    let metadata = callable_test_metadata(
        "csharp",
        "Prepare",
        "tests/CalcTests.cs",
        &[s("testinitialize")],
    );
    assert_eq!(role(&metadata), Some("fixture_setup"));
}

#[test]
fn dotnet_one_time_teardown_carries_the_fixture_teardown_role() {
    let metadata = callable_test_metadata(
        "csharp",
        "Shutdown",
        "tests/CalcTests.cs",
        &[s("onetimeteardown")],
    );
    assert_eq!(role(&metadata), Some("fixture_teardown"));
}

#[test]
fn python_set_up_class_carries_the_fixture_setup_role() {
    let metadata = callable_test_metadata("python", "setUpClass", "tests/test_calc.py", &[]);
    assert_eq!(role(&metadata), Some("fixture_setup"));
}

#[test]
fn python_tear_down_carries_the_fixture_teardown_role() {
    let metadata = callable_test_metadata("python", "tearDown", "tests/test_calc.py", &[]);
    assert_eq!(role(&metadata), Some("fixture_teardown"));
}

#[test]
fn bash_teardown_carries_the_fixture_teardown_role() {
    let metadata = callable_test_metadata("bash", "teardown", "tests/calc_test.sh", &[]);
    assert_eq!(role(&metadata), Some("fixture_teardown"));
}

#[test]
fn gdscript_before_all_carries_the_fixture_setup_role() {
    let metadata = callable_test_metadata("gdscript", "before_all", "tests/test_calc.gd", &[]);
    assert_eq!(role(&metadata), Some("fixture_setup"));
}

#[test]
fn scala_after_each_carries_the_fixture_teardown_role() {
    let metadata =
        callable_test_metadata("scala", "afterEach", "src/test/scala/CalcSpec.scala", &[]);
    assert_eq!(role(&metadata), Some("fixture_teardown"));
}

#[test]
fn qml_cleanup_test_case_carries_the_fixture_teardown_role() {
    let metadata = callable_test_metadata("qml", "cleanupTestCase", "tests/tst_calc.qml", &[]);
    assert_eq!(role(&metadata), Some("fixture_teardown"));
}

#[test]
fn swift_test_case_without_a_lifecycle_arm_carries_the_test_case_role() {
    let metadata = callable_test_metadata("swift", "testAddition", "Tests/CalcTests.swift", &[]);
    assert_eq!(role(&metadata), Some("test_case"));
    assert!(!metadata.contains_key("test_lifecycle"));
}

#[test]
fn a_production_callable_carries_no_test_metadata_at_all() {
    let metadata = callable_test_metadata("java", "connect", "src/main/java/Client.java", &[]);
    assert!(metadata.is_empty());
}

#[test]
fn every_emitted_role_agrees_with_the_emitted_booleans() {
    let cases = [
        (
            "java",
            "run",
            "src/test/java/CalcTest.java",
            vec![s("test")],
        ),
        (
            "java",
            "prepare",
            "src/test/java/CalcTest.java",
            vec![s("beforeeach")],
        ),
        (
            "java",
            "shutdown",
            "src/test/java/CalcTest.java",
            vec![s("afterclass")],
        ),
        ("python", "setUp", "tests/test_calc.py", vec![]),
        ("python", "test_adds", "tests/test_calc.py", vec![]),
        ("bash", "setup", "tests/calc_test.sh", vec![]),
        ("gdscript", "after_each", "tests/test_calc.gd", vec![]),
        ("qml", "init", "tests/tst_calc.qml", vec![]),
    ];

    for (language, name, file_path, annotation_keys) in cases {
        let metadata = callable_test_metadata(language, name, file_path, &annotation_keys);
        let emitted = role(&metadata).unwrap_or_else(|| panic!("{language}::{name} needs a role"));
        let expected_lifecycle = matches!(emitted, "fixture_setup" | "fixture_teardown");
        assert!(flag(&metadata, "is_test"), "{language}::{name}");
        assert_eq!(
            flag(&metadata, "test_lifecycle"),
            expected_lifecycle,
            "{language}::{name}"
        );
    }
}

#[test]
fn a_python_test_case_class_carries_the_test_container_role() {
    let code = r#"
import unittest


class CalcTests(unittest.TestCase):
    def test_adds(self):
        self.assertEqual(1 + 1, 2)
"#;
    let symbols = extract_symbols_for("python", "tests/test_calc.py", code);

    let container = symbols
        .iter()
        .find(|symbol| symbol.name == "CalcTests")
        .expect("CalcTests class should be extracted");
    assert_eq!(symbol_role(container), Some("test_container"));
    assert!(!has_is_test(container));

    let case = symbols
        .iter()
        .find(|symbol| symbol.name == "test_adds")
        .expect("test_adds method should be extracted");
    assert_eq!(symbol_role(case), Some("test_case"));
}

#[test]
fn a_qml_test_case_function_carries_a_role_and_a_data_provider_carries_none() {
    let code = r#"
import QtQuick 2.15
import QtTest 1.15

TestCase {
    name: "CalcTests"

    function initTestCase() {}
    function test_adds_data() { return []; }
    function test_adds(data) {}
    function helper() {}
}
"#;
    let symbols = extract_symbols_for("qml", "tests/tst_calc.qml", code);

    let named = |name: &str| {
        symbols
            .iter()
            .find(|symbol| symbol.name == name)
            .unwrap_or_else(|| panic!("{name} should be extracted"))
    };

    assert_eq!(symbol_role(named("initTestCase")), Some("fixture_setup"));
    assert_eq!(symbol_role(named("test_adds")), Some("test_case"));
    assert_eq!(symbol_role(named("test_adds_data")), None);
    assert_eq!(symbol_role(named("helper")), None);
}

// ===========================================================================
// Shared path guard — one accept case and one near-miss control per convention
// ===========================================================================

fn path_reads_as_test(file_path: &str) -> bool {
    check(
        "zig",
        "test_thing",
        file_path,
        &SymbolKind::Function,
        &[],
        None,
    )
}

fn windows_path(file_path: &str) -> String {
    file_path.replace('/', "\\")
}

#[test]
fn ruby_test_suffix_reads_as_a_test_path() {
    assert!(path_reads_as_test("app/models/user_test.rb"));
}

#[test]
fn a_ruby_file_that_only_looks_like_a_test_suffix_reads_as_production() {
    assert!(!path_reads_as_test("app/models/contest.rb"));
}

#[test]
fn ruby_spec_suffix_reads_as_a_test_path() {
    assert!(path_reads_as_test("app/models/user_spec.rb"));
}

#[test]
fn a_ruby_file_that_only_looks_like_a_spec_suffix_reads_as_production() {
    assert!(!path_reads_as_test("app/models/codespec.rb"));
}

#[test]
fn python_test_suffix_reads_as_a_test_path() {
    assert!(path_reads_as_test("src/calc_test.py"));
}

#[test]
fn a_python_file_that_only_looks_like_a_test_suffix_reads_as_production() {
    assert!(!path_reads_as_test("src/contest.py"));
}

#[test]
fn a_pytest_conftest_file_reads_as_a_test_path() {
    assert!(path_reads_as_test("src/pkg/conftest.py"));
}

#[test]
fn a_file_that_only_ends_in_conftest_py_reads_as_production() {
    assert!(!path_reads_as_test("src/pkg/myconftest.py"));
}

#[test]
fn phpunit_test_suffix_reads_as_a_test_path() {
    assert!(path_reads_as_test("src/UserTest.php"));
}

#[test]
fn a_php_file_that_only_looks_like_a_test_suffix_reads_as_production() {
    assert!(!path_reads_as_test("src/Contest.php"));
}

#[test]
fn codeception_cest_suffix_reads_as_a_test_path() {
    assert!(path_reads_as_test("src/LoginCest.php"));
}

#[test]
fn a_php_file_that_only_looks_like_a_cest_suffix_reads_as_production() {
    assert!(!path_reads_as_test("src/recest.php"));
}

#[test]
fn phpspec_spec_suffix_reads_as_a_test_path() {
    assert!(path_reads_as_test("src/UserSpec.php"));
}

#[test]
fn a_php_file_that_only_looks_like_a_spec_suffix_reads_as_production() {
    assert!(!path_reads_as_test("src/prospec.php"));
}

#[test]
fn an_xcode_tests_directory_reads_as_a_test_path() {
    assert!(path_reads_as_test("MyAppTests/Helper.swift"));
}

#[test]
fn a_directory_that_only_looks_like_a_tests_directory_reads_as_production() {
    assert!(!path_reads_as_test("Contests/Helper.swift"));
}

#[test]
fn an_xcode_tests_swift_file_reads_as_a_test_path() {
    assert!(path_reads_as_test("Sources/CalculatorTests.swift"));
}

#[test]
fn a_swift_file_that_only_looks_like_a_tests_file_reads_as_production() {
    assert!(!path_reads_as_test("Sources/Manifests.swift"));
}

#[test]
fn an_e2e_directory_reads_as_a_test_path() {
    assert!(path_reads_as_test("apps/web/e2e/login.ts"));
}

#[test]
fn a_directory_that_only_starts_with_e2e_reads_as_production() {
    assert!(!path_reads_as_test("apps/web/e2ee/login.ts"));
}

#[test]
fn a_cypress_directory_reads_as_a_test_path() {
    assert!(path_reads_as_test("cypress/support/commands.js"));
}

#[test]
fn a_directory_that_only_starts_with_cypress_reads_as_production() {
    assert!(!path_reads_as_test("cypress-helpers/support/commands.js"));
}

#[test]
fn an_integration_directory_reads_as_a_test_path() {
    assert!(path_reads_as_test("src/integration/payment_flow.js"));
}

#[test]
fn an_integrations_directory_reads_as_production() {
    assert!(!path_reads_as_test("src/integrations/payment_flow.js"));
}

#[test]
fn a_cypress_spec_infix_reads_as_a_test_path() {
    assert!(path_reads_as_test("src/login.cy.ts"));
}

#[test]
fn a_file_that_only_looks_like_a_cypress_spec_reads_as_production() {
    assert!(!path_reads_as_test("reports/latest.cy-report.txt"));
}

#[test]
fn a_gradle_integration_test_source_set_reads_as_a_test_path() {
    assert!(path_reads_as_test("src/integrationTest/java/CalcIT.java"));
}

#[test]
fn a_directory_that_only_starts_with_integration_test_reads_as_production() {
    assert!(!path_reads_as_test(
        "src/integrationTestUtils/java/Calc.java"
    ));
}

#[test]
fn a_gradle_test_fixtures_source_set_reads_as_a_test_path() {
    assert!(path_reads_as_test("src/testFixtures/java/Fake.java"));
}

#[test]
fn a_directory_that_only_starts_with_test_fixtures_reads_as_production() {
    assert!(!path_reads_as_test("src/testFixturesGen/java/Fake.java"));
}

#[test]
fn a_gradle_android_test_source_set_reads_as_a_test_path() {
    assert!(path_reads_as_test("app/src/androidTest/java/UiCheck.java"));
}

#[test]
fn a_directory_that_only_starts_with_android_test_reads_as_production() {
    assert!(!path_reads_as_test(
        "app/src/androidTestShared/java/UiCheck.java"
    ));
}

#[test]
fn a_gradle_functional_test_source_set_reads_as_a_test_path() {
    assert!(path_reads_as_test(
        "src/functionalTest/groovy/CalcFuncSpec.groovy"
    ));
}

#[test]
fn a_directory_that_only_starts_with_functional_test_reads_as_production() {
    assert!(!path_reads_as_test(
        "src/functionalTesting/groovy/Calc.groovy"
    ));
}

#[test]
fn every_accepted_convention_also_holds_with_windows_separators() {
    let accepted = [
        "src/test/java/CalcTest.java",
        "src/tests/calc.zig",
        "spec/models/user.rb",
        "src/__tests__/calc.js",
        "MyProject.Tests/Calc.cs",
        "app/models/user_test.rb",
        "app/models/user_spec.rb",
        "src/calc_test.py",
        "src/pkg/conftest.py",
        "src/UserTest.php",
        "src/LoginCest.php",
        "src/UserSpec.php",
        "MyAppTests/Helper.swift",
        "Sources/CalculatorTests.swift",
        "apps/web/e2e/login.ts",
        "cypress/support/commands.js",
        "src/integration/payment_flow.js",
        "src/login.cy.ts",
        "src/integrationTest/java/CalcIT.java",
        "src/testFixtures/java/Fake.java",
        "app/src/androidTest/java/UiCheck.java",
        "src/functionalTest/groovy/CalcFuncSpec.groovy",
    ];

    for file_path in accepted {
        assert!(path_reads_as_test(file_path), "{file_path}");
        let windows = windows_path(file_path);
        assert!(path_reads_as_test(&windows), "{windows}");
    }
}

#[test]
fn every_previously_accepted_convention_is_still_accepted() {
    let accepted = [
        "src/test/java/CalcTest.java",
        "src/tests/calc.zig",
        "src/Test/Calc.cs",
        "src/Tests/Calc.cs",
        "spec/models/user.rb",
        "Spec/Models/User.cs",
        "src/__tests__/calc.js",
        "src/autotests/calc.py",
        "MyProject.Tests/Calc.cs",
        "MyProject.Test/Calc.cs",
        "pkg/calc_test.go",
        "src/calc.test.js",
        "src/calc.spec.ts",
        "src/test_calc.py",
        "src/tst_calc.qml",
    ];

    for file_path in accepted {
        assert!(path_reads_as_test(file_path), "{file_path}");
    }
}

// ===========================================================================
// test_role on call-style test DSL symbols
// ===========================================================================

#[test]
fn a_js_test_dsl_call_carries_a_role_for_every_captured_category() {
    let code = r#"
describe("calculator", () => {
  beforeEach(() => {});
  afterEach(() => {});
  it("adds", () => {});
});
"#;
    let symbols = extract_symbols_for("javascript", "src/calc.test.js", code);

    let named = |name: &str| {
        symbols
            .iter()
            .find(|symbol| symbol.name == name)
            .unwrap_or_else(|| panic!("{name} should be extracted"))
    };

    assert_eq!(symbol_role(named("calculator")), Some("test_container"));
    assert_eq!(symbol_role(named("adds")), Some("test_case"));
    assert_eq!(symbol_role(named("beforeEach")), Some("fixture_setup"));
    assert_eq!(symbol_role(named("afterEach")), Some("fixture_teardown"));
}

#[test]
fn a_js_test_dsl_call_keeps_the_boolean_flags_it_has_always_emitted() {
    let code = r#"
describe("calculator", () => {
  beforeEach(() => {});
  it("adds", () => {});
});
"#;
    let symbols = extract_symbols_for("javascript", "src/calc.test.js", code);

    let metadata_of = |name: &str| {
        symbols
            .iter()
            .find(|symbol| symbol.name == name)
            .and_then(|symbol| symbol.metadata.clone())
            .unwrap_or_else(|| panic!("{name} should carry metadata"))
    };

    let container = metadata_of("calculator");
    assert!(flag(&container, "test_container"));
    assert!(!container.contains_key("is_test"));

    let case = metadata_of("adds");
    assert!(flag(&case, "is_test"));
    assert!(!case.contains_key("test_lifecycle"));
    assert!(!case.contains_key("test_container"));

    let hook = metadata_of("beforeEach");
    assert!(flag(&hook, "is_test"));
    assert!(flag(&hook, "test_lifecycle"));
    assert!(!hook.contains_key("test_container"));
}
