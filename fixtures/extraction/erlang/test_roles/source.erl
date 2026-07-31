%% EUnit test roles. The module is a test container two ways over — it is
%% named `*_tests` AND it includes `eunit.hrl` — and the golden separates the
%% two case shapes EUnit recognises from the lookalikes it does not:
%%
%%   container - bank_tests                         test_container
%%   case      - balance_test/0                     is_test, a plain test
%%   case      - deposit_test_/0                    is_test, a test generator
%%
%% Negative controls in the same file: check_test/1 carries the suffix but
%% takes an argument, so EUnit never runs it, and setup/0 is an ordinary
%% helper. Neither may carry a test role.
-module(bank_tests).

-include_lib("eunit/include/eunit.hrl").

-export([balance_test/0, deposit_test_/0, check_test/1, setup/0]).

-spec balance_test() -> ok.
balance_test() ->
    ok.

-spec deposit_test_() -> [term()].
deposit_test_() ->
    [].

check_test(_Account) ->
    ok.

setup() ->
    ok.
