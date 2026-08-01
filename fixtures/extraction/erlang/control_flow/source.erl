%% Branch and literal fixture. Erlang spells every branch inside an expression,
%% so the golden carries the whole set at once:
%%
%%   decisions - case/if/try/receive containers and their arms, the old-style
%%               `catch Expr`, and each `;`-separated guard alternative, in a
%%               clause head as well as in an arm
%%   loops     - list and binary comprehensions; Erlang has no loop statement
%%   literals  - string arguments of a call, carried by the verbatim callee:
%%               `io:format` for a remote call, `audit` for a local one
%%
%% Negative controls in the same file: handle/1 branches nowhere, so its metric
%% must be all zeros, and the `-moduledoc` and `-include_lib` strings are
%% declaration text rather than call arguments, so neither may become a literal.
-module(flow).
-moduledoc "Branch shapes measured by the complexity metric.".

-include_lib("stdlib/include/assert.hrl").

-export([classify/2, serve/1, drain/1]).

classify(Value, Items) when is_integer(Value); is_float(Value) ->
    Doubled = [X * 2 || X <- Items],
    Packed = << <<X:8>> || X <- Items >>,
    case Value of
        0 -> audit("zero", Packed);
        N when N > 10 -> io:format("big ~p~n", [N]);
        _ -> {other, Doubled}
    end.

serve(Timeout) ->
    receive
        {call, From} ->
            try handle(From) of
                Result -> Result
            catch
                error:Reason -> io:format("failed ~p~n", [Reason])
            after
                audit("closed", From)
            end
    after Timeout ->
        timeout
    end.

drain(Pid) ->
    if
        is_pid(Pid) -> catch exit(Pid, kill);
        true -> ok
    end.

audit(Label, Payload) ->
    {Label, Payload}.

handle(From) ->
    From.
