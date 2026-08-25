//! Call-style test detection for the JavaScript family.
//!
//! `js` and `jsx` route to `JavaScriptExtractor`; `ts` and `tsx` route to
//! `TypeScriptExtractor` (see `registry.rs`). Both extractors share one
//! classifier, so every case here runs against all four dialects.

use crate::base::Symbol;
use crate::pipeline::extract_canonical;
use std::path::Path;

const DIALECTS: [&str; 4] = ["js", "jsx", "ts", "tsx"];

fn symbols_at(path: &str, code: &str) -> Vec<Symbol> {
    extract_canonical(path, code, Path::new("/repo"))
        .unwrap_or_else(|error| panic!("{path} should extract: {error}"))
        .symbols
}

fn role(symbols: &[Symbol], name: &str) -> Option<String> {
    symbols
        .iter()
        .find(|symbol| symbol.name == name)?
        .metadata
        .as_ref()?
        .get("test_role")?
        .as_str()
        .map(str::to_string)
}

fn roles(symbols: &[Symbol]) -> Vec<String> {
    symbols
        .iter()
        .filter_map(|symbol| symbol.metadata.as_ref()?.get("test_role")?.as_str())
        .map(str::to_string)
        .collect()
}

fn assert_role_in_every_dialect(code: &str, name: &str, expected: &str) {
    for extension in DIALECTS {
        let path = format!("src/feature.test.{extension}");
        let symbols = symbols_at(&path, code);
        assert_eq!(
            role(&symbols, name).as_deref(),
            Some(expected),
            "{path}: {name} should be {expected}, symbols: {:?}",
            symbols
                .iter()
                .map(|symbol| (&symbol.name, &symbol.metadata))
                .collect::<Vec<_>>()
        );
    }
}

fn assert_no_roles_in_every_dialect(path_stem: &str, code: &str) {
    for extension in DIALECTS {
        let path = format!("{path_stem}.{extension}");
        let symbols = symbols_at(&path, code);
        assert!(
            roles(&symbols).is_empty(),
            "{path} should emit no test roles, got: {:?}",
            symbols
                .iter()
                .map(|symbol| (&symbol.name, &symbol.metadata))
                .collect::<Vec<_>>()
        );
    }
}

const PLAYWRIGHT_SUITE: &str = r#"
import { test, expect } from "@playwright/test";

test.describe("checkout", () => {
  test.beforeEach(async () => {});
  test.afterAll(async () => {});
  test("pays with a card", async () => {});
});
"#;

#[test]
fn playwright_describe_is_a_test_container() {
    assert_role_in_every_dialect(PLAYWRIGHT_SUITE, "checkout", "test_container");
}

#[test]
fn playwright_dotted_lifecycle_hooks_emit_fixture_roles() {
    assert_role_in_every_dialect(PLAYWRIGHT_SUITE, "beforeEach", "fixture_setup");
    assert_role_in_every_dialect(PLAYWRIGHT_SUITE, "afterAll", "fixture_teardown");
}

#[test]
fn playwright_dotted_test_call_is_a_test_case() {
    assert_role_in_every_dialect(PLAYWRIGHT_SUITE, "pays with a card", "test_case");
}

const EACH_SUITE: &str = r#"
import { describe, it, test } from "vitest";

test.each([[1, 2]])("adds %i", () => {});
it.each([[3]])("checks %i", () => {});
describe.each([[4]])("groups %i", () => {});
"#;

#[test]
fn each_table_calls_are_parameterized_tests() {
    assert_role_in_every_dialect(EACH_SUITE, "adds %i", "parameterized_test");
    assert_role_in_every_dialect(EACH_SUITE, "checks %i", "parameterized_test");
    assert_role_in_every_dialect(EACH_SUITE, "groups %i", "parameterized_test");
}

#[test]
fn tagged_template_each_is_a_parameterized_test() {
    let code = r#"
import { it } from "vitest";

it.each`
  a    | b
  ${1} | ${2}
`("adds $a and $b", () => {});
"#;
    assert_role_in_every_dialect(code, "adds $a and $b", "parameterized_test");
}

const BARE_DSL_VOCABULARY: &str = r#"
describe("config loader", () => {});
context("defaults", () => {});
suite("legacy group", () => {});
it("reads a value", () => {});
test("writes a value", () => {});
before(() => {});
after(() => {});
setup(() => {});
teardown(() => {});
"#;

#[test]
fn bare_dsl_vocabulary_in_a_production_file_emits_no_test_roles() {
    assert_no_roles_in_every_dialect("src/loader", BARE_DSL_VOCABULARY);
}

#[test]
fn bare_dsl_vocabulary_in_a_test_path_still_emits_test_roles() {
    assert_role_in_every_dialect(BARE_DSL_VOCABULARY, "config loader", "test_container");
    assert_role_in_every_dialect(BARE_DSL_VOCABULARY, "reads a value", "test_case");
    assert_role_in_every_dialect(BARE_DSL_VOCABULARY, "before", "fixture_setup");
    assert_role_in_every_dialect(BARE_DSL_VOCABULARY, "teardown", "fixture_teardown");
}

#[test]
fn a_framework_import_enables_detection_outside_a_test_path() {
    let code = format!("import {{ describe, it }} from \"vitest\";\n{BARE_DSL_VOCABULARY}");
    for extension in DIALECTS {
        let path = format!("src/loader.{extension}");
        let symbols = symbols_at(&path, &code);
        assert_eq!(
            role(&symbols, "config loader").as_deref(),
            Some("test_container"),
            "{path}: a vitest import should enable detection"
        );
    }
}

#[test]
fn a_require_of_a_framework_enables_detection_outside_a_test_path() {
    let code = format!("const {{ describe }} = require(\"mocha\");\n{BARE_DSL_VOCABULARY}");
    for extension in DIALECTS {
        let path = format!("src/loader.{extension}");
        let symbols = symbols_at(&path, &code);
        assert_eq!(
            role(&symbols, "config loader").as_deref(),
            Some("test_container"),
            "{path}: a mocha require should enable detection"
        );
    }
}

#[test]
fn node_test_subtests_are_test_cases() {
    let code = r#"
import test from "node:test";

test("parent", async (t) => {
  await t.test("child", () => {});
});
"#;
    assert_role_in_every_dialect(code, "parent", "test_case");
    assert_role_in_every_dialect(code, "child", "test_case");
}

const MOCHA_TDD_SUITE: &str = r#"
import "mocha";

suite("array", () => {
  suiteSetup(() => {});
  setup(() => {});
  teardown(() => {});
  suiteTeardown(() => {});
  test("indexOf", () => {});
  specify("also finds", () => {});
});
"#;

#[test]
fn mocha_tdd_lifecycle_hooks_emit_fixture_roles() {
    assert_role_in_every_dialect(MOCHA_TDD_SUITE, "suiteSetup", "fixture_setup");
    assert_role_in_every_dialect(MOCHA_TDD_SUITE, "setup", "fixture_setup");
    assert_role_in_every_dialect(MOCHA_TDD_SUITE, "teardown", "fixture_teardown");
    assert_role_in_every_dialect(MOCHA_TDD_SUITE, "suiteTeardown", "fixture_teardown");
}

#[test]
fn mocha_specify_alias_is_a_test_case() {
    assert_role_in_every_dialect(MOCHA_TDD_SUITE, "also finds", "test_case");
}

const ALIAS_SUITE: &str = r#"
import { describe, it } from "vitest";

xdescribe("skipped group", () => {});
fdescribe("focused group", () => {});
xcontext("skipped context", () => {});
xit("skipped case", () => {});
fit("focused case", () => {});
xtest("skipped test", () => {});
"#;

#[test]
fn focused_and_disabled_container_aliases_are_test_containers() {
    assert_role_in_every_dialect(ALIAS_SUITE, "skipped group", "test_container");
    assert_role_in_every_dialect(ALIAS_SUITE, "focused group", "test_container");
    assert_role_in_every_dialect(ALIAS_SUITE, "skipped context", "test_container");
}

#[test]
fn focused_and_disabled_case_aliases_are_test_cases() {
    assert_role_in_every_dialect(ALIAS_SUITE, "skipped case", "test_case");
    assert_role_in_every_dialect(ALIAS_SUITE, "focused case", "test_case");
    assert_role_in_every_dialect(ALIAS_SUITE, "skipped test", "test_case");
}

#[test]
fn vitest_bench_is_a_test_case() {
    let code = r#"
import { bench } from "vitest";

bench("sorts a large array", () => {});
"#;
    assert_role_in_every_dialect(code, "sorts a large array", "test_case");
}

const QUNIT_SUITE: &str = r#"
import QUnit from "qunit";

QUnit.module("math", () => {
  QUnit.test("adds", (assert) => {});
});
"#;

#[test]
fn qunit_module_is_a_test_container() {
    assert_role_in_every_dialect(QUNIT_SUITE, "math", "test_container");
}

#[test]
fn qunit_test_is_a_test_case() {
    assert_role_in_every_dialect(QUNIT_SUITE, "adds", "test_case");
}

#[test]
fn a_bare_module_call_is_not_a_test_container() {
    let code = r#"
import { describe } from "vitest";

module("not a qunit suite", () => {});
"#;
    for extension in DIALECTS {
        let path = format!("src/feature.test.{extension}");
        let symbols = symbols_at(&path, code);
        assert_eq!(
            role(&symbols, "not a qunit suite"),
            None,
            "{path}: bare module() is CommonJS-adjacent production code"
        );
    }
}

#[test]
fn a_member_call_on_an_ordinary_object_is_not_a_test() {
    let code = r#"
import { describe } from "vitest";

const ordinary = { test(name, callback) { return callback(); } };

ordinary.test("ordinary member call", () => {});
"#;
    for extension in DIALECTS {
        let path = format!("src/feature.test.{extension}");
        let symbols = symbols_at(&path, code);
        assert_eq!(
            role(&symbols, "ordinary member call"),
            None,
            "{path}: an unknown receiver never carries the test DSL"
        );
    }
}

#[test]
fn a_dotted_configuration_call_is_not_a_test() {
    let code = r#"
import { test } from "@playwright/test";

test.describe.configure({ mode: "parallel" });
"#;
    for extension in DIALECTS {
        let path = format!("src/feature.test.{extension}");
        let symbols = symbols_at(&path, code);
        assert!(
            roles(&symbols).is_empty(),
            "{path}: test.describe.configure() declares no test, got: {:?}",
            roles(&symbols)
        );
    }
}

#[test]
fn skip_and_only_modifiers_keep_the_underlying_role() {
    let code = r#"
import { describe, it } from "vitest";

describe.only("focused group", () => {
  it.skip("skipped case", () => {});
  it.concurrent("parallel case", () => {});
});
"#;
    assert_role_in_every_dialect(code, "focused group", "test_container");
    assert_role_in_every_dialect(code, "skipped case", "test_case");
    assert_role_in_every_dialect(code, "parallel case", "test_case");
}

#[test]
fn testdeck_decorated_methods_are_test_cases() {
    let code = r#"
import { suite, test, params } from "@testdeck/mocha";

class MathSuite {
  @test
  addsTwoNumbers() {}

  @params([1, 2])
  addsATable() {}
}
"#;
    for extension in ["ts", "tsx"] {
        let path = format!("src/math.suite.{extension}");
        let symbols = symbols_at(&path, code);
        assert_eq!(
            role(&symbols, "addsTwoNumbers").as_deref(),
            Some("test_case"),
            "{path}: @test marks a test case"
        );
        assert_eq!(
            role(&symbols, "addsATable").as_deref(),
            Some("parameterized_test"),
            "{path}: @params marks a parameterized test"
        );
    }
}

#[test]
fn a_named_function_matching_the_dsl_vocabulary_is_not_a_test() {
    let code = "function testNamedButOrdinary() {}\n";
    assert_no_roles_in_every_dialect("src/helpers", code);
}
