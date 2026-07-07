# variable_ref Identifier Emission Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use razorback:subagent-driven-development when subagent delegation is available. Fall back to razorback:executing-plans for single-task, tightly-sequential, or no-delegation runs.

**Goal:** Emit `variable_ref` identifiers for bare name reads — including the receiver/object of a member access — across every general-purpose language, so a symbol referenced by a plain read (`return VisibilityUnknown;`) or a static-access qualifier (`GraphTraversal.Reach()`) appears in the `identifiers` table by name.

**Architecture:** Each language extractor already emits `call` / `member_access` / `type_usage` identifiers from `src/<lang>/identifiers.rs`. This plan adds a `variable_ref` arm to that same per-language dispatch, following the pattern the `r` and `qml` extractors already ship. Prior art beyond r/qml (verified 2026-07-07, corrected after Codex review): **yaml** emits `variable_ref` for alias references (`*name`, `src/yaml/mod.rs`). **bash** (CORRECTED by Batch F probe 2026-07-07): plain `$NAME`/`${NAME}` emitted NOTHING pre-lane — the `mod.rs` member_access handling covers only array-subscript names; Batch F added an additive `simple_expansion|expansion` → `variable_ref` arm (commit on the lane branch). **powershell does NOT emit `variable_ref`** — its `variables.rs::extract_variable_reference` creates *Symbols* (`SymbolKind::Variable`) for automatic/environment variables only and skips regular variable references entirely; do not mistake that function name for identifier emission. PowerShell is a normal greenfield rollout task in `src/powershell/identifiers.rs`. No schema change, no resolver change — the new rows land in the existing `identifiers` table and are consumed downstream by name-match (verified: Miller's `DeadCodeCandidateReader` name-count query has **no kind filter**, uses BINARY/case-sensitive comparison, and is backed by `idx_identifiers_name_kind`). Risk-first: C# is the reference implementation that locks the cross-language semantic contract; the other languages replicate it in parallel; a final task verifies coverage, updates capability evidence, and bumps the extractor version.

**Tech Stack:** Rust, tree-sitter, `cargo` / `cargo xtask`, golden-fixture JSON (`fixtures/extraction/<lang>/**/expected.json`), `fixtures/extraction/capabilities.json`, `scripts/language-data-quality-report.mjs`.

**Architecture Quality:** The change is localized to each `src/<lang>/identifiers.rs` `extract_identifier_from_node` dispatch, mirroring the existing `call`/`member_access`/`type_usage` arms and the shipped `r`/`qml` `variable_ref` arms. No new module, interface, or schema seam. The one architecture risk is **over- and under-emission**: emitting for declarations/keywords/write-targets (false rows) or flooding the table with locals (perf). The contract's exclusion rules + per-language golden fixtures + the performance gate contain both. No public contract changes; `variable_ref` is already a defined `IdentifierKind` and a valid `identifiers.kind` value.

## Global Constraints

These bind every task. Copy verbatim into every worker prompt.

**Why this exists (load-bearing consumer requirement).** Miller's dead-code candidate reader decides name-liveness by whether any row in `identifiers` has `name = S.name` outside `S`'s own definition. Today C# (and most other languages) emit **no** identifier for a bare read (`return VisibilityUnknown;`) or for the receiver of a static access (`GraphTraversal` in `GraphTraversal.Reach()`), so live symbols are falsely flagged as dead. **Evidence framing (be precise):** the **328 candidates → ~28** result (`/Users/murphy/source/miller/docs/findings/2026-07-07-dead-code-candidates-dogfood.md`, "LEAD ADDENDUM") was measured with **Miller-side rescue signals** — `pending_relationships` receiver/terminal matching (65 rescued) + a same-file lexical code-occurrence scan (235 rescued) — NOT with a `variable_ref` prototype. `variable_ref` emission is expected to reproduce both rescue classes structurally (receivers via rule 1's member-access arm; bare reads via the value-read arm, and cross-file rather than same-file-only), but that equivalence is an inference until the Downstream step 1 gate re-run proves it. **If the re-run residual materially exceeds ~28, classify which rescue class the emission missed (write-target forms? comment-only mentions? a receiver shape?) before touching Miller-side signals.** This is the acceptance driver; the gate re-run in the Downstream section is the success criterion.

**The `variable_ref` emission contract.** Emit `IdentifierKind::VariableRef` (serialized `variable_ref`) for a name node **N** when ALL hold:
1. **Read in value or receiver position** — N is a reference to a binding used as an expression, operand, argument, initializer, return value, collection element, OR **the object/receiver of a member access** (`X` in `X.Y` / `X.Y()` / `X::Y`), OR a **member-reference LHS in an initializer/named-argument context** — the member name in an object/record initializer (`Bar` in `new Foo { Bar = 5 }`), an attribute/annotation named argument (`Bar` in `[Foo(Bar = 1)]`), and equivalent per-language constructs (Python keyword-arg names are parameter refs, NOT this — skip those). These are syntactically distinguishable member references, not local write targets; without them, an internal property set only via initializers is falsely flagged dead (its only textual reference is one the old same-file lexical scan caught and pure read-emission would miss).
2. **Not already emitted by another arm** — N is not a call callee (`Call`), not the accessed `.name` of a member access (`MemberAccess`/`Call`), not a type usage (`TypeUsage`). The `variable_ref` arm is the *complement* of the arms already in that file.
3. **Not a declaration name** — N is not the defining identifier of a type/method/property/field/enum-member/parameter/local declaration, a label, or an import/using-alias LHS.
4. **Not a write-only target** — N is not the LHS of a **plain** assignment (`x = 5`). A **compound assignment** (`x += 1`, `x ||= y`) IS a read — emit. An `out`/`ref`-style argument slot MAY emit as a read (as-built C# reference behavior: any non-label argument child is a read) — emitting is the safe direction and bare write-slot targets are locals, which are never dead-code candidates; do not add per-language complexity to exclude them. Initializer/named-argument member LHS is a read per rule 1 — emit. Liveness counts *reads*; a binding only ever plain-written is not "used." **Accepted residual (document, don't fight):** a non-local written only via bare same-class assignment (`Bar = 5;` where `Bar` is a property) is grammatically indistinguishable from a local write without scope analysis and will NOT emit; a write-only property/constant that surfaces as a candidate is classified a **true dead-ish find** at the gate (see gate classification note), not a false positive.
5. **Not a keyword/builtin** — reuse each language's existing builtin/keyword filter (the same one the `type_usage` arm uses, e.g. C#'s `is_csharp_builtin_type`); do not emit `true`, `null`, `this`, `base`, contextual keywords, or builtin type names.
6. **`containing_symbol_id`** is set via the existing byte-range containment helper (`find_containing_symbol[_from_map]`), exactly as the sibling arms do.

**Non-goals (do NOT do these):**
- Do **not** make `variable_ref` resolvable — `ReferenceKind::from_identifier_kind` in `crates/julie-extract-cli/src/resolution.rs` stays `call`/`type_usage`/`member_access` → the new rows are consumed by name-match only, not the resolver tier chain. (A resolvable `ValueRef` kind is a possible future enhancement, explicitly out of scope here.)
- Do **not** change the `identifiers` table schema, the `IdentifierKind` enum, or `sqlite_schema_version` — `variable_ref` is already a valid kind. This is a **data** change, not a contract change, so it is a **minor** version bump.
- Do **not** touch `fixtures/extraction/capabilities.json` from a per-language rollout task — it is a single shared file; the final verification task owns it (prevents parallel merge conflicts).

**Coverage / parity rule (per the repo Data Quality Bar).** Every general-purpose language gains `variable_ref` where its grammar has name reads. Data / markup / query / pattern languages are **assessed, not skipped**: mark `not_applicable` **only** when the language genuinely lacks variable references (with a concrete reason), otherwise record `open_gaps` debt with a closure task. The assessment universe is **every language directory in `src/`**, not just those with an `identifiers.rs` — Batch F additionally assesses **bash** (already name-visible: `$var` expansions emit `member_access` from `mod.rs`; record that determination, optionally normalize kind), **yaml** (already ships alias `variable_ref`; record it), and **json / toml / markdown** (`identifiers: false` in capabilities today; expect `not_applicable` with reasons). No silent gaps — the final task keeps `scripts/language-data-quality-report.mjs --strict` at `silent_cells=0` / `quality_bar_debts=0`.

**Serialization / kind string:** `variable_ref` (snake_case), identical to the rows `r`/`qml` already emit — verify against a regenerated `r` fixture, do not hand-write the string.

---

## Verification Strategy

**Project source of truth:** `/Users/murphy/source/julie-extractors/CLAUDE.md` (Test Discipline, Data Quality Bar) and `xtask` (`xtask/src/commands.rs`: `test`, `dogfood`, `performance`, `release`).

**Worker red/green scope:** the narrowest per-language command — `cargo test -p julie-extractors <lang>` (e.g. `cargo test -p julie-extractors csharp`) plus that language's golden-fixture comparison. Per the Test Discipline rule, per-language work MUST test one language without paying for all languages.

**Worker ceiling:** a worker may run its own language's unit + fixture tests and regenerate/verify its own `fixtures/extraction/<lang>/**`. Workers do NOT run or own the cross-language `dogfood`, `performance`, the `--strict` data-quality report, or the release build — those are lead/final-task gates.

**Worker gate invariant:** each rollout task proves, via a regenerated golden fixture it hand-verifies, that (a) every genuine bare read and member-access receiver in the fixture now carries a `variable_ref` row, and (b) NO declaration, keyword/builtin, write-target, or already-captured call/member/type node gained a spurious `variable_ref`. A green fixture with wrong rows is NOT acceptance evidence — the diff must be read.

**Lead affected-change scope:** after each parallel batch lands, the lead runs the full `cargo test` (all languages) and the dogfood gate to confirm no cross-language regression. **Exact invocations (the bare `cargo xtask dogfood` / `cargo xtask performance` forms exit with usage errors):**
- `cargo xtask dogfood repo --root . --out-dir target/dogfood/julie-extractors`
- `cargo xtask performance baseline --root /Users/murphy/source/miller --out-dir target/performance/variable-ref-miller --binary target/release/julie-extract --runs 3`

**Branch gate (final task):** `cargo build --release` (0 warnings), full `cargo test`, `cargo xtask dogfood repo --root . --out-dir target/dogfood/julie-extractors`, `cargo xtask performance baseline --root /Users/murphy/source/miller --out-dir target/performance/variable-ref-miller --binary target/release/julie-extract --runs 3` (baseline/summary harness — no automatic budget; Task 8 records and judges the growth deltas per its verification section), and `node scripts/language-data-quality-report.mjs --strict` (0 debts). Per-language `variable_ref` presence proven by the coverage query below on a real extract.

**Replay/metric evidence — hard gates:** (1) `variable_ref` rows exist for every general-purpose language on a real extract; (2) `--strict` data-quality report at 0 debts; (3) the downstream Miller dead-code gate re-run shows **zero confirmed-live candidates** (the ultimate acceptance metric). **Report-only:** identifier-table row-count growth and extractor wall-clock delta (watched against the perf budget, but growth itself is expected, not a failure).

**Per-language coverage query (hard gate, run on a real extract in the final task):**
```sql
SELECT language, COUNT(*) FROM identifiers WHERE kind='variable_ref' GROUP BY language ORDER BY 1;
```
Every general-purpose language must appear with a non-trivial count; a general-purpose language emitting `0` is an `open_gaps` debt, not a pass.

**Escalation triggers:** a language whose grammar makes read/write-target or declaration/reference disambiguation infeasible without scope analysis → record `open_gaps` with the specific grammar limitation and continue; do not emit guessed rows. Identifier-row growth **>5×** the 2.9.0 baseline or scan wall-clock growth **>2×** on the Miller-repo extract → stop and report (candidate mitigations: exclude provably-local reads, drop `code_context` for `variable_ref` rows, or gate emission).

**Assigned verification failure:** workers stop and report when their assigned per-language gate fails; they do not weaken a fixture to make it green.

**Verification ledger:** record invariant, command, scope label, commit SHA, result, timestamp under `.razorback/sdd/progress.md`. Reuse a passing ledger entry for the same HEAD instead of rerunning an expensive gate.

---

## Parallel Execution Contract

| Task | Parallel batch | File ownership | Serialization required | Dependency reason |
|---|---|---|---|---|
| Task 1: C# reference impl + contract lock | None - serial | `crates/julie-extractors/src/csharp/identifiers.rs`; `fixtures/extraction/csharp/**` | Yes | Locks the cross-language `variable_ref` semantic contract + the fixture-regeneration procedure every rollout task consumes. |
| Task 2: Batch A — c, cpp, go, rust, zig | Batch A | `src/{c,cpp,go,rust,zig}/identifiers.rs`; `fixtures/extraction/{c,cpp,go,rust,zig}/**` | No | None - safe parallel batch (disjoint per-language files; contract fixed by Task 1). |
| Task 3: Batch B — java, kotlin, scala, swift, dart | Batch B | `src/{java,kotlin,scala,swift,dart}/identifiers.rs`; `fixtures/extraction/{java,kotlin,scala,swift,dart}/**` | No | None - safe parallel batch. |
| Task 4: Batch C — javascript, typescript, vue, razor | Batch C | `src/{javascript,typescript,vue,razor}/identifiers.rs`; `fixtures/extraction/{javascript,typescript,vue,razor}/**` | No | None - safe parallel batch. |
| Task 5: Batch D — python, ruby, php, lua, elixir | Batch D | `src/{python,ruby,php,lua,elixir}/identifiers.rs`; `fixtures/extraction/{python,ruby,php,lua,elixir}/**` | No | None - safe parallel batch. |
| Task 6: Batch E — gdscript, vbnet, powershell | Batch E | `src/{gdscript,vbnet,powershell}/identifiers.rs`; `fixtures/extraction/{gdscript,vbnet,powershell}/**` | No | None - safe parallel batch. |
| Task 7: Batch F — css, sql, html, regex + bash, yaml, json, toml, markdown (assess/N-A) | Batch F | `src/{css,sql,html,regex}/identifiers.rs`; `src/{bash,yaml}/mod.rs` (assessment only unless emission is added); `fixtures/extraction/{css,sql,html,regex,bash,yaml,json,toml,markdown}/**` | No | None - safe parallel batch. |
| Task 8: Cross-language verify + capabilities + version bump | None - serial | `fixtures/extraction/capabilities.json`; `crates/julie-extract-cli/Cargo.toml`; `docs/release-notes/**`, `docs/release-evidence/**`; `xtask` perf/dogfood expectations if they encode counts | Yes | Must run after all rollout batches; owns the shared `capabilities.json` and the version bump. |

Batches 2–7 (A–F) have fully disjoint file ownership and depend only on Task 1's locked contract, so they may all dispatch concurrently once Task 1 is approved. Group them into review-sized batches; the lead runs `affected-change` after each batch reports.

---

### Task 1: C# reference implementation + contract lock (risk-first)

**Files:**
- Modify: `crates/julie-extractors/src/csharp/identifiers.rs` (add the `variable_ref` arm + `is_csharp_value_read_identifier` helper alongside the existing `is_csharp_type_usage_identifier` / `is_csharp_declaration_name`)
- Modify/Create: `fixtures/extraction/csharp/basic/**` (+ a purpose-built fixture proving receiver + bare-read capture)
- Test: `crates/julie-extractors/src/csharp/` unit tests + the csharp golden fixtures

**Interfaces:**
- Consumes: the existing C# dispatch (`extract_identifier_from_node`, lines ~44–122), `is_csharp_declaration_name`, `is_csharp_builtin_type`, `contains_node`, `find_containing_symbol_id`; the `r`/`qml` `variable_ref` arms as the shipped template.
- Produces: (1) the **locked semantic contract** every rollout task follows — a concise doc comment in `identifiers.rs` restating the 6 emission rules + non-goals; (2) the **fixture-regeneration procedure** (the exact command/steps to regenerate and hand-verify `expected.json`), recorded in the Task 1 report as a contract input for Tasks 2–7.

**Contract inputs:** the Global Constraints emission contract (all 6 rules + non-goals) verbatim.

**File ownership:** `crates/julie-extractors/src/csharp/identifiers.rs`; `fixtures/extraction/csharp/**`

**Serialization required:** Yes

**Dependency reason:** Locks the cross-language `variable_ref` semantic contract + the fixture-regeneration procedure every rollout task consumes.

**Step 1 — Write the failing test.** Add a C# identifier test (or fixture) over a source that contains: a static-access call (`GraphTraversal.Reach()` → expect a `variable_ref` named `GraphTraversal` for the receiver, plus the existing `call` named `Reach`), a bare const read in a return/collection (`return VisibilityUnknown;` → expect `variable_ref` `VisibilityUnknown`), a method-group argument (`.Where(IsCSharpUserType)` → expect `variable_ref` `IsCSharpUserType`), an **object-initializer member** (`new Foo { Bar = 5 }` → expect a row named `Bar` per rule 1), an **attribute named argument** (`[Foo(Bar = 1)]` → expect a row named `Bar`), a **compound assignment** (`count += 1` → expect `variable_ref` `count` per rule 4), a **`nameof` operand** (`nameof(VisibilityUnknown)` → expect a row), and negative cases that must NOT gain a `variable_ref`: a declaration name, a parameter name, a **plain** assignment LHS local (`x = 5`), a builtin (`int`, `true`), and a name that appears **only in a comment** (no row — comments are not reads).

**Step 2 — Run it, verify it fails.** `cargo test -p julie-extractors csharp` → FAIL (no `variable_ref` rows emitted).

**Step 3 — Implement the `variable_ref` arm.** In `extract_identifier_from_node`, add handling so that:
- the **object/receiver** child of a `member_access_expression` (the field that is NOT `name`), when it is a bare `identifier` passing `is_csharp_value_read_identifier`, emits `IdentifierKind::VariableRef`;
- a bare `identifier` in value position (not a declaration name, not a param, not an assignment LHS target, not a builtin, not already captured as call/member/type) emits `IdentifierKind::VariableRef`.
Add `fn is_csharp_value_read_identifier(node) -> bool` mirroring the structure of `is_csharp_type_usage_identifier` but for value/receiver reads, reusing `is_csharp_declaration_name` and `is_csharp_builtin_type` for exclusions. Keep the walk single-pass; do not double-emit (guard against the node already handled by the call/member/type arms).

**Step 4 — Run tests, verify pass.** `cargo test -p julie-extractors csharp` → PASS. Regenerate the csharp golden fixtures and **read the diff**: confirm the added `variable_ref` rows are exactly the real reads/receivers and that no declaration/keyword/write-target/duplicate row appeared. Record the regeneration command in the report.

**Step 5 — Apply commit mode (`serial-worker-commit`).** Commit `crates/julie-extractors/src/csharp/identifiers.rs` + `fixtures/extraction/csharp/**` after the per-language gate passes. Commit body ends with `Claude-Session: https://claude.ai/code/session_011wAsc41pUFZpynGDGyxrrm`.

**Acceptance criteria:**
- [x] `GraphTraversal.Reach()` yields a `variable_ref` named `GraphTraversal` (receiver) + a `call` named `Reach`.
- [x] `return VisibilityUnknown;` and a method-group arg yield `variable_ref` rows by name.
- [x] Object-initializer members, attribute named args, compound-assignment targets, and `nameof` operands yield rows (rule 1/4 amendments).
- [x] Declaration names, parameter names, plain-assignment-LHS locals, builtins, and comment-only mentions get NO `variable_ref`.
- [x] No duplicate identifier rows; existing call/member/type rows unchanged.
- [x] The locked contract doc-comment + the fixture-regen procedure are recorded for rollout tasks.
- [x] `cargo test -p julie-extractors csharp` green; change committed.

---

### Tasks 2–7: Per-language rollout (Batches A–F)

**What to build (all rollout tasks):** apply the Task 1 `variable_ref` contract to each owned language's `src/<lang>/identifiers.rs`, add the complement `variable_ref` arm to that language's existing `extract_identifier_from_node` dispatch, regenerate + hand-verify that language's golden fixtures, and keep the per-language test green.

**Approach (all rollout tasks):**
- Read the language's existing `identifiers.rs` first: it already encodes that grammar's call / member-access / type-usage node kinds and its declaration/builtin filters. The `variable_ref` arm is the **complement** — the read/receiver positions those arms leave uncaptured.
- Follow Task 1's C# arm and the shipped `r`/`qml` arms as templates. Reuse the language's existing declaration-name and builtin filters for exclusions (rules 3 + 5). Emit for member-access receivers and initializer/named-arg member LHS (rule 1) and bare value reads including compound-assignment targets; skip plain-write targets (rule 4).
- **Batch E note (powershell):** powershell's `variables.rs::extract_variable_reference` emits **Symbols** (`SymbolKind::Variable`, automatic/env vars only) — NOT `variable_ref` identifiers; regular variable references are skipped there. Treat powershell as a normal greenfield `variable_ref` task in `src/powershell/identifiers.rs` (with `$var` reads, receivers, and the standard exclusion rules), and do not double-count the `variables.rs` symbol path as coverage.
- Regenerate the owned `fixtures/extraction/<lang>/**` via the Task 1 procedure and **read every diff** — the golden fixture is the acceptance evidence, so verify the new rows are real reads and nothing spurious appeared. Do NOT edit `fixtures/extraction/capabilities.json` (Task 8 owns it).
- Per-language grammar notes: derive the receiver/read node kinds from the language's tree-sitter grammar and its existing `identifiers.rs`; where a language's grammar cannot separate a read from a write-target or a declaration without scope analysis, record an `open_gaps` note with the specific limitation rather than emitting guessed rows (escalation trigger).

**Batch F special-case (css, sql, html, regex + bash, yaml, json, toml, markdown — data/markup/query/pattern/shell):** these are assessment tasks. Determine per language whether a "variable reference" concept genuinely exists: CSS custom-property references (`var(--x)`) plausibly warrant `variable_ref`; SQL column/table references are already `member_access`/type-like; HTML has none in the code-symbol sense; regex has none. **bash**: `$var` expansions already emit `member_access` identifiers from `mod.rs` (no `identifiers.rs` exists) — verify on a fixture, then EITHER record "covered via member_access" as the determination OR add a small `variable_ref` emission for consistency; do not build a full identifiers module for this plan. **yaml**: alias references (`*name`) already emit `variable_ref` — verify and record. **json/toml/markdown**: `identifiers: false` today — expect `not_applicable` with concrete reasons. For each, EITHER add domain-appropriate emission OR record `not_applicable` with a concrete reason (Task 8 folds these into `capabilities.json`). Do not emit meaningless rows to force parity — `not_applicable` with a reason is the correct outcome where the construct is absent.

**Files (per task):** `src/<owned-langs>/identifiers.rs`; `fixtures/extraction/<owned-langs>/**` (exact set per the Parallel Execution Contract row).

**Interfaces:**
- Consumes: Task 1's locked contract doc-comment + fixture-regen procedure; each language's existing `identifiers.rs` arms and filters.
- Produces: `variable_ref` rows for the owned languages; per-language `not_applicable`/`open_gaps` determinations handed to Task 8.

**File ownership / Serialization / Dependency reason:** copy the owning row from the Parallel Execution Contract. Serialization required: No. Dependency reason: `None - safe parallel batch.`

**Commit mode:** `parallel-lead-commit` when a batch is dispatched concurrently (workers hand verified diffs to the lead; the lead stages each language's owned files after inline review to avoid index races). If a batch is run alone, `serial-worker-commit` is acceptable.

**Acceptance criteria (per language in the task):**
- [ ] The language's real bare reads + member-access receivers now emit `variable_ref`; declarations/keywords/write-targets/duplicates do not.
- [ ] The owned `fixtures/extraction/<lang>/**` regenerated and the diff hand-verified (real rows only).
- [ ] `cargo test -p julie-extractors <lang>` green.
- [ ] Any genuine grammar gap recorded as `open_gaps`; any absent-construct language recorded as `not_applicable` with a reason (for Task 8).
- [ ] Owned files handed to the lead (parallel-lead-commit) or committed (serial).

---

### Task 8: Cross-language verification, capability evidence, version bump

**Files:**
- Modify: `fixtures/extraction/capabilities.json` (identifier `variable_ref` coverage rows for all languages; `not_applicable`/`open_gaps` from Batch F and any gaps)
- Modify: **all three release crate manifests** (`version = "2.9.0"` → `"2.10.0"`) — `crates/julie-extract-artifact/Cargo.toml`, `crates/julie-extract-cli/Cargo.toml`, `crates/julie-extractors/Cargo.toml` (release preflight's `RELEASE_CRATE_MANIFESTS` in `xtask/src/release.rs` checks all three; bumping only the CLI fails preflight late) — plus the refreshed `Cargo.lock`
- Create/Modify: `docs/release-notes/**` + `docs/release-evidence/**` (the `variable_ref` capability delta + coverage query evidence)
- Modify (only if they encode identifier counts): `xtask` dogfood/performance expectations

**Interfaces:**
- Consumes: every rollout task's per-language emission + `not_applicable`/`open_gaps` determinations.
- Produces: the 2.10.0 extractor with `variable_ref` coverage, verified capability evidence, and the release-note/evidence the Miller pin bump (Downstream) references.

**Contract inputs:** the Global Constraints coverage/parity rule + the per-language coverage query.

**File ownership:** `fixtures/extraction/capabilities.json`; `crates/julie-extract-cli/Cargo.toml`; `docs/release-notes/**`, `docs/release-evidence/**`; `xtask` count expectations.

**Serialization required:** Yes

**Dependency reason:** Runs after all rollout batches; owns the shared `capabilities.json` + version bump.

**What to build:** consolidate the rollout into a released capability. Run the branch-gate battery; update `capabilities.json` so every general-purpose language records `variable_ref` support (with fixture backing) and every absent-construct language records `not_applicable` with a reason; run the per-language coverage query on a real extract and paste the table into release evidence; bump to `2.10.0`; write the release note describing the new emission + the Miller dead-code driver.

**Verification (branch gate):**
- `cargo build --release` → 0 warnings.
- `cargo test` (all) + `cargo xtask dogfood repo --root . --out-dir target/dogfood/julie-extractors` → green.
- `cargo xtask performance baseline --root /Users/murphy/source/miller --out-dir target/performance/variable-ref-miller --binary target/release/julie-extract --runs 3` → green. **Honesty note (verified 2026-07-07): the performance harness generates baselines/summaries; it has NO enforced identifier-count budget.** Task 8 therefore records the before/after deltas explicitly and judges them: identifiers row count, artifact size, and scan wall-clock on a real Miller-repo extract (baseline at 2.9.0: **94,721 identifier rows** — 52,498 call / 26,271 member_access / 15,952 type_usage — **237 MB** artifact, ~4 m scan). Escalate (stop and report, don't ship) if identifier rows grow **>5×** or scan wall-clock grows **>2×**; otherwise report the delta as evidence.
- `node scripts/language-data-quality-report.mjs --strict` → `silent_cells=0`, `quality_bar_debts=0`.
- Coverage query shows a non-trivial `variable_ref` count for every general-purpose language.

**Acceptance criteria:**
- [x] `capabilities.json` reflects real per-language `variable_ref` support / `not_applicable` reasons; `--strict` report at 0 debts.
- [x] Per-language coverage query pasted into release evidence; no general-purpose language at 0.
- [x] `cargo build --release` 0 warnings; full `cargo test` + `dogfood` + `performance` green/in-budget.
- [x] All three crate manifests bumped to `2.10.0` + `Cargo.lock` refreshed; `cargo xtask release preflight --version 2.10.0` passes; release note + evidence written.
- [x] Change committed (`serial-worker-commit`).

---

## Release + Downstream Consumption (Miller) — success criterion, executed separately

This plan's tasks land the extractor capability. The **acceptance driver** — the Miller dead-code gate — is verified downstream, in the Miller repo on its own branch. Captured here so the arc is complete; **not** part of this plan's task table (different repo/branch/verification).

1. **Local end-to-end validation BEFORE any release (no approval needed).** From Miller's `feat/dead-code-candidates` branch, build against the source extractor:
   `MILLER_JULIE_SOURCE=/Users/murphy/source/julie-extractors scripts/restore-julie-extract.sh --from-source`, rebuild, and re-run the dead-code dogfood gate (`docs/plans/2026-07-07-dead-code-candidates-implementation-plan.md` Task 5). Expected: false positives collapse to roughly the ~28-candidate level the Miller-side rescue-signal measurement reached (NOT a proven prototype of this emission — see the evidence framing in Global Constraints; the residual should be `*ForTest` methods now visible cross-file + genuine dead code + framework overrides). **This step is the actual proof of equivalence: if the residual materially exceeds that level, classify which rescue class the emission missed (write-target forms, comment-only mentions, a receiver shape) and fix the emission BEFORE any release — do not proceed to step 2 on a diverging result.** Because `variable_ref` captures static-access receivers **and** bare reads, the previously-planned Miller-side "graph-receiver (signal A)" and same-file lexical scan become **unnecessary** — Miller consumes the richer identifiers through its existing name-match unchanged.
2. **Extractor release (APPROVAL-GATED — do not publish without explicit user approval).** Build + publish `julie-extract v2.10.0` (all four platform archives + `.sha256`), per `docs/release.md` / `xtask release`.
3. **Miller pin bump.** Update `/Users/murphy/source/miller/scripts/julie-pins.json` (`version` → `2.10.0` + the four real archive `sha256`s), re-run restore, rebuild (the `VerifyPinnedJulieExtractVersion` guard enforces the pin), and re-run the gate on a full-repo scan. **Hard gate:** zero confirmed-live candidates **after** the pre-agreed classification below — and the framework-override suppression must be resolved BEFORE this gate is scored, not argued afterward. **Gate classification rules (agreed up front so the re-run can't be argued after the fact):** (a) a candidate whose only out-of-definition references are **plain writes** (write-only property/constant) is a true dead-ish find, NOT a gate failure; (b) a candidate whose only "reference" is a **comment or doc mention** (the `rg -w` hand-verification sweep counts these) is a true find, NOT a gate failure — hand-verification must classify comment-only hits as non-references; (c) **framework-invoked overrides** (`BackgroundService.ExecuteAsync` etc.): if any surface in the step-1 local run, implement the small Miller-side override/entry-point suppression FIRST and re-run — the final gate is scored with that suppression in place, so overrides appearing in the scored run ARE gate failures, not an accepted residual.
4. **Finish the Miller feature.** Once the gate passes: Miller Task 4 (contract docs + boundary amendments) against the now-validated shapes, then `razorback:finishing-a-development-branch`. Update `docs/findings/2026-07-07-dead-code-candidates-dogfood.md` with the post-fix precision.

**Residual after this capability (known, acceptable):** framework-invoked overrides (`BackgroundService.ExecuteAsync`) have no by-name reference in any language; if they surface as candidates, clear them with a small Miller-side override/entry-point suppression, not more extraction. Genuinely-dead symbols the gate then surfaces (e.g. `RegionBackendMetadata`, `SearchBackendMetadata`, `UnknownWorkspaceIdNote`) are true positives — the feature working as intended.

**Known recall cost (accepted, precision-first):** flooding the table with local/parameter reads means any candidate symbol that shares an exact name with ANY read binding anywhere in the workspace is masked alive by name-match. C#'s casing conventions limit collisions (locals are camelCase, the match is case-sensitive/BINARY); snake_case languages (rust, python, ruby) will mask more. The feature's contract is high-precision candidates, so under-reporting is the correct failure direction — but expect the candidate count to drop well below the "true" dead set and do not treat a short list as exhaustive.
