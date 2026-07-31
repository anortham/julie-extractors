%% Reference-shaped Erlang that must not produce relationship, pending, or
%% identifier rows while Erlang ships the symbol tier only. Every construct
%% below looks like a reference to a resolver: a remote call, a macro call, a
%% record construction, a behaviour declaration, and an include directive.
-module(negative).
-behaviour(gen_server).

-include_lib("stdlib/include/assert.hrl").

-export([run/1]).

-define(TIMEOUT, 5000).

-record(request, {id, payload}).

run(Payload) ->
    Request = #request{id = erlang:unique_integer(), payload = Payload},
    timer:sleep(?TIMEOUT),
    lists:reverse(queue(Request)).

queue(#request{payload = Payload}) ->
    Payload.
