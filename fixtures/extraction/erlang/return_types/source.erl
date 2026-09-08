-module(factory).
-type result() :: integer().
-spec make() -> result().
make() -> 1.
