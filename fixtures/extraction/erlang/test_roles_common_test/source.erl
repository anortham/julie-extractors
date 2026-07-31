%% Common Test roles. A `*_SUITE` module is a test container; inside it the
%% golden carries all three roles at once:
%%
%%   container - bank_SUITE                         test_container
%%   lifecycle - init_per_suite/1, end_per_suite/1  is_test + test_lifecycle
%%               init_per_testcase/2, end_per_testcase/2
%%               init_per_group/2, end_per_group/2
%%   case      - opens_account/1, closes_account/1  is_test, run as Case(Config)
%%
%% Negative controls in the same file: all/0 and groups/0 configure the suite
%% rather than exercise it, and format_report/2 is an exported helper whose
%% arity is not the Case(Config) shape Common Test invokes. None of the three
%% may carry a test role.
-module(bank_SUITE).

-include_lib("common_test/include/ct.hrl").

-export([all/0, groups/0,
         init_per_suite/1, end_per_suite/1,
         init_per_testcase/2, end_per_testcase/2,
         init_per_group/2, end_per_group/2,
         opens_account/1, closes_account/1,
         format_report/2]).

-spec all() -> [atom()].
all() ->
    [opens_account, closes_account].

groups() ->
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

format_report(_Case, _Config) ->
    ok.
