use super::extract_symbols;

fn role<'a>(symbols: &'a [crate::base::Symbol], name: &str, key: &str) -> bool {
    symbols
        .iter()
        .find(|symbol| symbol.name == name)
        .and_then(|symbol| symbol.metadata.as_ref())
        .and_then(|metadata| metadata.get(key))
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false)
}

#[test]
fn pgtap_setof_text_routines_emit_case_and_lifecycle_roles() {
    let symbols = extract_symbols(
        r#"
CREATE FUNCTION test_user() RETURNS SETOF TEXT LANGUAGE SQL AS $$ SELECT ok(true); $$;
CREATE FUNCTION setup_user() RETURNS SETOF TEXT LANGUAGE SQL AS $$ SELECT pass('setup'); $$;
CREATE FUNCTION teardown_user() RETURNS SETOF TEXT LANGUAGE SQL AS $$ SELECT pass('teardown'); $$;
CREATE FUNCTION startup_suite() RETURNS SETOF TEXT LANGUAGE SQL AS $$ SELECT pass('startup'); $$;
CREATE FUNCTION shutdown_suite() RETURNS SETOF TEXT LANGUAGE SQL AS $$ SELECT pass('shutdown'); $$;
SELECT * FROM runtests('public', '^test');
"#,
    );

    assert!(role(&symbols, "test_user", "is_test"));
    assert!(!role(&symbols, "test_user", "test_lifecycle"));
    for name in [
        "setup_user",
        "teardown_user",
        "startup_suite",
        "shutdown_suite",
    ] {
        assert!(role(&symbols, name, "is_test"));
        assert!(role(&symbols, name, "test_lifecycle"));
    }
}

#[test]
fn pgtap_roles_require_runner_and_setof_text() {
    let without_runner = extract_symbols(
        r#"
CREATE FUNCTION test_without_runner() RETURNS SETOF TEXT LANGUAGE SQL AS $$ SELECT ok(true); $$;
"#,
    );
    let wrong_return = extract_symbols(
        r#"
CREATE FUNCTION test_wrong_return() RETURNS TEXT LANGUAGE SQL AS $$ SELECT 'ordinary'; $$;
SELECT * FROM runtests('public', '^test');
"#,
    );
    let ordinary = extract_symbols(
        r#"
CREATE FUNCTION helper_text() RETURNS SETOF TEXT LANGUAGE SQL AS $$ SELECT 'ordinary'; $$;
SELECT * FROM runtests('public', '^test');
"#,
    );

    assert!(!role(&without_runner, "test_without_runner", "is_test"));
    assert!(!role(&wrong_return, "test_wrong_return", "is_test"));
    assert!(!role(&ordinary, "helper_text", "is_test"));
}

#[test]
fn do_tap_runner_gates_case_roles() {
    let symbols = extract_symbols(
        r#"
CREATE FUNCTION test_do_tap() RETURNS SETOF TEXT LANGUAGE SQL AS $$ SELECT ok(true); $$;
SELECT * FROM do_tap('public', '^test');
"#,
    );

    assert!(role(&symbols, "test_do_tap", "is_test"));
}

#[test]
fn pgtap_runners_mark_only_exactly_targeted_emitted_schemas_as_containers() {
    let symbols = extract_symbols(
        r#"
CREATE SCHEMA app;
CREATE SCHEMA analytics;
CREATE SCHEMA near_match;
SELECT * FROM runtests('app', '^test');
SELECT * FROM do_tap('analytics', '^test');
SELECT * FROM runtests('missing', '^test');
"#,
    );

    assert!(role(&symbols, "app", "test_container"));
    assert!(role(&symbols, "analytics", "test_container"));
    assert!(!role(&symbols, "near_match", "test_container"));
    assert!(!symbols.iter().any(|symbol| symbol.name == "missing"));
}
