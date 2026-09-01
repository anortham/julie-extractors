# Receiver Type Facts Wave 2

Date: 2026-09-08
Status: accepted

## Decision

Wave 2 extends the wave-1 declared-type contract. Languages that wrap the
base type with trailing decorations pass a structurally reduced base name
and the full declared text. The public wrapper stays
`BaseExtractor::record_declared_type_fact`. The new helper is
`BaseExtractor::record_declared_type_fact_with_declared`.

`record_declared_type_fact` calls the new helper with `base_text` equal to
`declared_text`. There is no new fact table. There is no shared
enclosing-scope abstraction in `base/`.

## Contract marker

- `EXTRACTION_CONTRACT_VERSION` gains `.receiver-type-facts-v2`.
- `EXTRACTION_IDENTITY_EPOCH` stays 9 unless a release ships from main
  before this branch lands; then bump to 10.
- `SQLITE_SCHEMA_VERSION` stays unchanged. This repo does no Miller-side
  work and no workspace-global resolution.

## Wave-2 rules

- Parameter symbols: kind `variable`, metadata `"role": "parameter"`,
  `parent_id` = the enclosing callable symbol, span = the parameter node.
  Never a new `SymbolKind`.
- `TypeInfo.resolved_type` = the base type name: strip generic argument
  lists, nullable suffixes, by-ref/pointer/borrow sigils, and language
  type-keyword prefixes (`struct`, `const`, `inout`). Never strip array
  suffixes. Never strip namespace qualifiers. Each language declares one
  `TypeNameRules` constant; the rules per language are fixed in the Task 1
  table.
- Structural base-name rule: a language reduces the type node to the
  single node that names the base type (the rust `base_type_name_node`
  pattern: final path segment, generic/reference/pointer wrappers dropped)
  and passes that node's text to the helper; it passes the full declared
  text as `declared`. Trailing decorations (C/C++ declarator `*` and `&`,
  F# postfix `int list`, PowerShell `[Foo]`) never reach
  `strip_type_decorations`; the language reduces them structurally first.
  Shapes with no single base name (tuples, function types, unions,
  intersections, inline object types) record nothing.
- When the declared text differs from `resolved_type`, keep the full
  declared text in `TypeInfo.metadata["declared"]`.
- `is_inferred=false` only for a type the syntax states.
  `is_inferred=true` for initializer-derived types.
- Same-file constructor rule (dynamic languages and languages without
  `new`): an initializer records an inferred fact only when it is a
  constructor-shaped call or literal (`Foo(...)`, `Foo.new(...)`,
  `new Foo()`, `Foo{...}`, `%Foo{}`, `#foo{}`, `[Foo]::new()`,
  `New Foo(...)`, `Foo$new()`) AND `Foo` names a class-like symbol
  declared in the same file. Otherwise record nothing. Never guess from
  casing. Every task that applies this rule carries a negative test: an
  unknown name, an imported or namespace-qualified name, and a
  non-constructor call each record no fact but still yield the symbol.
- Local kind rule: a declaration whose nearest symbol ancestor is a
  callable (function, method, constructor) is kind `variable`, regardless
  of the language's immutability keyword (`val`, `let`, `const`, `final`).
  Class-level declarations keep the language's existing kind (`property`,
  `field`, `constant`). This applies to kotlin, swift, gdscript, scala,
  zig, dart, and ruby locals.
- Primary-constructor rule: kotlin `class_parameter` and scala
  `class_parameter` (case and non-case classes) are class members: kind
  `property`, parent = the class symbol, with a declared fact. They never
  become parameter symbols. Secondary constructors (`constructor(...)`,
  `def this(...)`) get parameter symbols like any callable.
- Field kind rule: language-level instance state keeps or moves to the
  kind the debt entry names: ruby `@x`/`@@x` → `field`, razor `@code`
  fields → `field`. Nothing else changes kind.
- `receiver_type` rule: record it on the call identifier and on the
  structured pending relationship when the receiver is the language's self
  reference (`this`, `self`, `super`, `base`, `Me`, `MyBase`, `$this`,
  `self::`, `static::`) or, for languages without a self keyword, when the
  receiver is the enclosing method's own receiver/self parameter (go
  receiver name, zig first parameter, lua colon-method `self`, fsharp
  member instance identifier, r `self` inside an R6 method). The value is
  the enclosing type's name (for `super`/`base`/`MyBase`: the declared
  base type name, when the syntax states one). Languages with no receiver
  concept (c, elixir, erlang, bash) record nothing, and this document
  says so.
- `EXTRACTION_CONTRACT_VERSION` gains `.receiver-type-facts-v2`.
  `EXTRACTION_IDENTITY_EPOCH` stays 9 unless a release ships from main
  before this branch lands; then bump to 10.
- No SQLite schema change; `SQLITE_SCHEMA_VERSION` untouched. No
  Miller-side changes; no workspace-global resolution in this repo.

## Per-language table

| Language | Self receiver → receiver_type | `TypeNameRules` (nullable suffixes / reference prefixes / generic open) | Inferred-fact initializer shapes | Not applicable |
|---|---|---|---|---|
| kotlin | `this.m()` → enclosing class/object; `super.m()` → first supertype name | `?` / — / `<` | same-file `Foo(...)` | — |
| swift | `self.m()` → enclosing class/struct/enum/actor/extension target; `super.m()` → first inheritance entry | `?`, `!` / `inout` / `<` | same-file `Foo(...)` | — |
| dart | `this.m()` → enclosing class; `super.m()` → `extends` name | `?` / — / `<` | same-file `Foo(...)`, `new Foo(...)`, `Foo.named(...)` | — |
| gdscript | `self.m()` → enclosing `class_name` or inner class; `super.m()` → `extends` name | — / — / `[` | same-file `Foo.new(...)` | — |
| scala | `this.m()` → enclosing class/object/trait | — / — / `[` | `new Foo(...)`, same-file `Foo(...)` | — |
| c | — | — / `struct`, `union`, `enum`, `const`, `volatile` / — (declarator `*` reduced structurally) | — | receiver_type |
| cpp | `this->m()` → enclosing class/struct; out-of-line `Foo::m()` bodies → `Foo` | — / `const`, `volatile`, `struct`, `class` / `<` (declarator `*`, `&`, `&&` reduced structurally) | `Foo(...)` direct-init, `new Foo(...)`, `Foo{...}` (all syntax-stated; declared locals dominate) | — |
| zig | first parameter of a container method whose declared type is `*Foo`, `Foo`, or `@This()` → container name | — / `*const`, `*`, `?`, `[]const`, `[]` / `(` | `Foo{...}`, `Foo.init(...)` when `Foo` is a same-file container | — |
| vbnet | `Me.M()` → enclosing class/structure/module; `MyBase.M()` → `Inherits` name | `?` / `ByRef`, `ByVal` / `(` | `New Foo(...)` | — |
| powershell | `$this.M()` → enclosing class | — / — / `[` (outer `[`…`]` reduced structurally from `type_literal`) | `[Foo]::new(...)`, `New-Object Foo` | — |
| fsharp | call receiver equal to the enclosing member's `instance` identifier → enclosing type | — / `byref`, `inref`, `outref` / `<` (postfix generics: last type identifier is the base, reduced structurally) | `Foo(...)` when `Foo` is a same-file type | — |
| qml | `this.m()` or `<id>.m()` where `<id>` is the enclosing object's `id` → that object's type name (root object → the file's component symbol name) | — / — / `<` | `new Foo(...)` | — |
| php | `$this->m()`, `self::m()`, `static::m()` → enclosing class; `parent::m()` → `extends` name | `?` / `&`, `\` / — | `new Foo(...)` | — |
| ruby | `self.m` → enclosing class/module | — / — / — | same-file `Foo.new(...)` | declared types (core syntax has none) |
| lua | `self:m()` / `self.m()` inside a colon method → the method's owning table name | — / — / — | same-file `Foo.new(...)`, `setmetatable({...}, Foo)` | declared types |
| r | `self$m()` inside an R6 method → the enclosing R6 class name | — / — / — | same-file `Foo$new(...)`, `new("Foo")`, `Foo(...)` where `Foo` is a same-file `setClass`/`R6Class`/`setRefClass` symbol | declared types |
| elixir | — | — / — / — | `%Foo{...}` | receiver_type; declared types outside struct patterns |
| erlang | — | — / — / — | `#foo{...}` | receiver_type; declared types outside record patterns |
| bash | — | — / — / — | — | receiver_type; declared types; inferred types |
| razor | `this.M()` inside `@code`/`@functions` → the component class name (file-derived); identifiers only (no pending rows by recorded exception) | same as csharp: `?` / `ref`, `out`, `in`, `scoped` / `<` | `new Foo(...)` | pending-row receiver_type |

## Not applicable cells

### C: `receiver_type`

C has no self keyword (`this`, `self`, `super`). The C extractor walks
function calls, member access, and type identifiers
(`crates/julie-extractors/src/c/identifiers.rs`). `tree-sitter-c` models C
functions, not method receivers. Wave 2 records no `receiver_type`.

### Ruby: declared types

Core Ruby syntax states no declared types. RBS and Sorbet annotations are
out of scope. Wave 2 records inferred facts only from same-file
`Foo.new(...)`.

### Lua: declared types

Core Lua states no declared types. Luau-style annotations are out of
scope. Wave 2 records inferred facts only from same-file constructor
shapes, and the closeout claims the lua `types` capability on that
evidence.

### R: declared types

R has no static type system. Roxygen `@param` tags are documentation
only. Wave 2 records inferred facts only from same-file constructor
shapes, and the closeout claims the r `types` capability on that
evidence.

### Elixir: `receiver_type`

Elixir has no self receiver keyword. Calls are module and function calls,
not method receivers on `this` or `self`. Calls carry no `receiver_type`
metadata. Wave 2 records no `receiver_type`.

### Elixir: declared types outside struct patterns

Elixir states types in `@spec` and struct annotations, not at local
declaration sites. Wave 2 records inferred facts from `%Foo{...}`
struct patterns. It records no declared-type facts for other locals.

### Erlang: `receiver_type`

Erlang has no self receiver keyword. Calls are local or MFA calls, not
method receivers. Calls carry no `receiver_type` metadata. Wave 2
records no `receiver_type`.

### Erlang: declared types outside record patterns

Erlang type information lives in `-spec` and `-type` attributes, not at
declaration sites. Wave 2 records inferred facts from `#foo{...}`
record patterns. It records no declared-type facts for other locals.

### Bash: `receiver_type`, declared types, inferred types

Bash has no self receiver, no declared types, and no constructor-shaped
initializers. Functions take positional parameters (`$1`) with no
declaration site. Wave 2 records no `receiver_type` and no declared-type
facts. The pre-existing literal inference (`readonly MAX=3` -> `integer`)
stays; wave 2 adds no inferred facts.

### Razor: pending-row `receiver_type`

Razor `pending_relationships` is a recorded exception. Cross-file
references resolve through the embedded C# pipeline, not Razor's own
pending path. Test `razor_pending_relationships_handled_by_csharp_embed`
locks this (`fixtures/extraction/capabilities.json`, razor
`pending_relationships` exception). Wave 2 records `receiver_type` on
identifiers only.

## Helper contract

`record_declared_type_fact_with_declared(symbol_id, base_text,
declared_text, rules, is_inferred)`:

- `resolved_type` = `strip_type_decorations(base_text, rules)`.
- `metadata["declared"]` = `declared_text.trim()` when that text differs
  from `resolved_type`.
- An existing row for the symbol wins.
- Empty results record nothing.

## Review outcomes (2026-09-01)

The post-implementation review of the wave-2 branch fixed the defects it
found and recorded these rulings where the code and the plan disagreed.

- Dart callable spans: `function_declaration` and `method_declaration`
  symbols now span the whole declaration instead of the signature only.
  Identifiers inside a method body are contained by the method, not the
  class. The golden containing keys moved on purpose; the old spans made
  the Miller scope walk skip every Dart method body.
- Elixir `%Foo{}`: an unqualified struct literal records an inferred fact
  without the same-file check. The compiler rejects a struct literal whose
  module does not define a struct, so the name is never a guess. Only an
  `alias` child counts; `%__MODULE__{}` and `%var{}` record nothing.
- Lua `local x = setmetatable({}, Foo)`: the pre-existing class heuristic
  still classifies this local as `class`, because the same shape declares
  Lua classes. The inferred fact `Foo` records on that symbol. Task 21
  asked for kind `variable`; the class classification wins until a
  separate decision changes the Lua class heuristic.
- VB.NET arrays: `Worker()` and `Integer()` stay as recorded array types.
  The contract keeps array suffixes, and the VB generic opener `(` never
  runs on an `array_type`.
- Ruby `self.m`: the receiver_type rides the `member_access` identifier
  row, which is the row Ruby emits for the call, plus the pending row.
- Legacy inference: every legacy `infer_types` path in the wave-2
  languages now records only base type names or nothing. Wave-2 facts win
  over legacy rows for the same symbol. Function types, tuples, unions,
  slices, and keyword text (`final`, `var`, `async`) record nothing.
- R6 members: `R6Class` and `setRefClass` member symbols span their own
  `name = value` argument, not the whole class call.
- Go `a, err := f()`: every `:=` target is a `variable` symbol; targets
  with no matching value record no fact.
