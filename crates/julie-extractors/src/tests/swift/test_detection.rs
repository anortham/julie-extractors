//! Swift test detection signals.
//!
//! These are extractor-level assertions for signals preserved in artifact v1.
//! Swift has three frameworks:
//! - **XCTest**: a `class … : XCTestCase` is a test container. The extractor
//!   records the inherited types under the canonical `base_types` metadata key;
//!   `func test*` methods are cases and `setUp`/`tearDown` are hooks, both in
//!   test paths only.
//! - **Swift Testing**: `@Test` / `@Suite` macros, captured as annotation
//!   markers (`annotation_key` "test"/"suite"), path-independent.
//! - **Quick**: `describe`/`it`/hook calls, materialised by the call adapter.

use crate::base::SymbolKind;
use crate::swift::SwiftExtractor;
use std::path::PathBuf;
use tree_sitter::Parser;

fn symbols(code: &str, file: &str) -> Vec<crate::base::Symbol> {
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_swift::LANGUAGE.into())
        .expect("load Swift grammar");
    let tree = parser.parse(code, None).expect("parse Swift");
    let mut ext = SwiftExtractor::new(
        "swift".to_string(),
        file.to_string(),
        code.to_string(),
        &PathBuf::from("/test/workspace"),
    );
    ext.extract_symbols(&tree)
}

fn base_types(symbol: &crate::base::Symbol) -> Vec<String> {
    symbol
        .metadata
        .as_ref()
        .and_then(|m| m.get("base_types"))
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

fn annotation_keys(symbol: &crate::base::Symbol) -> Vec<String> {
    symbol
        .annotations
        .iter()
        .map(|a| a.annotation_key.clone())
        .collect()
}

#[test]
fn swift_qualified_callee_is_not_materialized() {
    // FALSE-POSITIVE GUARD (#66): a member call whose RECEIVER is a vocab word
    // (`it.register("x") { }`) must not be misclassified as a Quick `it`. The
    // adapter resolves the callee as the first DIRECT `simple_identifier` child of
    // the `call_expression`; on a member call the receiver is nested inside a
    // `navigation_expression`, so no bare identifier is found and nothing is built.
    // Also locks in `classify_call_exact` (centralized #66 fix).
    let code = r#"
func helper() {
    it.register("plugin") {
        configure()
    }
}
"#;
    let syms = symbols(code, "Sources/App/Helper.swift");
    let has_role = |s: &crate::base::Symbol| {
        s.metadata.as_ref().is_some_and(|m| {
            m.get("is_test").and_then(|v| v.as_bool()).unwrap_or(false)
                || m.get("test_container")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false)
        })
    };
    assert!(
        !syms.iter().any(has_role),
        "qualified callee `it.register(...) {{ }}` must not materialize a test symbol, got {syms:?}"
    );
}

#[test]
fn xctest_subclass_emits_base_types_metadata() {
    // `class MathTests: XCTestCase` — the canonical base-type signal. The class
    // symbol must carry base_types=["XCTestCase"] so the classifier flags it as a
    // TestContainer with no annotation. Path-independent (Package source dir).
    let code = r#"
import XCTest

class MathTests: XCTestCase {
    func testAddition() {
        XCTAssertEqual(2 + 2, 4)
    }
}
"#;
    let syms = symbols(code, "Sources/MathTests.swift");
    let class_sym = syms
        .iter()
        .find(|s| s.name == "MathTests" && s.kind == SymbolKind::Class)
        .unwrap_or_else(|| panic!("expected MathTests class, got {syms:?}"));
    assert!(
        base_types(class_sym).iter().any(|b| b == "XCTestCase"),
        "class must record XCTestCase in base_types metadata, got {:?}",
        base_types(class_sym)
    );
}

#[test]
fn multiple_conformances_all_recorded_in_base_types() {
    // `class FooTests: XCTestCase, Sendable` — every inherited type/protocol is
    // recorded, not just the first, so the last-segment match in the classifier
    // can find XCTestCase regardless of ordering.
    let code = r#"
class FooTests: XCTestCase, Sendable {
    func testThing() {}
}
"#;
    let syms = symbols(code, "Sources/FooTests.swift");
    let class_sym = syms.iter().find(|s| s.name == "FooTests").unwrap();
    let bt = base_types(class_sym);
    assert!(bt.iter().any(|b| b == "XCTestCase"), "got {bt:?}");
    assert!(bt.iter().any(|b| b == "Sendable"), "got {bt:?}");
}

#[test]
fn swift_testing_test_macro_captured_as_annotation() {
    // Swift Testing: `@Test func example()` — the `@Test` macro is captured as an
    // annotation marker with annotation_key "test" (lowercased). The classifier
    // maps this to a TestCase via `test_case = ["test"]` — no test path required.
    let code = r#"
import Testing

@Test func example() {
    #expect(1 == 1)
}
"#;
    let syms = symbols(code, "Sources/Feature.swift");
    let func_sym = syms
        .iter()
        .find(|s| s.name == "example")
        .unwrap_or_else(|| panic!("expected example function, got {syms:?}"));
    assert!(
        annotation_keys(func_sym).iter().any(|k| k == "test"),
        "@Test must yield annotation_key 'test', got {:?}",
        annotation_keys(func_sym)
    );
}

#[test]
fn xctest_test_method_flagged_is_test_in_test_path() {
    // XCTest `func testAddition()` in a test path — the existing naming detector
    // (`detect_swift`) must still flag it via the `is_test` metadata flag.
    let code = r#"
class MathTests: XCTestCase {
    func testAddition() {
        XCTAssertEqual(2 + 2, 4)
    }
}
"#;
    let syms = symbols(code, "Tests/MathTests.swift");
    let method = syms
        .iter()
        .find(|s| s.name == "testAddition")
        .unwrap_or_else(|| panic!("expected testAddition method, got {syms:?}"));
    let is_test = method
        .metadata
        .as_ref()
        .and_then(|m| m.get("is_test"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    assert!(is_test, "test* method in a Tests/ path must be is_test");
}

// ── Wave-3: Quick/Nimble call-style test detection ────────────────────────
//
// Quick declares tests as `call_expression` DSL calls. Grammar confirmed
// (tree-sitter-swift, live AST probe):
//   - Node kind: `call_expression`
//   - Callee: first `simple_identifier` child
//   - Description string: `call_suffix` → `value_arguments` → first
//     `value_argument` → `value` field → `line_string_literal`
//   - Lifecycle calls (beforeEach/afterEach/beforeAll/afterAll) have no
//     description arg — only a trailing closure in `call_suffix`.

fn meta_bool(s: &crate::base::Symbol, key: &str) -> bool {
    s.metadata
        .as_ref()
        .and_then(|m| m.get(key))
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
}

#[test]
fn quick_describe_context_it_lifecycle_emit_test_role_metadata() {
    let code = r#"
describe("math module") {
    context("addition") {
        beforeEach { }
        afterEach { }
        it("should add two numbers") {
            expect(1 + 1).to(equal(2))
        }
    }
    beforeAll { }
    afterAll { }
}
"#;
    let syms = symbols(code, "Tests/MathSpec.swift");

    let desc = syms
        .iter()
        .find(|s| s.name == "math module")
        .unwrap_or_else(|| panic!("expected describe container, got: {syms:?}"));
    assert!(
        meta_bool(desc, "test_container"),
        "describe → test_container"
    );
    assert!(!meta_bool(desc, "is_test"), "container is not a test case");

    let ctx = syms
        .iter()
        .find(|s| s.name == "addition")
        .unwrap_or_else(|| panic!("expected context container, got: {syms:?}"));
    assert!(meta_bool(ctx, "test_container"), "context → test_container");

    let it = syms
        .iter()
        .find(|s| s.name == "should add two numbers")
        .unwrap_or_else(|| panic!("expected it test case, got: {syms:?}"));
    assert!(meta_bool(it, "is_test"), "it → is_test");
    assert!(
        !meta_bool(it, "test_container"),
        "test case is not a container"
    );

    for lifecycle_name in ["beforeEach", "afterEach", "beforeAll", "afterAll"] {
        let lc = syms
            .iter()
            .find(|s| s.name == lifecycle_name)
            .unwrap_or_else(|| panic!("expected {lifecycle_name} lifecycle symbol, got: {syms:?}"));
        assert!(
            meta_bool(lc, "is_test"),
            "{lifecycle_name} → is_test (lifecycle)",
        );
        assert!(
            meta_bool(lc, "test_lifecycle"),
            "{lifecycle_name} → test_lifecycle",
        );
    }
}

#[test]
fn non_quick_swift_calls_do_not_become_test_symbols() {
    // URL init, print, and ordinary method calls must not carry test-role metadata.
    let code = r#"
let url = URL(string: "https://example.com")
print("hello world")
let x = someFunction("argument")
"#;
    let syms = symbols(code, "Sources/App.swift");
    assert_eq!(
        syms.iter()
            .filter(|s| meta_bool(s, "is_test") || meta_bool(s, "test_container"))
            .count(),
        0,
        "non-Quick calls must not carry test-role metadata: {syms:?}"
    );
}

fn role(symbol: &crate::base::Symbol) -> Option<&str> {
    symbol
        .metadata
        .as_ref()
        .and_then(|m| m.get("test_role"))
        .and_then(|v| v.as_str())
}

fn named<'a>(syms: &'a [crate::base::Symbol], name: &str) -> &'a crate::base::Symbol {
    syms.iter()
        .find(|s| s.name == name)
        .unwrap_or_else(|| panic!("expected {name}, got {syms:?}"))
}

const XCTEST_SOURCE: &str = r#"
import XCTest

final class CalculatorTests: XCTestCase {
    override func setUp() { }
    override func setUpWithError() throws { }
    override func tearDown() { }
    override func tearDownWithError() throws { }

    func testAddition() {
        XCTAssertEqual(2 + 2, 4)
    }

    func calculateTotal() -> Int { 4 }
}

extension CalculatorTests {
    func testSubtraction() { }
}

struct CalculatorSupport {
    func testHelperNamedLikeACase() { }
}
"#;

const SWIFT_TESTING_SOURCE: &str = r#"
import Testing

@Suite("Math suite")
struct MathSuite {
    init() { }
    deinit { }

    @Test func addsTwoNumbers() { }

    @Test("adds many", arguments: [1, 2, 3])
    func addsManyNumbers(value: Int) { }

    func makeSubject() -> Int { 1 }
}
"#;

#[test]
fn xctest_class_is_a_test_container() {
    let syms = symbols(XCTEST_SOURCE, "Tests/CalculatorTests.swift");
    assert_eq!(
        role(named(&syms, "CalculatorTests")),
        Some("test_container")
    );
}

#[test]
fn xctest_hooks_report_fixture_roles_instead_of_cases() {
    let syms = symbols(XCTEST_SOURCE, "Tests/CalculatorTests.swift");
    assert_eq!(role(named(&syms, "setUp")), Some("fixture_setup"));
    assert_eq!(role(named(&syms, "setUpWithError")), Some("fixture_setup"));
    assert_eq!(role(named(&syms, "tearDown")), Some("fixture_teardown"));
    assert_eq!(
        role(named(&syms, "tearDownWithError")),
        Some("fixture_teardown")
    );
    assert!(meta_bool(named(&syms, "setUp"), "test_lifecycle"));
}

#[test]
fn xctest_case_and_plain_method_are_told_apart() {
    let syms = symbols(XCTEST_SOURCE, "Tests/CalculatorTests.swift");
    assert_eq!(role(named(&syms, "testAddition")), Some("test_case"));
    assert_eq!(role(named(&syms, "calculateTotal")), None);
}

#[test]
fn test_prefixed_method_outside_a_container_stays_unclassified() {
    let syms = symbols(XCTEST_SOURCE, "Tests/CalculatorTests.swift");
    assert_eq!(role(named(&syms, "testHelperNamedLikeACase")), None);
    assert!(!meta_bool(
        named(&syms, "testHelperNamedLikeACase"),
        "is_test"
    ));
}

#[test]
fn xctest_case_in_an_extension_of_the_container_keeps_its_role() {
    let syms = symbols(XCTEST_SOURCE, "Tests/CalculatorTests.swift");
    assert_eq!(role(named(&syms, "testSubtraction")), Some("test_case"));
}

#[test]
fn swift_testing_suite_is_a_test_container_outside_a_test_path() {
    let syms = symbols(SWIFT_TESTING_SOURCE, "Sources/App/Math.swift");
    assert_eq!(role(named(&syms, "MathSuite")), Some("test_container"));
}

#[test]
fn swift_testing_test_macro_is_a_case_outside_a_test_path() {
    let syms = symbols(SWIFT_TESTING_SOURCE, "Sources/App/Math.swift");
    assert_eq!(role(named(&syms, "addsTwoNumbers")), Some("test_case"));
}

#[test]
fn swift_testing_arguments_macro_is_a_parameterized_case() {
    let syms = symbols(SWIFT_TESTING_SOURCE, "Sources/App/Math.swift");
    assert_eq!(
        role(named(&syms, "addsManyNumbers")),
        Some("parameterized_test")
    );
}

#[test]
fn swift_testing_unannotated_member_stays_unclassified() {
    let syms = symbols(SWIFT_TESTING_SOURCE, "Sources/App/Math.swift");
    assert_eq!(role(named(&syms, "makeSubject")), None);
}

#[test]
fn swift_testing_suite_init_and_deinit_are_fixture_hooks() {
    let syms = symbols(SWIFT_TESTING_SOURCE, "Sources/App/Math.swift");
    assert_eq!(role(named(&syms, "init")), Some("fixture_setup"));
    assert_eq!(role(named(&syms, "deinit")), Some("fixture_teardown"));
}

#[test]
fn quick_shared_examples_and_suite_hooks_emit_roles() {
    let code = r#"
sharedExamples("a collection") {
    it("is not empty") { }
}

describe("calculator") {
    beforeSuite { }
    afterSuite { }
    aroundEach { runExample in runExample() }
    itBehavesLike("a collection")
}
"#;
    let syms = symbols(code, "Tests/CalculatorSpec.swift");
    let groups: Vec<&crate::base::Symbol> =
        syms.iter().filter(|s| s.name == "a collection").collect();
    assert_eq!(groups.len(), 2, "got {syms:?}");
    assert!(groups.iter().all(|s| role(s) == Some("test_container")));
    assert_eq!(role(named(&syms, "beforeSuite")), Some("fixture_setup"));
    assert_eq!(role(named(&syms, "afterSuite")), Some("fixture_teardown"));
    assert_eq!(role(named(&syms, "aroundEach")), Some("fixture_setup"));
}

#[test]
fn quick_top_level_container_keeps_its_role_after_scoping() {
    let code = r#"
describe("calculator") {
    it("adds") { }
}
"#;
    let syms = symbols(code, "Tests/CalculatorSpec.swift");
    assert_eq!(role(named(&syms, "calculator")), Some("test_container"));
    assert_eq!(role(named(&syms, "adds")), Some("test_case"));
}
