# Receiver Type Facts Wave 2 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use razorback:subagent-driven-development when subagent delegation is available. Fall back to razorback:executing-plans for single-task, tightly-sequential, or no-delegation runs.

**Goal:** Extend the wave-1 receiver-type fact shapes (parameter symbols, declared-type facts, `receiver_type` call metadata) to the remaining 20 general-purpose languages and close the wave-1 refinements the spec deferred.

**Architecture:** Every language reuses the wave-1 base contract: `BaseExtractor::record_declared_type_fact` for type rows, parameter symbols of kind `variable` with metadata `role: "parameter"`, and `create_identifier_with_receiver_type` / `StructuredPendingRelationship::with_receiver_type` for call sites. Task 1 adds one base helper, `record_declared_type_fact_with_declared`, so a language can pass the structurally reduced base-name text and the full declared text separately; this is the shared answer for trailing decorations (C `*`, C++ `&`, F# postfix generics, PowerShell brackets) that the front-and-generic stripper cannot reduce. Each language task adds a `type_facts.rs` (and a `parameters.rs` where parameters are not symbols today) beside its existing walker, following the wave-1 rust/java/python modules as templates. Dynamic languages record parameter symbols without facts and initializer-derived facts only for constructor-shaped initializers that name a same-file type. Capability claims move from `open_gaps` to `supported` only in the closeout task, backed by regenerated goldens.

**Tech Stack:** Rust, tree-sitter, existing julie-extractors base infrastructure. No new dependencies. No grammar upgrades.

**Architecture Quality:** Approved shape: per-language modules call the wave-1 base helpers plus the one new helper from Task 1 and the existing symbol-creation path; no new fact table, no artifact schema change, no registry restructuring, no shared "enclosing scope" abstraction in `base/` (each language keeps its own ancestor-walk helper). Normalization contract: `resolved_type` always comes from `strip_type_decorations` applied to a base-name text the language picked structurally from the syntax tree; `declared` metadata always holds the full declared text when it differs. Risk after Task 1 lands: low for facts; medium for kind retargets. Retargeting the symbol kind of function-local declarations in kotlin, swift, gdscript, scala, ruby, and zig changes golden row kinds and capability claims; every kind change must be limited to declaration rows whose parent is a callable, and class-level rows must keep their kind. If code reality contradicts this shape, report a plan mismatch instead of redesigning locally.

## Consumer contract (read before any task)

Same as wave 1 (`docs/decisions/2026-09-01-receiver-type-facts.md`). Miller resolution policy v6, Tier 3 Receiver:

- The receiver is found by a scope walk from the call's `caller_scope_symbol_id` up `parent_symbol_id`, filtering by name and language, then file top-level symbols. Locals and parameters must be **symbols whose parent is the enclosing callable**.
- A `type_facts` row binds only when `resolved_type` **verbatim-matches the name of exactly one type-like symbol** in the same language. So `resolved_type` is always the base type name.
- Confidence 0.75 when `is_inferred=false`, 0.65 when true.
- `receiver_type` must land on **both** the call identifier row and the structured pending relationship row. Editing a language's identifier module alone is not enough; every language task lists its pending-relationship module too. Razor is the one exception: it emits no pending rows by recorded capability exception, so its `receiver_type` rides identifiers only.

Wave-1 evidence and the wave-1 code to copy from:

- Typed parameters as symbols: `crates/julie-extractors/src/java/parameters.rs` (`extract_parameter_symbols`, `parameter_name_node`, `declared_parameter_type`).
- Structural base-name reduction plus declared metadata: `crates/julie-extractors/src/rust/type_facts.rs` (`record_type_node`, `base_type_name_node`, `record_initializer_type`); `crates/julie-extractors/src/java/type_facts.rs` (`record_new_expression_type`).
- Same-file constructor rule for dynamic languages: `crates/julie-extractors/src/python/assignments.rs` (`same_file_constructor_class`).
- `receiver_type` emission on both rows: `crates/julie-extractors/src/csharp/identifiers.rs` (`self_receiver_type`, `enclosing_type_name`) and `crates/julie-extractors/src/csharp/relationships.rs` (`handle_call_target` with `.with_receiver_type`); `crates/julie-extractors/src/javascript/identifiers.rs` (`ecmascript_enclosing_class_name`).
- Test shapes: `crates/julie-extractors/src/tests/{rust,java,go,python}/type_facts.rs`.

## Global Constraints

- Parameter symbols: kind `variable`, metadata `"role": "parameter"`, `parent_id` = the enclosing callable symbol, span = the parameter node. Never a new `SymbolKind`.
- `TypeInfo.resolved_type` = the base type name: strip generic argument lists, nullable suffixes, by-ref/pointer/borrow sigils, and language type-keyword prefixes (`struct`, `const`, `inout`). Never strip array suffixes. Never strip namespace qualifiers. Each language declares one `TypeNameRules` constant; the rules per language are fixed in the Task 1 table.
- **Structural base-name rule:** a language reduces the type node to the single node that names the base type (the rust `base_type_name_node` pattern: final path segment, generic/reference/pointer wrappers dropped) and passes that node's text to the helper; it passes the full declared text as `declared`. Trailing decorations (C/C++ declarator `*` and `&`, F# postfix `int list`, PowerShell `[Foo]`) never reach `strip_type_decorations`; the language reduces them structurally first. Shapes with no single base name (tuples, function types, unions, intersections, inline object types) record nothing.
- When the declared text differs from `resolved_type`, keep the full declared text in `TypeInfo.metadata["declared"]`.
- `is_inferred=false` only for a type the syntax states. `is_inferred=true` for initializer-derived types.
- **Same-file constructor rule** (dynamic languages and languages without `new`): an initializer records an inferred fact only when it is a constructor-shaped call or literal (`Foo(...)`, `Foo.new(...)`, `new Foo()`, `Foo{...}`, `%Foo{}`, `#foo{}`, `[Foo]::new()`, `New Foo(...)`, `Foo$new()`) **and** `Foo` names a class-like symbol declared in the same file. Otherwise record nothing. Never guess from casing. **Every task that applies this rule carries a negative test**: an unknown name, an imported or namespace-qualified name, and a non-constructor call each record no fact but still yield the symbol.
- **Local kind rule:** a declaration whose nearest symbol ancestor is a callable (function, method, constructor) is kind `variable`, regardless of the language's immutability keyword (`val`, `let`, `const`, `final`). Class-level declarations keep the language's existing kind (`property`, `field`, `constant`). This applies to kotlin, swift, gdscript, scala, zig, dart, and ruby locals.
- **Primary-constructor rule:** kotlin `class_parameter` and scala `class_parameter` (case and non-case classes) are class members: kind `property`, parent = the class symbol, with a declared fact. They never become parameter symbols. Secondary constructors (`constructor(...)`, `def this(...)`) get parameter symbols like any callable.
- **Field kind rule:** language-level instance state keeps or moves to the kind the debt entry names: ruby `@x`/`@@x` → `field`, razor `@code` fields → `field`. Nothing else changes kind.
- **`receiver_type` rule:** record it on the call identifier and on the structured pending relationship when the receiver is the language's self reference (`this`, `self`, `super`, `base`, `Me`, `MyBase`, `$this`, `self::`, `static::`) or, for languages without a self keyword, when the receiver is the enclosing method's own receiver/self parameter (go receiver name, zig first parameter, lua colon-method `self`, fsharp member instance identifier, r `self` inside an R6 method). The value is the enclosing type's name (for `super`/`base`/`MyBase`: the declared base type name, when the syntax states one). Languages with no receiver concept (c, elixir, erlang, bash) record nothing, and the decision doc says so.
- `EXTRACTION_CONTRACT_VERSION` gains `.receiver-type-facts-v2`. `EXTRACTION_IDENTITY_EPOCH` stays 9 unless a release ships from main before this branch lands; then bump to 10.
- No SQLite schema change; `SQLITE_SCHEMA_VERSION` untouched. No Miller-side changes; no workspace-global resolution in this repo.
- Golden regeneration is per language: `UPDATE_GOLDEN=1 JULIE_GOLDEN_LANGUAGE=<lang> cargo test -p julie-extractors --features test-golden --lib golden_fixtures_match_canonical_extraction`. A language task regenerates only its own language.
- When a golden source lacks a shape the task must prove, extend `fixtures/extraction/<lang>/basic/source.<ext>` (never a `test_roles` or framework fixture) with the smallest addition, and list the added lines in the task's commit message.
- Test discipline: per-language commands in the inner loop; the full default suite runs once at the branch gate.
- Test files get zero comments; no narration comments anywhere.
- All `fixtures/extraction/capabilities.json` edits happen only in Task 28 (closeout). Task 28 closes all 19 debt entries; a language task that cannot deliver its anchored kind is a plan mismatch to escalate during that task, not a debt to roll forward.

## Verification Strategy

**Project source of truth:** `CLAUDE.md` (test discipline), `cargo xtask test --help` for suite names (`language <name>`, `golden`, `capability`), `scripts/language-data-quality-report.mjs`.

**Worker red/green scope:** `cargo xtask test language <lang>` (runs `tests::<lang>::` unit tests plus that language's golden check) plus the focused `cargo test -p julie-extractors tests::<lang>::type_facts` for new tests.

**Worker ceiling:** one language's suite plus directly assigned focused tests. Workers do not run the full workspace suite, the capability suite (except Task 28), or corpus scans.

**Worker gate invariant:** per language task — every acceptance row asserted by a unit test (parameter symbols exist with `role` metadata and, where typed, facts; locals parent to the callable; constructor-shaped initializers record inferred facts and the negative cases record none; field facts carry untruncated base names; `receiver_type` present on identifier and pending rows where the language has a self receiver); goldens regenerate cleanly and diffs contain only intended row changes.

**Lead affected-change scope:** after each batch: `cargo test -p julie-extractors --lib` and `cargo test -p julie-extractors --features test-capability-matrix --lib structural_fact_registry`.

**Branch gate:** `cargo test --workspace`, `cargo xtask test capability`, `cargo xtask test golden`, `node scripts/language-data-quality-report.mjs --strict` (silent_cells=0, quality_bar_debts=0), `cargo fmt --all -- --check`, `git diff --check`.

**Security scope:** none declared.

**Replay/metric evidence:** Task 27's scan is a hard gate for: zero corrupt `resolved_type` values (whitespace, trailing `<`, `[`, `(`, `*`, `&`, or `?`) across all scanned artifacts; parameter symbols with `role="parameter"` present in every one of the 20 wave-2 languages plus python; `receiver_type` rows present on identifiers **and** pending relationships in every language the decision doc lists as having a self receiver (razor: identifiers only). Per-language counts of typed locals and fields are report-only.

**Escalation triggers:** any change to `base/` shared files after Task 1 lands → rerun the affected-change scope before the next batch dispatch. Any fixture diff showing `containing_symbol_id` shifts on non-declaration rows, or a kind change on a class-level row → stop, report as plan mismatch.

**Assigned verification failure:** Workers stop and report when assigned verification fails, unless this plan explicitly says to update that gate.

**Verification ledger:** append entries (invariant, command, scope label, commit SHA, result, timestamp) to the `## Verification Ledger` section at the bottom of this document.

## Parallel Execution Contract

| Task | Parallel batch | File ownership | Serialization required | Dependency reason |
|---|---|---|---|---|
| Task 1: Wave-2 contract + declared helper | None - serial | Create: `docs/decisions/2026-09-08-receiver-type-facts-wave-2.md`. Modify: `crates/julie-extractors/src/base/creation_methods.rs`, `crates/julie-extractors/src/lib.rs`, `crates/julie-extractors/src/tests/api_surface.rs` | Yes | Every language task consumes the per-language rules table, the new helper, and the contract marker. |
| Task 2: Python locals + receiver_type | Batch A | Modify: `crates/julie-extractors/src/python/**`, `crates/julie-extractors/src/tests/python/**`, `fixtures/extraction/python/**` | No | None - safe parallel batch. |
| Task 3: Rust receiver_type | Batch A | Modify: `crates/julie-extractors/src/rust/**`, `crates/julie-extractors/src/tests/rust/**`, `fixtures/extraction/rust/**` | No | None - safe parallel batch. |
| Task 4: Go constructor calls + receiver_type | Batch A | Modify: `crates/julie-extractors/src/go/**`, `crates/julie-extractors/src/tests/go/**`, `fixtures/extraction/go/**` | No | None - safe parallel batch. |
| Task 5: Java bindings + receiver_type | Batch A | Modify: `crates/julie-extractors/src/java/**`, `crates/julie-extractors/src/tests/java/**`, `fixtures/extraction/java/**` | No | None - safe parallel batch. |
| Task 6: C# indexer types | Batch A | Modify: `crates/julie-extractors/src/csharp/operators.rs`, `crates/julie-extractors/src/tests/csharp/type_facts.rs`, `fixtures/extraction/csharp/**`, `fixtures/extraction/razor/code-behind/expected.cs.json` | No | None - safe parallel batch. |
| Task 7: Kotlin | Batch B | Modify: `crates/julie-extractors/src/kotlin/**`, `crates/julie-extractors/src/tests/kotlin/**`, `fixtures/extraction/kotlin/**` | No | None - safe parallel batch. |
| Task 8: Swift | Batch B | Modify: `crates/julie-extractors/src/swift/**`, `crates/julie-extractors/src/tests/swift/**`, `fixtures/extraction/swift/**` | No | None - safe parallel batch. |
| Task 9: Dart | Batch B | Modify: `crates/julie-extractors/src/dart/**`, `crates/julie-extractors/src/tests/dart/**`, `fixtures/extraction/dart/**` | No | None - safe parallel batch. |
| Task 10: GDScript | Batch B | Modify: `crates/julie-extractors/src/gdscript/**`, `crates/julie-extractors/src/tests/gdscript/**`, `fixtures/extraction/gdscript/**` | No | None - safe parallel batch. |
| Task 11: Scala | Batch B | Modify: `crates/julie-extractors/src/scala/**`, `crates/julie-extractors/src/tests/scala/**`, `fixtures/extraction/scala/**` | No | None - safe parallel batch. |
| Task 12: C | Batch B | Modify: `crates/julie-extractors/src/c/**`, `crates/julie-extractors/src/tests/c/**`, `fixtures/extraction/c/**` | No | None - safe parallel batch. |
| Task 13: C++ | Batch B | Modify: `crates/julie-extractors/src/cpp/**`, `crates/julie-extractors/src/tests/cpp/**`, `fixtures/extraction/cpp/**` | No | None - safe parallel batch. |
| Task 14: Zig | Batch B | Modify: `crates/julie-extractors/src/zig/**`, `crates/julie-extractors/src/tests/zig/**`, `fixtures/extraction/zig/**` | No | None - safe parallel batch. |
| Task 15: VB.NET | Batch B | Modify: `crates/julie-extractors/src/vbnet/**`, `crates/julie-extractors/src/tests/vbnet/**`, `fixtures/extraction/vbnet/**` | No | None - safe parallel batch. |
| Task 16: PowerShell | Batch B | Modify: `crates/julie-extractors/src/powershell/**`, `crates/julie-extractors/src/tests/powershell/**`, `fixtures/extraction/powershell/**` | No | None - safe parallel batch. |
| Task 17: F# | Batch B | Modify: `crates/julie-extractors/src/fsharp/**`, `crates/julie-extractors/src/tests/fsharp/**`, `fixtures/extraction/fsharp/**` | No | None - safe parallel batch. |
| Task 18: QML | Batch B | Modify: `crates/julie-extractors/src/qml/**`, `crates/julie-extractors/src/javascript/parameters.rs` (visibility only), `crates/julie-extractors/src/tests/qml/**`, `fixtures/extraction/qml/**` | No | None - safe parallel batch (the javascript edit is a one-line `pub(crate)` change no other task touches). |
| Task 19: PHP | Batch C | Modify: `crates/julie-extractors/src/php/**`, `crates/julie-extractors/src/tests/php/**`, `fixtures/extraction/php/**` | No | None - safe parallel batch. |
| Task 20: Ruby | Batch C | Modify: `crates/julie-extractors/src/ruby/**`, `crates/julie-extractors/src/tests/ruby/**`, `fixtures/extraction/ruby/**` | No | None - safe parallel batch. |
| Task 21: Lua | Batch C | Modify: `crates/julie-extractors/src/lua/**`, `crates/julie-extractors/src/tests/lua/**`, `fixtures/extraction/lua/**` | No | None - safe parallel batch. |
| Task 22: R | Batch C | Modify: `crates/julie-extractors/src/r/**`, `crates/julie-extractors/src/tests/r/**`, `fixtures/extraction/r/**` | No | None - safe parallel batch. |
| Task 23: Elixir | Batch C | Modify: `crates/julie-extractors/src/elixir/**`, `crates/julie-extractors/src/tests/elixir/**`, `fixtures/extraction/elixir/**` | No | None - safe parallel batch. |
| Task 24: Erlang | Batch C | Modify: `crates/julie-extractors/src/erlang/**`, `crates/julie-extractors/src/tests/erlang/**`, `fixtures/extraction/erlang/**` | No | None - safe parallel batch. |
| Task 25: Bash | Batch C | Modify: `crates/julie-extractors/src/bash/**`, `crates/julie-extractors/src/tests/bash/**`, `fixtures/extraction/bash/**` | No | None - safe parallel batch. |
| Task 26: Razor | Batch C | Modify: `crates/julie-extractors/src/razor/**`, `crates/julie-extractors/src/tests/razor/**`, `fixtures/extraction/razor/**` except `code-behind/expected.cs.json` | No | None - safe parallel batch (Task 6 owns `expected.cs.json` and lands in Batch A first). |
| Task 27: Evidence scan | None - serial | Create: `docs/findings/2026-09-08-receiver-type-facts-wave-2-evidence.md` | Yes | Needs Tasks 2–26 landed to measure. |
| Task 28: Closeout | None - serial | Modify: `fixtures/extraction/capabilities.json`, `docs/plans/2026-09-01-receiver-typed-call-resolution.md` (status), this plan (ledger) | Yes | Single owner of capabilities.json; needs all prior tasks. |

Commit modes: serial tasks use `serial-worker-commit`. Batches A, B, and C use `parallel-lead-commit`; as in wave 1, the lead may run batch workers in per-agent isolated worktrees with a serial worker commit each, then merge, because golden regeneration rewrites fixture files. Batches run in order A → B → C; each batch dispatches only after the previous batch's affected-change scope passes.

---

### Task 1: Wave-2 contract + declared helper

**Files:**
- Create: `docs/decisions/2026-09-08-receiver-type-facts-wave-2.md`
- Modify: `crates/julie-extractors/src/base/creation_methods.rs` (beside `record_declared_type_fact`, line 364), `crates/julie-extractors/src/lib.rs:131` (`EXTRACTION_CONTRACT_VERSION`), `crates/julie-extractors/src/tests/api_surface.rs` (marker list)
- Test: `crates/julie-extractors/src/tests/api_surface.rs`; helper unit tests in the existing `#[cfg(test)] mod tests` at `crates/julie-extractors/src/base/creation_methods.rs:475`, beside the wave-1 `record_declared_type_fact` tests

**Interfaces:**
- Consumes: wave-1 decision doc; helper signatures `record_declared_type_fact(&mut self, symbol_id: &str, declared_text: &str, rules: &TypeNameRules, is_inferred: bool)` (`base/creation_methods.rs:364`), `strip_type_decorations` (`base/types.rs:514`), `create_identifier_with_receiver_type(&mut self, node, name, kind, containing_symbol_id, receiver_type: Option<String>)` (`base/creation_methods.rs:103`), `StructuredPendingRelationship::with_receiver_type` (`base/relationship_resolution.rs:113`).
- Produces: `impl BaseExtractor { pub fn record_declared_type_fact_with_declared(&mut self, symbol_id: &str, base_text: &str, declared_text: &str, rules: &TypeNameRules, is_inferred: bool) }` — `resolved_type` = `strip_type_decorations(base_text, rules)`; `metadata["declared"]` = `declared_text.trim()` when it differs from `resolved_type`; an existing row for the symbol wins; empty results record nothing. `record_declared_type_fact` becomes a call to it with `base_text == declared_text`. Also the decision doc every later task cites, holding (a) the Global Constraints above as the wave-2 rules, (b) the per-language table below, (c) the contract marker.

**Per-language table to put in the decision doc (verbatim; tasks implement exactly these):**

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

**Contract inputs:** `EXTRACTION_CONTRACT_VERSION` currently ends `.receiver-type-facts-v1`; epoch 9 assertion in `tests/api_surface.rs`.

**File ownership:** Copy of contract row — Create: `docs/decisions/2026-09-08-receiver-type-facts-wave-2.md`. Modify: `crates/julie-extractors/src/base/creation_methods.rs`, `crates/julie-extractors/src/lib.rs`, `crates/julie-extractors/src/tests/api_surface.rs`.

**Serialization required:** Yes

**Dependency reason:** Every language task consumes the per-language rules table, the new helper, and the contract marker.

**What to build:** The new base helper, the decision doc (status accepted), the contract marker, and the api_surface assertion.

**Approach:** RED: table-driven tests for the new helper: (`foo`, `struct foo *`) → resolved `foo`, declared `struct foo *`; (`list`, `int list`) → `list` / `int list`; (`Foo`, `Foo`) → `Foo`, no metadata; second call for the same symbol is ignored; empty base records nothing. Extend the api_surface marker test to expect `.receiver-type-facts-v2`. GREEN: implement, re-route `record_declared_type_fact`, append the suffix. Write the decision doc with the Global Constraints and the table above, plus one paragraph per "Not applicable" cell stating the source-verified reason (grammar has no self receiver; core syntax states no types; razor pending exception).

**Acceptance criteria:**
- [x] The helper table passes; wave-1 `record_declared_type_fact` tests still pass unchanged.
- [x] api_surface test asserts `.receiver-type-facts-v2` and epoch 9 (or 10 per the release clause).
- [x] Decision doc holds the wave-2 rules and the full per-language table; every "not applicable" cell has a reason.
- [x] Worker-scope verification passes and the change is committed (serial-worker-commit).

### Task 2: Python locals + receiver_type

**Files:**
- Modify: `crates/julie-extractors/src/python/helpers.rs` (`find_parent_class_id`, lines 8-27), `crates/julie-extractors/src/python/assignments.rs` (`extract_assignment` line 81, `extract_multiple_assignment_targets` line 147), `crates/julie-extractors/src/python/identifiers.rs` (`call` → `attribute` arm, lines 77-92), `crates/julie-extractors/src/python/relationships.rs` (`extract_call_relationships` lines 134-204, `extract_target_from_call` lines 207-239)
- Test: `crates/julie-extractors/src/tests/python/type_facts.rs`, regenerate `fixtures/extraction/python/**`

**Interfaces:**
- Consumes: wave-1 python parameter mechanism (`signatures.rs::extract_parameter_symbols` sets `parent_id` from the caller-supplied function id, `python/mod.rs:80-95`); symbol ids are `generate_id_for_node(name, node)` = `stable_location_id(file, name, span-of-node)` (`base/extractor.rs:366`), and `functions::extract_function` creates the function symbol from the `function_definition` node with its `name` field text, so the id of an enclosing function is `generate_id_for_node(&name_text, &function_definition_node)`.
- Produces: a new helper `find_enclosing_callable_id(extractor, node) -> Option<String>` in `python/helpers.rs` that walks `node.parent()` and, at the first `function_definition` or `async_function_definition`, returns `generate_id_for_node(&name_text, &that_node)`; at a `class_definition` reached first, returns the class id (existing behavior); else `None`. Locals from `extract_assignment` and `extract_multiple_assignment_targets` use it. `self.m(...)` and `cls.m(...)` call identifiers and pending relationships carry `receiver_type` = enclosing `class_definition` name.

**Contract inputs:** python open_gaps entry (`variable`): locals parent to class or file scope; policy v6 scope walk needs the callable as parent. Wave-1 finding in `docs/findings/2026-09-01-receiver-type-facts-evidence.md`.

**File ownership:** Copy of contract row — `crates/julie-extractors/src/python/**`, `crates/julie-extractors/src/tests/python/**`, `fixtures/extraction/python/**`.

**Serialization required:** No

**Dependency reason:** None - safe parallel batch.

**What to build:** Local re-parenting and `receiver_type` emission for python.

**Approach:** RED: a test asserting a local assigned inside a method has `parent_id` = the method symbol (not the class), and a local inside a module-level function has `parent_id` = the function; a second test asserting `self.helper()` inside a class method yields an identifier with `receiver_type = "Widget"` and the pending relationship carries the same, while `other.helper()` carries none on both rows. Keep class attributes (assignments directly in the class body) parented to the class. Regenerate goldens; verify `containing_symbol_id` shifts appear only on local-declaration rows.

**Acceptance criteria:**
- [x] Locals in functions and methods parent to the callable; class-body assignments still parent to the class.
- [x] `self.`/`cls.` calls carry `receiver_type` on identifier and pending rows; other receivers carry nothing.
- [x] `cargo xtask test language python` passes.
- [x] Verified diff handed to the lead (parallel-lead-commit).

### Task 3: Rust receiver_type

**Files:**
- Modify: `crates/julie-extractors/src/rust/identifiers.rs` (`call_expression` with `field_expression` callee, lines 70-146), `crates/julie-extractors/src/rust/relationships.rs` (`field_expression` branch, lines 302-329), `crates/julie-extractors/src/rust/type_facts.rs` (`record_type_node` lines 63-82 → call `record_declared_type_fact_with_declared` instead of patching metadata after insert)
- Test: `crates/julie-extractors/src/tests/rust/type_facts.rs`, regenerate `fixtures/extraction/rust/**`

**Interfaces:**
- Consumes: `RustExtractor::get_impl_blocks() -> &[ImplBlockInfo]` (`rust/mod.rs:248`, populated by `functions.rs::extract_impl`), the byte-range lookup pattern in `rust/locals.rs::extract_self_parameter` (lines 54-79); Task 1 helper.
- Produces: `self.m(...)` and `Self::m(...)` call identifiers and pending relationships carry `receiver_type` = the enclosing impl block's `type_name`; trait default methods (no impl target) carry nothing. `record_type_node` uses the Task 1 helper; rust facts and goldens are unchanged by that refactor.

**Contract inputs:** Global `receiver_type` rule; spec deferred item "receiver_type for rust".

**File ownership:** Copy of contract row — `crates/julie-extractors/src/rust/**`, `crates/julie-extractors/src/tests/rust/**`, `fixtures/extraction/rust/**`.

**Serialization required:** No

**Dependency reason:** None - safe parallel batch.

**What to build:** Impl-aware `receiver_type` for `self` and `Self` receivers, plus the helper migration.

**Approach:** RED: tests for `self.helper()` inside `impl Store` (`receiver_type = "Store"` on identifier and pending rows), inside `impl Trait for Store` (`"Store"`), inside a trait default method (none), and `other.helper()` (none). Find the impl block by `node.start_byte()` within `[start_byte, end_byte)`. Migrate `record_type_node` and prove the existing type_facts tests still pass byte-for-byte. Regenerate goldens (expect no drift from the migration).

**Acceptance criteria:**
- [x] The four cases pass for both identifier and pending rows.
- [x] `record_type_node` uses the Task 1 helper; wave-1 rust tests unchanged and green.
- [x] `cargo xtask test language rust` passes.
- [x] Verified diff handed to the lead (parallel-lead-commit).

### Task 4: Go constructor calls + receiver_type

**Files:**
- Modify: `crates/julie-extractors/src/go/type_facts.rs` (`composite_literal_type_node` lines 60-72, `binds_base_type` lines 38-53), `crates/julie-extractors/src/go/specs.rs` (`extract_short_var_symbols` lines 231-291), `crates/julie-extractors/src/go/identifiers.rs` (`selector_expression` call arm, lines 61-76), `crates/julie-extractors/src/go/relationships.rs` (`selector_expression` branch lines 169-197 and the `create_pending_relationship` calls at lines 223-269)
- Test: `crates/julie-extractors/src/tests/go/type_facts.rs` (flip `constructor_call_local_records_nothing`), regenerate `fixtures/extraction/go/**`

**Interfaces:**
- Consumes: `go/helpers.rs::extract_receiver_type_from_param` (lines 119-135).
- Produces: `x := NewFoo(...)` records an inferred fact when `NewFoo` is a same-file `function_declaration` whose `result` is a single `type_identifier` or `*type_identifier` (the callee's declared result type is the fact, base name only); `x := pkg.NewFoo(...)`, `x := unknown(...)`, and `x := helper()` returning a non-named type record nothing. Every `:=` target still gets a `variable` symbol even when no fact applies (verify the existing symbol path in `specs.rs` and add it if `extract_short_var_symbols` currently drops fact-less targets). `c.M()` where `c` is the enclosing `method_declaration`'s receiver name carries `receiver_type` = receiver base type on identifier and pending rows.

**Contract inputs:** Spec deferred item "go `:=` extension"; `constructor_call_local_records_nothing` currently asserts the gap.

**File ownership:** Copy of contract row — `crates/julie-extractors/src/go/**`, `crates/julie-extractors/src/tests/go/**`, `fixtures/extraction/go/**`.

**Serialization required:** No

**Dependency reason:** None - safe parallel batch.

**What to build:** Same-file constructor-result facts for `:=` and receiver-name `receiver_type`.

**Approach:** RED: rename and invert the constructor-call test (same-file `NewStore() *Store` → `x` gets `Store`, inferred); add the three negative cases (imported callee, unknown callee, same-file callee returning `error`) asserting no fact and a symbol; add `c.Get()` inside `func (c *Client) Run()` → `receiver_type = "Client"` on both rows, and `other.Get()` → none. Regenerate goldens.

**Acceptance criteria:**
- [x] Same-file constructor results record inferred facts; the three negative cases record nothing but keep the symbol.
- [x] Receiver-name calls carry `receiver_type` on identifier and pending rows.
- [x] `cargo xtask test language go` passes.
- [x] Verified diff handed to the lead (parallel-lead-commit).

### Task 5: Java bindings + receiver_type

**Files:**
- Modify: `crates/julie-extractors/src/java/mod.rs` (`walk_tree` lines 90-166 dispatch), `crates/julie-extractors/src/java/locals.rs`, `crates/julie-extractors/src/java/parameters.rs`, `crates/julie-extractors/src/java/classes.rs` (`extract_record_components`, line 306), `crates/julie-extractors/src/java/identifiers.rs` (`method_invocation` arm lines 57-103), `crates/julie-extractors/src/java/relationships.rs` (`extract_method_call_relationship` lines 215-314, pending emission at lines 288 and 303)
- Test: `crates/julie-extractors/src/tests/java/type_facts.rs` (flip `lambda_parameters_produce_no_symbols` and `catch_parameter_produces_no_symbol`), regenerate `fixtures/extraction/java/**`

**Interfaces:**
- Consumes: `java/type_facts.rs::record_declared_type`, `record_new_expression_type`.
- Produces: `variable` symbols with facts for `resource` (try-with-resources, declared type), `enhanced_for_statement` (loop variable, declared type; `var` records nothing), `catch_formal_parameter` (single catch type; multi-catch `|` records a symbol without fact), explicit typed lambda parameters (`lambda_expression` with `formal_parameters`), and `instanceof_expression` pattern bindings (`Foo f`). Inferred lambda parameters (`inferred_parameters` or a bare `identifier`) get symbols with `role = "parameter"` and no fact, parented to the enclosing method (lambdas are not symbols). Record components already exist via `classes::extract_record_components`; assert they carry declared facts and add the `record_declared_type` call in `classes.rs` if they do not. `this.m()` carries `receiver_type` = enclosing class/interface/enum/record name on identifier and pending rows; `super.m()` = the `superclass` name when declared.

**Contract inputs:** Spec deferred item "java binding forms"; the two tests that currently assert the gaps.

**File ownership:** Copy of contract row — `crates/julie-extractors/src/java/**`, `crates/julie-extractors/src/tests/java/**`, `fixtures/extraction/java/**`.

**Serialization required:** No

**Dependency reason:** None - safe parallel batch.

**What to build:** The five binding forms plus `this`/`super` `receiver_type`.

**Approach:** RED per binding form with exact `resolved_type`, `is_inferred=false`, and parent assertions; `receiver_type` tests mirror Task 3's four cases on both rows. Regenerate goldens.

**Acceptance criteria:**
- [x] All five binding forms and record components carry facts; inferred lambda params are symbols without facts.
- [x] `this.`/`super.` calls carry `receiver_type` on identifier and pending rows.
- [x] `cargo xtask test language java` passes.
- [x] Verified diff handed to the lead (parallel-lead-commit).

### Task 6: C# indexer types

**Files:**
- Modify: `crates/julie-extractors/src/csharp/operators.rs` (`extract_indexer` lines 184-229)
- Test: `crates/julie-extractors/src/tests/csharp/type_facts.rs`, regenerate `fixtures/extraction/csharp/**`, `fixtures/extraction/razor/code-behind/expected.cs.json`

**Interfaces:**
- Consumes: `csharp/type_inference.rs::record_declared_type` (line 15); the `return_type_node` `extract_indexer` already computes (lines 192-198).
- Produces: every `indexer_declaration` symbol carries a declared fact for its return type (`IReadOnlyList<Foo> this[int i]` → `IReadOnlyList`, declared metadata kept).

**Contract inputs:** Spec deferred item "csharp indexer return types".

**File ownership:** Copy of contract row — `crates/julie-extractors/src/csharp/operators.rs`, `crates/julie-extractors/src/tests/csharp/type_facts.rs`, `fixtures/extraction/csharp/**`, `fixtures/extraction/razor/code-behind/expected.cs.json`.

**Serialization required:** No

**Dependency reason:** None - safe parallel batch.

**What to build:** One `record_declared_type` call on the already-computed return type node.

**Approach:** RED: a test with a generic indexer and a nullable indexer asserting base name, declared metadata, `is_inferred=false`. Regenerate csharp goldens; regenerate razor only if `expected.cs.json` drifts.

**Acceptance criteria:**
- [x] Indexer return types record declared facts.
- [x] `cargo xtask test language csharp` passes; razor golden check still passes.
- [x] Verified diff handed to the lead (parallel-lead-commit).

### Task 7: Kotlin

**Files:**
- Create: `crates/julie-extractors/src/kotlin/type_facts.rs`, `crates/julie-extractors/src/kotlin/parameters.rs`
- Modify: `crates/julie-extractors/src/kotlin/mod.rs` (`visit_node` lines 90-236), `crates/julie-extractors/src/kotlin/declarations.rs` (`extract_function` lines 14-130, `extract_secondary_constructor` lines 141-209), `crates/julie-extractors/src/kotlin/properties.rs` (`extract_property` lines 13-129, `extract_constructor_parameters` lines 132-268), `crates/julie-extractors/src/kotlin/identifiers.rs` (`call_expression` arm lines 47-105), `crates/julie-extractors/src/kotlin/relationships.rs` (pending call emission)
- Test: create `crates/julie-extractors/src/tests/kotlin/type_facts.rs` (register in `tests/kotlin/mod.rs`), regenerate `fixtures/extraction/kotlin/**`

**Interfaces:**
- Consumes: Task 1 rules row for kotlin; grammar nodes `parameter` (children `identifier`, `type`), `property_declaration` → `variable_declaration` (children `identifier`, `type`), `class_parameter` (`identifier`, `type`), `primary_constructor`, `secondary_constructor` (`function_value_parameters`), `this_expression`, `super_expression`.
- Produces: (1) `parameter` nodes under a function or secondary constructor → parameter symbols with declared facts, parent = that callable. (2) `class_parameter` rows keep their existing `Property` symbol and gain a declared fact (Primary-constructor rule). (3) `property_declaration` whose nearest symbol ancestor is a callable → kind `variable` (today `property`), declared fact from `variable_declaration.type`, inferred fact from a same-file `Foo(...)` initializer. Class-level `property_declaration` keeps `property`/`constant` and gains a declared fact. (4) `this.m()`/`super.m()` per the rules row, on identifier and pending rows.

**Contract inputs:** kotlin open_gaps entry (`variable`). Survey finding: kotlin already emits locals as `property` parented to the function; wave 2 retargets the kind.

**File ownership:** Copy of contract row — `crates/julie-extractors/src/kotlin/**`, `crates/julie-extractors/src/tests/kotlin/**`, `fixtures/extraction/kotlin/**`.

**Serialization required:** No

**Dependency reason:** None - safe parallel batch.

**What to build:** The three fact shapes plus the local kind retarget.

**Approach:** TDD per shape: typed parameter (`fun f(x: Foo)` → `Foo`), generic parameter (`List<Foo>` → `List`, declared kept), nullable local (`val x: Foo? = null` → `Foo`), same-file constructor local (`val x = Foo()` → `Foo` inferred), negative locals (`val y = Unknown()`, `val z = listOf(1)`, `val w = com.acme.Foo()` → symbol, no fact), class property fact, primary-constructor property fact, `this.m()` on both rows. Add a `this.` call and a nullable local to `fixtures/extraction/kotlin/basic/source.kt` if absent. Regenerate goldens; review that only local rows changed kind.

**Acceptance criteria:**
- [x] The eight cases pass; class-level rows keep their kind.
- [x] `cargo xtask test language kotlin` passes.
- [x] Verified diff handed to the lead (parallel-lead-commit).

### Task 8: Swift

**Files:**
- Create: `crates/julie-extractors/src/swift/type_facts.rs`, `crates/julie-extractors/src/swift/parameters.rs`
- Modify: `crates/julie-extractors/src/swift/mod.rs` (`visit_node` lines 75-171; delete the unreachable `"variable_declaration"` arm at line 133 and `properties.rs::extract_variable` it calls), `crates/julie-extractors/src/swift/callables.rs` (`extract_function` lines 12-103, `extract_initializer` lines 106-164), `crates/julie-extractors/src/swift/properties.rs` (`extract_property` lines 98-188), `crates/julie-extractors/src/swift/identifiers.rs` (`call_expression` arm from line 53), `crates/julie-extractors/src/swift/relationships.rs` (pending call emission)
- Test: create `crates/julie-extractors/src/tests/swift/type_facts.rs` (register in `tests/swift/mod.rs`), regenerate `fixtures/extraction/swift/**`

**Interfaces:**
- Consumes: Task 1 rules row for swift; grammar nodes `parameter` (fields `external_name`, `name`, `type`), `property_declaration` (same node for locals and members; type via `type_annotation`), `init_declaration`, `self_expression`, `super_expression`.
- Produces: parameter symbols with declared facts under `function_declaration`/`init_declaration` (parent = the function/constructor symbol); `property_declaration` inside a callable → kind `variable` with declared or same-file-inferred fact; member `property_declaration` keeps `property` and gains a fact; `self.m()`/`super.m()` per the rules row on both rows.

**Contract inputs:** swift open_gaps entry (`variable`). Survey finding: locals already emit as `property` parented to the function; `tree-sitter-swift 0.7.3` has no `variable_declaration` node.

**File ownership:** Copy of contract row — `crates/julie-extractors/src/swift/**`, `crates/julie-extractors/src/tests/swift/**`, `fixtures/extraction/swift/**`.

**Serialization required:** No

**Dependency reason:** None - safe parallel batch.

**What to build:** The three fact shapes plus the local kind retarget and dead-arm removal.

**Approach:** TDD per shape: `func f(x: Foo, y: inout Bar)` → `Foo`, `Bar`; `let x: Foo? = nil` → `Foo`; `let x = Foo()` same-file → inferred; negatives (`let a = Unknown()`, `let b = UIKit.UIView()`, `let c = makeFoo()` → symbol, no fact); `var items: [Foo]` → no fact (array literal type is a collection shape, record nothing); stored property fact; `self.m()` inside class and inside `extension Foo`, both rows. Extend `basic/source.swift` when a shape is absent. Regenerate goldens.

**Acceptance criteria:**
- [x] The seven cases pass; member rows keep `property`.
- [x] `cargo xtask test language swift` passes.
- [x] Verified diff handed to the lead (parallel-lead-commit).

### Task 9: Dart

**Files:**
- Create: `crates/julie-extractors/src/dart/type_facts.rs`, `crates/julie-extractors/src/dart/parameters.rs`, `crates/julie-extractors/src/dart/locals.rs`
- Modify: `crates/julie-extractors/src/dart/mod.rs` (dispatch; add a `local_variable_declaration` arm), `crates/julie-extractors/src/dart/functions.rs` (`extract_function` lines 78-129, `extract_method` lines 132-223, `extract_constructor` lines 226-314), `crates/julie-extractors/src/dart/members.rs` (`extract_field` lines 11-104), `crates/julie-extractors/src/dart/identifiers.rs` (`call_target_name_node` lines 392-404), `crates/julie-extractors/src/dart/pending_calls.rs`, `crates/julie-extractors/src/dart/relationships.rs`
- Test: create `crates/julie-extractors/src/tests/dart/type_facts.rs` (register in `tests/dart/mod.rs`), regenerate `fixtures/extraction/dart/**`

**Interfaces:**
- Consumes: Task 1 rules row for dart; grammar nodes `formal_parameter` (field `name`; children `type`, `constructor_param` for `this.x`), `local_variable_declaration` → `initialized_variable_definition` (fields `name`, `value`; child `type`), `declaration` (field with `type` + `initialized_identifier_list`), `constructor_signature`, `this`.
- Produces: parameter symbols with declared facts under functions, methods, and constructors (`this.x` initializing formals get a symbol; their fact comes from the matching field's declared type when found in the same class, else none); `local_variable_declaration` → `variable` symbols (new) with declared fact or same-file inferred fact; fields gain declared facts; `this.m()`/`super.m()` per the rules row on identifier and pending rows.

**Contract inputs:** dart open_gaps entry (`variable`). Survey finding: dart emits no local symbols today at all.

**File ownership:** Copy of contract row — `crates/julie-extractors/src/dart/**`, `crates/julie-extractors/src/tests/dart/**`, `fixtures/extraction/dart/**`.

**Serialization required:** No

**Dependency reason:** None - safe parallel batch.

**What to build:** The three fact shapes, including a new local-variable walker.

**Approach:** TDD per shape: `void f(Foo x, List<Foo> xs)`; `final Foo x = Foo()` (declared wins, `is_inferred=false`); `final x = Foo()` and `var x = new Foo()` same-file → inferred; negatives (`final a = Unknown()`, `final b = http.Client()`, `final c = build()` → symbol, no fact); `Foo? x` → `Foo`; field fact; `this.m()` on both rows. Extend `basic/source.dart` when a shape is absent. Regenerate goldens.

**Acceptance criteria:**
- [x] The seven cases pass; locals parent to the enclosing callable.
- [x] `cargo xtask test language dart` passes.
- [x] Verified diff handed to the lead (parallel-lead-commit).

### Task 10: GDScript

**Files:**
- Create: `crates/julie-extractors/src/gdscript/type_facts.rs`, `crates/julie-extractors/src/gdscript/parameters.rs`
- Modify: `crates/julie-extractors/src/gdscript/mod.rs` (dispatch on `"var"`/`"const"`/`"func"`), `crates/julie-extractors/src/gdscript/variables.rs` (`extract_variable_statement` lines 13-88, `extract_constant_statement` lines 91-153), `crates/julie-extractors/src/gdscript/functions.rs` (`extract_constructor_definition` lines 24-47, `extract_function_definition` lines 50-135), `crates/julie-extractors/src/gdscript/identifiers.rs` (`call`/`attribute_call` arms lines 48-107), `crates/julie-extractors/src/gdscript/relationships.rs` (pending call emission)
- Test: create `crates/julie-extractors/src/tests/gdscript/type_facts.rs` (register in `tests/gdscript/mod.rs`), regenerate `fixtures/extraction/gdscript/**`

**Interfaces:**
- Consumes: Task 1 rules row for gdscript; grammar nodes `typed_parameter` (field `type`), `typed_default_parameter` (`type`, `value`), bare `identifier` parameters, `variable_statement` (fields `name`, `type`, `value`; same node at class and function scope), `const_statement`, `constructor_definition`; `self`/`super` are plain identifiers.
- Produces: parameter symbols (typed ones with facts, bare ones without) under functions and `_init`; `variable_statement` inside a callable → kind `variable` (today `field`) with declared fact or same-file `Foo.new()` inferred fact; class-level `variable_statement` keeps `field` and gains a fact; `self.m()`/`super.m()` per the rules row on identifier and pending rows.

**Contract inputs:** gdscript open_gaps entry (`variable`). Survey finding: locals already emit as `field` parented to the function.

**File ownership:** Copy of contract row — `crates/julie-extractors/src/gdscript/**`, `crates/julie-extractors/src/tests/gdscript/**`, `fixtures/extraction/gdscript/**`.

**Serialization required:** No

**Dependency reason:** None - safe parallel batch.

**What to build:** The three fact shapes plus the local kind retarget.

**Approach:** TDD per shape: `func f(x: Foo, y := 2, z)`; `var x: Foo = null`; `var x := Foo.new()` same-file; negatives (`var a = Unknown.new()`, `var b = load("res://x.tscn").instantiate()`, `var c = make()` → symbol, no fact); `var items: Array[Foo]` → `Array` with declared metadata; class field fact; `self.m()` inside a `class_name Foo` script (receiver_type `Foo`) and inside an inner `class Bar:` (`Bar`), both rows. Extend `basic/source.gd` when a shape is absent. Regenerate goldens.

**Acceptance criteria:**
- [x] The seven cases pass; class-level rows keep `field`.
- [x] `cargo xtask test language gdscript` passes.
- [x] Verified diff handed to the lead (parallel-lead-commit).

### Task 11: Scala

**Files:**
- Create: `crates/julie-extractors/src/scala/type_facts.rs`, `crates/julie-extractors/src/scala/parameters.rs`
- Modify: `crates/julie-extractors/src/scala/mod.rs` (dispatch), `crates/julie-extractors/src/scala/declarations.rs` (`extract_function` lines 13-113), `crates/julie-extractors/src/scala/properties.rs` (`val_kind_for_scope` lines 11-24, `extract_val` lines 27-101, `extract_var` lines 104-160, `extract_case_class_constructor_fields` lines 163-278), `crates/julie-extractors/src/scala/identifiers.rs` (`call_expression` arm from line 47), `crates/julie-extractors/src/scala/relationships.rs` (pending call emission)
- Test: create `crates/julie-extractors/src/tests/scala/type_facts.rs` (register in `tests/scala/mod.rs`), regenerate `fixtures/extraction/scala/**`

**Interfaces:**
- Consumes: Task 1 rules row for scala; grammar nodes `parameter` (fields `name`, `type`, `default_value`), `class_parameter` (`name`, `type`), `val_definition`/`var_definition` (fields `pattern`, `type`, `value`), `function_definition` named `this` (secondary constructor), `this`.
- Produces: parameter symbols with declared facts under `function_definition` (including the `this`-named secondary constructor, which becomes kind `constructor` with name = the class name and gets parameter symbols); every `class_parameter` (case and non-case classes) → kind `property`, parent = the class symbol, with a declared fact (Primary-constructor rule; `extract_case_class_constructor_fields` drops its `case` gate); local `val`/`var` (nearest symbol ancestor is a callable) → kind `variable` (today `constant` for `val`) with declared fact or inferred fact from `new Foo(...)` / same-file `Foo(...)`; class-level `val`/`var` keeps `property`/`variable` and gains a fact; `this.m()` per the rules row on identifier and pending rows.

**Contract inputs:** No scala open_gaps entry exists; the user added scala to wave 2 on 2026-09-01. Survey finding: scala already emits local `var` as `variable`, local `val` as `constant`, class `val` as `property`; no parameter symbols; no facts; `def this(...)` extracts as a method named `this`; non-case-class primary-constructor parameters emit nothing today.

**File ownership:** Copy of contract row — `crates/julie-extractors/src/scala/**`, `crates/julie-extractors/src/tests/scala/**`, `fixtures/extraction/scala/**`.

**Serialization required:** No

**Dependency reason:** None - safe parallel batch.

**What to build:** The three fact shapes, the local-`val` kind retarget, primary-constructor properties for all classes, and the secondary-constructor kind fix.

**Approach:** TDD per shape: `def f(x: Foo, xs: List[Foo])`; `val x: Foo = null` local; `val x = new Foo()`; `val x = Foo()` same-file; negatives (`val a = Unknown()`, `val b = scala.collection.mutable.ListBuffer()`, `val c = build()` → symbol, no fact); `case class P(a: Foo)` and `class Q(a: Foo)` both yield property `a` under the class with fact `Foo`; `def this(...)` → constructor with parameter symbols; `this.m()` on both rows. Extend `basic/source.scala` when a shape is absent. Regenerate goldens.

**Acceptance criteria:**
- [x] The eight cases pass; class-level rows keep their kind; no parameter symbol has a class as parent.
- [x] `cargo xtask test language scala` passes.
- [x] Verified diff handed to the lead (parallel-lead-commit).

### Task 12: C

**Files:**
- Create: `crates/julie-extractors/src/c/type_facts.rs`, `crates/julie-extractors/src/c/parameters.rs`
- Modify: `crates/julie-extractors/src/c/mod.rs` (`"declaration"` arm line 190), `crates/julie-extractors/src/c/declarations.rs` (`extract_function_definition` line 136, `extract_variable_declaration` line 272), `crates/julie-extractors/src/c/structs.rs` (`extract_struct_field_symbols` line 100)
- Test: create `crates/julie-extractors/src/tests/c/type_facts.rs` (register in `tests/c/mod.rs`), regenerate `fixtures/extraction/c/**`

**Interfaces:**
- Consumes: Task 1 rules row for c and the Task 1 helper; grammar nodes `parameter_declaration` (fields `type`, `declarator`), `declaration` (fields `type`, `declarator`), `field_declaration` (`type`, `declarator`), wrappers `pointer_declarator`, `init_declarator`, `array_declarator`, `parenthesized_declarator`, `function_declarator`.
- Produces: a structural reducer in `c/type_facts.rs`: `base_type_name_node(type_node)` returns the identifier inside `struct_specifier`/`union_specifier`/`enum_specifier`, or a `primitive_type`/`type_identifier`/`sized_type_specifier` node; `declared_type_text(decl)` = the `type` field text plus one `*` per `pointer_declarator` wrapper and `[]` when an `array_declarator` wraps the name (so `struct foo *x` → base `foo`, declared `struct foo *`; `int buf[8]` → base `int[]`, declared `int[8]`). Both go to `record_declared_type_fact_with_declared`. Parameter symbols with facts under `function_definition` only (prototypes get none); existing local `variable` symbols and `field` symbols gain facts; declarators wrapping a `function_declarator` record nothing.

**Contract inputs:** c open_gaps entry (`variable`). `receiver_type` is not applicable (decision doc).

**File ownership:** Copy of contract row — `crates/julie-extractors/src/c/**`, `crates/julie-extractors/src/tests/c/**`, `fixtures/extraction/c/**`.

**Serialization required:** No

**Dependency reason:** None - safe parallel batch.

**What to build:** Parameter symbols and declared facts for locals, parameters, and fields.

**Approach:** TDD: `void f(struct foo *x, const char *s, int n)` → `foo` (declared `struct foo *`), `char` (declared `const char *`), `int`; local `struct foo *p = make();` → `foo`; `int buf[8]` → `int[]`; field `struct bar *next` → `bar`; function-pointer parameter → symbol without fact. Regenerate goldens.

**Acceptance criteria:**
- [x] The five cases pass; no `resolved_type` ends in `*`; locals and parameters parent to the function.
- [x] `cargo xtask test language c` passes.
- [x] Verified diff handed to the lead (parallel-lead-commit).

### Task 13: C++

**Files:**
- Create: `crates/julie-extractors/src/cpp/type_facts.rs`, `crates/julie-extractors/src/cpp/parameters.rs`
- Modify: `crates/julie-extractors/src/cpp/mod.rs` (`"declaration"` arm line 252), `crates/julie-extractors/src/cpp/declarations.rs` (`extract_declaration` reached from line 137), `crates/julie-extractors/src/cpp/functions.rs` (`extract_function` line 24, `extract_method` line 235), `crates/julie-extractors/src/cpp/fields.rs` (`extract_field` line 17), `crates/julie-extractors/src/cpp/identifiers.rs` (`call_expression` arm lines 43-95), `crates/julie-extractors/src/cpp/relationships.rs` (pending call emission)
- Test: create `crates/julie-extractors/src/tests/cpp/type_facts.rs` (register in `tests/cpp/mod.rs`), regenerate `fixtures/extraction/cpp/**`

**Interfaces:**
- Consumes: Task 1 rules row for cpp and the Task 1 helper; the C declarator shapes plus `reference_declarator`; `template_type` (fields `name`, `arguments`), `qualified_identifier` (`scope`, `name`); `field_expression` (fields `argument`, `operator`, `field`); `this`.
- Produces: the same structural reducer as C extended for `reference_declarator` (`&`, `&&`), `template_type` (base = `name`), and `qualified_identifier` (base = the full dotted text, per the namespace rule); parameter symbols with facts under `function_definition` bodies (free functions, methods, constructors, out-of-line definitions); `declaration` inside a function body → existing `variable` symbol gains a fact (declared type; `auto x = Foo(...)`/`auto x = new Foo(...)` → inferred `Foo`; `Foo x(...)`/`Foo x{...}` → declared `Foo`); `field_declaration` gains a fact; `this->m()` and `(*this).m()` carry `receiver_type` = enclosing class on identifier and pending rows, and calls inside an out-of-line `Foo::m()` body whose receiver is `this` carry `Foo`.

**Contract inputs:** cpp open_gaps entry (`variable`). Survey: cpp emits locals and fields today; no parameter symbols; no facts; `this` unused.

**File ownership:** Copy of contract row — `crates/julie-extractors/src/cpp/**`, `crates/julie-extractors/src/tests/cpp/**`, `fixtures/extraction/cpp/**`.

**Serialization required:** No

**Dependency reason:** None - safe parallel batch.

**What to build:** The three fact shapes plus `this` receiver metadata.

**Approach:** TDD: `void f(const Foo& a, Foo* b, std::vector<Foo> c, Foo&& d)` → `Foo`, `Foo`, `std::vector` (declared kept), `Foo`; `auto x = std::make_unique<Foo>()` → no fact (not a constructor shape); `auto y = Unknown()` → no fact; `Foo x;` → `Foo`; field fact; `this->run()` inside class and inside `void Foo::run()`, both rows. Extend `basic/source.cpp` when a shape is absent. Regenerate goldens.

**Acceptance criteria:**
- [x] The seven cases pass; no `resolved_type` ends in `*` or `&`.
- [x] `cargo xtask test language cpp` passes.
- [x] Verified diff handed to the lead (parallel-lead-commit).

### Task 14: Zig

**Files:**
- Create: `crates/julie-extractors/src/zig/type_facts.rs`, `crates/julie-extractors/src/zig/parameters.rs`
- Modify: `crates/julie-extractors/src/zig/mod.rs` (dispatch lines 74-187), `crates/julie-extractors/src/zig/variables.rs` (`extract_variable` line 22, `extract_standard_variable` line 327; remove the dead `const_declaration` kind check at line 29), `crates/julie-extractors/src/zig/functions.rs` (`extract_function` line 9), `crates/julie-extractors/src/zig/types.rs` (`extract_struct_field`), `crates/julie-extractors/src/zig/identifiers.rs` (call emission), `crates/julie-extractors/src/zig/relationships.rs` (pending call emission)
- Test: create `crates/julie-extractors/src/tests/zig/type_facts.rs` (register in `tests/zig/mod.rs`), regenerate `fixtures/extraction/zig/**`

**Interfaces:**
- Consumes: Task 1 rules row for zig; grammar nodes `parameter` (fields `name`, `type`), `variable_declaration` (field `type`; both `const` and `var`), `container_field` (fields `name`, `type`), `@This()` builtin.
- Produces: parameter symbols with declared facts (parent = function/method); `variable_declaration` inside a function body → kind `variable` for both `const` and `var` (Local kind rule), container-level `const` stays `constant`, container-level `var` stays `variable`; declared facts from `type`, inferred facts from `Foo{...}` / `Foo.init(...)` when `Foo` is a same-file container declared as `const Foo = struct {...}`; `container_field` facts; `receiver_type` on identifier and pending rows when the call receiver is the enclosing method's first parameter and that parameter's declared type reduces to a same-file container name or `@This()`.

**Contract inputs:** zig open_gaps entry (`constant`): container-level `const` must be `constant` with golden evidence. Survey: `tree-sitter-zig 1.1.2` never emits `const_declaration`.

**File ownership:** Copy of contract row — `crates/julie-extractors/src/zig/**`, `crates/julie-extractors/src/tests/zig/**`, `fixtures/extraction/zig/**`.

**Serialization required:** No

**Dependency reason:** None - safe parallel batch.

**What to build:** The three fact shapes, the local kind rule, and self-parameter receiver metadata.

**Approach:** TDD: `fn f(self: *Store, n: u32, list: ArrayList(u8))` → `Store`, `u32`, `ArrayList`; `const s = Store{ .x = 1 };` local → `variable` with inferred `Store`; negatives (`const a = Unknown{};`, `const b = std.ArrayList(u8).init(alloc);`, `const c = make();` → symbol, no fact); `var buf: [8]u8` → no fact (array shape); container `const Store = struct { items: ArrayList(u8) }` → `constant` plus field fact `ArrayList`; `self.run()` inside `pub fn go(self: *Store)` → `Store` on both rows. Extend `basic/source.zig` when a shape is absent. Regenerate goldens.

**Acceptance criteria:**
- [x] The six cases pass; container-level `const` rows are `constant`, local `const` rows are `variable`.
- [x] `cargo xtask test language zig` passes.
- [x] Verified diff handed to the lead (parallel-lead-commit).

### Task 15: VB.NET

**Files:**
- Create: `crates/julie-extractors/src/vbnet/type_facts.rs`, `crates/julie-extractors/src/vbnet/parameters.rs`, `crates/julie-extractors/src/vbnet/locals.rs`
- Modify: `crates/julie-extractors/src/vbnet/mod.rs` (dispatch lines 65-163; add `dim_statement`), `crates/julie-extractors/src/vbnet/members.rs` (`extract_method` line 10, `extract_constructor` line 80, `extract_property` line 108, `extract_fields` line 156), `crates/julie-extractors/src/vbnet/identifiers.rs` (`invocation` arms lines 42-79, `member_access` arm from line 80), `crates/julie-extractors/src/vbnet/relationships.rs` (pending call emission)
- Test: create `crates/julie-extractors/src/tests/vbnet/type_facts.rs` (register in `tests/vbnet/mod.rs`), regenerate `fixtures/extraction/vbnet/**`

**Interfaces:**
- Consumes: Task 1 rules row for vbnet; grammar nodes `parameter` → `as_clause` (field `type`), `dim_statement` (`as_clause`, `initializer`; `variable_declarator`), `field_declaration` → `variable_declarator` → `as_clause`, `constructor_declaration` (field `parameters`), `member_access` (fields `object`, `member`), `me_expression`, `mybase_expression`, `object_creation_expression`.
- Produces: parameter symbols with declared facts under methods and constructors; `dim_statement` → new `variable` symbols with declared fact (`Dim x As Foo`) or inferred fact (`Dim x = New Foo()`); fields and properties gain declared facts; `Me.M()`/`MyBase.M()` per the rules row on identifier and pending rows.

**Contract inputs:** vbnet open_gaps entry (`variable`).

**File ownership:** Copy of contract row — `crates/julie-extractors/src/vbnet/**`, `crates/julie-extractors/src/tests/vbnet/**`, `fixtures/extraction/vbnet/**`.

**Serialization required:** No

**Dependency reason:** None - safe parallel batch.

**What to build:** The three fact shapes including a new `Dim` walker.

**Approach:** TDD: `Sub F(ByVal a As Foo, ByRef b As List(Of Foo))` → `Foo`, `List`; `Dim x As Foo?` → `Foo`; `Dim x = New Foo()` → inferred; `Dim y As New Foo()` → declared; `Dim z = Build()` → symbol, no fact; field and property facts; `Me.Run()` on both rows. Extend `basic/source.vb` when a shape is absent. Regenerate goldens.

**Acceptance criteria:**
- [x] The seven cases pass; locals parent to the method or constructor.
- [x] `cargo xtask test language vbnet` passes.
- [x] Verified diff handed to the lead (parallel-lead-commit).

### Task 16: PowerShell

**Files:**
- Create: `crates/julie-extractors/src/powershell/type_facts.rs`
- Modify: `crates/julie-extractors/src/powershell/mod.rs` (parameter call gated at lines 75-79), `crates/julie-extractors/src/powershell/functions.rs` (`extract_function_parameters` line 116), `crates/julie-extractors/src/powershell/classes.rs` (`extract_method` line 44, `extract_property` line 80), `crates/julie-extractors/src/powershell/variables.rs` (`extract_variable`), `crates/julie-extractors/src/powershell/identifiers.rs` (`invocation_expression` arm lines 79-115), `crates/julie-extractors/src/powershell/relationships.rs` (pending call emission)
- Test: create `crates/julie-extractors/src/tests/powershell/type_facts.rs` (register in `tests/powershell/mod.rs`), regenerate `fixtures/extraction/powershell/**`

**Interfaces:**
- Consumes: Task 1 rules row for powershell and the Task 1 helper; existing parameter symbols (`parameter_definition`, `script_parameter`); grammar nodes `class_method_parameter` (children `type_literal`, `variable`), `class_method_definition`, `class_property_definition`, `assignment_expression` with a leading `type_literal`, `member_access`.
- Produces: a structural reducer that takes a `type_literal` node, drops its outer `[`…`]`, and returns the inner type text as base (`[System.Collections.Generic.List[string]]` → base text `System.Collections.Generic.List[string]`, then generic-open `[` yields `System.Collections.Generic.List`; declared keeps the full bracketed text); existing function parameter symbols gain `role = "parameter"` metadata and declared facts; class-method parameters become symbols (today skipped because `mod.rs` gates on `SymbolKind::Function`); typed locals (`[Foo]$x = ...`) gain declared facts, `$x = [Foo]::new()` and `$x = New-Object Foo` gain inferred facts (`$x = Get-Thing` records nothing); class properties gain facts; a `class_method_definition` whose name equals the enclosing class name becomes kind `constructor`; `$this.M()` per the rules row on identifier and pending rows.

**Contract inputs:** powershell open_gaps entry (`constructor`).

**File ownership:** Copy of contract row — `crates/julie-extractors/src/powershell/**`, `crates/julie-extractors/src/tests/powershell/**`, `fixtures/extraction/powershell/**`.

**Serialization required:** No

**Dependency reason:** None - safe parallel batch.

**What to build:** Role metadata and facts on existing parameter symbols, class-method parameters, constructor kind, and `$this` receiver metadata.

**Approach:** TDD: advanced function with `[Parameter()] [string] $Name` → fact `string`; class method `[void] Run([Foo]$f)` → parameter symbol with `Foo`; `[System.Collections.Generic.List[string]]$items` → `System.Collections.Generic.List` with declared metadata; `$w = [Widget]::new()` → inferred; `$g = Get-Thing` → no fact; class constructor → `constructor`; `$this.Run()` → `Widget` on both rows. Extend `basic/source.ps1` when a shape is absent. Regenerate goldens.

**Acceptance criteria:**
- [x] The seven cases pass; no `resolved_type` contains `[` or `]` except a trailing array `[]`.
- [x] `cargo xtask test language powershell` passes.
- [x] Verified diff handed to the lead (parallel-lead-commit).

### Task 17: F#

**Files:**
- Create: `crates/julie-extractors/src/fsharp/parameters.rs`
- Modify: `crates/julie-extractors/src/fsharp/types.rs` (`insert_type` lines 117-139 → route through the Task 1 helper with a structural base-name reducer for `long_identifier`, `postfix_type`, and generic `type_arguments`), `crates/julie-extractors/src/fsharp/declarations.rs` (`extract_type` lines 115-130 for `type_abbrev_defn`, `extract_member` lines 151-171, `extract_function_or_value` lines 216-242), `crates/julie-extractors/src/fsharp/identifiers.rs` (`emit` lines 199-237), `crates/julie-extractors/src/fsharp/relationships.rs` (pending call emission)
- Test: extend `crates/julie-extractors/src/tests/fsharp/semantic_facts.rs` or create `tests/fsharp/type_facts.rs` (register in `tests/fsharp/mod.rs`), regenerate `fixtures/extraction/fsharp/**`

**Interfaces:**
- Consumes: Task 1 rules row for fsharp and the Task 1 helper; grammar nodes `argument_patterns` → `typed_pattern` (`_pattern`, `_type`), `method_or_prop_defn` (field `args`), `value_declaration_left` with `typed_pattern`, `record_field`, `union_type_field`, `type_abbrev_defn` (child `type_name`), `property_or_ident` (fields `instance`, `method`).
- Produces: parameter symbols with declared facts for typed patterns under let-functions and members (untyped identifier patterns get symbols without facts); all existing hand-built `TypeInfo` rows go through the Task 1 helper with a structural base (`Foo<int>` → `Foo`, `int list` → `list`, `Foo.Bar` → `Foo.Bar`, declared kept), and literal-inferred let rows now carry `is_inferred=true`; `type_abbrev_defn` → a `type` symbol; calls whose receiver text equals the enclosing member's `instance` identifier carry `receiver_type` = enclosing type name on identifier and pending rows.

**Contract inputs:** fsharp open_gaps entry (`type`). Survey: fsharp inserts `TypeInfo` directly with `is_inferred=false` always; the self identifier is not a fixed keyword.

**File ownership:** Copy of contract row — `crates/julie-extractors/src/fsharp/**`, `crates/julie-extractors/src/tests/fsharp/**`, `fixtures/extraction/fsharp/**`.

**Serialization required:** No

**Dependency reason:** None - safe parallel batch.

**What to build:** Parameter symbols, normalized facts, type abbreviations, and instance-identifier receiver metadata.

**Approach:** TDD: `let f (x: Foo) (xs: Foo list) y = ...` → `Foo`, `list` (declared `Foo list`), `y` symbol without fact; `member this.Run(a: Bar) = this.Helper()` → parameter `Bar`, receiver_type = enclosing type on both rows; `member x.Go() = x.Helper()` → same with `x`; `other.Helper()` → none; `type Id = int` → `type` symbol; a literal-inferred `let n = 1` row carries `is_inferred=true`. Extend `basic/source.fs` when a shape is absent. Regenerate goldens.

**Acceptance criteria:**
- [x] The six cases pass; no `resolved_type` contains `<` or whitespace.
- [x] `cargo xtask test language fsharp` passes.
- [x] Verified diff handed to the lead (parallel-lead-commit).

### Task 18: QML

**Files:**
- Create: `crates/julie-extractors/src/qml/type_facts.rs`, `crates/julie-extractors/src/qml/locals.rs`
- Modify: `crates/julie-extractors/src/javascript/parameters.rs` (`extract_parameter_symbols` visibility → `pub(crate)`), `crates/julie-extractors/src/qml/mod.rs` (`traverse_node`: `ui_property` arm lines 122-139, `function_declaration` arm lines 297-329), `crates/julie-extractors/src/qml/identifiers.rs` (`call_expression` arm lines 89-128), `crates/julie-extractors/src/qml/relationships.rs` (pending call emission)
- Test: create `crates/julie-extractors/src/tests/qml/type_facts.rs` (register in `tests/qml/mod.rs`), regenerate `fixtures/extraction/qml/**`

**Interfaces:**
- Consumes: Task 1 rules row for qml; `javascript/parameters.rs::extract_parameter_symbols` (same `formal_parameters`/`required_parameter`/`optional_parameter` grammar); grammar nodes `ui_property` (fields `name`, `type`, `value`), `lexical_declaration`/`variable_declaration` inside `statement_block`, `ui_object_definition`, `ui_binding` with `id`.
- Produces: JS function parameters inside `.qml` become parameter symbols (parent = the `function` symbol) via the shared javascript walker; `ui_property` symbols gain a declared fact from `type` (`property Foo x` → `Foo`; `property alias` records nothing; `property var` records `var`); `let`/`const`/`var` inside function bodies → `variable` symbols with inferred facts for `new Foo()` (`let x = build()` records nothing); calls per the rules row carry `receiver_type` on identifier and pending rows.

**Contract inputs:** qml has no open_gaps entry; the user added qml to wave 2 on 2026-09-01. Survey: `.qmltypes` parameter rows are descriptor metadata and stay untouched.

**File ownership:** Copy of contract row — `crates/julie-extractors/src/qml/**`, `crates/julie-extractors/src/javascript/parameters.rs` (visibility only), `crates/julie-extractors/src/tests/qml/**`, `fixtures/extraction/qml/**`.

**Serialization required:** No

**Dependency reason:** None - safe parallel batch (the javascript edit is a one-line `pub(crate)` change no other task touches).

**What to build:** Parameter symbols, property facts, JS locals, and id-based receiver metadata.

**Approach:** TDD: `function format(title, count)` → two parameter symbols without facts; `property LocalCard card` → `LocalCard`; `property list<Item> rows` → `list` with declared metadata; `let card = new LocalCard()` → inferred; `let d = new Date()` → inferred `Date` (syntax-stated `new`, no same-file rule for `new`); `let n = compute()` → no fact; `root.format(x)` inside `Item { id: root }` in `Widget.qml` → `receiver_type = "Widget"` on both rows. Extend `basic/source.qml` when a shape is absent. Regenerate goldens (all four qml fixtures).

**Acceptance criteria:**
- [x] The seven cases pass; `.qmltypes` goldens are unchanged.
- [x] `cargo xtask test language qml` and `cargo xtask test language javascript` pass.
- [x] Verified diff handed to the lead (parallel-lead-commit).

### Task 19: PHP

**Files:**
- Create: `crates/julie-extractors/src/php/type_facts.rs`, `crates/julie-extractors/src/php/parameters.rs`, `crates/julie-extractors/src/php/locals.rs`
- Modify: `crates/julie-extractors/src/php/mod.rs` (dispatch lines 129-145), `crates/julie-extractors/src/php/functions.rs` (`extract_function` lines 10-100), `crates/julie-extractors/src/php/members.rs` (`extract_property` lines 12-116, `extract_constant` lines 143-229), `crates/julie-extractors/src/php/identifiers.rs` (`member_call_expression` line 34, `scoped_call_expression` line 52), `crates/julie-extractors/src/php/call_relationships.rs` (pending call emission)
- Test: create `crates/julie-extractors/src/tests/php/type_facts.rs` (register in `tests/php/mod.rs`), regenerate `fixtures/extraction/php/**`

**Interfaces:**
- Consumes: Task 1 rules row for php; grammar nodes `simple_parameter` (fields `type`, `name`, `default_value`, `reference_modifier`), `variadic_parameter`, `property_promotion_parameter` (`visibility`, `type`, `name`), `property_declaration` with `type`, `const_declaration` inside class bodies, `assignment_expression` inside function bodies, `object_creation_expression`, `member_call_expression`, `scoped_call_expression`.
- Produces: parameter symbols (typed → declared fact; union types → no fact) under functions, methods, and constructors; promoted parameters keep their `Property` symbol (with a fact) and also get a parameter symbol; `$x = new Foo()` inside a callable → `variable` symbol with inferred fact (today only top-level assignments become symbols); typed properties gain facts; `$this->m()`, `self::m()`, `static::m()`, `parent::m()` per the rules row on identifier and pending rows; class `const X` proven by adding one to `basic/source.php` (the code path exists; the golden lacks evidence).

**Contract inputs:** php open_gaps entry (`constant`). Survey correction: class constants already extract; only golden evidence is missing.

**File ownership:** Copy of contract row — `crates/julie-extractors/src/php/**`, `crates/julie-extractors/src/tests/php/**`, `fixtures/extraction/php/**`.

**Serialization required:** No

**Dependency reason:** None - safe parallel batch.

**What to build:** The three fact shapes plus class-constant golden evidence.

**Approach:** TDD: `function f(?Foo $a, Foo|Bar $b, \App\Foo $c, Foo ...$rest)` → `Foo`, none, `App\Foo`, `Foo`; `public function __construct(private Foo $svc)`; `$w = new Widget()` local; `$u = new \Vendor\Unknown()` → inferred `Vendor\Unknown` (syntax-stated `new`; dotted stays unmatched by Miller, recorded as written); `$m = make()` → symbol, no fact; typed property; `$this->run()`, `self::make()`, `parent::boot()` on both rows. Regenerate goldens.

**Acceptance criteria:**
- [x] The six cases pass; a class constant appears in the basic golden.
- [x] `cargo xtask test language php` passes.
- [x] Verified diff handed to the lead (parallel-lead-commit).

### Task 20: Ruby

**Files:**
- Create: `crates/julie-extractors/src/ruby/type_facts.rs`, `crates/julie-extractors/src/ruby/parameters.rs`
- Modify: `crates/julie-extractors/src/ruby/helpers.rs` (`infer_symbol_kind_from_assignment` lines 60-77), `crates/julie-extractors/src/ruby/assignments.rs` (`extract_assignment` line 34, parallel assignment lines 79-114), `crates/julie-extractors/src/ruby/mod.rs` (bare ivar/cvar arm lines 213-217), `crates/julie-extractors/src/ruby/symbols.rs` (`extract_method` lines 147-208, `extract_singleton_method` lines 211-237, `extract_variable` lines 240-260), `crates/julie-extractors/src/ruby/identifiers.rs` (`call` arm lines 60-101), `crates/julie-extractors/src/ruby/relationships.rs` (pending call emission), `crates/julie-extractors/src/ruby/calls.rs`
- Test: create `crates/julie-extractors/src/tests/ruby/type_facts.rs` (register in `tests/ruby/mod.rs`), regenerate `fixtures/extraction/ruby/**`

**Interfaces:**
- Consumes: Task 1 rules row for ruby; grammar nodes `method_parameters` children (`identifier`, `optional_parameter`, `keyword_parameter`, `splat_parameter`, `hash_splat_parameter`, `block_parameter`), `assignment` (field `left`), `instance_variable`, `class_variable`, `call` (fields `receiver`, `method`).
- Produces: parameter symbols without facts (all parameter kinds) under methods, `initialize`, and singleton methods; `@x`/`@@x` assignment targets inside a class body or its methods → kind `field` with parent = the class (one symbol per name per class; first assignment wins; bare reads create nothing); locals `x = Foo.new(...)` inside a method → `variable` with inferred fact when `Foo` is a same-file class; `self.m` per the rules row on identifier and pending rows.

**Contract inputs:** ruby open_gaps entry (`field`). Declared types are not applicable (decision doc).

**File ownership:** Copy of contract row — `crates/julie-extractors/src/ruby/**`, `crates/julie-extractors/src/tests/ruby/**`, `fixtures/extraction/ruby/**`.

**Serialization required:** No

**Dependency reason:** None - safe parallel batch.

**What to build:** Parameter symbols, ivar/cvar fields, constructor-inferred local facts, and `self` receiver metadata.

**Approach:** TDD: `def run(a, b = 1, *rest, key:, &blk)` → five parameter symbols; `@count = 0` in `initialize` → `field` under the class, and a second `@count = 1` in another method creates no duplicate; `w = Widget.new` → inferred `Widget`; negatives (`u = Unknown.new`, `n = Net::HTTP.new`, `v = build` → symbol, no fact); `self.helper` → enclosing class on both rows. Extend `basic/source.rb` when a shape is absent. Regenerate goldens.

**Acceptance criteria:**
- [x] The five cases pass; no ivar row is kind `variable` any more.
- [x] `cargo xtask test language ruby` passes.
- [x] Verified diff handed to the lead (parallel-lead-commit).

### Task 21: Lua

**Files:**
- Create: `crates/julie-extractors/src/lua/type_facts.rs`, `crates/julie-extractors/src/lua/parameters.rs`
- Modify: `crates/julie-extractors/src/lua/functions.rs` (`extract_function_definition_statement` lines 43-177), `crates/julie-extractors/src/lua/variables.rs` (local assignment kinds lines 37-58), `crates/julie-extractors/src/lua/identifiers.rs` (`method_index_expression` arm lines 109-128; `self` filter lines 165-169), `crates/julie-extractors/src/lua/relationships.rs` and `crates/julie-extractors/src/lua/core.rs` (pending call emission), `crates/julie-extractors/src/lua/mod.rs` (`infer_types` stub lines 76-78 stays; facts ride `base.type_info`)
- Test: create `crates/julie-extractors/src/tests/lua/type_facts.rs` (register in `tests/lua/mod.rs`), regenerate `fixtures/extraction/lua/**`

**Interfaces:**
- Consumes: Task 1 rules row for lua; grammar nodes `parameters` → `name_list` identifiers and vararg, `variable_declaration` (`local`), `function_call`, `method_index_expression`, `dot_index_expression`; the existing class-promotion heuristics in `lua/classes.rs` (`Class.new` + colon methods, `setmetatable`).
- Produces: parameter symbols without facts for every named parameter (parent = function/method); colon-method definitions (`function Foo:bar()`) get an implicit `self` parameter symbol with a declared fact `Foo` (`is_inferred=false`: the syntax states the owner); `local x = Foo.new(...)` and `local x = setmetatable({}, Foo)` → `variable` with inferred fact when `Foo` is a same-file class-promoted symbol; `self:m()`/`self.m()` inside a colon method carry `receiver_type` = the owning table name on identifier and pending rows.

**Contract inputs:** lua open_gaps entry (`variable`); lua `capabilities.types` is `false` with a recorded exception. This task emits `type_facts` rows, so Task 28 flips `types` to `true` and removes the exception.

**File ownership:** Copy of contract row — `crates/julie-extractors/src/lua/**`, `crates/julie-extractors/src/tests/lua/**`, `fixtures/extraction/lua/**`.

**Serialization required:** No

**Dependency reason:** None - safe parallel batch.

**What to build:** Parameter symbols, implicit-self facts, constructor-inferred locals, and colon-method receiver metadata.

**Approach:** TDD: `function Account:deposit(amount)` → `self` (fact `Account`) and `amount` symbols; `local a = Account.new(10)` → inferred `Account`; negatives (`local u = Unknown.new()`, `local r = require("x").new()`, `local t = {}` → symbol, no fact); `self:log()` → `Account` on both rows. Extend `basic/source.lua` when a shape is absent. Regenerate goldens.

**Acceptance criteria:**
- [x] The four cases pass.
- [x] `cargo xtask test language lua` passes.
- [x] Verified diff handed to the lead (parallel-lead-commit).

### Task 22: R

**Files:**
- Create: `crates/julie-extractors/src/r/type_facts.rs`, `crates/julie-extractors/src/r/parameters.rs`
- Modify: `crates/julie-extractors/src/r/idioms.rs` (`call_name` lines 328-335 → accept `namespace_operator` callees and return the right-hand identifier; `extract_assignment_class_factory` lines 35-78), `crates/julie-extractors/src/r/mod.rs` (`extract_from_binary_op` lines 80-213, `extract_parameters` lines 234-278), `crates/julie-extractors/src/r/identifiers.rs` (`call` arm lines 58-93), `crates/julie-extractors/src/r/relationships.rs` (pending call emission)
- Test: create `crates/julie-extractors/src/tests/r/type_facts.rs` (register in `tests/r/mod.rs`), regenerate `fixtures/extraction/r/**`

**Interfaces:**
- Consumes: Task 1 rules row for r; grammar nodes `parameter` (optional `default`), `binary_operator` assignments, `call`, `extract_operator` (`$`), `namespace_operator` (`::`).
- Produces: parameter symbols without facts (parent = function); `R6::R6Class(...)`, `setClass(...)`, and `setRefClass(...)` factories yield `class` symbols for namespaced and bare callees (the basic golden uses `R6::R6Class`); `x <- Foo$new(...)`, `x <- new("Foo")`, and `x <- Foo(...)` where `Foo` is a same-file class symbol → `variable` with inferred fact; `self$m()` inside a function nested in an R6Class `public`/`private` list carries `receiver_type` = that class name on identifier and pending rows.

**Contract inputs:** r open_gaps entry (`class`); r `capabilities.types` is `false` with a recorded exception. Survey: the class code path exists but `call_name` drops namespaced callees.

**File ownership:** Copy of contract row — `crates/julie-extractors/src/r/**`, `crates/julie-extractors/src/tests/r/**`, `fixtures/extraction/r/**`.

**Serialization required:** No

**Dependency reason:** None - safe parallel batch.

**What to build:** Parameter symbols, the namespaced class-factory fix, constructor-inferred locals, and R6 `self` receiver metadata.

**Approach:** TDD: `Worker <- R6::R6Class("Worker", public = list(run = function(n) self$log(n)))` → class `Worker`, parameter `n`, `receiver_type = "Worker"` on both rows; `w <- Worker$new()` → inferred `Worker`; `p <- new("Point")` with a same-file `setClass("Point", ...)` → inferred `Point`; negatives (`u <- Unknown$new()`, `d <- data.frame()`, `f <- fit(x)` → symbol, no fact). Regenerate goldens.

**Acceptance criteria:**
- [x] The four cases pass; the basic golden now has a `class` row.
- [x] `cargo xtask test language r` passes.
- [x] Verified diff handed to the lead (parallel-lead-commit).

### Task 23: Elixir

**Files:**
- Create: `crates/julie-extractors/src/elixir/type_facts.rs`, `crates/julie-extractors/src/elixir/parameters.rs`
- Modify: `crates/julie-extractors/src/elixir/calls.rs` (`extract_def` line 118, `extract_defmacro` line 174), `crates/julie-extractors/src/elixir/identifiers.rs` (`extract_identifier_from_node` lines 45-135; add `map` with `struct` child → `type_usage` identifier)
- Test: create `crates/julie-extractors/src/tests/elixir/type_facts.rs` (register in `tests/elixir/mod.rs`), regenerate `fixtures/extraction/elixir/**`

**Interfaces:**
- Consumes: Task 1 rules row for elixir; grammar nodes `call` heads for `def`/`defp`/`defmacro`, `map` (`%` + optional `struct` child + body), `binary_operator` `=` match.
- Produces: parameter symbols for identifier patterns in function heads (parent = function; a `%Foo{} = name` or `%Foo{field: x}` pattern binding gets a declared fact `Foo` for `name`, since the syntax states the struct); `name = %Foo{...}` inside a function body → `variable` with inferred fact `Foo`; `%Foo{}` literals emit a `type_usage` identifier. No `receiver_type` (decision doc).

**Contract inputs:** elixir open_gaps entry (`variable`): the plan must scope feasible facts. Scope = struct patterns and struct literals only; `@spec` stays out.

**File ownership:** Copy of contract row — `crates/julie-extractors/src/elixir/**`, `crates/julie-extractors/src/tests/elixir/**`, `fixtures/extraction/elixir/**`.

**Serialization required:** No

**Dependency reason:** None - safe parallel batch.

**What to build:** Parameter symbols, struct-pattern facts, struct-literal locals, and struct-literal type usages.

**Approach:** TDD: `def run(%Worker{} = w, n)` → `w` with fact `Worker`, `n` without; `def go(x), do: y = %Job{id: x}` → `y` inferred `Job`; `z = Map.new()` and `q = %{a: 1}` → symbol, no fact; `%Job{}` type_usage identifier. Extend `basic/source.ex` when a shape is absent. Regenerate goldens.

**Acceptance criteria:**
- [x] The four cases pass.
- [x] `cargo xtask test language elixir` passes.
- [x] Verified diff handed to the lead (parallel-lead-commit).

### Task 24: Erlang

**Files:**
- Create: `crates/julie-extractors/src/erlang/type_facts.rs`, `crates/julie-extractors/src/erlang/parameters.rs`
- Modify: `crates/julie-extractors/src/erlang/definition_forms.rs` (`extract_function` lines 41-88), `crates/julie-extractors/src/erlang/mod.rs` (walker), `crates/julie-extractors/src/erlang/identifiers.rs` (record references lines 142-144 stay)
- Test: create `crates/julie-extractors/src/tests/erlang/type_facts.rs` (register in `tests/erlang/mod.rs`), regenerate `fixtures/extraction/erlang/**`

**Interfaces:**
- Consumes: Task 1 rules row for erlang; grammar nodes `function_clause` → `expr_args`/`var_args`, `record_expr` (`record_name`), `match_expr`, `var`.
- Produces: parameter symbols for variable patterns in clause heads, one per name per `name/arity` function (dedupe across clauses; parent = the collapsed function symbol); `#foo{} = X` head patterns give `X` a declared fact `foo`; `X = #foo{...}` inside a body → `variable` with inferred fact `foo`. No `receiver_type` (decision doc).

**Contract inputs:** erlang open_gaps entry (`variable`). Scope = record patterns and record literals only; `-spec` matching stays as it is.

**File ownership:** Copy of contract row — `crates/julie-extractors/src/erlang/**`, `crates/julie-extractors/src/tests/erlang/**`, `fixtures/extraction/erlang/**`.

**Serialization required:** No

**Dependency reason:** None - safe parallel batch.

**What to build:** Parameter symbols and record-derived facts.

**Approach:** TDD: `run(#state{} = S, N) -> ...; run(S, 0) -> ...` → `S` (fact `state`) and `N` once each; `go(X) -> R = #req{id = X}, R.` → `R` inferred `req`; `M = maps:new()` → symbol, no fact. Extend `basic/source.erl` when a shape is absent. Regenerate goldens.

**Acceptance criteria:**
- [x] The three cases pass; multi-clause functions yield one parameter symbol per name.
- [x] `cargo xtask test language erlang` passes.
- [x] Verified diff handed to the lead (parallel-lead-commit).

### Task 25: Bash

**Files:**
- Modify: `crates/julie-extractors/src/bash/functions.rs` (`extract_positional_parameters` lines 56-95), `crates/julie-extractors/src/bash/variables.rs` (`extract_declarations` lines 60-113: `local` handling at line 155 region)
- Test: create `crates/julie-extractors/src/tests/bash/type_facts.rs` (register in `tests/bash/mod.rs`), regenerate `fixtures/extraction/bash/**`

**Interfaces:**
- Consumes: Task 1 rules row for bash; existing `$1`/`$2` usage-derived parameter symbols; `declaration_command` with `local`.
- Produces: positional parameter symbols gain metadata `role = "parameter"`; `local x=...` inside a function yields a `variable` symbol with parent = the function (assert, add if missing); `readonly`/exported-uppercase `constant` rows stay as they are and get asserted by a unit test so Task 28 can claim `constant`. No facts, no `receiver_type` (decision doc).

**Contract inputs:** bash open_gaps entry (`constant`). Survey: classification already exists; the claim lacks a unit assertion.

**File ownership:** Copy of contract row — `crates/julie-extractors/src/bash/**`, `crates/julie-extractors/src/tests/bash/**`, `fixtures/extraction/bash/**`.

**Serialization required:** No

**Dependency reason:** None - safe parallel batch.

**What to build:** Role metadata on positional parameters and local/constant assertions.

**Approach:** TDD: function using `$1` and `$2` → two parameter symbols with `role`; `local count=$1` → `variable` under the function; `readonly MAX=3` and `export API_URL=x` → `constant`. Regenerate goldens.

**Acceptance criteria:**
- [x] The three cases pass.
- [x] `cargo xtask test language bash` passes.
- [x] Verified diff handed to the lead (parallel-lead-commit).

### Task 26: Razor

**Files:**
- Create: `crates/julie-extractors/src/razor/type_facts.rs`, `crates/julie-extractors/src/razor/parameters.rs`
- Modify: `crates/julie-extractors/src/razor/csharp.rs` (`extract_method` lines 220-337, `extract_property` lines 340-468), `crates/julie-extractors/src/razor/stubs.rs` (`extract_field` lines 9-87 → kind `field`; `extract_local_function` lines 90-193; `extract_local_variable` lines 196-293), `crates/julie-extractors/src/razor/identifiers.rs` (`invocation_expression` lines 56-92, `member_access_expression` lines 96-117), `crates/julie-extractors/src/razor/type_inference.rs` (legacy regex path stays as fallback)
- Test: create `crates/julie-extractors/src/tests/razor/type_facts.rs` (register in `tests/razor/mod.rs`), regenerate `fixtures/extraction/razor/**` except `code-behind/expected.cs.json`

**Interfaces:**
- Consumes: Task 1 rules row for razor; the C# node shapes inside `razor_block` (`parameter` with `name`/`type`, `local_declaration_statement` → `variable_declaration` → `variable_declarator`, `field_declaration`, `property_declaration`); csharp wave-1 code as the template (`csharp/locals.rs`, `csharp/type_inference.rs::record_declared_type`, `csharp/identifiers.rs::self_receiver_type`).
- Produces: parameter symbols with declared facts under methods and local functions in `@code`/`@functions`; `@code` fields become kind `field` (today `variable` with metadata `type: field`) with declared facts; locals gain declared facts and `new Foo()` inferred facts; properties gain facts; `this.M()` carries `receiver_type` = the component class name on the identifier row only (razor emits no pending rows; recorded exception `razor_pending_relationships_handled_by_csharp_embed`).

**Contract inputs:** razor open_gaps entry (`field`). Survey: razor reimplements C# extraction in `razor/csharp.rs` and `razor/stubs.rs`; nothing from csharp wave 1 flows through.

**File ownership:** Copy of contract row — `crates/julie-extractors/src/razor/**`, `crates/julie-extractors/src/tests/razor/**`, `fixtures/extraction/razor/**` except `code-behind/expected.cs.json`.

**Serialization required:** No

**Dependency reason:** None - safe parallel batch (Task 6 owns `expected.cs.json` and lands in Batch A first).

**What to build:** The three fact shapes inside razor code blocks plus the field kind fix.

**Approach:** TDD: `@code { private Widget _w; [Parameter] public string Title { get; set; } void Run(Widget w) { var x = new Widget(); var y = Build(); this.Refresh(); } }` → field `_w` (kind `field`, fact `Widget`), property fact `string`, parameter `w` with `Widget`, local `x` inferred, local `y` symbol without fact, `receiver_type` = component name on the identifier. Regenerate razor goldens; `expected.cs.json` must not change.

**Acceptance criteria:**
- [x] The six cases pass; no `@code` field row is kind `variable` any more.
- [x] `cargo xtask test language razor` passes.
- [x] Verified diff handed to the lead (parallel-lead-commit).

### Task 27: Evidence scan

**Files:**
- Create: `docs/findings/2026-09-08-receiver-type-facts-wave-2-evidence.md`

**Interfaces:**
- Consumes: the built `julie-extract` binary with Tasks 1–26 landed.
- Produces: the hard-gate numbers Task 28 and the final report cite.

**Contract inputs:** Replay/metric evidence section above. Corpus: for each of the 21 languages (20 wave-2 plus python), scan the language's golden `basic` source plus, when cached locally, the real-world corpus entry from `fixtures/extraction/tree-sitter-real-world-corpus.toml`; say per language which inputs were scanned.

**File ownership:** Copy of contract row — `docs/findings/2026-09-08-receiver-type-facts-wave-2-evidence.md`.

**Serialization required:** Yes

**Dependency reason:** Needs Tasks 2–26 landed to measure.

**What to build:** `julie-extract scan` into a scratch SQLite per language, then SQL counts: parameter symbols (`metadata_json` role), typed locals, typed fields, `receiver_type` rows on identifiers and on pending relationships (two separate counts), and the corrupt-`resolved_type` query from wave 1 extended with `[`, `(`, `*`, `&`, and `?`.

**Approach:** Reuse the wave-1 queries from `docs/findings/2026-09-01-receiver-type-facts-evidence.md`; write the findings doc with the exact queries and per-language tables; if a hard gate fails, add a fix task before Task 28 (as wave 1 did) instead of narrowing the gate.

**Acceptance criteria:**
- [x] Hard gates pass: 0 corrupt `resolved_type` rows; parameter symbols with role in all 21 languages; `receiver_type` rows on identifiers and pending relationships in every language the decision doc marks applicable (razor: identifiers only).
- [x] Findings doc records queries, counts, and inputs per language.
- [x] Change committed (serial-worker-commit).

### Task 28: Closeout

**Files:**
- Modify: `fixtures/extraction/capabilities.json`, `docs/plans/2026-09-01-receiver-typed-call-resolution.md`, this plan's ledger section

**Interfaces:**
- Consumes: all prior tasks; the strict quality report; `cargo xtask test capability` claim rules (every supported kind needs golden evidence).
- Produces: honest capability rows; the spec doc reflects delivery state.

**Contract inputs:** The 19 `open_gaps` entries that name this plan (c, cpp, zig, python, vbnet, php, ruby, swift, kotlin, dart, elixir, fsharp, erlang, lua, r, bash, powershell, gdscript, razor). Anchored kinds to move to `supported`: c `variable`, cpp `variable`, zig `constant`, python `variable`, vbnet `variable`, php `constant`, ruby `field`, swift `variable`, kotlin `variable`, dart `variable`, elixir `variable`, fsharp `type`, erlang `variable`, lua `variable`, r `class`, bash `constant`, powershell `constructor`, gdscript `variable`, razor `field`. Additional evidence-backed claims: scala `variable`, `property`, `function`, `constructor` (goldens already emit some of these unclaimed); qml unchanged kinds (already claims `variable`, `property`, `function`); every language whose goldens now emit `variable` parameter rows claims `variable`. Flip `capabilities.types` to `true` for lua and r and delete their recorded exceptions. Kinds a task retargeted away (gdscript locals no longer `field` at function scope) keep their claim only if class-level evidence remains.

**File ownership:** Copy of contract row — `fixtures/extraction/capabilities.json`, `docs/plans/2026-09-01-receiver-typed-call-resolution.md`, this plan.

**Serialization required:** Yes

**Dependency reason:** Single owner of capabilities.json; needs all prior tasks.

**What to build:** Capability updates, spec status update (wave 2 landed), branch-gate run, ledger entries.

**Approach:** Follow the existing capabilities.json row shapes; run the full branch gate; fix anything it finds. Every claim must survive `cargo xtask test capability` and the strict report with `silent_cells=0`, `quality_bar_debts=0`. This task does not write new `open_gaps` entries for any of the 19 languages; if an anchored kind lacks evidence at this point, stop and report a plan mismatch naming the language task that fell short.

**Acceptance criteria:**
- [x] No `open_gaps` entry references `docs/plans/2026-09-08-receiver-type-facts-wave-2.md` any more; all 19 anchored kinds are `supported` with golden evidence.
- [x] scala and qml claims match their goldens; lua and r claim `types: true`.
- [x] `node scripts/language-data-quality-report.mjs --strict` passes with silent_cells=0, quality_bar_debts=0.
- [x] Branch gate green: `cargo test --workspace`, `cargo xtask test capability`, `cargo xtask test golden`, fmt, `git diff --check`.
- [x] Spec doc status updated (wave 1 landed, wave 2 landed).
- [x] Change committed (serial-worker-commit).

## Verification Ledger

| Scope | Invariant | Command | Commit | Result | Time |
|-------|-----------|---------|--------|--------|------|
| worker-red-green | helper stores resolved_type from base_text; v2 marker; epoch 9 | `cargo test -p julie-extractors --lib -- record_declared_type_fact` and `tests::api_surface` | cfe17f65 | pass | 2026-09-01T19:33:53Z |
| affected-change | lib suite after Task 1 base helper | `cargo test -p julie-extractors --lib` | cfe17f65 | 3678 passed | 2026-09-01T19:35:00Z |
| affected-change | structural_fact_registry after Task 1 | `cargo test -p julie-extractors --features test-capability-matrix --lib structural_fact_registry` | cfe17f65 | 16 passed | 2026-09-01T19:35:30Z |
| worker-red-green | python locals parent callable; self/cls receiver_type | `cargo xtask test language python` | b985bae5 | pass | 2026-09-01 |
| worker-red-green | rust self/Self receiver_type on identifier and pending | `cargo xtask test language rust` | 221fa45a | pass | 2026-09-01 |
| worker-red-green | go constructor facts and receiver-name receiver_type | `cargo xtask test language go` | 7335aa12 | pass | 2026-09-01 |
| worker-red-green | java bindings and this/super receiver_type | `cargo xtask test language java` | 1c5f39fc | pass | 2026-09-01 |
| worker-red-green | csharp indexer declared facts | `cargo xtask test language csharp` | 4147705f | pass | 2026-09-01 |
| affected-change | go walker traversal budget | `cargo test -p julie-extractors --lib -- tests::go::type_facts tests::traversal_guard_convention` | fd34fe46 | 15 passed | 2026-09-01 |
| affected-change | lib suite after Batch C merge | `cargo test -p julie-extractors --lib` | 1952697b | 3860 passed | 2026-09-01 |
| replay | 0 corrupt resolved_type; params in 21 langs; receiver_type on applicable langs | `julie-extract scan` of 21 basic fixtures | 7bb6f63b | pass | 2026-09-01 |
| branch-gate | workspace tests | `cargo test --workspace` | pending-closeout | 4811 passed | 2026-09-01 |
| branch-gate | golden | `cargo xtask test golden` | pending-closeout | 6 passed | 2026-09-01 |
| branch-gate | capability | `cargo xtask test capability` | pending-closeout | 39 passed | 2026-09-01 |
| branch-gate | quality report | `node scripts/language-data-quality-report.mjs --strict` | pending-closeout | silent_cells=0 quality_bar_debts=0 | 2026-09-01 |

Security scope: none declared in this plan, so the branch gate runs no security commands.
