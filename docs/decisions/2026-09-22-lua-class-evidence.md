# Lua class evidence

Date: 2026-09-22. Plan: [language gap closure](../plans/2026-09-22-language-gap-closure.md),
wave 2 (`lua.setmetatable-instance-as-class`,
`lua.false-class-from-setmetatable-instances`, `lua.class-patterns-missed`,
`lua.no-extends-relationships`). This replaces the class heuristic that
[receiver type facts wave 2](2026-09-08-receiver-type-facts-wave-2.md) kept.

## Decision

- A form that only declares classes makes a `class` symbol on its own:
  `Base:extend()`, `Base:subclass("Name")`, `class("Name", Base)`
  (middleclass), module-scope `setmetatable({}, { __index = Base })`, and a
  `---@class Name : Base` annotation.
- A form that also builds instances needs evidence from the class body:
  `setmetatable({}, Base)` and `Base:new(...)` / `Base.new(...)` values become
  `class` only when the name owns an `__index` field, a `new` method, or colon
  methods. Without that evidence the symbol stays a `variable` and records
  the inferred type fact `Base`.
- Plain tables keep the old rule: an `__index` field, or both `new` and colon
  methods. `function Account:new` counts like `function Account.new`.
- Inside a function, `local self = setmetatable({}, Class)` is an instance.
  Field writes through it (`self.x = v`) belong to `Class`, and one field row
  stands for every write of the same name on the same owner.
- A class with a named base emits an `extends` relationship when the base binds
  in the file, and a pending `extends` row otherwise (with the import context
  when the base is a `require` binding).
- A class symbol records no inferred constructor type fact: a class is not an
  instance of its base.

## Why

`local self = setmetatable({}, Foo)` is the instance line of almost every Lua
constructor, and `setmetatable({}, { __mode = "k" })` is a weak table. The old
rule made both a `class` named `self` or `cache`, and parented the instance's
fields to it. The declaring forms above are unambiguous, and the instance forms
are classes only when the file also defines behavior on the name.
