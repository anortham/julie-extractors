%% Reference-shaped Erlang: every construct below looks like an edge to a
%% resolver. The golden proves the identifier and relationship tiers emit the
%% RIGHT rows and no wrong ones - no module reference is invented for an
%% auto-imported BIF or for a dynamic call through a variable, a fun reference
%% stays distinct from a call to the same function and produces no call edge,
%% and only queue/1 (defined here) resolves to a same-file target.
-module(negative).
-behaviour(gen_server).

-include_lib("stdlib/include/assert.hrl").

-export([run/2]).

-define(TIMEOUT, 5000).

-record(request, {id, payload}).

run(Payload, Fun) ->
    Request = #request{id = erlang:unique_integer(), payload = Payload},
    timer:sleep(?TIMEOUT),
    _ = length(queue(Request)),
    _ = Fun(Request),
    Queue = fun queue/1,
    {lists:reverse(queue(Request)), Queue}.

queue(#request{payload = Payload}) ->
    Payload.
