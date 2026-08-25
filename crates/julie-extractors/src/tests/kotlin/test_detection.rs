//! Kotlin Kotest / Spek call-style test detection (Miller bridge, Wave-3).
//!
//! Kotest and Spek express tests as call expressions, not named function
//! declarations or class annotations:
//!   - DescribeSpec: `describe("subject") { it("behaves") { … } }`
//!   - FunSpec:      `test("name") { … }`, `context("group") { … }`
//!   - BehaviorSpec: `given("…") { When("…") { then("…") { } } }`
//!   - ShouldSpec:   `should("name") { … }`
//!   - Spek:         `describe("…") { it("…") { } }`, `beforeEachTest { }`
//!
//! The dominant Kotlin test idiom (JUnit annotations) is already handled by the
//! declaration/annotation path. This adapter is additive — it materializes the
//! call-DSL forms that were previously invisible to the extractor.

use crate::base::{Relationship, SymbolKind};
use crate::kotlin::KotlinExtractor;
use std::path::PathBuf;
use tree_sitter::Parser;

fn symbols(code: &str, file: &str) -> Vec<crate::base::Symbol> {
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_kotlin_ng::LANGUAGE.into())
        .expect("load Kotlin grammar");
    let tree = parser.parse(code, None).expect("parse Kotlin");
    let mut ext = KotlinExtractor::new(
        "kotlin".to_string(),
        file.to_string(),
        code.to_string(),
        &PathBuf::from("/test/workspace"),
    );
    ext.extract_symbols(&tree)
}

fn symbols_and_relationships(
    code: &str,
    file: &str,
) -> (Vec<crate::base::Symbol>, Vec<Relationship>) {
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_kotlin_ng::LANGUAGE.into())
        .expect("load Kotlin grammar");
    let tree = parser.parse(code, None).expect("parse Kotlin");
    let mut ext = KotlinExtractor::new(
        "kotlin".to_string(),
        file.to_string(),
        code.to_string(),
        &PathBuf::from("/test/workspace"),
    );
    let symbols = ext.extract_symbols(&tree);
    let relationships = ext.extract_relationships(&tree, &symbols);
    (symbols, relationships)
}

fn meta_bool(symbol: &crate::base::Symbol, key: &str) -> bool {
    symbol
        .metadata
        .as_ref()
        .and_then(|m| m.get(key))
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
}

// ---------------------------------------------------------------------------
// Kotest DescribeSpec
// ---------------------------------------------------------------------------

#[test]
fn kotest_describespec_it_is_test() {
    // DescribeSpec `it("name") { }` → Function symbol named "name" flagged is_test.
    let code = r#"class CalcSpec : DescribeSpec({
  describe("calculator") {
    it("adds two numbers") {
      1 + 1 shouldBe 2
    }
  }
})
"#;
    let syms = symbols(code, "src/test/kotlin/CalcSpec.kt");
    let t = syms
        .iter()
        .find(|s| s.name == "adds two numbers")
        .unwrap_or_else(|| panic!("expected materialized it() test symbol; got {syms:?}"));
    assert_eq!(t.kind, SymbolKind::Function);
    assert!(meta_bool(t, "is_test"), "it(...) must set is_test");
    assert!(
        !meta_bool(t, "test_container"),
        "it() must not be a container"
    );
}

#[test]
fn kotest_describespec_describe_is_container() {
    // DescribeSpec `describe("subject") { }` → Function symbol flagged test_container.
    let code = r#"class CalcSpec : DescribeSpec({
  describe("calculator") {
    it("adds") { }
  }
})
"#;
    let syms = symbols(code, "src/test/kotlin/CalcSpec.kt");
    let d = syms
        .iter()
        .find(|s| s.name == "calculator" && meta_bool(s, "test_container"))
        .unwrap_or_else(|| panic!("expected describe() container; got {syms:?}"));
    assert_eq!(d.kind, SymbolKind::Function);
    assert!(!meta_bool(d, "is_test"), "describe() must not set is_test");
}

#[test]
fn kotest_describespec_nested_it_parents_to_describe() {
    // Nested `it` must record the enclosing `describe` as its parent.
    let code = r#"class ParentSpec : DescribeSpec({
  describe("math") {
    it("adds numbers") {
      assert(true)
    }
  }
})
"#;
    let syms = symbols(code, "src/test/kotlin/ParentSpec.kt");
    let desc = syms
        .iter()
        .find(|s| s.name == "math" && meta_bool(s, "test_container"))
        .unwrap_or_else(|| panic!("expected describe container; got {syms:?}"));
    let it = syms
        .iter()
        .find(|s| s.name == "adds numbers" && meta_bool(s, "is_test"))
        .unwrap_or_else(|| panic!("expected it() test symbol; got {syms:?}"));
    assert_eq!(
        it.parent_id.as_deref(),
        Some(desc.id.as_str()),
        "it() must parent to the enclosing describe()"
    );
}

// ---------------------------------------------------------------------------
// Kotest FunSpec
// ---------------------------------------------------------------------------

#[test]
fn kotest_funspec_test_is_test() {
    // FunSpec `test("name") { }` → Function symbol flagged is_test.
    let code = r#"class MathSpec : FunSpec({
  test("addition returns correct result") {
    1 + 1 shouldBe 2
  }
})
"#;
    let syms = symbols(code, "src/test/kotlin/MathSpec.kt");
    let t = syms
        .iter()
        .find(|s| s.name == "addition returns correct result")
        .unwrap_or_else(|| panic!("expected test() symbol; got {syms:?}"));
    assert_eq!(t.kind, SymbolKind::Function);
    assert!(meta_bool(t, "is_test"), "test(...) must set is_test");
}

#[test]
fn kotest_funspec_context_is_container() {
    // FunSpec `context("group") { test("…") { } }` → context is test_container.
    let code = r#"class MathSpec : FunSpec({
  context("arithmetic") {
    test("adds") { }
    test("subtracts") { }
  }
})
"#;
    let syms = symbols(code, "src/test/kotlin/MathSpec.kt");
    let ctx = syms
        .iter()
        .find(|s| s.name == "arithmetic" && meta_bool(s, "test_container"))
        .unwrap_or_else(|| panic!("expected context() container; got {syms:?}"));
    assert_eq!(ctx.kind, SymbolKind::Function);
}

// ---------------------------------------------------------------------------
// Kotest BehaviorSpec
// ---------------------------------------------------------------------------

#[test]
fn kotest_behaviorspec_given_when_then() {
    // BehaviorSpec `given { When { then { } } }`:
    //   given → container, When → container, then → test.
    let code = r#"class BehaviorTest : BehaviorSpec({
  given("a calculator") {
    When("adding two numbers") {
      then("should return the correct sum") {
        1 + 1 shouldBe 2
      }
    }
  }
})
"#;
    let syms = symbols(code, "src/test/kotlin/BehaviorTest.kt");
    let given = syms
        .iter()
        .find(|s| s.name == "a calculator" && meta_bool(s, "test_container"))
        .unwrap_or_else(|| panic!("expected given() container; got {syms:?}"));
    assert_eq!(given.kind, SymbolKind::Function);

    let when_sym = syms
        .iter()
        .find(|s| s.name == "adding two numbers" && meta_bool(s, "test_container"))
        .unwrap_or_else(|| panic!("expected When() container; got {syms:?}"));
    assert_eq!(when_sym.kind, SymbolKind::Function);

    let then_sym = syms
        .iter()
        .find(|s| s.name == "should return the correct sum" && meta_bool(s, "is_test"))
        .unwrap_or_else(|| panic!("expected then() test; got {syms:?}"));
    assert_eq!(then_sym.kind, SymbolKind::Function);
    // then must not also be a container
    assert!(!meta_bool(then_sym, "test_container"));
}

// ---------------------------------------------------------------------------
// Kotest lifecycle hooks
// ---------------------------------------------------------------------------

#[test]
fn kotest_lifecycle_hooks_are_lifecycle() {
    // `beforeEach { }` / `afterAll { }` → is_test + test_lifecycle.
    let code = r#"class LifecycleSpec : DescribeSpec({
  beforeEach {
    println("setup")
  }
  afterAll {
    println("teardown")
  }
  describe("something") {
    it("works") { }
  }
})
"#;
    let syms = symbols(code, "src/test/kotlin/LifecycleSpec.kt");

    let before = syms
        .iter()
        .find(|s| s.name == "beforeEach")
        .unwrap_or_else(|| panic!("expected beforeEach lifecycle symbol; got {syms:?}"));
    assert!(meta_bool(before, "is_test"), "lifecycle must set is_test");
    assert!(
        meta_bool(before, "test_lifecycle"),
        "lifecycle must set test_lifecycle"
    );

    let after = syms
        .iter()
        .find(|s| s.name == "afterAll")
        .unwrap_or_else(|| panic!("expected afterAll lifecycle symbol; got {syms:?}"));
    assert!(meta_bool(after, "is_test"), "lifecycle must set is_test");
    assert!(
        meta_bool(after, "test_lifecycle"),
        "lifecycle must set test_lifecycle"
    );
}

// ---------------------------------------------------------------------------
// Spek
// ---------------------------------------------------------------------------

#[test]
fn spek_describe_and_it() {
    // Spek `describe("…") { it("…") { } }` — same DSL as Kotest DescribeSpec.
    let code = r#"class CalculatorSpec : Spek({
  describe("a calculator") {
    it("returns the sum of its arguments") {
      val calculator = Calculator()
      assertEquals(4, calculator.sum(2, 2))
    }
  }
})
"#;
    let syms = symbols(code, "src/test/kotlin/CalculatorSpec.kt");
    let d = syms
        .iter()
        .find(|s| s.name == "a calculator" && meta_bool(s, "test_container"))
        .unwrap_or_else(|| panic!("expected Spek describe() container; got {syms:?}"));
    assert_eq!(d.kind, SymbolKind::Function);
    let t = syms
        .iter()
        .find(|s| s.name == "returns the sum of its arguments" && meta_bool(s, "is_test"))
        .unwrap_or_else(|| panic!("expected Spek it() test; got {syms:?}"));
    assert_eq!(t.kind, SymbolKind::Function);
}

#[test]
fn spek_beforeeachtest_is_lifecycle() {
    // Spek `beforeEachTest { }` → is_test + test_lifecycle.
    let code = r#"class SetupSpec : Spek({
  describe("setup") {
    beforeEachTest {
      initDb()
    }
    it("runs after setup") { }
  }
})
"#;
    let syms = symbols(code, "src/test/kotlin/SetupSpec.kt");
    let lc = syms
        .iter()
        .find(|s| s.name == "beforeEachTest")
        .unwrap_or_else(|| panic!("expected beforeEachTest lifecycle; got {syms:?}"));
    assert!(meta_bool(lc, "is_test"));
    assert!(meta_bool(lc, "test_lifecycle"));
}

// ---------------------------------------------------------------------------
// Negative control
// ---------------------------------------------------------------------------

#[test]
fn non_test_calls_not_materialized() {
    // Guards exercised:
    //   - `describe("x")` used as a plain return value (no trailing lambda) →
    //     trailing-lambda guard rejects it.
    //   - `println("hello")` — vocab guard rejects it.
    //   - `someAction("label") { run() }` — arbitrary lambda call, callee not in vocab.
    //   - `assert(x == y)` — assertion, no trailing lambda and not in vocab.
    // None may produce a test symbol or test-role metadata.
    let code = r#"object Demo {
    fun produceLabel(): String {
        return describe("my-api")
    }

    fun run() {
        println("hello world")
        someAction("label") {
            doWork()
        }
        assert(1 == 1)
    }
}
"#;
    let syms = symbols(code, "src/main/kotlin/Demo.kt");
    assert!(
        !syms
            .iter()
            .any(|s| s.name == "my-api" || s.name == "hello world" || s.name == "label"),
        "non-DSL calls must not materialize test symbols; got {syms:?}"
    );
    assert!(
        !syms
            .iter()
            .any(|s| meta_bool(s, "is_test") || meta_bool(s, "test_container")),
        "no test-role metadata should appear for production code; got {syms:?}"
    );
}

#[test]
fn qualified_vocab_callee_not_materialized() {
    // Regression lock for the `navigation_expression` false-positive.
    //
    // `it` is the single most dangerous word: it IS in the test vocab AND it is
    // the standard implicit lambda-parameter name in Kotlin.  In production code:
    //
    //   list.forEach {
    //     it.register("widget") { configure() }   // it = lambda param; .register = builder call
    //   }
    //
    // `it.register("widget") { }` parses as a CURRIED call_expression whose inner
    // call has a `navigation_expression` callee (not a bare `identifier`).  The
    // leading segment "it" IS in the test vocab — so without the bare-identifier
    // guard, `classify_call("it.register", …)` would fire (split('.').next()=="it")
    // and materialize a bogus test symbol named "widget".
    //
    // `obj.describe("x") { }` exercises the same path for a container-vocab word
    // used as a method name on an arbitrary receiver — must also be rejected.
    let code = r#"class WidgetRegistry {
    fun register(items: List<Item>) {
        items.forEach {
            it.register("widget") {
                configure()
            }
        }
        obj.describe("x") {
            doSomething()
        }
    }
}
"#;
    let syms = symbols(code, "src/main/kotlin/WidgetRegistry.kt");
    assert!(
        !syms.iter().any(|s| s.name == "widget"),
        "it.register(\"widget\") must NOT materialize a test symbol; got {syms:?}"
    );
    assert!(
        !syms.iter().any(|s| s.name == "x"),
        "obj.describe(\"x\") must NOT materialize a test symbol; got {syms:?}"
    );
    assert!(
        !syms
            .iter()
            .any(|s| meta_bool(s, "is_test") || meta_bool(s, "test_container")),
        "qualified vocab-word calls must produce zero test-role metadata; got {syms:?}"
    );
}

// ---------------------------------------------------------------------------
// Kotest StringSpec / WordSpec / FreeSpec
// ---------------------------------------------------------------------------

#[test]
fn kotest_stringspec_string_invoke_is_test() {
    let code = r#"class LengthSpec : StringSpec({
    "length returns the size of the string" {
        "hello".length shouldBe 5
    }
})
"#;
    let syms = symbols(code, "src/test/kotlin/LengthSpec.kt");
    let case = syms
        .iter()
        .find(|s| s.name == "length returns the size of the string")
        .unwrap_or_else(|| panic!("expected StringSpec case symbol; got {syms:?}"));
    assert_eq!(case.kind, SymbolKind::Function);
    assert!(meta_bool(case, "is_test"));
    assert!(!meta_bool(case, "test_container"));
    assert_eq!(
        case.signature.as_deref(),
        Some("invoke(\"length returns the size of the string\")")
    );
}

#[test]
fn kotest_wordspec_should_is_container_holding_its_cases() {
    let code = r#"class WordsSpec : WordSpec({
    "String.length" should {
        "return the length of the string" {
            "sam".length shouldBe 3
        }
    }
})
"#;
    let syms = symbols(code, "src/test/kotlin/WordsSpec.kt");
    let container = syms
        .iter()
        .find(|s| s.name == "String.length should")
        .unwrap_or_else(|| panic!("expected WordSpec should container; got {syms:?}"));
    assert!(meta_bool(container, "test_container"));
    assert!(!meta_bool(container, "is_test"));

    let case = syms
        .iter()
        .find(|s| s.name == "return the length of the string")
        .unwrap_or_else(|| panic!("expected WordSpec case; got {syms:?}"));
    assert!(meta_bool(case, "is_test"));
    assert_eq!(case.parent_id.as_deref(), Some(container.id.as_str()));
}

#[test]
fn kotest_freespec_dash_is_container_holding_its_cases() {
    let code = r#"class FreeStyleSpec : FreeSpec({
    "String.length" - {
        "returns the length of the string" {
            "sam".length shouldBe 3
        }
    }
})
"#;
    let syms = symbols(code, "src/test/kotlin/FreeStyleSpec.kt");
    let container = syms
        .iter()
        .find(|s| s.name == "String.length")
        .unwrap_or_else(|| panic!("expected FreeSpec container; got {syms:?}"));
    assert!(meta_bool(container, "test_container"));

    let case = syms
        .iter()
        .find(|s| s.name == "returns the length of the string")
        .unwrap_or_else(|| panic!("expected FreeSpec case; got {syms:?}"));
    assert!(meta_bool(case, "is_test"));
    assert_eq!(case.parent_id.as_deref(), Some(container.id.as_str()));
}

#[test]
fn string_invoke_outside_a_spec_class_is_not_a_test() {
    let code = r#"object Router {
    fun install() {
        "GET /orders" {
            handle()
        }
    }
}
"#;
    let syms = symbols(code, "src/main/kotlin/Router.kt");
    assert!(
        !syms.iter().any(|s| meta_bool(s, "is_test")),
        "a string-invoke in production code must not publish a role; got {syms:?}"
    );
}

// ---------------------------------------------------------------------------
// Kotest FeatureSpec / ExpectSpec and the disabled prefixes
// ---------------------------------------------------------------------------

#[test]
fn kotest_feature_scenario_and_expect_vocabulary() {
    let code = r#"class CheckoutSpec : FeatureSpec({
    feature("checkout") {
        scenario("charges the card") { }
    }
    context("totals") {
        expect("adds tax") { }
    }
})
"#;
    let syms = symbols(code, "src/test/kotlin/CheckoutSpec.kt");
    let feature = syms
        .iter()
        .find(|s| s.name == "checkout")
        .unwrap_or_else(|| panic!("expected feature container; got {syms:?}"));
    assert!(meta_bool(feature, "test_container"));

    let scenario = syms
        .iter()
        .find(|s| s.name == "charges the card")
        .unwrap_or_else(|| panic!("expected scenario case; got {syms:?}"));
    assert!(meta_bool(scenario, "is_test"));

    let expect = syms
        .iter()
        .find(|s| s.name == "adds tax")
        .unwrap_or_else(|| panic!("expected expect case; got {syms:?}"));
    assert!(meta_bool(expect, "is_test"));
}

#[test]
fn kotest_disabled_prefixes_keep_their_roles() {
    let code = r#"class SkippedSpec : DescribeSpec({
    xdescribe("skipped group") {
        xit("skipped case") { }
    }
    xcontext("other group") {
        xtest("other case") { }
    }
})
"#;
    let syms = symbols(code, "src/test/kotlin/SkippedSpec.kt");
    for container in ["skipped group", "other group"] {
        let sym = syms
            .iter()
            .find(|s| s.name == container)
            .unwrap_or_else(|| panic!("expected {container} container; got {syms:?}"));
        assert!(meta_bool(sym, "test_container"));
    }
    for case in ["skipped case", "other case"] {
        let sym = syms
            .iter()
            .find(|s| s.name == case)
            .unwrap_or_else(|| panic!("expected {case} case; got {syms:?}"));
        assert!(meta_bool(sym, "is_test"));
    }
}

// ---------------------------------------------------------------------------
// Spec classes are test containers
// ---------------------------------------------------------------------------

#[test]
fn spec_base_class_is_a_test_container() {
    let code = r#"class EmptySpec : StringSpec()

class OrdinaryService : BaseService()
"#;
    let syms = symbols(code, "src/test/kotlin/EmptySpec.kt");
    let spec = syms
        .iter()
        .find(|s| s.name == "EmptySpec")
        .unwrap_or_else(|| panic!("expected spec class; got {syms:?}"));
    assert!(
        meta_bool(spec, "test_container"),
        "a class extending a Kotest spec base is a test container"
    );

    let ordinary = syms.iter().find(|s| s.name == "OrdinaryService").unwrap();
    assert!(!meta_bool(ordinary, "test_container"));
}

#[test]
fn spec_lambda_body_makes_the_class_a_test_container() {
    let code = r#"class CalcSpec : ProjectSpecBase({
    describe("calculator") {
        it("adds") { }
    }
})
"#;
    let syms = symbols(code, "src/test/kotlin/CalcSpec.kt");
    let spec = syms
        .iter()
        .find(|s| s.name == "CalcSpec")
        .unwrap_or_else(|| panic!("expected spec class; got {syms:?}"));
    assert!(
        meta_bool(spec, "test_container"),
        "a class whose body is a spec lambda is a test container"
    );
}

#[test]
fn test_factory_property_is_a_test_container() {
    let code = r#"private val factory = funSpec {
    beforeEach {
        seedDatabase()
    }
    test("a") { }
}
"#;
    let syms = symbols(code, "src/test/kotlin/FactoryTest.kt");
    let factory = syms
        .iter()
        .find(|s| s.name == "factory")
        .unwrap_or_else(|| panic!("expected the factory property; got {syms:?}"));
    assert!(
        meta_bool(factory, "test_container"),
        "a Kotest test factory holds test steps, so it is a container"
    );

    let hook = syms.iter().find(|s| s.name == "beforeEach").unwrap();
    assert!(
        meta_bool(hook, "test_lifecycle"),
        "a factory hook must survive the scoping pass"
    );
}

#[test]
fn spec_class_records_its_base_types() {
    let code = r#"class CalcSpec : DescribeSpec({
    it("adds") { }
})
"#;
    let syms = symbols(code, "src/test/kotlin/CalcSpec.kt");
    let spec = syms.iter().find(|s| s.name == "CalcSpec").unwrap();
    let base_types = spec
        .metadata
        .as_ref()
        .and_then(|m| m.get("base_types"))
        .and_then(|v| v.as_array())
        .map(|values| {
            values
                .iter()
                .filter_map(|value| value.as_str().map(str::to_string))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    assert_eq!(base_types, vec!["DescribeSpec".to_string()]);
}

// ---------------------------------------------------------------------------
// Scoping now covers Kotlin
// ---------------------------------------------------------------------------

#[test]
fn name_convention_role_outside_a_container_is_scoped_away() {
    let code = r#"class LedgerTestHelpers {
    fun testDataForLedger(): String {
        return "rows"
    }
}

class LedgerSpec : StringSpec({
    "keeps its role" { }
})
"#;
    let syms = symbols(code, "src/test/kotlin/LedgerSpec.kt");
    let helper = syms
        .iter()
        .find(|s| s.name == "testDataForLedger")
        .unwrap_or_else(|| panic!("expected helper symbol; got {syms:?}"));
    assert!(
        !meta_bool(helper, "is_test"),
        "a testXxx helper outside a container must lose the name-convention role"
    );

    let case = syms.iter().find(|s| s.name == "keeps its role").unwrap();
    assert!(
        meta_bool(case, "is_test"),
        "a Kotest case inside a spec class keeps its role under scoping"
    );
}

// ---------------------------------------------------------------------------
// Lifecycle call symbols carry no self-referential calls edge
// ---------------------------------------------------------------------------

#[test]
fn lifecycle_call_symbol_has_no_self_referential_calls_edge() {
    let code = r#"class LifecycleSpec : DescribeSpec({
    beforeEach {
        seedDatabase()
    }
})
"#;
    let (syms, relationships) = symbols_and_relationships(code, "src/test/kotlin/LifecycleSpec.kt");
    let before = syms
        .iter()
        .find(|s| s.name == "beforeEach")
        .unwrap_or_else(|| panic!("expected beforeEach symbol; got {syms:?}"));
    assert!(
        !relationships
            .iter()
            .any(|r| r.from_symbol_id == before.id && r.to_symbol_id == before.id),
        "beforeEach must not call itself; got {relationships:?}"
    );
}

#[test]
fn a_case_named_after_a_function_does_not_capture_calls_to_it() {
    let code = r#"class MatcherSpec : StringSpec({
    "shouldBeZero" {
        BigDecimal.ZERO.shouldBeZero()
    }
})
"#;
    let (syms, relationships) = symbols_and_relationships(code, "src/test/kotlin/MatcherSpec.kt");
    let case = syms
        .iter()
        .find(|s| s.name == "shouldBeZero")
        .unwrap_or_else(|| panic!("expected the StringSpec case; got {syms:?}"));
    assert!(
        !relationships.iter().any(|r| r.to_symbol_id == case.id),
        "a case named after the function it exercises must not answer that call; got {relationships:?}"
    );
}
