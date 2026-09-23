%% Cowboy dispatch tables, outbound HTTP clients (httpc, hackney, gun), and
%% SQL/URL literal carriers, including binary-string arguments.
-module(kv_http).
-export([start/2, fetch/2]).

start(_Type, _Args) ->
    Dispatch = cowboy_router:compile([
        {'_', [
            {"/", kv_root_handler, []},
            {"/keys/:key", [{key, nonempty}], kv_key_handler, #{mode => single}},
            {"/static/[...]", cowboy_static, {priv_dir, kv, "static"}}
        ]},
        {"admin.example.com", [{<<"/stats">>, kv_stats_handler, []}]}
    ]),
    cowboy:start_clear(kv_listener, [{port, 8080}], #{env => #{dispatch => Dispatch}}).

fetch(Conn, Body) ->
    httpc:request("https://api.example.com/v1/health"),
    httpc:request(post, {"http://audit.internal/events", [], "application/json", Body}, [], []),
    hackney:request(delete, <<"https://api.example.com/v1/orders/1">>, [], <<>>, []),
    hackney:get(<<"https://api.example.com/v1/orders">>),
    gun:post(Conn, "/v1/events", [], Body),
    epgsql:squery(Conn, "SELECT id, name FROM users WHERE active = true"),
    epgsql:equery(Conn, <<"SELECT id FROM users WHERE email = $1">>, [Body]).
