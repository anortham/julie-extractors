-module(factory).
-type result() :: integer().
-spec make() -> result().
make() -> 1.

-record(workspace, {root}).

-spec load() -> #workspace{}.
load() -> #workspace{}.

-spec pick(T) -> T.
pick(Value) -> Value.

-spec open() -> {ok, #workspace{}} | {error, term()}.
open() -> {ok, #workspace{}}.

-spec stop() -> ok.
stop() -> ok.

use() ->
    Result = make(),
    Workspace = ?MODULE:load(),
    Picked = pick(Result),
    Remote = other:load(),
    Updated = Workspace#workspace{root = "/"},
    {ok, Opened} = open(),
    Stopped = stop(),
    {Result, Workspace, Picked, Remote, Updated, Opened, Stopped}.
