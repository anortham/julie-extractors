# Lua call-initializer type gaps

Open gaps of Lua call-initializer type inference. The supported cases are in
`docs/languages/lua.md`. Each task keeps the same rules: same file only, a
wrong type is worse than no type, and every candidate must agree.

## Tasks

1. Multi-value returns. Keep every `---@return` type of a callee in the
   return type index. In `local a, b = load()`, bind `b` to the second type
   when `load()` is the last expression. Test: `---@return Foo` and
   `---@return Bar` records `a = Foo` and `b = Bar`, and a call that is not
   last records only the first value.
2. Plain global assignments. Record an inferred fact for `x = load()` when
   the file assigns the global `x` exactly once and `x` is not a local at
   that statement. Test: a second assignment, a `local x`, or a loop
   variable `x` records no fact.
3. Scope check for the class constructor rule. For `Foo.new()`, `Foo:new()`,
   `Foo()`, and `setmetatable({}, Foo)`, require that `Foo` at the call
   resolves to the declaration of the class `Foo`. Test: a parameter or
   local named `Foo` at the call records no fact.

## Gates

- `cargo xtask test language lua`
- `node scripts/language-data-quality-report.mjs --strict`
