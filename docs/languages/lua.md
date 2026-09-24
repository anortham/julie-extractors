# Lua support

`lua` handles `.lua` files.

## Continuous testing

Run the language target when changing Lua extraction:

```bash
cargo xtask test language lua
```

The command runs `tests::lua::` and the golden extraction test with
`JULIE_GOLDEN_LANGUAGE=lua`.

## Type facts

Core Lua has no written types. LuaLS / LuaCATS annotations are the declared
types.

- `---@param`, `---@return`, and `---@type` record declared type facts. A
  union drops `nil`; a union of several other types records nothing. A
  `---@return A, B` records `A` on the function.
- A colon method's implicit `self` records the owner table name.
- A `local` with no `---@type` gets an inferred type fact from its
  initializer. `Foo.new(..)`, `Foo:new(..)`, `Foo(..)`, and
  `setmetatable({..}, Foo)` record `Foo` for a same-file class `Foo`.
  Otherwise, a call to a same-file function with a `---@return` annotation
  records that type: `load()`, `M.load()`, `M:load()`, `a.b.load()`, or
  `self:load()` / `self.load()` inside a colon method of `M`. The callee
  is a `function` declaration or a single-name `name = function ... end`
  assignment. `---@return self` names the owner table.
- Same-named candidates with the same owner must all carry the same
  annotation. No fact is recorded for a generic parameter (`---@generic`,
  `---@class Name<T>`), `any`, `unknown`, a non-name type such as `Foo[]`,
  or a function with `---@overload`.
- Lexical scope must match. The free name, or the root name of the owner
  (`M` in `M.load()`, `a` in `a.b.load()`), must resolve at the call to the
  same declaration as at the function definition: the same `local`, local
  function, parameter, or loop variable, or the global name on both sides. A
  `local function` is visible only inside its block and after its
  declaration, so a call outside that range records no fact.
- A free name that the file also binds as a variable, parameter, or import,
  an owner name bound more than once, an owner path that the file assigns
  a second time (`M = require("other")` after `local M = {}`), a member that
  the file also assigns a value that is not a function
  (`M.get = memoize(M.get)`), an explicit `self` parameter or local, a call
  on any other receiver, and a chain that ends in another call or field
  record no fact.
- Spaces around `|` do not end a type: `---@return Foo | Bar` records nothing
  and `---@return Foo | nil` records `Foo`. The literal types `true` and
  `false` record no inferred fact. Only the first return value is bound: in
  `local a, b = load()`, `b` gets no fact.
- Callees in other files record no fact, because each file is extracted
  alone.
