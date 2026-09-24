# Erlang support

`erlang` handles `.erl`, `.hrl`, and `.escript` files with
`tree-sitter-erlang`.

## Types

- A function with a `-spec` for its name and arity gets the base name of the
  declared return type (`integer()` -> `integer`, `#state{}` -> `state`,
  `unicode:chardata()` -> `unicode:chardata`). `-type`, `-opaque`, and
  `-callback` forms record their base type the same way. Unions, tuples,
  list and map literal syntax (`[t()]`, `#{..}`), funs, ranges, binaries,
  and type variables record nothing. The named types `list(t())`,
  `nonempty_list(t())`, and `map()` record their base name (`list`,
  `nonempty_list`, `map`).
- A `#state{} = S` head pattern records the declared fact `state`.
- A body match `X = Value`, or `X ?= Value` in a `maybe` block, records an
  inferred fact when `Value` is one of these:
  - a record literal (`#state{}`) or record update (`Old#state{..}`) of a
    `-record` in the same file.
  - a call to a function defined in the same file with a `-spec` of the same
    arity: `load()`, `?MODULE:load()`, or `this_module:load()`. Every spec
    clause must declare the same type. The fact is its base type. Comments
    inside the parentheses do not count as arguments.
  - one of these values in parentheses (`(load())`) or as the last
    expression of `begin .. end`.
- `X = Y = load()` gives both variables the type.
- `{ok, X} = load()` and `{ok, X} ?= load()` bind `X` as a local. `X` gets `T` when every `{ok, T}`
  alternative of the spec agrees on `T`. The other alternatives must be atoms
  or tuples with a different tag or size (`{error, term()}`, `ignore`),
  because only those cannot match `{ok, X}`. The rule is the same for `=`
  and `?=`: in both, `X` is bound only when the match succeeds.
- A variable bound by several matches in one function keeps a fact only when
  every match gives it the same type. Erlang gets one symbol per variable
  name per function, so the fact is also dropped when any other pattern in
  the function binds the name: a match of another shape
  (`{error, S} = f()`, `[S | _] = f()`, `#{k := S} = f()`), a `case`,
  `receive`, `try`, `maybe .. else`, or `catch` clause pattern, a generator,
  or a fun head. A record match inside such a pattern
  (`case .. of S = #state{} ->`, `{ok, #state{} = S} = f()`) still types the
  variable.
- These cases record no inferred fact:
  - a call to another module, a fun variable (`F()`), a macro call, or
    `catch load()`.
  - a call with an arity that no same-file definition has, a callee with no
    `-spec`, or a `-spec` with no same-file definition. Other files are out
    of scope because each file is extracted alone.
  - `?MODULE:load()` in a `.hrl` header, which has no module name.
  - a spec that returns a type variable (`-> T`, `-> T when T :: t()`), a
    union, a tuple, list or map literal syntax (`[t()]`, `#{..}`), or a fun.
    A spec that returns `list(t())` or `map()` gives `list` or `map`.
  - a spec that returns an atom literal (`-> ok`). The atom is one value, not
    a type, so it must not join to a user type named `ok()`.
  - a spec that returns `no_return()` or `none()`. The call never returns a
    value.
  - spec clauses or duplicate specs that disagree on the return type. The
    same base name is not enough: `#state{}` and `state()`, or
    `list(integer())` and `list(atom())`, disagree.
  - a call or a `-spec` with a macro argument (`load(?ARGS)`). A macro can
    expand to several arguments, so the arity is unknown.
  - a function with clauses on both sides of a preprocessor conditional
    (`-ifdef`, `-ifndef`, `-if`, `-elif`, `-else`, `-endif`). Which
    definition a build compiles is unknown.
  - for `{ok, X} = load()`: two different `{ok, T}` payloads, a payload that
    is not a named type (`{ok, [t()]}`, `{ok, done}`, `{ok, T}`), a named type
    or type variable among the alternatives, and a return through an alias
    such as `-> result()` with `-type result() :: {ok, t()} | error`.
  - other tuple patterns (`{error, R} = load()`, `{ok, A, B} = load()`) and
    `case` or `receive` clause patterns. These do not bind body locals.
  - `begin .. end` whose last expression is not one of the values above.
