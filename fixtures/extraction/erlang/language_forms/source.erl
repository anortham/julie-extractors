%% Wave-2 Erlang language forms: remote-call arity, MFA carriers, test-only
%% conditional blocks, record field types and defaults, nominal types,
%% native-record exports and imports, spec parameter types, type arguments,
%% OTP 27 ?MODULEDOC/?DOC macros, and body comments.
-module(inventory).
?MODULEDOC("Inventory service.").

-export([start/0, reserve/2, loop/1, run/1, init/1]).
-export_type([sku/0]).
-export_record([item]).
-import_record(geo, [point]).
-import(ledger, [flush/1]).

-ifdef(TEST).
-compile([export_all, nowarn_export_all]).
-include_lib("eunit/include/eunit.hrl").
-endif.

-nominal sku() :: binary().
-type result(T) :: {ok, T} | {error, term()}.
-type conn() :: gen_tcp:socket().

-record(item, {sku :: sku(), qty = 0 :: non_neg_integer(),
               added = erlang:monotonic_time() :: integer()}).
-record(state, {conn :: conn(), items = #{} :: #{sku() => #item{}}}).

?DOC("Start the inventory loop.").
-spec start() -> pid().
start() ->
    Pid = spawn(?MODULE, loop, [#state{}]),
    timer:apply_after(1000, ?MODULE, run, [tick]),
    rpc:call(node(), audit, record, [start]),
    Pid.

-doc("Reserve a quantity of a SKU.").
-spec reserve(sku(), pos_integer()) -> result(#item{}).
reserve(Sku, Qty) ->
    %% Stock levels are checked by the ledger.
    ledger:record(Sku, Qty),
    ledger:record(Sku, Qty, now),
    flush(Sku),
    {ok, #item{sku = Sku, qty = Qty}}.

loop(State) -> State.

run(Tick) -> Tick.

init(Opts) ->
    Children = [#{id => worker, start => {inventory_worker, start_link, [Opts]}},
                {cache, {inventory_cache, start_link, []}, permanent, 5000, worker, [inventory_cache]}],
    {ok, {#{strategy => one_for_one}, Children}}.

internal(Point) -> #geo:point{x = Point}.

-ifdef(TEST).
reserve_test() -> {ok, _} = reserve(<<"a">>, 1).
-endif.
