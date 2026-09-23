%% EUnit fixture roles and PropEr properties in one test module.
%%
%%   setup/0        fixture_setup     named in a {setup, ...} Setup slot
%%   cleanup/1      fixture_teardown  named in the Cleanup slot
%%   put_then_get/0 test_case         an instantiator in the Tests list
%%   missing/0      test_case         a {foreach, Where, ...} test
%%   prop_roundtrip/0 test_case       a PropEr property
%%   helper/1       none              an ordinary helper
-module(prop_kv).
-include_lib("eunit/include/eunit.hrl").
-include_lib("proper/include/proper.hrl").

kv_test_() ->
    {setup, fun setup/0, fun cleanup/1, [fun put_then_get/0]}.

loop_test_() ->
    {foreach, local, fun setup/0, fun cleanup/1, [fun missing/0]}.

setup() -> ok.
cleanup(_) -> ok.
put_then_get() -> ok.
missing() -> ok.

prop_roundtrip() ->
    ?FORALL(L, list(integer()), helper(L) =:= L).

helper(L) -> L.
