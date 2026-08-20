//! C Criterion call-style test extraction (Miller bridge test-roles).
//!
//! Criterion's `Test(suite, name) { ... }` macro parses as a `call_expression`
//! (not a function_definition), so it is materialized via the shared
//! `crate::test_calls` core through `c/test_calls.rs`. The name is the two
//! identifier arguments joined `suite.name`; detection is structural (call-based),
//! so it is path-independent — proven here with a `src/` path.

use super::extract_symbols_with_name;
use crate::base::{Symbol, SymbolKind};

fn is_test(sym: &Symbol) -> bool {
    sym.metadata
        .as_ref()
        .and_then(|m| m.get("is_test"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
}

fn has_role(sym: &Symbol, role: &str) -> bool {
    sym.metadata
        .as_ref()
        .and_then(|m| m.get(role))
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
}

#[test]
fn c_qualified_callee_is_not_materialized() {
    // FALSE-POSITIVE GUARD (#66): `classify_call` keys on the segment before the
    // first '.', so a member call whose RECEIVER is a vocab word
    // (`Test.run(...)`) would otherwise be misclassified as a Criterion `Test`.
    // Criterion's `Test` macro is always a bare identifier, never a member access
    // (`field_expression`), so a qualified callee must never materialize a test.
    let syms = extract_symbols_with_name(
        r#"
int run_config(void) {
    Test.run(alpha, beta);
    return 0;
}
"#,
        "src/config.c",
    );
    let has_role = |s: &Symbol| {
        is_test(s)
            || s.metadata
                .as_ref()
                .and_then(|m| m.get("test_container"))
                .and_then(|v| v.as_bool())
                .unwrap_or(false)
    };
    assert!(
        !syms.iter().any(has_role),
        "qualified callee `Test.run(...)` must not materialize a test symbol, got {syms:?}"
    );
}

#[test]
fn criterion_test_is_named_suite_dot_name_and_flagged() {
    // NON-test path proves call-based (not name/path) detection.
    let syms = extract_symbols_with_name(
        r#"
Test(math, addition) {
    cr_assert(2 + 2 == 4);
}
"#,
        "src/math.c",
    );
    let t = syms
        .iter()
        .find(|s| s.name == "math.addition")
        .unwrap_or_else(|| {
            panic!("expected a Criterion test symbol `math.addition`, got {syms:?}")
        });
    assert_eq!(t.kind, SymbolKind::Function);
    assert!(is_test(t), "Criterion Test must be is_test=true, got {t:?}");
}

#[test]
fn criterion_test_with_options_ignores_trailing_args() {
    // `Test(suite, name, .init = setup)` — the fixture options are trailing args;
    // only the first two identifiers form the name.
    let syms = extract_symbols_with_name(
        r#"
Test(suite, with_setup, .init = setup_fn, .fini = teardown_fn) {
    cr_assert(1);
}
"#,
        "tests/test_suite.c",
    );
    let t = syms
        .iter()
        .find(|s| s.name == "suite.with_setup")
        .unwrap_or_else(|| panic!("expected `suite.with_setup`, got {syms:?}"));
    assert!(
        is_test(t),
        "Criterion Test with options must be is_test=true"
    );
}

#[test]
fn criterion_suite_and_referenced_fixtures_emit_roles() {
    let syms = extract_symbols_with_name(
        r#"
TestSuite(math, .init = setup_suite, .fini = teardown_suite);

Test(math, addition, .init = setup_test, .fini = teardown_test) {
    cr_assert(2 + 2 == 4);
}

void setup_suite(void) {}
void teardown_suite(void) {}
void setup_test(void) {}
void teardown_test(void) {}
void setup_unreferenced(void) {}
"#,
        "src/math.c",
    );
    let suite = syms
        .iter()
        .find(|s| s.name == "math")
        .unwrap_or_else(|| panic!("expected Criterion suite symbol, got {syms:?}"));
    assert!(has_role(suite, "test_container"));
    assert!(!is_test(suite));

    let test = syms
        .iter()
        .find(|s| s.name == "math.addition")
        .unwrap_or_else(|| panic!("expected Criterion test symbol, got {syms:?}"));
    assert!(is_test(test));

    for name in [
        "setup_suite",
        "teardown_suite",
        "setup_test",
        "teardown_test",
    ] {
        let hook = syms
            .iter()
            .find(|s| s.name == name)
            .unwrap_or_else(|| panic!("expected hook function {name}, got {syms:?}"));
        assert!(is_test(hook), "{name} must be is_test");
        assert!(has_role(hook, "test_lifecycle"), "{name} must be lifecycle");
    }

    let ordinary = syms
        .iter()
        .find(|s| s.name == "setup_unreferenced")
        .unwrap_or_else(|| panic!("expected ordinary helper, got {syms:?}"));
    assert!(!is_test(ordinary));
    assert!(!has_role(ordinary, "test_lifecycle"));
}

#[test]
fn criterion_suite_name_arguments_do_not_create_containers() {
    let syms = extract_symbols_with_name(
        r#"
Test(math, addition) {
    cr_assert(1);
}
"#,
        "src/math.c",
    );
    assert!(syms.iter().any(|s| s.name == "math.addition" && is_test(s)));
    assert!(!syms.iter().any(|s| has_role(s, "test_container")));
}

#[test]
fn non_criterion_call_is_not_materialized() {
    // A plain function call (assertion helper) must NOT become a test symbol.
    let syms = extract_symbols_with_name(
        r#"
void helper(void) {
    cr_assert(1 == 1);
    printf("hi");
}
"#,
        "tests/test_suite.c",
    );
    assert!(
        !syms.iter().any(is_test),
        "no call inside a plain helper should be flagged is_test, got {syms:?}"
    );
    // The helper function itself is a normal symbol, not a Criterion test.
    let helper = syms
        .iter()
        .find(|s| s.name == "helper")
        .expect("helper function should still be extracted");
    assert!(!is_test(helper), "the helper function must not be is_test");
}
