%% Cross-file fixture: module `ledger_client` is the A side of an A/B pair.
%% Every target it names except `retry/2` lives in another file, so the golden
%% carries both relationship shapes at once:
%%
%%   resolved  - settle/2 -> retry/2, a same-file call matched on name AND arity
%%   pending   - -behaviour(gen_server)                  implements, target gen_server
%%               ledger:record/2 and ledger:flush/0      calls, namespace ["ledger"]
%%               -include("ledger_records.hrl")          imports, import_context include
%%               -include_lib("stdlib/include/assert.hrl") imports, namespace ["stdlib","include"]
%%               -import(lists, [reverse/1])             imports, target lists
%%               reverse/1 called unqualified            calls, namespace ["lists"]
%%
%% Negative controls in the same file: length/1 is an auto-imported BIF and
%% `Fun(Ordered)` is a dynamic call through a variable, so neither may produce
%% an edge, and the -spec return type must not be read as a call.
-module(ledger_client).
-behaviour(gen_server).

-include("ledger_records.hrl").
-include_lib("stdlib/include/assert.hrl").
-import(lists, [reverse/1]).

-export([settle/2, replay/2]).

-spec settle(term(), integer()) -> {ok, integer()}.
settle(Entry, Amount) ->
    ledger:record(Entry, Amount),
    {ok, retry(Entry, Amount)}.

retry(_Entry, Amount) ->
    Amount.

replay(Entries, Fun) ->
    Ordered = reverse(Entries),
    _ = length(Ordered),
    _ = Fun(Ordered),
    ledger:flush(),
    Ordered.
