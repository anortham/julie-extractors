-module(factory).
-type result() :: integer().
-spec make() -> result().
make() -> 1.

-record(workspace, {root}).

-spec load() -> #workspace{}.
load() -> #workspace{}.

-spec pick(T) -> T.
pick(Value) -> Value.

use() ->
    Result = make(),
    Workspace = ?MODULE:load(),
    Picked = pick(Result),
    Remote = other:load(),
    Updated = Workspace#workspace{root = "/"},
    {Result, Workspace, Picked, Remote, Updated}.
