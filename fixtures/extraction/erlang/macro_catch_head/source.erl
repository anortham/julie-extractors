-module(macro_catch_head).
-export([handle/1, later/0]).

handle(Value) ->
    try Value of
        ok -> ok;
        _ -> ok
    catch
        ?WITH_STACKTRACE(Class, Reason, Stacktrace)
            detach(Reason),
            log(Class, Stacktrace)
    end.

later() -> ok.
