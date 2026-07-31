# Task 6: Erlang relationships + pending relationships — report

**Status:** DONE (committed — serial-worker-commit)
**Worktree state:** path `/Users/murphy/source/julie-extractors/.worktrees/erlang-xml-language-support`,
branch `erlang-xml-language-support`, HEAD before work `86ace43`, working tree clean at start.

---

## 1. Worktree guard (step 0)

```
pwd    = /Users/murphy/source/julie-extractors/.worktrees/erlang-xml-language-support
branch = erlang-xml-language-support
HEAD   = 86ace43  (chore: checkpoint batch A milestone — after 517f04e)
status = clean
```

---

## 2. Miller calls and what each confirmed

Workspace `julie-extractors-91c17adbdab9` (MAIN checkout — erlang exists only in this worktree, so
erlang files were read raw; elixir/base were read through Miller).

| Call | Confirmed |
| --- | --- |
| `context(query="how the elixir extractor emits relationships and structured pending relationships", workspace_id=…)` | Pivots `get_structured_pending_relationships` (`base/extractor.rs:242`), `add_structured_pending_relationship` (`:233`), `ElixirExtractor::get_pending_relationships` (`elixir/mod.rs:135`), `StructuredPendingRelationship` (`base/relationship_resolution.rs:50`), `PendingRelationship` (`base/types.rs:443`), plus elixir's own pending tests. Established the two-vector model: a structured row is pushed and its degraded `pending` payload is mirrored automatically. |
| `inspect(target="StructuredPendingRelationship", depth="full")` | Exact field set: `pending`, `target: UnresolvedTarget`, `caller_scope_symbol_id`, `span`, `reference_site_is_exact`; constructors `new` / `with_context_span` / `with_target_span`. |
| `inspect(target="crates/julie-extractors/src/base/kinds.rs")` | `RelationshipKind` at `:226` — the closed vocabulary (`Calls Extends Implements Uses Returns Parameter Imports Instantiates References Defines Overrides Contains Joins Composition`). No kind was invented. |
| `search(query="structured pending relationship shape contract", mode="source")` | No hits — the gate is named `pending_shape_contract`, found by file listing instead (`tests/pending_shape_contract.rs`). Recorded because the brief asked for this call. |

Raw reads (worktree-only or needed verbatim): `erlang/{mod,helpers,identifiers,definition_forms}.rs`,
`elixir/relationships.rs`, `base/{relationship_resolution,creation_methods,kinds,extractor}.rs`,
`go/relationships.rs`, `c/relationships.rs`, `tests/{capability_matrix,golden,pending_shape_contract}.rs`,
`tests/elixir/cross_file_pending.rs`, `julie-extract-cli/src/resolution.rs`, `fixtures/extraction/capabilities.json`.

---

## 3. API-shape evidence

| Shape | Where proven |
| --- | --- |
| Resolved edge creation | `base/creation_methods.rs:130` `create_relationship`, `:162` `create_relationship_at_target` (sets `reference_site_is_exact = true`). |
| Pending creation | `base/creation_methods.rs:183` `create_pending_relationship` (context span), `:204` `create_pending_relationship_at_target` (target span + exact flag). |
| Structured pending required fields | `tests/pending_shape_contract.rs:33` — non-empty `target.terminal_name`, non-empty `target.display_name`, non-empty `caller_scope_key` when present, `pending.line_number > 0`, non-empty `pending.file_path`. |
| Degraded/structured parity | `base/extractor.rs:233` pushes both vectors; `tests/capability_matrix.rs:1993` `assert_fixture_pending_parity` asserts `pending_relationships == structured.map(.pending)` per fixture. |
| Elixir emit-vs-defer split | `elixir/relationships.rs`: `use` → `Uses`, `@behaviour` → `Implements`, `defimpl` → `Implements`, plain call → `Calls`; **each of them resolves in-file if a matching non-Import symbol exists, otherwise becomes a structured pending row**. Definition macros (`def`, `defmodule`, `import`, `alias`, `require`, …) are skipped entirely. |
| Matrix relationship-kind observation | `tests/capability_matrix.rs:1572-1583` — `kind_coverage.relationships` is observed from `relationships` + `pending_relationships` + `structured_pending_relationships[].pending`, so a pending-only kind is legitimate evidence. |
| Cross_file fixture layout | `fixtures/extraction/elixir/cross_file/` is **one** `source.ex` + `expected.json`; `capabilities.json` fixture rows carry exactly one `source` and one `expected` path. See §8.1. |

---

## 4. Grammar node kinds (verified on a real parse, scratch test deleted)

A scratch dump test (`tests/erlang/scratch_dump.rs`, written, run, **deleted** before commit) printed the
`tree-sitter-erlang` 0.20.0 tree for the attributes and calls this task switches on:

| Construct | Node kinds observed |
| --- | --- |
| `-behaviour(gen_server).` | `behaviour_attribute` → `atom` |
| `-include("bank_records.hrl").` | `pp_include` → `string` |
| `-include_lib("stdlib/include/assert.hrl").` | `pp_include_lib` → `string` |
| `-import(lists, [reverse/1]).` | `import_attribute` → `atom` (module) + `fa`* |
| `other_mod:handle(1)` | `remote` → `remote_module` (→ `atom` + `:`) + `call` (→ `atom` + `expr_args`) |
| `local(2)` | `call` → `atom` + `expr_args` |
| `?MODULE:helper(1)` | `remote` → `remote_module` → `macro_call_expr` (**no `atom`**) |

---

## 5. What was built

`crates/julie-extractors/src/erlang/relationships.rs` (new, ~330 lines) wired through
`ErlangExtractor::extract_relationships` plus two new accessors (`get_pending_relationships`,
`get_structured_pending_relationships`, mirroring `elixir/mod.rs:135,148`) and through `registry.rs` (§8.2).
No cross-file resolution logic: pending rows carry structure only.

### Emitted model

| Source form | Edge | Confidence |
| --- | --- | --- |
| `helper(X)` where `helper/1` is defined in the same file | **resolved** `Calls`, caller function → callee function, anchored on the callee atom (`reference_site_is_exact = true`) | 0.9 |
| `ledger:record(E, A)` | pending `Calls`, `display_name "ledger:record"`, `terminal_name "record"`, `namespace_path ["ledger"]` | 0.7 |
| `reverse(X)` under `-import(lists, [reverse/1])` | pending `Calls`, `display_name "reverse"` (as written), `namespace_path ["lists"]`, `import_context "import"` | 0.7 |
| `-behaviour(gen_server)` | pending `Implements` from the module symbol | 0.9 |
| `-include("x.hrl")` | pending `Imports`, `terminal_name "x.hrl"`, `import_context "include"` | 0.9 |
| `-include_lib("stdlib/include/assert.hrl")` | pending `Imports`, `terminal_name "assert.hrl"`, `namespace_path ["stdlib","include"]`, `import_context "include_lib"` | 0.9 |
| `-import(lists, [...])` | pending `Imports` from the module symbol, target `lists`, `import_context "import"` | 0.9 |
| `length(X)` / `self()` (auto-imported BIF) | *(nothing)* | — |
| `Fun(X)` (dynamic call) | *(nothing)* | — |
| `?MODULE:helper(X)` | *(nothing — no spelled module atom)* | — |
| `fun helper/1`, `fun lists:reverse/1` | *(nothing — a fun reference names a value)* | — |
| `-spec` / `-type` / `-opaque` / `-callback` / record field types | *(nothing — walk never enters them)* | — |

Every pending row carries `caller_scope_symbol_id` (= the `from_symbol_id`) and an exact target span
(`create_pending_relationship_at_target`), so a pending row's span is byte-identical to the identifier row
the identifier tier emits at the same site — the join key documented at
`base/relationship_resolution.rs:7-11`.

Scope resolution: a `fun_decl` resolves its scope through the shared `(name, arity)` identity, not span
containment, so clauses 2+ of a multi-clause function bind to the same symbol
(`later_clauses_bind_call_edges_to_the_same_function_symbol`). A `pp_define` body binds to the macro's
`Constant` symbol (`macro_body_calls_bind_to_the_macro_symbol`).

---

## 6. Judgment calls (plan-consistent, each with file:line + reason)

1. **`-behaviour` is ALWAYS pending, never resolved** (`erlang/relationships.rs`, `emit_behaviour`).
   The task text describes the behaviour edge under "RESOLVED (same-file)". Elixir resolves in-file because
   one `.ex` file can define many modules; a `.erl` file declares exactly **one** module, so a behaviour
   target is always in another file. Matching the behaviour name against a same-file *function* or *type*
   symbol (the only same-file candidates) would invent an edge the source never declared — precisely the
   wrong-edge class the negative fixture exists to forbid. Locked by
   `a_call_named_like_the_behaviour_does_not_resolve_to_the_behaviour_target`. The **resolved** arm of the
   cross_file golden is therefore the same-file call edge (`settle/2 → retry/2`), which still satisfies
   checklist §5 ("BOTH resolved and structured pending shapes").

2. **Remote calls use `namespace_path`, not `receiver`** (`emit_remote_call`).
   `julie-extract-cli/src/resolution.rs:220-224` documents `receiver_qualifier` as *"the dotted
   qualification standing in front of `receiver`"*, computed from `target_namespace_json`. Setting both
   `receiver = Some("ledger")` and `namespace_path = ["ledger"]` would therefore read as `ledger.ledger:record`
   — factually wrong. Elixir (the binding model) encodes a module-qualified call as `namespace_path` with
   `receiver = None` (`elixir/relationships.rs:254` `unresolved_elixir_alias`), and an Erlang module *is* a
   namespace rather than a value receiver, so the elixir shape was copied. Go's `receiver` shape
   (`go/relationships.rs:187`) was deliberately not copied. Side benefit: `applicable_tiers`
   (`resolution.rs:609`) restricts receiver-bearing pending calls to the Receiver/StaticType tiers, which
   Erlang (no type tier) cannot feed; the namespace shape keeps Import/Global available.
   The plan text (`…-plan.md:216`) asks for "terminal name/namespace/import context" and does not name
   `receiver`, so this is plan-consistent.

3. **An unqualified call that resolves to neither a same-file function nor an `-import` emits nothing**
   (`emit_local_call`). In Erlang an unqualified call is a local function, an imported function, or an
   auto-imported BIF. Emitting a pending row for `length/1` would ask a resolver to bind it to whatever
   workspace function shares the name. Consistent with Task 4 judgment 3 (BIFs get no module identifier).
   Asserted by `auto_imported_bif_call_emits_no_edge_and_no_pending`. Cost: a call to a function defined in
   an `-include`d header also emits nothing; noted as a known limitation, not a silent one.

4. **Local calls match on name AND arity** (`function_index` / `symbol_arity`). Erlang identity is
   name/arity; `helper/1` and `helper/2` are different functions with different symbols. Arity comes from
   the `arity` metadata the symbol tier already writes (`definition_forms.rs:51`). Asserted by
   `call_arity_selects_between_same_named_functions`.

5. **`import_context` carries the attribute keyword** (`"include"` / `"include_lib"` / `"import"`), not the
   full attribute source. Both precedents exist (`rust/relationships.rs:130` stores the whole `use` text;
   `bash/commands.rs:69` stores a categorising token). The keyword is the resolution-relevant fact:
   `-include_lib` resolves through an application's lib directory while `-include` resolves against the
   include path, and `"import"` marks a call whose module was **inferred** from an attribute rather than
   spelled at the call site. The module itself is already in `namespace_path`, so storing the attribute text
   would only duplicate it.

6. **`-import` emits a module-level `Imports` edge** (`emit_import`). The task made this conditional on
   elixir precedent. Elixir skips `import`/`alias`/`require` in its *call* dispatch, but it does emit a
   module-level edge for the analogous module-naming attribute `use Mod` → `Uses`/pending-`Uses` from the
   module symbol (`elixir/relationships.rs:95`). `-import(lists, …)` is the same shape: a module-level
   attribute naming another module. `Imports` is the honest kind from the fixed vocabulary.

7. **Macro invocations (`?LOG(X)`) produce no call edge.** The plan says to mirror elixir's choices for
   same-file calls; a macro is not a function and expansion is not a call. Its *identifier* row already
   exists from Task 4. Deferred, not forgotten.

8. **Fun references (`fun helper/1`, `fun lists:reverse/1`) produce no edge.** They name a value; Task 4
   already records them as `VariableRef` identifiers. Elixir has no capture arm to copy and the plan does
   not list them. Asserted by `fun_references_emit_no_call_edges`.

9. **A header (`.hrl`) drops attribute-anchored edges.** No `-module` attribute means no module symbol to
   anchor `-behaviour`/`-include`/`-import` on, and synthesising a `file:`-style id (the C approach,
   `c/relationships.rs:241`) would put a non-symbol key in `from_symbol_id`, which the golden normaliser
   would render as an unresolved key. Macro-body calls in a header still emit, because the macro's own
   symbol is a valid anchor. Locked by
   `header_attributes_emit_no_module_anchored_edges_but_macro_bodies_still_do`.

---

## 7. Fixtures (hand-reviewed row by row, not just regenerated)

### `erlang/cross_file` (new)

`ledger_client` is the A side of an A/B pair; every target except `retry/2` lives in another file.
8 rows, all hand-checked:

```
resolved  calls    L28  settle:26 -> retry:30                    exact=true conf=0.900
pending   implements L17 from ledger_client   gen_server                                   conf=0.900
pending   imports  L19  from ledger_client   ledger_records.hrl        ctx=include         conf=0.900
pending   imports  L20  from ledger_client   stdlib/include/assert.hrl ns=[stdlib,include] ctx=include_lib
pending   imports  L21  from ledger_client   lists                     ctx=import          conf=0.900
pending   calls    L27  from settle          ledger:record  term=record ns=[ledger]        conf=0.700
pending   calls    L34  from replay          reverse        term=reverse ns=[lists] ctx=import
pending   calls    L37  from replay          ledger:flush   term=flush  ns=[ledger]        conf=0.700
```

Negative controls that emit **nothing**: `length(Ordered)` (BIF, L35), `Fun(Ordered)` (dynamic call, L36),
and the `-spec settle(term()) -> {ok, integer()}` signature (L25). `types`, `literals`, `structural_facts`,
`complexity_metrics`, `parse_diagnostics` stay empty.

### `erlang/basic` (regenerated, source unchanged)

5 new rows, no other field changed (the diff is additions only):

```
resolved  calls    L50  balance_test:49 -> balance:28
pending   calls    L13  from LOG (macro body)  io:format   ns=[io]
pending   implements L6 from bank              gen_server
pending   imports  L10  from bank              lists       ctx=import
pending   calls    L43  from history           reverse     ns=[lists] ctx=import
```

`self()` (L47), `?LOG(Acct)` (L38), `fun balance/1` (L45) and `fun erlang:length/1` (L46) emit no edges.

### `erlang/negative` (regenerated; header comment rewritten)

The comment claimed "no relationship or pending row is emitted at all" — false after this task. It was
rewritten to the real duty: the tiers emit the **right** rows and no wrong ones. 7 rows:

```
resolved  calls    L20  run:17 -> queue:25
resolved  calls    L23  run:17 -> queue:25
pending   implements L7 from negative  gen_server
pending   imports  L9  from negative   stdlib/include/assert.hrl  ns=[stdlib,include] ctx=include_lib
pending   calls    L18 from run        erlang:unique_integer      ns=[erlang]
pending   calls    L19 from run        timer:sleep                ns=[timer]
pending   calls    L23 from run        lists:reverse              ns=[lists]
```

`length(...)` (BIF, L20), `Fun(Request)` (dynamic, L21) and `fun queue/1` (L22) still emit nothing — the
fixture's original duty is intact and now covers relationships too.

---

## 8. Plan mismatches (LEAD ACTION)

### 8.1 The cross_file golden is single-file, not two files

The task brief says "multi-file golden … two modules where A implements a behaviour, calls B remotely, and
includes a header" and points at `fixtures/extraction/elixir/cross_file/` for the multi-source layout.
**That layout does not exist.** A `capabilities.json` fixture row carries exactly one `source` and one
`expected` path (`tests/capability_matrix.rs:24-28` `FixtureRow`), and `elixir/cross_file/` is a single
`source.ex`. Erlang also forbids two `-module` attributes in one file, so a two-module single file would be
invalid Erlang and a dishonest fixture. Resolved plan-consistently: one module A whose behaviour, remote
callee, header, and import all live in other files — which is exactly what the pending rows are *for*. The
plan's Task 6 file list (`…-plan.md:211`) says "Create: `fixtures/extraction/erlang/cross_file/*`" and does
not itself require two sources; only the dispatch brief did.

### 8.2 `crates/julie-extractors/src/registry.rs` — required, not optional (same as Task 4 §8.1)

`extract_erlang` hardcoded `relationships/pending_relationships/structured_pending_relationships` to
`Vec::new()`. No golden can show a relationship row without wiring it. The dispatch brief anticipated this
("wire relationships/pending for erlang if the dispatch fn hardcodes empty vectors — Task 4 §8.1
precedent"), so it is in-bounds. The stale doc comment ("ships the symbol tier only") was corrected to name
only the type tier. Nothing else in the file was touched.

### 8.3 `kind_coverage.structural_facts` open gap re-pointed away from Task 6

The `erlang.behaviour_declaration` structural-facts gap named **Task 6** as its closure task. Task 6 ships
no structural facts (the plan's Task 6 scope is relationships/pending only), so leaving the pointer would
make a shipped task look like it owed work. Re-pointed to **Task 8: Erlang real-world corpus gate**.
*This is a recommendation, not a decision* — the lead should confirm Task 8 is the right owner. Same class
of correction as Task 4 §8.3. No status/capability/evidence value changed, and only `kind_coverage` gaps are
affected (which the matrix validates for non-empty fields only, not against the plan body).

### 8.4 `language_spec/specs.rs` — `PENDING_NO_TYPES_CAPABILITIES` matched exactly

The dispatch brief asked whether an existing const fits. `PENDING_NO_TYPES_CAPABILITIES`
(`language_spec/mod.rs:118`) is `symbols + relationships + pending + identifiers, types = false` — exactly
erlang's post-Task-6 tier. No new const was added. `DATA_ONLY_CAPABILITIES` still has one user (xml,
`specs.rs:304`), so nothing was orphaned and no dead-code warning appeared.

### 8.5 Migration-plan doc NOT touched

`docs/plans/2026-05-31-julie-code-migration-implementation-plan.md` was left alone. The only remaining
erlang `capability_gaps` row (`types`) still points at "Task 13: Erlang Capability Closure", which still
appears in the plan body, so `capability_matrix_open_rows_have_planned_closure_task` passes unchanged. No
gate forced a doc edit, so per the brief's instruction the checkboxes were left untouched.

### 8.6 No CLI contract impact

Raising erlang's spec const changes `capability_snapshot()` output. `cargo test -p julie-extract-cli` was
run to check: **279 tests, 0 failed** — including `operations_contract`. No CLI file was touched.

---

## 9. Files changed

Created:
- `crates/julie-extractors/src/erlang/relationships.rs`
- `crates/julie-extractors/src/tests/erlang/relationships.rs`
- `fixtures/extraction/erlang/cross_file/source.erl`
- `fixtures/extraction/erlang/cross_file/expected.json`

Modified (assigned):
- `crates/julie-extractors/src/erlang/mod.rs` (`mod relationships;`, real `extract_relationships`, two pending accessors)
- `crates/julie-extractors/src/erlang/identifiers.rs` (`imported_functions` + `ImportedFunctions` raised to `pub(super)` so the relationship layer reuses the one `-import` parser instead of duplicating it)
- `crates/julie-extractors/src/tests/erlang/mod.rs` (`mod relationships;` + `extract_with_relationships` / `extract_from_with_relationships` / `pending_named` / `pending_inventory` / `relationship_inventory` helpers)
- `crates/julie-extractors/src/language_spec/specs.rs` (capability ratchet only)
- `fixtures/extraction/capabilities.json` (erlang row only)
- `fixtures/extraction/erlang/basic/expected.json`
- `fixtures/extraction/erlang/negative/{source.erl,expected.json}`

Modified (forced, outside the assigned list — §8.2):
- `crates/julie-extractors/src/registry.rs`

**Not touched:** any xml file, `crates/julie-extract-cli/**`, `base/**`, `tests/capability_matrix.rs`,
`docs/plans/**`.

---

## 10. Capability matrix row (honesty audit)

- `capabilities.relationships`: `false` → **`true`**; `capabilities.pending_relationships`: `false` → **`true`**.
- Both matching `capability_gaps` rows removed (an implemented capability with a recorded gap is the
  dishonest state the matrix guards against). `types` remains the single open gap.
- `kind_coverage.relationships`: `supported = ["calls", "implements", "imports"]`, `open_gaps = []`.
  Each of the three is emitted by a registered golden: `calls` resolved (basic/negative/cross_file) and
  pending (all three), `implements` pending (all three), `imports` pending (all three). No kind is claimed
  that a golden does not emit, and none is classified twice —
  `capability_matrix_supported_kind_claims_have_fixture_evidence` agrees.
- `cross_file` fixture registered, satisfying `capability_matrix_negative_cases_emit_no_wrong_edges` and
  `capability_matrix_requires_relationship_fixture_evidence`.

---

## 11. Verification

| Command | Result |
| --- | --- |
| `RUSTUP_TOOLCHAIN=1.97.1 cargo xtask test language erlang` | **67 passed, 0 failed** (49 pre-existing + 18 new) |
| `RUSTUP_TOOLCHAIN=1.97.1 cargo xtask test golden` | **3 passed, 0 failed** (basic, negative, cross_file regenerated + hand-reviewed) |
| `RUSTUP_TOOLCHAIN=1.97.1 cargo xtask test capability` | **39 + 1 passed, 0 failed** (includes `pending_shape_contract`) |
| `RUSTUP_TOOLCHAIN=1.97.1 cargo xtask test changed crates/julie-extractors/src/language_spec/specs.rs fixtures/extraction/capabilities.json` | exit 0 — full `julie-extractors` suite (3124 tests), capability matrix, pending-shape gate, parser-upgrade gate |
| `RUSTUP_TOOLCHAIN=1.97.1 cargo test -p julie-extract-cli` | **279 passed, 0 failed** (checked because the spec const feeds `capability_snapshot`) |
| `RUSTUP_TOOLCHAIN=1.97.1 cargo clippy -p julie-extractors --all-targets` | 0 warnings, 0 errors |

Scratch artefacts: `crates/julie-extractors/src/tests/erlang/scratch_dump.rs` was created to dump real parse
trees (§4) and **deleted** before commit; its `mod` line was removed from `tests/erlang/mod.rs`.

---

## 12. Concerns for the lead

1. **§8.3 re-pointing needs confirmation** — the `erlang.behaviour_declaration` structural-facts gap now
   names Task 8. If structural facts belong to a different task (or to none in this plan), correct the
   pointer.
2. **§8.1 single-file cross_file** — if the lead genuinely wants a two-source cross-file golden, the golden
   harness needs a multi-source fixture row first; that is a `tests/golden.rs` + `capabilities.json` schema
   change, outside this task's ownership.
3. **Calls into `-include`d functions emit nothing** (§6.3). Closing that needs header resolution, which is
   the CLI resolver's job, not the extractor's. Recorded here rather than in the matrix because it is a
   recall limitation of an implemented capability, not an unimplemented kind.
4. **Task 7 (types) will want `-spec`/`-type` walking.** This task deliberately leaves the two type-bearing
   declaration kinds unvisited (§5); Task 7 should add a separate type-tier walk rather than lifting the
   relationship walk's `fun_decl`/`pp_define` restriction, which exists to stop `integer()` in a `-spec`
   from being read as a call.
