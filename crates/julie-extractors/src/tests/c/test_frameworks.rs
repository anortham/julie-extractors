use crate::base::{ExtractionResults, Symbol, SymbolKind};
use crate::extract_canonical;
use std::path::Path;

fn extract(path: &str, source: &str) -> ExtractionResults {
    extract_canonical(path, source, Path::new("/tmp/test")).expect("C extraction succeeds")
}

fn function<'a>(result: &'a ExtractionResults, name: &str) -> &'a Symbol {
    result
        .symbols
        .iter()
        .find(|symbol| symbol.name == name && symbol.kind == SymbolKind::Function)
        .unwrap_or_else(|| panic!("missing function {name}: {:#?}", result.symbols))
}

fn meta<'a>(symbol: &'a Symbol, key: &str) -> Option<&'a serde_json::Value> {
    symbol.metadata.as_ref()?.get(key)
}

fn test_role<'a>(result: &'a ExtractionResults, name: &str) -> Option<&'a str> {
    let symbol = function(result, name);
    let role = meta(symbol, "test_role").and_then(|role| role.as_str());
    let is_test = meta(symbol, "is_test").and_then(|flag| flag.as_bool()) == Some(true);
    assert_eq!(role.is_some(), is_test, "{name}: {symbol:#?}");
    role
}

const UNITY_RUNNER: &str = "#include \"unity.h\"\n\
#include \"calc.h\"\n\
void setUp(void) {}\n\
void tearDown(void) {}\n\
void suiteSetUp(void) {}\n\
int suiteTearDown(int failures) { return failures; }\n\
void test_subtracts(void) { TEST_ASSERT_EQUAL(1, 2 - 1); }\n\
void testAdds(void) { TEST_ASSERT_EQUAL(3, 1 + 2); }\n\
void spec_divides(void) { }\n\
void should_wrap(void) { }\n\
static void test_static_helper(void) { }\n\
int test_count(void) { return 1; }\n\
void tested_value(void);\n\
void run_checks(void) { }\n\
int main(void) {\n\
  UNITY_BEGIN();\n\
  RUN_TEST(test_subtracts);\n\
  RUN_TEST(checks_bounds, 20);\n\
  return UNITY_END();\n\
}\n\
void checks_bounds(void) { }\n";

#[test]
fn unity_file_outside_a_test_path_flags_its_tests_and_hooks() {
    let result = extract("unit/calc_runner.c", UNITY_RUNNER);
    for name in [
        "test_subtracts",
        "testAdds",
        "spec_divides",
        "should_wrap",
        "checks_bounds",
    ] {
        assert_eq!(test_role(&result, name), Some("test_case"), "{name}");
    }
    for (name, role) in [
        ("setUp", "fixture_setup"),
        ("suiteSetUp", "fixture_setup"),
        ("tearDown", "fixture_teardown"),
        ("suiteTearDown", "fixture_teardown"),
    ] {
        assert_eq!(test_role(&result, name), Some(role), "{name}");
        assert_eq!(
            meta(function(&result, name), "test_lifecycle"),
            Some(&serde_json::json!(true)),
            "{name}"
        );
    }
    for name in ["test_static_helper", "test_count", "run_checks", "main"] {
        assert_eq!(test_role(&result, name), None, "{name}");
    }
}

#[test]
fn unity_include_is_found_in_angle_brackets_and_under_a_folder() {
    for include in [
        "#include <unity.h>",
        "#include \"vendor/Unity/src/unity.h\"",
        "#ifdef TEST\n#include \"unity.h\"\n#endif",
    ] {
        let source = format!("{include}\nvoid test_adds(void) {{ }}\nvoid setUp(void) {{ }}\n");
        let result = extract("unit/calc_runner.c", &source);
        assert_eq!(
            test_role(&result, "test_adds"),
            Some("test_case"),
            "{include}"
        );
        assert_eq!(
            test_role(&result, "setUp"),
            Some("fixture_setup"),
            "{include}"
        );
    }
}

#[test]
fn functions_without_a_test_framework_stay_production_code() {
    for source in [
        "#include <stdio.h>\nvoid test_mode(void) { }\nint test_flag(int x) { return x; }\nvoid setUp(void) { }\n",
        "#include \"myunity.h\"\nvoid test_mode(void) { }\n",
        "#include \"unity_config.h\"\nvoid test_mode(void) { }\nvoid setUp(void) { }\n",
    ] {
        let result = extract("src/mode.c", source);
        for symbol in result
            .symbols
            .iter()
            .filter(|s| s.kind == SymbolKind::Function)
        {
            assert_eq!(meta(symbol, "is_test"), None, "{source}: {symbol:#?}");
            assert_eq!(meta(symbol, "test_role"), None, "{source}: {symbol:#?}");
        }
    }
}

#[test]
fn run_test_registration_needs_no_include_or_test_path() {
    let result = extract(
        "src/runner.c",
        "void verifies_parse(void) { }\nvoid helper(void) { }\nint main(void) { RUN_TEST(verifies_parse); helper(); return 0; }\n",
    );
    assert_eq!(test_role(&result, "verifies_parse"), Some("test_case"));
    assert_eq!(test_role(&result, "helper"), None);
}

const CHECK_SUITE: &str = "#include <check.h>\n\
static void make_up(void) { }\n\
static void break_down(void) { }\n\
static void cache_up(void) { }\n\
START_TEST(test_check_adds)\n\
{\n\
  ck_assert_int_eq(2, 1 + 1);\n\
}\n\
END_TEST\n\
\n\
START_TEST(loops_over_values)\n\
{\n\
  ck_assert_int_ge(_i, 0);\n\
}\n\
END_TEST\n\
\n\
Suite *calc_suite(void) {\n\
  Suite *s = suite_create(\"calc\");\n\
  TCase *tc = tcase_create(\"core\");\n\
  tcase_add_checked_fixture(tc, make_up, break_down);\n\
  tcase_add_unchecked_fixture(tc, cache_up, NULL);\n\
  tcase_add_test(tc, test_check_adds);\n\
  tcase_add_loop_test(tc, loops_over_values, 0, 3);\n\
  suite_add_tcase(s, tc);\n\
  return s;\n\
}\n";

#[test]
fn check_start_test_blocks_are_test_functions() {
    let result = extract("src/check_calc.c", CHECK_SUITE);
    for (name, line) in [("test_check_adds", 5), ("loops_over_values", 11)] {
        let test = function(&result, name);
        assert_eq!(test_role(&result, name), Some("test_case"), "{name}");
        assert_eq!(test.start_line, line, "{name}");
        assert_eq!(
            test.signature.as_deref(),
            Some(format!("START_TEST({name})").as_str())
        );
        assert!(
            result
                .identifiers
                .iter()
                .filter(|i| i.name.starts_with("ck_assert_int_"))
                .all(|i| i.containing_symbol_id.is_some()),
            "{:#?}",
            result.identifiers
        );
    }
    let adds = function(&result, "test_check_adds");
    assert!(
        result
            .identifiers
            .iter()
            .any(|i| i.name == "ck_assert_int_eq"
                && i.containing_symbol_id.as_deref() == Some(adds.id.as_str()))
    );
    for (name, role) in [
        ("make_up", "fixture_setup"),
        ("break_down", "fixture_teardown"),
        ("cache_up", "fixture_setup"),
    ] {
        assert_eq!(test_role(&result, name), Some(role), "{name}");
    }
    assert_eq!(test_role(&result, "calc_suite"), None);
    assert_eq!(
        meta(function(&result, "calc_suite"), "returnType"),
        Some(&serde_json::json!("Suite*"))
    );
    assert!(
        result
            .symbols
            .iter()
            .all(|s| s.name != "START_TEST" && s.name != "END_TEST"),
        "{:#?}",
        result.symbols
    );
    let header_tokens: Vec<(u32, &str)> = result
        .identifiers
        .iter()
        .filter(|i| matches!(i.start_line, 5 | 9 | 11 | 15))
        .map(|i| (i.start_line, i.name.as_str()))
        .collect();
    assert!(header_tokens.is_empty(), "{header_tokens:?}");
    assert!(
        result
            .pending_relationships
            .iter()
            .all(|p| !matches!(p.callee_name.as_str(), "START_TEST" | "END_TEST")),
        "{:#?}",
        result.pending_relationships
    );
    let registered: Vec<&str> = result
        .identifiers
        .iter()
        .filter(|i| i.start_line == 22 || i.start_line == 23)
        .map(|i| i.name.as_str())
        .collect();
    for name in ["test_check_adds", "loops_over_values"] {
        assert!(registered.contains(&name), "{name}: {registered:?}");
    }
}

#[test]
fn cmocka_registrations_still_flag_tests_outside_a_test_path() {
    let result = extract(
        "src/list_runner.c",
        "#include <cmocka.h>\nstatic void pushes(void **state) { }\nint main(void) {\n  const struct CMUnitTest tests[] = { cmocka_unit_test(pushes) };\n  return cmocka_run_group_tests(tests, NULL, NULL);\n}\n",
    );
    assert_eq!(test_role(&result, "pushes"), Some("test_case"));
    assert_eq!(test_role(&result, "main"), None);
}
