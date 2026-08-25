//! PHP Pest call-style test detection (Miller bridge test-roles).
//!
//! Pest declares tests as call expressions (`test(...)`, `it(...)`,
//! `describe(...)`), not named function declarations. The php extractor
//! recognises these via the shared `crate::test_calls` core and emits the
//! canonical `is_test` / `test_container` / `test_lifecycle` metadata,
//! byte-identical to the Lua/R/JS/TS call-style paths. These tests assert that
//! metadata on the public `extract_symbols` output and confirm that non-DSL
//! calls (`array_map`, `expect(...)` matchers) do NOT become test symbols.

use crate::base::Symbol;
use crate::php::PhpExtractor;
use std::path::PathBuf;
use tree_sitter::Parser;

fn symbols(code: &str) -> Vec<Symbol> {
    symbols_at("ExampleTest.php", code)
}

fn symbols_at(file_path: &str, code: &str) -> Vec<Symbol> {
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_php::LANGUAGE_PHP.into())
        .expect("load PHP grammar");
    let tree = parser.parse(code, None).expect("parse PHP");
    let mut ext = PhpExtractor::new(
        "php".to_string(),
        file_path.to_string(),
        code.to_string(),
        &PathBuf::from("/test/workspace"),
    );
    ext.extract_symbols(&tree)
}

fn meta_bool(s: &Symbol, key: &str) -> bool {
    s.metadata
        .as_ref()
        .and_then(|m| m.get(key))
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
}

fn role<'a>(symbols: &'a [Symbol], name: &str) -> Option<&'a str> {
    symbols
        .iter()
        .find(|symbol| symbol.name == name)
        .unwrap_or_else(|| panic!("expected a symbol named {name}, got {symbols:?}"))
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get("test_role"))
        .and_then(|value| value.as_str())
}

const PHPUNIT_SUITE: &str = r#"<?php

use PHPUnit\Framework\Attributes\Before;
use PHPUnit\Framework\Attributes\DataProvider;
use PHPUnit\Framework\Attributes\Test;
use PHPUnit\Framework\TestCase;

final class PaymentTest extends TestCase
{
    protected function setUp(): void
    {
    }

    protected function tearDown(): void
    {
    }

    #[Before]
    public function seedLedger(): void
    {
    }

    /**
     * @test
     */
    public function itRefundsAnOrder(): void
    {
    }

    #[Test]
    public function chargesAnOrder(): void
    {
    }

    #[DataProvider('provideAmounts')]
    public function testAddsAmounts(int $amount): void
    {
    }

    public static function provideAmounts(): array
    {
        return [[1], [2]];
    }

    public function buildLedger(): int
    {
        return 2;
    }
}
"#;

#[test]
fn phpunit_members_carry_their_roles_outside_a_test_path() {
    let syms = symbols_at("src/Billing/PaymentSuite.php", PHPUNIT_SUITE);

    assert_eq!(role(&syms, "PaymentTest"), Some("test_container"));
    assert_eq!(role(&syms, "setUp"), Some("fixture_setup"));
    assert_eq!(role(&syms, "tearDown"), Some("fixture_teardown"));
    assert_eq!(role(&syms, "seedLedger"), Some("fixture_setup"));
    assert_eq!(role(&syms, "itRefundsAnOrder"), Some("test_case"));
    assert_eq!(role(&syms, "chargesAnOrder"), Some("test_case"));
    assert_eq!(role(&syms, "testAddsAmounts"), Some("parameterized_test"));
}

#[test]
fn a_data_provider_method_is_a_helper_not_a_test() {
    let syms = symbols_at("tests/PaymentTest.php", PHPUNIT_SUITE);

    assert_eq!(role(&syms, "provideAmounts"), None);
    assert_eq!(role(&syms, "buildLedger"), None);
}

#[test]
fn a_class_records_its_base_types() {
    let syms = symbols_at("tests/PaymentTest.php", PHPUNIT_SUITE);
    let suite = syms
        .iter()
        .find(|symbol| symbol.name == "PaymentTest")
        .expect("expected the suite class");

    assert_eq!(
        suite
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.get("base_types")),
        Some(&serde_json::json!(["TestCase"]))
    );
}

#[test]
fn a_test_attribute_makes_its_class_a_container() {
    let code = r#"<?php

final class ArithmeticSuite
{
    #[Test]
    public function addsNumbers(): void
    {
    }
}
"#;
    let syms = symbols_at("src/Math/ArithmeticSuite.php", code);

    assert_eq!(role(&syms, "ArithmeticSuite"), Some("test_container"));
    assert_eq!(role(&syms, "addsNumbers"), Some("test_case"));
}

#[test]
fn production_pest_lookalike_calls_earn_no_role() {
    let code = r#"<?php

describe('report sections', function () {
    it('renders a header', function () {
    });
});

test('renders a footer', function () {
});

beforeEach(function () {
});

final class ConnectionProbe
{
    public function testConnection(): bool
    {
        return true;
    }
}
"#;
    let syms = symbols_at("src/Reporting/Sections.php", code);

    assert!(
        syms.iter()
            .all(|symbol| !meta_bool(symbol, "is_test") && !meta_bool(symbol, "test_container")),
        "a production file must publish no test role, got {syms:?}"
    );
}

#[test]
fn php_qualified_callee_is_not_materialized() {
    // FALSE-POSITIVE GUARD (#66): only bare-name `function_call_expression` nodes
    // are Pest DSL calls. Method calls (`$obj->it(...)`, a `member_call_expression`)
    // and static calls (`Klass::describe(...)`, a `scoped_call_expression`) are
    // DIFFERENT node kinds, filtered before classification — even when the METHOD
    // name is a vocab word. Locks in `classify_call_exact` (centralized #66 fix).
    let code = r#"<?php
class Runner {
    public function run(): void {
        $this->it('does work', fn() => null);
        Suite::describe('group', fn() => null);
    }
}
"#;
    let syms = symbols(code);
    assert!(
        !syms
            .iter()
            .any(|s| meta_bool(s, "is_test") || meta_bool(s, "test_container")),
        "method/static callees (`$this->it`, `Suite::describe`) must not materialize a test symbol, got {syms:?}"
    );
}

#[test]
fn pest_test_it_describe_beforeeach_emit_test_role_metadata() {
    let code = r#"<?php

test('computes totals correctly', function () {
    expect(1 + 1)->toBe(2);
});

it('can create a user', function () {
    expect(true)->toBeTrue();
});

describe('User management', function () {
    it('can login', function () {
        expect(true)->toBeTrue();
    });
});

beforeEach(function () {
    // shared setup
});
"#;
    let syms = symbols(code);

    // test('...') → is_test
    let test_sym = syms
        .iter()
        .find(|s| s.name == "computes totals correctly")
        .unwrap_or_else(|| panic!("expected a `test()` symbol, got {syms:?}"));
    assert!(
        meta_bool(test_sym, "is_test"),
        "test() should be a test case"
    );
    assert!(
        !meta_bool(test_sym, "test_container"),
        "test() should not be a container"
    );

    // it('...') → is_test
    let it_sym = syms
        .iter()
        .find(|s| s.name == "can create a user")
        .unwrap_or_else(|| panic!("expected an `it()` symbol, got {syms:?}"));
    assert!(meta_bool(it_sym, "is_test"), "it() should be a test case");
    assert!(
        !meta_bool(it_sym, "test_container"),
        "it() should not be a container"
    );

    // describe('...') → test_container (not is_test)
    let describe_sym = syms
        .iter()
        .find(|s| s.name == "User management")
        .unwrap_or_else(|| panic!("expected a `describe()` container symbol, got {syms:?}"));
    assert!(
        meta_bool(describe_sym, "test_container"),
        "describe() should be a test container"
    );
    assert!(
        !meta_bool(describe_sym, "is_test"),
        "a container is not itself a test case"
    );

    // beforeEach(...) → is_test + test_lifecycle
    let before_sym = syms
        .iter()
        .find(|s| s.name == "beforeEach")
        .unwrap_or_else(|| panic!("expected a `beforeEach()` lifecycle symbol, got {syms:?}"));
    assert!(
        meta_bool(before_sym, "is_test"),
        "a lifecycle hook counts as is_test"
    );
    assert!(
        meta_bool(before_sym, "test_lifecycle"),
        "beforeEach should be a lifecycle hook"
    );
}

#[test]
fn non_dsl_calls_do_not_become_test_symbols() {
    // array_map, expect()->toBe() matchers, and bare function definitions are
    // not Pest DSL — their string args must not be materialised as test symbols.
    let code = r#"<?php

$result = array_map(fn($x) => $x * 2, [1, 2, 3]);

function helper(): void {
    echo 'not a test';
}
"#;
    let syms = symbols(code);
    assert!(
        syms.iter().all(|s| s.name != "not a test"),
        "string args of non-DSL calls must not become symbols: {syms:?}"
    );
    assert_eq!(
        syms.iter().filter(|s| meta_bool(s, "is_test")).count(),
        0,
        "no is_test metadata should come from non-DSL calls: {syms:?}"
    );
}
