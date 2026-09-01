# Receiver Type Facts Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use razorback:subagent-driven-development when subagent delegation is available. Fall back to razorback:executing-plans for single-task, tightly-sequential, or no-delegation runs.

**Goal:** Emit declared-type facts for locals, parameters, and fields/properties so Miller policy v6's receiver tier can bind cross-parent same-name calls.

**Architecture:** Extractors record `TypeInfo` rows keyed by symbol id (existing `type_facts` table; no schema change). Parameters become symbols (kind `variable`, metadata `role: "parameter"`) so Miller's scope walk can find them. Call sites gain a `receiver_type` metadata key for self-style receivers. Wave 1 covers csharp, typescript, javascript, python, rust, go, java; the rest of the general-purpose languages become `open_gaps` debt with a named closure plan.

**Tech Stack:** Rust, tree-sitter, existing julie-extractors base infrastructure. No new dependencies.

**Architecture Quality:** Approved shape: per-language walkers call two new base helpers (`record_declared_type_fact`, `strip_type_decorations`) and the existing symbol-creation path; no new fact table, no artifact schema change, no registry restructuring. Main risk: parameter symbols change fixture row counts and a few `containing_symbol_id` bindings; every such shift must be limited to declaration-site rows and reviewed in the golden diffs. If code reality contradicts this shape, report a plan mismatch instead of redesigning locally.

## Consumer contract (read before any task)

Miller resolution policy v6 (`/home/murphy/source/miller/docs/contracts/resolution-policy-v6.md`), Tier 3 Receiver:

- The receiver is found by a scope walk from the call's `caller_scope_symbol_id` up `parent_symbol_id`, filtering by name+language, falling back to file top-level symbols. So locals and parameters must be **symbols whose parent is the enclosing callable**; fields/properties are already reachable as class children.
- For each receiver symbol, each of its `type_facts` rows binds only when `resolved_type` **verbatim-matches the name of exactly one type-like symbol** in the same language. No namespace or generic stripping happens on the Miller side.
- Confidence: 0.75 when `is_inferred=false`, 0.65 when true. Both resolve; declaredness must be honest.

Known caveat (record, do not fix here): the spec's reproduced example (`SymbolGraph.ShortestPathWithEvidence` → `GraphTraversal.ShortestPathWithEvidence`) has a **static receiver naming an `internal` class**. Policy v6's static-type tier refuses non-public types cross-file, so that exact chain needs a Miller-side policy change, not these facts. The facts in this plan fix variable/parameter/field receivers, which are the bulk of the ~22k cross-parent gap. This caveat goes in the decision doc and the final report.

## Global Constraints

- Parameter symbols: kind `variable`, metadata `"role": "parameter"`, `parent_id` = the enclosing callable symbol, span = the parameter node. Never a new `SymbolKind` (Miller drops unknown kinds).
- `TypeInfo.resolved_type` for rows this plan emits = the **base type name**: strip generic argument lists, nullable suffixes (`?`), by-ref/pointer/borrow sigils (`ref`, `out`, `in`, `&`, `*`, `mut`). Do not strip array suffixes (`[]`) — an array receiver must not bind to the element type.
- When the declared text differs from `resolved_type`, keep the full declared text in `TypeInfo.metadata` under key `"declared"`.
- `is_inferred=false` only for a type the syntax states (annotation, declared type). `is_inferred=true` for initializer-derived types (`var x = new Foo()` → `Foo`).
- Recorded rows (`base.type_info`) must win over legacy `infer_types()` map rows (existing precedent: `types_with_base_info`).
- Call-site `receiver_type` metadata: when the receiver is the language's self reference (`this`, `self`, `base`, `super`, or equivalent), record metadata key `"receiver_type"` = enclosing type's name on the call identifier and on the structured pending relationship metadata. No artifact schema change — this rides existing `metadata_json` columns.
- `EXTRACTION_CONTRACT_VERSION` gains `.receiver-type-facts-v1`. `EXTRACTION_IDENTITY_EPOCH` stays 9 (epoch 8 is the last released; 9 is unreleased on this branch). If a release ships from main before this branch lands, bump to 10.
- No SQLite schema change; `SQLITE_SCHEMA_VERSION` untouched.
- No Miller-side changes; no workspace-global resolution in this repo.
- Test discipline: per-language commands in the inner loop; the full default suite runs once at the branch gate.
- Test files get zero comments; no narration comments anywhere.
- All capabilities.json edits happen only in Task 10 (closeout) to keep parallel tasks conflict-free.

## Verification Strategy

**Project source of truth:** `CLAUDE.md` (test discipline), `cargo xtask test --help` for suite names, `scripts/language-data-quality-report.mjs`.

**Worker red/green scope:** `cargo xtask test language <lang>` (unit + golden for that language) plus the focused `cargo test -p julie-extractors <module_filter>` for new tests.

**Worker ceiling:** one language's suite plus directly assigned focused tests. Workers do not run the full workspace suite, capability suite (except Task 10), or corpus scans.

**Worker gate invariant:** per language task — every acceptance fixture row asserted (parameter symbols exist with type facts; `new`-style locals get inferred type facts; field/property facts carry untruncated base names); golden fixtures regenerate cleanly and diffs contain only intended row changes.

**Lead affected-change scope:** after each batch: `cargo test -p julie-extractors --lib` and `cargo test -p julie-extractors --features test-capability-matrix --lib structural_fact_registry`.

**Branch gate:** `cargo test --workspace`, `cargo xtask test capability`, `node scripts/language-data-quality-report.mjs --strict` (silent_cells=0, quality_bar_debts=0), `cargo fmt --all -- --check`, `git diff --check`.

**Security scope:** none declared.

**Replay/metric evidence:** Task 10's miller-corpus scan is a hard gate for: zero `var x = new Foo(...)` locals without type facts in the two-file sample, zero truncated generic `resolved_type` values, parameter symbols present with type facts. The corpus-wide `missing` rate movement is report-only (full measurement needs Miller to pin the release).

**Escalation triggers:** any change to `base/` shared files after Task 1 lands → rerun the affected-change scope before the next batch dispatch. Any fixture diff showing `containing_symbol_id` shifts on non-declaration rows → stop, report as plan mismatch.

**Assigned verification failure:** Workers stop and report when assigned verification fails, unless this plan explicitly says to update that gate.

**Verification ledger:** append entries (invariant, command, scope label, commit SHA, result, timestamp) to the `## Verification Ledger` section at the bottom of this document.

## Parallel Execution Contract

| Task | Parallel batch | File ownership | Serialization required | Dependency reason |
|---|---|---|---|---|
| Task 1: Base contract + helpers | None - serial | Create: `docs/decisions/2026-09-01-receiver-type-facts.md`. Modify: `crates/julie-extractors/src/base/creation_methods.rs`, `crates/julie-extractors/src/base/types.rs` (helper only), `crates/julie-extractors/src/lib.rs`, `crates/julie-extractors/src/tests/api_surface.rs` | Yes | Every language task consumes the helper signatures and contract doc. |
| Task 2: C# type-fact correctness | None - serial | Modify: `crates/julie-extractors/src/csharp/**`, `crates/julie-extractors/src/tests/csharp/**`, `fixtures/extraction/csharp/**`, `fixtures/extraction/razor/code-behind/expected.cs.json` | Yes | Task 3 edits the same csharp files; base helpers from Task 1. |
| Task 3: C# parameters + receiver metadata | None - serial | Same ownership as Task 2 | Yes | Same-file dependency on Task 2's changes. |
| Task 4: TypeScript + JavaScript | Batch B | Modify: `crates/julie-extractors/src/typescript/**`, `crates/julie-extractors/src/javascript/**`, `crates/julie-extractors/src/tests/{typescript,javascript}/**` (if present), `fixtures/extraction/typescript/**`, `fixtures/extraction/javascript/**` | No | None - safe parallel batch. |
| Task 5: Python | Batch B | Modify: `crates/julie-extractors/src/python/**`, python tests, `fixtures/extraction/python/**` | No | None - safe parallel batch. |
| Task 6: Rust | Batch B | Modify: `crates/julie-extractors/src/rust/**`, rust tests, `fixtures/extraction/rust/**` | No | None - safe parallel batch. |
| Task 7: Go | Batch B | Modify: `crates/julie-extractors/src/go/**`, go tests, `fixtures/extraction/go/**` | No | None - safe parallel batch. |
| Task 8: Java | Batch B | Modify: `crates/julie-extractors/src/java/**`, java tests, `fixtures/extraction/java/**` | No | None - safe parallel batch. |
| Task 9: Evidence scan | None - serial | Create: `docs/findings/2026-09-01-receiver-type-facts-evidence.md` | Yes | Needs Tasks 2–8 landed to measure. |
| Task 10: Closeout | None - serial | Modify: `fixtures/extraction/capabilities.json`, `docs/plans/2026-09-01-receiver-typed-call-resolution.md` (status), this plan (ledger) | Yes | Single owner of capabilities.json; needs all prior tasks. |

Commit modes: serial tasks use `serial-worker-commit`; Batch B uses `parallel-lead-commit`.

---

### Task 1: Base contract + helpers

**Files:**
- Create: `docs/decisions/2026-09-01-receiver-type-facts.md`
- Modify: `crates/julie-extractors/src/base/creation_methods.rs`, `crates/julie-extractors/src/base/types.rs`, `crates/julie-extractors/src/lib.rs:131`, `crates/julie-extractors/src/tests/api_surface.rs`
- Test: `crates/julie-extractors/src/tests/api_surface.rs`, new base unit tests beside the helpers

**Interfaces:**
- Consumes: `BaseExtractor.type_info: HashMap<String, TypeInfo>` (`base/extractor.rs:40`), `TypeInfo` (`base/types.rs:469`).
- Produces (later tasks rely on these exact names):
  - `impl BaseExtractor { pub fn record_declared_type_fact(&mut self, symbol_id: &str, declared_text: &str, rules: &TypeNameRules, is_inferred: bool) }` — normalizes via `strip_type_decorations`, inserts a `TypeInfo` with `resolved_type` = base name, `metadata["declared"]` = declared text when different, and does not overwrite an existing row for the symbol.
  - `pub struct TypeNameRules { pub nullable_suffixes: &'static [&'static str], pub reference_prefixes: &'static [&'static str], pub generic_open: &'static [char] }` in `base/types.rs`.
  - `pub fn strip_type_decorations(declared: &str, rules: &TypeNameRules) -> String` in `base/types.rs`.
  - Metadata key contract (documented in the decision doc, plain strings at call sites): `"role"` = `"parameter"` on parameter symbols; `"receiver_type"` on call identifiers and structured pending metadata; `"declared"` in TypeInfo metadata.

**Contract inputs:** Consumer contract section above; policy v6 doc path; `EXTRACTION_CONTRACT_VERSION` currently ends `.marker-razorback-v1`; epoch 9 assertion at `tests/api_surface.rs:20`.

**File ownership:** Copy of contract row — Create: `docs/decisions/2026-09-01-receiver-type-facts.md`. Modify: `crates/julie-extractors/src/base/creation_methods.rs`, `crates/julie-extractors/src/base/types.rs`, `crates/julie-extractors/src/lib.rs`, `crates/julie-extractors/src/tests/api_surface.rs`.

**Serialization required:** Yes

**Dependency reason:** Every language task consumes the helper signatures and contract doc.

**What to build:** The shared normalization helper, the declared-type recording helper, the contract-version marker, and a decision doc that captures the policy-v6 consumer contract, the metadata key contract, and the static-internal caveat.

**Approach:** TDD the normalizer with table-driven cases: `List<int>` → `List`, `IReadOnlyDictionary<string, IReadOnlyList<GraphNeighbour>>` → `IReadOnlyDictionary`, `GraphTraversal?` → `GraphTraversal`, `ref Foo` → `Foo`, `&mut Foo` → `Foo`, `*Store` → `Store`, `string[]` → `string[]` (unchanged), `Foo.Bar` → `Foo.Bar` (namespaces kept; Miller matches bare names, dotted stays unmatched — record as-is). Append `.receiver-type-facts-v1` to `EXTRACTION_CONTRACT_VERSION` and add it to the api_surface marker list.

**Acceptance criteria:**
- [ ] `strip_type_decorations` passes the table above.
- [ ] `record_declared_type_fact` records base name + `declared` metadata and never overwrites an existing row.
- [ ] api_surface test asserts the new marker and epoch 9.
- [ ] Decision doc states the consumer contract, metadata keys, and the static-internal caveat, concretely.
- [ ] Worker-scope verification passes and the change is committed (serial-worker-commit).

### Task 2: C# type-fact correctness

**Files:**
- Modify: `crates/julie-extractors/src/csharp/type_inference.rs`, `crates/julie-extractors/src/csharp/locals.rs`, `crates/julie-extractors/src/csharp/members.rs`, `crates/julie-extractors/src/csharp/mod.rs`
- Test: `crates/julie-extractors/src/tests/csharp/` (new `type_facts.rs` module), regenerate `fixtures/extraction/csharp/**/expected.json`, `fixtures/extraction/razor/code-behind/expected.cs.json`

**Interfaces:**
- Consumes: Task 1 helpers (`record_declared_type_fact`, `TypeNameRules`, `strip_type_decorations`).
- Produces: C# `TypeNameRules` const (nullable `?`, prefixes `ref`/`out`/`in`/`scoped`, generic `<`); every C# local/field/property/constant with a syntactically stated type gets a `TypeInfo` with `is_inferred=false`; `var x = new Foo(...)` locals get `resolved_type="Foo"` with `is_inferred=true`.

**Contract inputs:** Ground truth measured 2026-09-01 on `Miller.Core/Graph/{SymbolGraph,GraphTraversal}.cs`: 30/215 variables untyped (all `new`-initializers), field facts truncated at whitespace inside generics (`IReadOnlyDictionary<string,`), all rows `is_inferred=1`.

**File ownership:** Copy of contract row — `crates/julie-extractors/src/csharp/**`, `crates/julie-extractors/src/tests/csharp/**`, `fixtures/extraction/csharp/**`, `fixtures/extraction/razor/code-behind/expected.cs.json`.

**Serialization required:** Yes

**Dependency reason:** Task 3 edits the same csharp files; base helpers from Task 1.

**What to build:** Replace the whitespace-token type parsing for fields/properties/locals with tree-sitter node reads (the declaration nodes carry a `type` child), record declared types via the base helper (`is_inferred=false`), and add a `new`-expression pass for `var` locals (`object_creation_expression` → type node text → helper with `is_inferred=true`). Keep the legacy signature-parsing fallback for shapes the node path misses.

**Approach:** RED with unit tests over C# snippets asserting exact `resolved_type`, `is_inferred`, and `metadata.declared` for: explicit local, `var` + `new` generic (`var x = new Dictionary<string, int>()` → `Dictionary`, declared `Dictionary<string, int>`), nullable field (`GraphTraversal? _t` → `GraphTraversal`), generic field (truncation regression), property, constant. Then regenerate C# goldens with the xtask fixture updater and review diffs: only type_facts rows change in this task.

**Acceptance criteria:**
- [ ] The six unit cases above pass.
- [ ] No C# `resolved_type` contains whitespace or a trailing `<` fragment in regenerated goldens.
- [ ] Declared types carry `is_inferred=false`; `new`-derived carry `is_inferred=true`.
- [ ] `cargo xtask test language csharp` passes.
- [ ] Change committed (serial-worker-commit).

### Task 3: C# parameters + receiver metadata

**Files:**
- Create: `crates/julie-extractors/src/csharp/parameters.rs`
- Modify: `crates/julie-extractors/src/csharp/mod.rs`, `crates/julie-extractors/src/csharp/members.rs`, C# identifier/relationship emission (`crates/julie-extractors/src/csharp/` call-site modules)
- Test: `crates/julie-extractors/src/tests/csharp/` (extend `type_facts.rs`, new parameter tests), regenerate `fixtures/extraction/csharp/**/expected.json`, `fixtures/extraction/razor/code-behind/expected.cs.json`

**Interfaces:**
- Consumes: Task 1 helpers; Task 2's C# `TypeNameRules`.
- Produces: every C# method/constructor/local-function parameter becomes a symbol (kind `variable`, metadata `role="parameter"`, parent = the callable, signature = parameter text) with a declared-type fact; `this.`/`base.` call sites carry metadata `receiver_type` = enclosing type name on both the call identifier and the structured pending relationship.

**Contract inputs:** Global constraint on parameter symbol shape; policy v6 scope walk (parameters must be children of the callable).

**File ownership:** Same ownership as Task 2.

**Serialization required:** Yes

**Dependency reason:** Same-file dependency on Task 2's changes.

**What to build:** A parameter walker producing parameter symbols + type facts, wired into method/constructor extraction; `receiver_type` metadata emission at call sites whose receiver is `this` or `base`.

**Approach:** RED: a test asserting a method's parameters exist as `variable` symbols with `role="parameter"`, correct parent, and type facts (`string from` → `string`, `Func<GraphNeighbour, bool> edgeFilter` → `Func` with declared metadata). A second test asserts `this.Helper()` emits `receiver_type` on the identifier metadata and pending metadata. Regenerate goldens; verify `containing_symbol_id` shifts appear only on declaration-site rows.

**Acceptance criteria:**
- [ ] Parameter symbols with type facts appear for methods, constructors, and local functions.
- [ ] `this.`/`base.` call sites carry `receiver_type` metadata on identifier and pending rows.
- [ ] Golden diffs show no `containing_symbol_id` change on body call-site rows.
- [ ] `cargo xtask test language csharp` passes.
- [ ] Change committed (serial-worker-commit).

### Task 4: TypeScript + JavaScript

**Files:**
- Modify: `crates/julie-extractors/src/typescript/**`, `crates/julie-extractors/src/javascript/**`
- Test: language test modules + regenerate `fixtures/extraction/typescript/**`, `fixtures/extraction/javascript/**`

**Interfaces:**
- Consumes: Task 1 helpers.
- Produces: TS: typed locals (`const x: Foo`), `const x = new Foo()` (inferred), typed parameters as symbols with facts, class field/property annotations; JS: `new`-initializer locals and parameter symbols (untyped params get symbols without type facts); `this.` call sites carry `receiver_type`.

**Contract inputs:** Global constraints; TS `TypeNameRules` (generic `<`, no nullable suffix — union/optional types record base name of a single-identifier type only; skip unions, intersections, and inline object types entirely rather than guessing).

**File ownership:** Copy of contract row — `crates/julie-extractors/src/typescript/**`, `crates/julie-extractors/src/javascript/**`, their test modules, `fixtures/extraction/{typescript,javascript}/**`.

**Serialization required:** No

**Dependency reason:** None - safe parallel batch.

**What to build:** Same three fact shapes as C#, adapted to the two grammars; JS records only what syntax states (`new Foo()`).

**Approach:** TDD per shape; skip-not-guess for types the syntax does not state plainly. Regenerate both languages' goldens.

**Acceptance criteria:**
- [ ] TS typed locals/params/fields and `new`-locals carry correct facts; JS `new`-locals and parameter symbols exist.
- [ ] Union/intersection/inline-object annotations produce no type fact.
- [ ] `cargo xtask test language typescript` and `cargo xtask test language javascript` pass.
- [ ] Verified diff handed to the lead (parallel-lead-commit).

### Task 5: Python

**Files:**
- Modify: `crates/julie-extractors/src/python/**`
- Test: python test module + regenerate `fixtures/extraction/python/**`

**Interfaces:**
- Consumes: Task 1 helpers.
- Produces: annotated locals (`x: Foo = ...`), annotated parameters as symbols with facts (unannotated params get symbols only), `x = Foo()` locals when `Foo` is a class defined in the same file (inferred), annotated class attributes, `self.` call sites carry `receiver_type`.

**Contract inputs:** Global constraints; Python rules: strip `Optional[...]`/subscription to the base name only when the base is a plain identifier; skip string annotations and unions.

**File ownership:** Copy of contract row — `crates/julie-extractors/src/python/**`, python tests, `fixtures/extraction/python/**`.

**Serialization required:** No

**Dependency reason:** None - safe parallel batch.

**What to build:** The three fact shapes for Python's grammar, syntax-stated only.

**Approach:** TDD per shape; `self` parameter gets a symbol but no type fact (its receiver binding comes from `receiver_type` metadata instead).

**Acceptance criteria:**
- [ ] Annotated locals/params/attributes and same-file constructor-call locals carry facts; `self.` calls carry `receiver_type`.
- [ ] `cargo xtask test language python` passes.
- [ ] Verified diff handed to the lead (parallel-lead-commit).

### Task 6: Rust

**Files:**
- Modify: `crates/julie-extractors/src/rust/**`
- Test: rust test module + regenerate `fixtures/extraction/rust/**`

**Interfaces:**
- Consumes: Task 1 helpers.
- Produces: `let x: Foo` locals, `let x = Foo::new(...)` / `Foo { .. }` locals (inferred `Foo`), typed parameters as symbols with facts, struct fields with facts, `self.` call sites carry `receiver_type` = impl target type name.

**Contract inputs:** Global constraints; Rust rules: prefixes `&`, `&mut`, `*const`, `*mut`, `mut`; generic `<`; path types record the full path text as declared and the final segment as `resolved_type`.

**File ownership:** Copy of contract row — `crates/julie-extractors/src/rust/**`, rust tests, `fixtures/extraction/rust/**`.

**Serialization required:** No

**Dependency reason:** None - safe parallel batch.

**What to build:** The three fact shapes for Rust's grammar plus impl-aware `receiver_type` for `self`.

**Approach:** TDD per shape. `Foo::new(...)` initializer rule: record `Foo` (inferred) only when the call path's final segment is `new` or the initializer is a struct expression.

**Acceptance criteria:**
- [ ] The four production shapes above carry correct facts.
- [ ] `cargo xtask test language rust` passes.
- [ ] Verified diff handed to the lead (parallel-lead-commit).

### Task 7: Go

**Files:**
- Modify: `crates/julie-extractors/src/go/**`
- Test: go test module + regenerate `fixtures/extraction/go/**`

**Interfaces:**
- Consumes: Task 1 helpers.
- Produces: `var x Foo` locals, `x := Foo{...}` / `x := &Foo{...}` / `x := NewFoo(...)`-style locals only where the syntax names the type (composite literals yes, constructor calls no), typed parameters as symbols with facts, **method receivers as parameter symbols with facts** (`(s *Store)` → `s`: `Store`), struct fields with facts.

**Contract inputs:** Global constraints; Go rules: prefix `*`; generics use `[`— add `[` to that language's `generic_open` only if `strip_type_decorations` can distinguish it from array types by position; otherwise skip generic instantiations and record plain named types only.

**File ownership:** Copy of contract row — `crates/julie-extractors/src/go/**`, go tests, `fixtures/extraction/go/**`.

**Serialization required:** No

**Dependency reason:** None - safe parallel batch.

**What to build:** The three fact shapes plus receiver parameters — Go's `s.Method()` receiver binding is the highest-value case.

**Approach:** TDD per shape; the method-receiver test asserts the receiver symbol's fact binds `s` → `Store`.

**Acceptance criteria:**
- [ ] Receiver, parameter, local (declared + composite literal), and field facts pass.
- [ ] `cargo xtask test language go` passes.
- [ ] Verified diff handed to the lead (parallel-lead-commit).

### Task 8: Java

**Files:**
- Modify: `crates/julie-extractors/src/java/**`
- Test: java test module + regenerate `fixtures/extraction/java/**`

**Interfaces:**
- Consumes: Task 1 helpers.
- Produces: declared locals (`Foo x = ...`), `var x = new Foo(...)` locals (inferred), typed parameters as symbols with facts, fields with facts, `this.`/`super.` call sites carry `receiver_type`.

**Contract inputs:** Global constraints; Java rules: generic `<`, no nullable suffix, no reference prefixes.

**File ownership:** Copy of contract row — `crates/julie-extractors/src/java/**`, java tests, `fixtures/extraction/java/**`.

**Serialization required:** No

**Dependency reason:** None - safe parallel batch.

**What to build:** The three fact shapes plus `this`/`super` receiver metadata for Java's grammar.

**Approach:** TDD per shape; regenerate goldens.

**Acceptance criteria:**
- [ ] Local, parameter, field, and `new`-local facts pass; `this.` calls carry `receiver_type`.
- [ ] `cargo xtask test language java` passes.
- [ ] Verified diff handed to the lead (parallel-lead-commit).

### Task 9: Evidence scan

**Files:**
- Create: `docs/findings/2026-09-01-receiver-type-facts-evidence.md`

**Interfaces:**
- Consumes: the built `julie-extract` binary with Tasks 1–8 landed.
- Produces: the hard-gate evidence numbers Task 10 and the final report cite.

**Contract inputs:** Baseline (2026-09-01, pre-change, `Miller.Core/Graph/{SymbolGraph,GraphTraversal}.cs`): 215 variables / 185 typed; 30 `new`-initializer locals untyped; truncated generic field facts; 0 parameter symbols.

**File ownership:** Copy of contract row — `docs/findings/2026-09-01-receiver-type-facts-evidence.md`.

**Serialization required:** Yes

**Dependency reason:** Needs Tasks 2–8 landed to measure.

**What to build:** Re-run the scratch scan on the same two miller files plus one representative file per wave-1 language from any available corpus (miller repo for C#/TS/Python; this repo for Rust; skip a language when no local corpus exists and say so). Record per-language counts: parameter symbols, typed locals, typed fields, `receiver_type` rows.

**Approach:** `julie-extract scan` into a scratch SQLite, then SQL counts; write the findings doc with the exact queries.

**Acceptance criteria:**
- [ ] Hard gates pass: 0 untyped `new`-initializer locals in the C# sample, 0 whitespace/truncated `resolved_type` values, ≥1 parameter symbol with a type fact per measured language.
- [ ] Findings doc records queries, counts, and the static-internal caveat.
- [ ] Change committed (serial-worker-commit).

### Task 10: Closeout

**Files:**
- Modify: `fixtures/extraction/capabilities.json`, `docs/plans/2026-09-01-receiver-typed-call-resolution.md`, this plan's ledger section

**Interfaces:**
- Consumes: all prior tasks; the strict quality report.
- Produces: honest capability claims and debt entries; the spec doc reflects delivery state.

**Contract inputs:** Capability schema in `fixtures/extraction/capabilities.json`; wave-2 general-purpose languages needing `open_gaps` entries: kotlin, swift, c, cpp, ruby, php, dart, gdscript, vbnet, fsharp, razor, lua, elixir, erlang, r, zig, bash, powershell (each entry: concrete reason, required closure = the same three fact shapes, planned closure task = `docs/plans/2026-09-08-receiver-type-facts-wave-2.md`).

**File ownership:** Copy of contract row — `fixtures/extraction/capabilities.json`, `docs/plans/2026-09-01-receiver-typed-call-resolution.md`, this plan.

**Serialization required:** Yes

**Dependency reason:** Single owner of capabilities.json; needs all prior tasks.

**What to build:** Capability updates for wave-1 languages, `open_gaps` debt for wave-2 languages, spec status update (proposed → wave 1 landed, wave 2 planned), branch-gate run, ledger entries.

**Approach:** Follow the existing capabilities.json row shapes; run the full branch gate; fix anything it finds.

**Acceptance criteria:**
- [ ] `node scripts/language-data-quality-report.mjs --strict` passes with silent_cells=0, quality_bar_debts=0.
- [ ] Branch gate green: `cargo test --workspace`, `cargo xtask test capability`, fmt, diff check.
- [ ] `open_gaps` entries exist for every wave-2 general-purpose language with the named closure plan.
- [ ] Spec doc status updated.
- [ ] Change committed (serial-worker-commit).

## Verification Ledger

(appended during execution)
