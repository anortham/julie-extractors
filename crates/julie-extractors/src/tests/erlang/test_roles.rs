use super::support::{extract_from, find, find_kind};
use crate::base::{Symbol, SymbolKind};

fn role(symbol: &Symbol, key: &str) -> bool {
    symbol
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get(key))
        .and_then(serde_json::Value::as_bool)
        == Some(true)
}

const EUNIT: &str = r#"-module(bank_tests).

-include_lib("eunit/include/eunit.hrl").

-export([balance_test/0, balance_test_/0, check_test/1, helper/0]).

balance_test() ->
    ok.

balance_test_() ->
    [].

check_test(_Account) ->
    ok.

helper() ->
    ok.
"#;

const SUITE: &str = r#"-module(bank_SUITE).

-include_lib("common_test/include/ct.hrl").

-export([all/0, groups/0, suite/0,
         init_per_suite/1, end_per_suite/1,
         init_per_testcase/2, end_per_testcase/2,
         init_per_group/2, end_per_group/2,
         opens_account/1, closes_account/1]).

all() ->
    [opens_account, closes_account].

groups() ->
    [].

suite() ->
    [].

init_per_suite(Config) ->
    Config.

end_per_suite(_Config) ->
    ok.

init_per_testcase(_Case, Config) ->
    Config.

end_per_testcase(_Case, _Config) ->
    ok.

init_per_group(_Group, Config) ->
    Config.

end_per_group(_Group, _Config) ->
    ok.

opens_account(_Config) ->
    ok.

closes_account(_Config) ->
    ok.
"#;

const PRODUCTION: &str = r#"-module(bank).

-export([all/0, init_per_suite/1, settle/1]).

all() ->
    [].

init_per_suite(Config) ->
    Config.

settle(Amount) ->
    Amount.
"#;

#[test]
fn eunit_module_named_tests_is_a_test_container() {
    let symbols = extract_from("bank_tests.erl", EUNIT);
    let module = find_kind(&symbols, "bank_tests", SymbolKind::Module);

    assert!(role(module, "test_container"));
}

#[test]
fn module_including_the_eunit_header_is_a_test_container() {
    let code = r#"-module(bank).
-include_lib("eunit/include/eunit.hrl").
-export([open/1]).
open(Id) -> Id.
"#;
    let symbols = extract_from("bank.erl", code);
    let module = find_kind(&symbols, "bank", SymbolKind::Module);

    assert!(role(module, "test_container"));
}

#[test]
fn production_module_is_not_a_test_container() {
    let symbols = extract_from("bank.erl", PRODUCTION);
    let module = find_kind(&symbols, "bank", SymbolKind::Module);

    assert!(!role(module, "test_container"));
}

#[test]
fn zero_arity_test_suffixed_functions_are_test_cases() {
    let symbols = extract_from("bank_tests.erl", EUNIT);

    for name in ["balance_test", "balance_test_"] {
        let function = find(&symbols, name);
        assert!(role(function, "is_test"), "{name} should be a test case");
        assert!(!role(function, "test_lifecycle"));
    }
}

#[test]
fn test_suffixed_function_with_arguments_is_not_a_test_case() {
    let symbols = extract_from("bank_tests.erl", EUNIT);

    assert!(!role(find(&symbols, "check_test"), "is_test"));
}

#[test]
fn plain_function_in_a_test_module_is_not_a_test_case() {
    let symbols = extract_from("bank_tests.erl", EUNIT);

    assert!(!role(find(&symbols, "helper"), "is_test"));
}

#[test]
fn common_test_suite_module_is_a_test_container() {
    let symbols = extract_from("bank_SUITE.erl", SUITE);
    let module = find_kind(&symbols, "bank_SUITE", SymbolKind::Module);

    assert!(role(module, "test_container"));
}

#[test]
fn exported_config_taking_functions_in_a_suite_are_test_cases() {
    let symbols = extract_from("bank_SUITE.erl", SUITE);

    for name in ["opens_account", "closes_account"] {
        let function = find(&symbols, name);
        assert!(role(function, "is_test"), "{name} should be a test case");
        assert!(!role(function, "test_lifecycle"));
    }
}

#[test]
fn common_test_hooks_are_lifecycle_not_cases() {
    let symbols = extract_from("bank_SUITE.erl", SUITE);

    for name in [
        "init_per_suite",
        "end_per_suite",
        "init_per_testcase",
        "end_per_testcase",
        "init_per_group",
        "end_per_group",
    ] {
        let hook = find(&symbols, name);
        assert!(role(hook, "test_lifecycle"), "{name} should be lifecycle");
        assert!(role(hook, "is_test"), "{name} should also carry is_test");
    }
}

#[test]
fn suite_configuration_callbacks_are_not_test_cases() {
    let symbols = extract_from("bank_SUITE.erl", SUITE);

    for name in ["all", "groups", "suite"] {
        let callback = find(&symbols, name);
        assert!(!role(callback, "is_test"), "{name} must not be a case");
        assert!(!role(callback, "test_lifecycle"));
    }
}

#[test]
fn suite_named_functions_outside_a_suite_module_carry_no_role() {
    let symbols = extract_from("bank.erl", PRODUCTION);

    for name in ["all", "init_per_suite", "settle"] {
        let function = find(&symbols, name);
        assert!(!role(function, "is_test"), "{name} must not be a case");
        assert!(!role(function, "test_lifecycle"));
    }
}
