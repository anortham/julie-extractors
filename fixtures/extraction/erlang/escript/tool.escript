#!/usr/bin/env escript
%%! -smp enable
%% A command-line script: `main/1` is the entry point.
main(Args) ->
    io:format("~p~n", [Args]).
