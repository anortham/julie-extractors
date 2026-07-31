%% @doc Account bookkeeping primitives.
%% Balances are stored in whole cents.
-module(bank).
-moduledoc "Account ledger entry points.".

-behaviour(gen_server).

-export([open/1, balance/1, deposit/2]).
-export_type([account/0]).

-define(MAX_BALANCE, 1000000).
-define(LOG(Msg), io:format("~p~n", [Msg])).

-record(account, {id :: integer(), balance = 0 :: integer()}).

-type account() :: #account{}.
-opaque token() :: binary().

-callback init(Args :: term()) -> {ok, term()}.

-spec open(integer()) -> #account{}.
%% @doc Open a new account with a zero balance.
open(Id) ->
    #account{id = Id}.

-doc "Read the stored balance of an account.".
balance(#account{balance = B}) ->
    B.

deposit(Acct, Amount) when Amount > 0 ->
    Acct#account{balance = Acct#account.balance + Amount};
deposit(Acct, _Amount) ->
    Acct.

% internal helper, never exported
audit(Acct) ->
    ?LOG(Acct),
    ok.

balance_test() ->
    0 = balance(#account{id = 1}).
