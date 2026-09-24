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
  initializer. A call to a same-file function with a `---@return`
  annotation records that type: `load()`, `M.load()`, `M:load()`,
  `a.b.load()`, or `self:load()` / `self.load()` inside a colon method of
  `M`. The callee is a `function` declaration, a single-name
  `name = function ... end` assignment, or a table-constructor field
  `local M = { name = function ... end }`. `---@return self` names the
  owner table.
- If no annotated callee matches, `Foo.new(..)`, `Foo:new(..)`, `Foo(..)`, and
  `setmetatable({..}, Foo)` record `Foo` for a same-file class `Foo`. An
  explicit `---@return` on the callee wins over this rule, so
  `---@return Circle` on `Shape.new` records `Circle`.
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
- Each owner path (`M`, and `a` and `a.b` for `a.b.load()`) must stay the
  same table. The file may leave the path unassigned, or assign it once to a
  new table (`{..}` or `setmetatable({..}, mt)`) before every definition,
  in a function that holds them. `local A = { b = {} }` assigns both `A` and
  `A.b`. Any other assignment records no fact, for example
  `local M = require("other")`, `M = M or {}`, or a second assignment.
- A member that the file also assigns a value that is not a function
  records no fact: `M.get = memoize(M.get)`, `M["get"] = 5`, `_G.load = 5`
  for a global `load`, and `self.update = throttle(..)` inside a colon
  method of `View` for `View:update()`. A second function value with no
  annotation, such as `self.open = function() end`, also records no fact.
- An explicit `self` parameter or local, a call on any other receiver, and
  a chain that ends in another call or field record no fact.
- Spaces around `|` do not end a type: `---@return Foo | Bar` records nothing
  and `---@return Foo | nil` records `Foo`. The literal types `true` and
  `false` record no inferred fact.
- Callees in other files record no fact, because each file is extracted
  alone.

### Not inferred yet

The closure tasks are in
`docs/plans/2026-09-24-lua-call-initializer-gaps.md`.

- Second and later return values: in `local a, b = load()`, `b` gets no
  fact. Reason: the index keeps only the first `---@return` type. Closure:
  keep every return type and bind the value at the same position (task 1).
- Plain global assignments: `x = load()` gets no fact; only `local`
  declarations do. Reason: a global can take other values in other
  statements and files. Closure: record the fact when the file assigns the
  global exactly once (task 2).
- The class constructor rule does not check scope. `Foo.new()` records
  `Foo` even when a local `Foo` at the call is not the class. Reason: this
  rule is older than the scope check. Closure: apply the same binding match
  to the class name (task 3).
