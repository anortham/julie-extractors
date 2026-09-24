# Elixir support

`elixir` handles `.ex` and `.exs` files with `tree-sitter-elixir`.

## Types

- A definition with a `@spec` for its module, name, and full arity gets an
  inferred type fact: the base name of the return type (`integer()` ->
  `integer`, `Workspace.t()` -> `Workspace.t`). The spec can come before or
  after the definition. Tuples, lists, unions, maps, and type variables
  record nothing. Two specs for one head with different return types record
  nothing.
- A `%Worker{} = w` parameter records the declared fact `Worker`. A local
  bound to an unqualified struct literal (`job = %Job{}`) records the
  inferred fact `Job`. `%__MODULE__{}`, `%mod{}`, and `%Foo.Bar{}` record
  nothing.
- A local bound to a call records the inferred spec type of the definition
  the call reaches in the same file: a local call in the enclosing module
  (`ws = load()`), or a call on a module defined in this file through its
  full name, an `alias`, a nested module's short name, or `__MODULE__`
  (`ws = Store.load()`). A piped call counts the piped argument
  (`doc = text |> parse()`). A call that omits default arguments reaches the
  definition with the defaults. Every definition the call can reach must
  have a spec, and all of them must agree.
- `{:ok, conn} = open()` binds `conn` as a local and gives it `T` when the
  spec returns `{:ok, T}`. The other alternatives of the union must be atoms,
  `nil`, booleans, or tuples with a different tag or size, because only those
  cannot match `{:ok, conn}`. A named type or type variable in the union, two
  different `{:ok, _}` payloads, or a payload that is not a named type
  (`{:ok, [Foo.t()]}`, `{:ok, t}`) records nothing.
- These cases record no inferred fact: a macro call, a call on a variable
  module (`mod.load()`), a call with an arity that no same-file definition
  accepts, and a callee in another file. Other files are out of scope because
  each file is extracted alone. `with {:ok, x} <- load()` and `case` clause
  patterns are not locals and record nothing.
