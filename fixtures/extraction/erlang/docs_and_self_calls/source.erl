%%
%% %CopyrightBegin%
%% SPDX-License-Identifier: Apache-2.0
%% %CopyrightEnd%
%%
-module(ledger).
-moduledoc("Ledger entry points.").

-export([start/0, loop/1, handle/2, open/1, lookup/1, route/2]).

-define(is_ok(X), X =:= ok).
-define(LIMIT, 10).

-record(account, {id, owner}).

%%--------------------------------------------------------------------
%% @doc
%% Starts the ledger loop.
%% @end
%%--------------------------------------------------------------------
-spec start() -> pid().
start() ->
    Handler = fun ?MODULE:handle/2,
    Local = fun lookup/1,
    spawn(fun() -> ?MODULE:loop(?LIMIT) end),
    ledger:loop(0),
    {Handler, Local}.

-doc("Receives until stopped.").
loop(N) ->
    receive
        stop -> ok;
        _ -> ?MODULE:loop(N + 1)
    end.

%%%===================================================================
%%% Internal functions
%%%===================================================================

handle(A, B) -> {A, B}.

open(#account{id = Id} = Account) ->
    Fresh = #account{id = Id, owner = maps:get(owner, #{owner => none})},
    ?is_ok(ok),
    {Fresh, Account}.

lookup(Key) -> Key.

%% Route a get request.
route({get, Key}, State) ->
    {lookup(Key), State};
%% Route a put request.
route({put, Key, Value}, State) when is_binary(Key) ->
    {open(#account{id = Value}), State};
%% Anything else.
route(_Other, State) ->
    {error, State}.
