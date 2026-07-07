# Workspace Reference Resolution Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use razorback:subagent-driven-development when
> subagent delegation is available. Fall back to razorback:executing-plans for single-task,
> tightly-sequential, or no-delegation runs.

**Goal:** Implement the workspace-level reference resolution pass specified in
[`2026-07-06-workspace-reference-resolution-design.md`](2026-07-06-workspace-reference-resolution-design.md)
(rev 4 — the design doc is the authoritative spec; every task below cites the sections it
implements and implementers MUST read those sections before coding).

**Architecture:** Resolution state lives in two FK-governed overlay tables
(`pending_resolutions`, `identifier_resolutions`) written only through atomic storage primitives
in `julie-extract-artifact`; resolver policy (tier chain, worklists) lives in a new
`julie-extract-cli` module and runs inside every writer transaction via a new `ResolutionHook`
seam. Pending rows and identifiers stay durable; invalidation is FK-first with a name-matched
demotion worklist for candidate-set growth.

**Tech Stack:** Rust, rusqlite (bundled SQLite), existing julie-extract crates.

**Architecture Quality:** Approved shape per design §"Module placement & interface": storage
primitives + hook signature in `julie-extract-artifact` (no language semantics), policy in
`julie-extract-cli::resolution`, per-file tier-1 resolution unchanged in `julie-extractors`.
Architecture risk: medium (writer-transaction seam, cross-file invalidation). If code reality
contradicts this shape, report a plan mismatch — do not redesign locally.

## Global Constraints

- The design doc rev 4 is the spec. Confidence constants: tier 1 = 0.95, tier 2 = 0.85,
  tier 3 = 0.75 (0.65 when `is_inferred`), tier 4 = 0.55. Outcomes: `resolved | ambiguous |
  missing | no_context`. Resolve ONLY on exactly-one kind-compatible same-language candidate.
- `SQLITE_SCHEMA_VERSION` goes 3 → 4 (`crates/julie-extract-artifact/src/schema.rs:3`).
  Schemas, reports, exit codes, and capability rows are API contracts (repo rule).
- No resolution state ever in `identifiers.metadata_json`. Overlay tables and the denormalized
  `identifiers.target_symbol_id` are written ONLY via the Task 1 storage primitives.
- Resolver errors are non-fatal: the scan commits, rows stay unresolved, the report records
  `ResolutionFailed` (stable code).
- Miller gates on `reference_resolution_status|version|last_full_revision` metadata keys —
  never on schema version or table probing.
- Default test suite stays fast (repo rule); slow/perf work goes behind the existing
  `test-perf` feature or a named tier, with the convention-test pattern of
  `crates/julie-extract-artifact/tests/test_tiers.rs`.
- Language parity honesty: tier coverage is advertised per language via capability rows and
  `language_capability_gaps` — a tier silently covering one language is a bug; a gated tier with
  a recorded gap is correct.
- `AGENTS.md`/`CLAUDE.md` byte-sync rule if guidance changes (run
  `scripts/check-agent-doc-sync.sh`).

## Verification Strategy

**Project source of truth:** `docs/testing-strategy.md` + `CLAUDE.md` (tier commands via
`cargo xtask test list`).

**Worker red/green scope:** the narrowest package test for the touched crate —
`cargo test -p julie-extract-artifact <test_name>` / `cargo test -p julie-extract-cli <test_name>`
/ `cargo test -p julie-extractors <test_name>` for the specific new tests in the task.

**Worker ceiling:** one package's default tests (`cargo test -p <crate>`). Workers do not run
golden/certification/real-world tiers on their own.

**Worker gate invariant:** each task's acceptance criteria name the behavior its tests prove;
a worker's gate passes only when those named tests exist, failed first (red), and now pass.

**Lead affected-change scope:** `cargo xtask test default` after each merged batch (fast package
tests for all three crates).

**Branch gate:** `cargo xtask test default && cargo xtask test contract && cargo xtask test capability`,
plus `cargo xtask dogfood repo --root . --out-dir target/dogfood/julie-extractors` once Task 6
lands (the dogfood scan is the first real-workspace resolution run; `ResolutionFailed` appearing
in its report fails the gate), plus `node scripts/language-data-quality-report.mjs --strict`
(zero `silent_cells` / `quality_bar_debts`) after Task 8's capability rows.

**Replay/metric evidence:** the Task 7 perf harness numbers are report-only on first
measurement; they become hard gates only after the measured budgets are written into the test
(design §"Performance & determinism": budgets move to measurement, not vice versa). Per-language
resolution rates from the dogfood run are report-only evidence recorded for release notes.

**Escalation triggers:** touching per-language extractor emitters (Task 2) requires
`cargo xtask test language <lang>` for each changed language plus `cargo xtask test golden` at
the batch boundary; touching `schema.rs`/writer requires `cargo xtask test contract`.

**Assigned verification failure:** Workers stop and report when assigned verification fails —
no gate edits without a plan mismatch report.

**Verification ledger:** Record invariant, command, scope label, commit SHA, result, timestamp
in the task report. Reuse same-HEAD passing evidence rather than rerunning expensive tiers.

## Parallel Execution Contract

| Task | Parallel batch | File ownership | Serialization required | Dependency reason |
|---|---|---|---|---|
| Task 1: Schema + storage primitives | Batch A | Create: `crates/julie-extract-artifact/src/resolution_store.rs`; Modify: `crates/julie-extract-artifact/src/schema.rs`, `crates/julie-extract-artifact/src/lib.rs`, `crates/julie-extract-artifact/src/metadata.rs`; Test: `crates/julie-extract-artifact/tests/resolution_store_contract.rs`, `crates/julie-extract-artifact/tests/schema_contract.rs` | No | None - safe parallel batch. |
| Task 2: Pending span emission | Batch A | Modify: `crates/julie-extractors/src/base/relationship_resolution.rs`, `crates/julie-extractors/src/base/types.rs`, per-language emitter call sites of `StructuredPendingRelationship::new`, `crates/julie-extract-cli/src/extraction.rs:501,523,814`; Test: `crates/julie-extract-cli/tests/` pending-span cases + touched language unit suites | No | None - safe parallel batch. |
| Task 3: ResolutionHook writer seam | Batch B | Modify: `crates/julie-extract-artifact/src/writer.rs`, `crates/julie-extract-artifact/src/writer/rows.rs`, `crates/julie-extract-artifact/src/reports.rs`; Test: `crates/julie-extract-artifact/tests/writer_contract.rs` | No | None - safe parallel batch (after Batch A; consumes Task 1's tables/report types). |
| Task 4: Resolver core (tier chain) | Batch B | Create: `crates/julie-extract-cli/src/resolution.rs` (+ submodules if split); Test: unit tests in-module | No | None - safe parallel batch (after Batch A; consumes Task 1's row/report types and Task 2's span-bearing pending shape). |
| Task 5: Workspace pass wiring | None - serial | Modify: `crates/julie-extract-cli/src/commands.rs` (call sites ~:223, ~:498, ~:1502-1510), `crates/julie-extract-cli/src/reports.rs`, `crates/julie-extract-cli/src/capability_snapshot.rs`, `crates/julie-extract-cli/src/resolution.rs`, `xtask/src/dogfood.rs` (ResolutionFailed gate); Test: `crates/julie-extract-cli/tests/` resolution flow cases | Yes | Consumes Tasks 1–4 (hook seam + primitives + tier chain). |
| Task 6: Contract + incremental fixtures | Batch C | Create: `crates/julie-extract-cli/tests/resolution_contract.rs`, fixture files under `fixtures/extraction/` per language; no src edits | No | None - safe parallel batch (after Task 5). |
| Task 7: Perf gate | Batch C | Modify: `crates/julie-extract-artifact/tests/writer_perf.rs` (or a sibling `resolution_perf.rs` behind `test-perf`), `crates/julie-extract-artifact/Cargo.toml` only if a new feature name is needed; Test: the perf harness itself | No | None - safe parallel batch (after Task 5). |
| Task 8: Contracts, capabilities, docs | None - serial | Modify: artifact contract doc under `docs/contracts/`, `fixtures/extraction/capabilities.json`, `scripts/language-data-quality-report.mjs` (strict gate hardening), `docs/release-notes/` draft, `CLAUDE.md`+`AGENTS.md` only if guidance changes | Yes | Needs measured rates and gap rows from Tasks 5–7. |

Commit mode: `serial-worker-commit` for serial tasks (5, 8); `parallel-lead-commit` for
Batch A, Batch B, and Batch C members.

---

### Task 1: Schema v4 + resolution storage primitives

**Files:**
- Create: `crates/julie-extract-artifact/src/resolution_store.rs`
- Modify: `crates/julie-extract-artifact/src/schema.rs` (SCHEMA_SQL + `SQLITE_SCHEMA_VERSION` 3→4), `crates/julie-extract-artifact/src/lib.rs` (export), `crates/julie-extract-artifact/src/metadata.rs` (resolution status keys)
- Test: `crates/julie-extract-artifact/tests/resolution_store_contract.rs`; extend `crates/julie-extract-artifact/tests/schema_contract.rs`

**Interfaces:**
- Consumes: design §"Resolution state model" — the exact DDL for `pending_resolutions` and
  `identifier_resolutions` is in the design doc, including the CHECK
  (`outcome='resolved' ⇔ target_symbol_id IS NOT NULL`) and FK actions.
- Produces (later tasks rely on these exact names):
  - `resolution_store::record_pending_resolution(tx, pending_relationship_id, target_symbol_id, tier: u8, confidence: f64, method: &str, revision: i64)`
  - `resolution_store::record_identifier_outcome(tx, identifier_id, outcome: Outcome, target: Option<&str>, tier/confidence/method/candidates, revision)` — atomically writes the overlay row AND the denormalized `identifiers.target_symbol_id` in one statement batch
  - `resolution_store::demote_pending(tx, pending_relationship_id)` / `demote_identifier(tx, identifier_id)` — delete overlay row AND clear the denormalized column atomically
  - `resolution_store::worklist_*` queries: unresolved pending rows by terminal/receiver-name set; resolved rows (both overlays) by terminal/receiver-name set; never-attempted identifiers by name set or file set; **and full-pass variants** (`worklist_full_pending`, `worklist_full_identifiers`: every unresolved pending row / every never-attempted or NULL-target identifier) for `Full` scope and v3-artifact backfill
  - `resolution_store::write_resolution_metadata(conn, status: ResolutionStatus, version, last_full_revision)` and `read_resolution_metadata(conn)` — upserts `reference_resolution_status|version|last_full_revision` into `artifact_metadata` (separate upsert; do NOT widen `ArtifactMetadata::rows()`'s fixed array)
  - `ResolutionCounts` (rows written per table) for revision accounting, and `ResolutionReportRow` (language, tier, outcome, count)

**Contract inputs:** design §"Resolution state model", §"Contract & rollout" items 1–2; existing
`SCHEMA_SQL` string layout in `schema.rs`; `artifact_metadata` key/value table shape.

**File ownership:** Create: `crates/julie-extract-artifact/src/resolution_store.rs`; Modify: `crates/julie-extract-artifact/src/schema.rs`, `crates/julie-extract-artifact/src/lib.rs`, `crates/julie-extract-artifact/src/metadata.rs`; Test: `crates/julie-extract-artifact/tests/resolution_store_contract.rs`, `crates/julie-extract-artifact/tests/schema_contract.rs`

**Serialization required:** No

**Dependency reason:** None - safe parallel batch.

**What to build:** The two overlay tables in `SCHEMA_SQL` with their FK/CHECK constraints and
covering indexes (design §"Performance & determinism": identifiers `(file_id, start_line, name)`
composite), plus the atomic storage primitives that are the ONLY write path to resolution state,
plus the durable resolution-status metadata keys.

**Approach:** Follow the existing table-block style in `SCHEMA_SQL`. Primitives take
`&rusqlite::Transaction` (they will be called from inside the writer hook). Test FK behavior
directly: insert symbol + pending + resolution, delete the symbol, assert the resolution row
cascaded and pending context survived; same for identifier overlays including the NULL-target
ambiguous rows surviving target deletion (they reference no target). Test the CHECK rejects
`resolved` with NULL target. Test demote clears the denormalized column (round-3 finding 1).
Schema-contract test asserts version 4 and both tables exist after `create_schema` on a fresh
AND an existing v3 database (additive upgrade path).

**Acceptance criteria:**
- [x] `SQLITE_SCHEMA_VERSION == 4`; both tables + indexes in `SCHEMA_SQL`; additive creation on a v3 artifact verified
- [x] CHECK and both FK actions proven by tests (cascade on target death, pending context intact, NULL-target rows unaffected)
- [x] Atomic primitives are the only exported write surface; demote clears denormalized column in the same batch
- [x] `write/read_resolution_metadata` round-trips status keys without touching `ArtifactMetadata::rows()`
- [x] Worker-scope verification passes and the change is handed to the lead per `parallel-lead-commit`

### Task 2: Pending span emission + occurrence-distinct IDs

**Files:**
- Modify: `crates/julie-extractors/src/base/relationship_resolution.rs` (`StructuredPendingRelationship`), `crates/julie-extractors/src/base/types.rs` (`PendingRelationship` if span lives there), per-language call sites of `StructuredPendingRelationship::new` (discover with a grep; mechanical), `crates/julie-extract-cli/src/extraction.rs` — the pending row mapping at `:501` (`start_column: None` at `:523`) and the `pending_id` helper at `:814` (the ID must incorporate occurrence identity)
- Test: pending-span assertions in the existing CLI extraction tests (`crates/julie-extract-cli/tests/`), plus touched language unit suites

**Interfaces:**
- Consumes: existing `StructuredPendingRelationship::new(from_symbol_id, target, caller_scope_symbol_id, kind, file_path, line_number, confidence)` at `relationship_resolution.rs:54`.
- Produces: pending rows whose `start_column/end_line/end_column/start_byte/end_byte` are
  populated wherever the emitting call site has a node span (an `Option<PendingSpan>` field —
  extractors without span info at hand pass `None` and the row keeps today's shape), and
  `pending_relationship_id` incorporating column/byte so two same-name calls on one line are
  distinct rows. Task 4/5 join identifiers to pending rows by byte span, falling back to
  `(file_id, start_line, name)` only when exactly one identifier matches.

**Contract inputs:** design §"Data flow" step 1; §"Verified current state" bullet on span
columns; extraction.rs mapping seen at lines ~505–535.

**File ownership:** Modify: `crates/julie-extractors/src/base/relationship_resolution.rs`, `crates/julie-extractors/src/base/types.rs`, per-language emitter call sites of `StructuredPendingRelationship::new`, `crates/julie-extract-cli/src/extraction.rs:501,523,814`; Test: `crates/julie-extract-cli/tests/` pending-span cases + touched language unit suites

**Serialization required:** No

**Dependency reason:** None - safe parallel batch.

**What to build:** Span plumbing from extractor call sites through the shared pending types into
the artifact mapping, and occurrence-distinct pending IDs.

**Approach:** Add the span as an `Option` on `StructuredPendingRelationship` with a
`with_span(...)` builder (avoid breaking every `::new` call site at once); populate it at call
sites where the AST node is in hand — start with the shared/base helpers so every language
routing through them benefits, then per-language sites. Keep the ID stable for rows without
spans (today's dedup semantics) and extend the ID input with `start_byte`/`start_column` when
present. Do not chase 100% span coverage in this task: the parity surface is "spans populated
for every language whose emitters route through the shared helpers", asserted per language in
Task 6 fixtures, with genuine gaps recorded there.

**Acceptance criteria:**
- [x] Pending rows carry spans where emitters supply them; live-artifact-style mapping test proves non-NULL span columns for at least C#, TypeScript, Python fixtures
- [x] Two same-name same-line calls produce two pending rows (ID test)
- [x] Rows without spans keep today's ID and dedup behavior (no regression in existing extraction tests)
- [x] `cargo xtask test language <lang>` green for every language whose emitter files changed (no per-language emitter changed — coverage flows through the shared `create_pending_relationship` helper; ran csharp/typescript/python + full extractors 2790 suite)
- [x] Worker-scope verification passes and the change is handed to the lead per `parallel-lead-commit`

### Task 3: ResolutionHook seam in the writer

**Files:**
- Modify: `crates/julie-extract-artifact/src/writer.rs` (all mutating paths: `write_scan` :324, `write_scan_spooled` :333, `write_scan_spooled_preserving_missing_paths` :344, `write_update` :356, `delete_file` :365, `remove_unsupported_file` :374), `crates/julie-extract-artifact/src/writer/rows.rs` (deleted-name collection), `crates/julie-extract-artifact/src/reports.rs` (`RowDomainCounts` for the two new tables, `resolution_failed` field)
- Test: extend `crates/julie-extract-artifact/tests/writer_contract.rs`

**Interfaces:**
- Consumes: Task 1's `ResolutionCounts`.
- Produces (Task 5 relies on): a hook parameter on each mutating method (design: `_with_resolution`
  variants or an `Option<&mut dyn ...>`-free generic — non-escaping HRTB closure
  `F: for<'t> FnMut(&rusqlite::Transaction<'t>, &ResolutionScopeInput) -> Result<ResolutionCounts, ResolutionHookError>`),
  where `ResolutionScopeInput { changed_file_ids: Vec<String>, touched_symbol_names: HashSet<String>, is_full_scan: bool }`.
  **Return-contract note (Codex plan review, blocker 1):** the writer consumes ONLY
  `ResolutionCounts` (all it needs for revision accounting); the per-language
  `ResolutionReport` is NOT returned through the writer — Task 5's closure writes it into its
  own captured state (`&mut Option<ResolutionReport>`), which is exactly why the hook is
  specified as a non-escaping `FnMut`. Task 3 also adds the stable
  `ReportCode::ResolutionFailed` variant to the existing `ReportCode` enum
  (`crates/julie-extract-artifact/src/reports.rs:315`);
  `touched_symbol_names` = names inserted this scan ∪ names collected from old DB rows **before**
  `delete_file_rows` runs (design §"Incremental correctness", round-3 note). Hook runs after all
  row writes, before `update_revision_counts` and commit, in EVERY path including the spooled
  deferred-FK transaction. Hook error → counts zeroed, scan commits, report carries
  `resolution_failed: Some(message)`.

**Contract inputs:** design §"Module placement & interface" (transaction seam + failure
semantics + contract details); writer transaction structure (`tx` created per method, commits
internally; `delete_file_rows` at writer.rs:496 pattern).

**File ownership:** Modify: `crates/julie-extract-artifact/src/writer.rs`, `crates/julie-extract-artifact/src/writer/rows.rs`, `crates/julie-extract-artifact/src/reports.rs`; Test: `crates/julie-extract-artifact/tests/writer_contract.rs`

**Serialization required:** No

**Dependency reason:** None - safe parallel batch (after Batch A; consumes Task 1's tables/report types).

**What to build:** The seam only — no policy. A test hook (closure incrementing counters /
writing marker rows via Task 1 primitives) proves ordering, name-set correctness, count folding,
and non-fatal error semantics in every mutating path.

**Approach:** Factor the existing per-method commit tails so the hook call + count folding +
failure wrap live in one helper. The deleted-name collection queries old symbol names for
to-be-deleted file_ids before `delete_file_rows`. Keep existing method signatures working
(hookless variants delegate with a no-op) so current callers/tests compile unchanged.

**Acceptance criteria:**
- [x] Hook fires in all six mutating paths inside the open transaction, before `update_revision_counts`
- [x] `touched_symbol_names` includes old-row names for updated AND deleted files (test: rewrite a file removing symbol `Foo` → hook sees `Foo`)
- [x] Hook error does not roll back the scan; report carries `resolution_failed`; counts stay truthful
- [x] Existing writer_contract tests pass unchanged via the hookless variants
- [x] Worker-scope verification passes and the change is handed to the lead per `parallel-lead-commit`

### Task 4: Resolver core — candidate filters + tier chain

**Files:**
- Create: `crates/julie-extract-cli/src/resolution.rs` (submodules `candidates`, `tiers` if it grows)
- Test: unit tests in-module (pure logic, no DB)

**Interfaces:**
- Consumes: Task 1's row types; design §"Resolution tiers" — the tier table, confidence
  constants, kind-compatibility map, same-language rule, exactly-one rule, and tier-4 kind
  restrictions are all specified there verbatim.
- Produces (Task 5 relies on): `resolution::resolve_one(edge: &UnresolvedEdge, index: &WorkspaceCandidateIndex) -> TierOutcome`
  where `UnresolvedEdge` abstracts a pending row or a bare identifier (kind, terminal name,
  receiver, import evidence, language, file, caller scope), `WorkspaceCandidateIndex` is built
  once per pass from in-memory symbol/import/type-fact rows, and
  `TierOutcome = Resolved { target_symbol_id, tier, confidence, method } | Ambiguous { candidates } | Missing | NoContext`.

**Contract inputs:** design §"Resolution tiers" (including tier-2 language gating and the tier-3
receiver chain: receiver name → scoped symbol via caller_scope/containing parent chain → file →
enclosing-type fields → `type_facts.resolved_type` → same-language unique type symbol → member
with terminal name); `ScopedSymbolIndex` patterns at
`crates/julie-extractors/src/base/relationship_resolution.rs:125-195` for candidate-filter idiom.

**File ownership:** Create: `crates/julie-extract-cli/src/resolution.rs` (+ submodules if split); Test: unit tests in-module

**Serialization required:** No

**Dependency reason:** None - safe parallel batch (after Batch A; consumes Task 1's row/report types and Task 2's span-bearing pending shape).

**What to build:** The pure tier-chain logic over in-memory candidate data. Tier 1 outcomes are
already materialized at extraction time (existing local resolution) — the workspace chain runs
tiers 2→4 for pending rows and the reduced chains for bare identifiers (design §"Data flow"
step 4: `type_usage` → 2&4; `call` → 2&4 Function/Constructor only; `member_access` → none).

**Approach:** TDD each tier against the design's fixture cases: import-guided hit, import-
ambiguous but receiver-typed hit (tier independence), overload → Ambiguous, partial-class →
Ambiguous at tier 4, cross-language collision → not a candidate, `is_inferred` confidence drop,
tier-2 gated language → skips to tier 3/4 with a recorded gap. Deterministic candidate ordering
by `symbol_id`.

**Acceptance criteria:**
- [x] Every tier-table row and restriction from the design has at least one positive and one negative unit test
- [x] Exactly-one rule + ambiguous/missing/no_context outcomes proven; no code path selects among >1 candidates
- [x] Tier-2 language gate is data-driven (a per-language allowlist constant with a doc comment pointing at the fixture evidence), not scattered ifs
- [x] Worker-scope verification passes and the change is handed to the lead per `parallel-lead-commit`

### Task 5: Workspace pass wiring — Full/Delta, demotion, reports, capabilities

**Files:**
- Modify: `crates/julie-extract-cli/src/resolution.rs` (add `resolve_workspace`), `crates/julie-extract-cli/src/commands.rs` (hook closures at the scan call site ~:223, update ~:498, delete/unsupported ~:1502-1510, and the force-rebuild scan path), `crates/julie-extract-cli/src/reports.rs` (resolution section in scan reports), `crates/julie-extract-cli/src/capability_snapshot.rs` (`reference_resolution.tier2_import` / `tier3_receiver` capability rows), `xtask/src/dogfood.rs` (fail the dogfood gate when `ResolutionFailed` appears in the scan output — today it checks exit success only, `dogfood.rs:321`)
- Test: `crates/julie-extract-cli/tests/` — end-to-end scan flows on tiny fixtures

**Interfaces:**
- Consumes: Tasks 1–4 exactly as produced (primitives, hook signature, `resolve_one`).
- Produces: `resolve_workspace(tx, scope: ResolutionScopeInput, deps) -> ResolutionReport`
  implementing design §"Data flow" steps 2–4 and §"Incremental correctness": Full = resolve all
  unresolved + rebuild candidate index; Delta = demotion sweep (resolved rows whose terminal OR
  receiver name ∈ touched names → re-run tier chain → demote via primitives if outcome changed)
  then fill sweep (unresolved rows matching touched names + all rows in changed files).
  Propagation: resolved pending → co-located identifier by byte span, line fallback only when
  exactly one identifier matches; tier-1 extraction-time `relationships` rows propagate to their
  co-located identifiers the same way. Writes `reference_resolution_status`
  (`complete` on clean Full, `partial` after Delta-only or gated-language gaps, `failed` on
  hook error) + version + last_full_revision via Task 1 metadata primitives. Emits the
  per-language/per-tier `ResolutionReport` into the scan report and capability snapshot.

**Contract inputs:** design §"Data flow", §"Incremental correctness", §"Honesty & parity
surfaces", §"Contract & rollout" item 2; CLI call sites at `commands.rs:223/498/1502-1510`.

**File ownership:** Modify: `crates/julie-extract-cli/src/commands.rs`, `crates/julie-extract-cli/src/reports.rs`, `crates/julie-extract-cli/src/capability_snapshot.rs`, `crates/julie-extract-cli/src/resolution.rs`; Test: `crates/julie-extract-cli/tests/` resolution flow cases

**Serialization required:** Yes

**Dependency reason:** Consumes Tasks 1–4 (hook seam + primitives + tier chain).

**What to build:** The complete pass, wired as the hook closure at every artifact-mutating CLI
flow, with set-based SQL through temp tables (no per-row round trips) and stable ordering.

**Approach:** Build the candidate index once per hook invocation from a single set of queries
(symbols by name+language+kind, imports by file, type facts by symbol). Delta scoping keys off
`ResolutionScopeInput`. The closure returns `ResolutionCounts` to the writer and stores the
`ResolutionReport` in captured state (Task 3's return-contract note). Old-artifact backfill:
opening a v3 artifact (Task 1's additive create) followed by any scan triggers a Full resolve
when `reference_resolution_status` is absent — this runs on the WRITE path; the existing
`--strict-schema` read preflight (`artifact_access.rs:278`) keeps rejecting un-upgraded
artifacts on read, unchanged (Task 8 documents this in the contract doc).
End-to-end tests: scan a two-file fixture → cross-file call resolved; rewrite the target file →
resolution cascades away, pending context intact, re-resolve restores it; add a colliding
same-name symbol → uniqueness-regression demotion; remove it → re-resolves; `status=failed`
path via an injected failing hook.

**Acceptance criteria:**
- [ ] All artifact-mutating CLI flows (scan, update, delete, remove-unsupported, force rebuild) run the pass with correct Full/Delta scope
- [ ] Incremental sequences from design §Testing pass: FK demotion, uniqueness regression, re-resolution, file move, no stale edges
- [ ] `reference_resolution_*` metadata keys maintained per the status rules; backfill on v3-artifact open proven
- [ ] Scan report + capability snapshot carry per-language/per-tier counts and gated-language gaps
- [ ] Determinism: two identical full scans → byte-identical resolution tables (test compares dumps)
- [ ] Worker-scope verification passes; `serial-worker-commit` with recorded SHA

### Task 6: Per-language contract fixtures

**Files:**
- Create: `crates/julie-extract-cli/tests/resolution_contract.rs`; fixture sources under `fixtures/extraction/` following the existing per-language fixture layout (inspect neighboring fixtures for the convention)
- Test: the new contract suite itself

**Interfaces:**
- Consumes: Task 5's shipped behavior; design §Testing case list.
- Produces: the per-language evidence rows Task 8 cites (which languages prove tier 1/2/4 today,
  where tier 3 applies, recorded gaps for the rest).

**Contract inputs:** design §Testing; parity rule from Global Constraints; existing fixture
conventions in `fixtures/extraction/` and `fixtures/extraction/capabilities.json`.

**File ownership:** Create: `crates/julie-extract-cli/tests/resolution_contract.rs`, fixture files under `fixtures/extraction/` per language; no src edits

**Serialization required:** No

**Dependency reason:** None - safe parallel batch (after Task 5).

**What to build:** Fixture-backed assertions per supported language: same-file (tier 1,
pre-existing), cross-file import (tier 2 where gated on), receiver-typed (tier 3 where
type_facts exist), unique-language-global (tier 4 for allowed kinds), ambiguous-stays-unresolved,
overload, partial-class (C#), cross-language name collision stays unresolved.

**Approach:** One fixture pair (definition file + reference file) per language per proven tier;
languages where a tier is gated off assert the recorded gap instead — a language with neither a
passing assertion nor a recorded gap fails the suite (that is the parity guard). Route the suite
out of the default tier only if it exceeds the fast budget; prefer keeping tiny fixtures fast.

**Acceptance criteria:**
- [ ] Every supported language has, per tier: a passing fixture assertion OR a recorded `language_capability_gaps` row — enforced by the suite itself
- [ ] All design §Testing contract cases implemented
- [ ] Suite runs inside the default tier budget or is named in `cargo xtask test list` with a tier route
- [ ] Worker-scope verification passes and the change is handed to the lead per `parallel-lead-commit`

### Task 7: Performance gate

**Files:**
- Modify: `crates/julie-extract-artifact/tests/writer_perf.rs` (or Create: `crates/julie-extract-cli/tests/resolution_perf.rs` behind the same `test-perf` feature — mirror the `test_tiers.rs` convention guard either way)
- Test: the harness itself

**Interfaces:**
- Consumes: Task 5's pass; design §"Performance & determinism" budgets (full < 2s on a
  92k-identifier-scale synthetic artifact; delta < 100ms single-file update).
- Produces: measured numbers for Task 8's release notes; hard-gate thresholds once measured.

**Contract inputs:** `test-perf` feature pattern (`writer_perf.rs` starts with
`#![cfg(feature = "test-perf")]`; `test_tiers.rs` guards it); design budget-moves-to-measurement
rule.

**File ownership:** Modify: `crates/julie-extract-artifact/tests/writer_perf.rs` (or sibling `resolution_perf.rs`), `crates/julie-extract-artifact/Cargo.toml` only if a new feature name is needed; Test: the perf harness itself

**Serialization required:** No

**Dependency reason:** None - safe parallel batch (after Task 5).

**What to build:** A synthetic-scale harness (generated symbols/identifiers/pending rows at
~92k-identifier scale) timing Full resolve and single-file Delta, first report-only, then
asserting the measured budgets with headroom.

**Acceptance criteria:**
- [ ] Harness behind `test-perf`; convention guard extended if a new file is added
- [ ] Full and Delta timings measured and recorded in the task report; thresholds set from measurement with stated headroom
- [ ] Worker-scope verification passes and the change is handed to the lead per `parallel-lead-commit`

### Task 8: Contracts, capabilities, release notes

**Files:**
- Modify: the artifact schema/contract doc under `docs/contracts/` (locate the existing artifact contract doc and extend it with the resolution state model, tiers, confidence values, outcome semantics, metadata keys, the Miller detection rule, and the `--strict-schema` read-preflight behavior for un-upgraded v3 artifacts), `fixtures/extraction/capabilities.json` (resolution capability claims backed by Task 6 evidence), `scripts/language-data-quality-report.mjs` (Codex plan review: `--strict` exits non-zero only for `silentCells` at `:468` — harden it to also fail on `quality_bar_debts > 0`, which AGENTS.md already requires kept at 0; if main has pre-existing debts, report a plan mismatch instead of weakening the rule), `docs/release-notes/` draft for 2.9.0 with measured per-language rates from the dogfood run
- Test: `node scripts/language-data-quality-report.mjs --strict` clean; `scripts/check-agent-doc-sync.sh` if guidance files changed

**Interfaces:**
- Consumes: Task 6 evidence rows, Task 7 measurements, Task 5 dogfood report
  (`cargo xtask dogfood repo --root . --out-dir target/dogfood/julie-extractors`).
- Produces: the documented contract Miller's pin-bump slice consumes (F5 in the design doc).

**Contract inputs:** design §"Contract & rollout"; repo rule that schemas/reports/capability
rows are API contracts; capabilities.json evidence rules in CLAUDE.md.

**File ownership:** Modify: artifact contract doc under `docs/contracts/`, `fixtures/extraction/capabilities.json`, `scripts/language-data-quality-report.mjs` (strict gate hardening), `docs/release-notes/` draft, `CLAUDE.md`+`AGENTS.md` only if guidance changes

**Serialization required:** Yes

**Dependency reason:** Needs measured rates and gap rows from Tasks 5–7.

**What to build:** The consumer-facing contract documentation and honest capability claims. No
code changes beyond capability data.

**Acceptance criteria:**
- [ ] Contract doc covers: both overlay tables, metadata keys and their status semantics, tier/confidence table, outcome vocabulary, the "Miller gates on metadata keys, never schema version" rule
- [ ] capabilities.json claims match Task 6 evidence exactly; `--strict` data-quality report clean (0 silent_cells, 0 quality_bar_debts) with the hardened script exiting non-zero on either
- [ ] Release-notes draft records measured per-language resolution rates and perf numbers
- [ ] Worker-scope verification passes; `serial-worker-commit` with recorded SHA

---

## Out of scope (tracked in the design doc)

F1 identifier context enrichment, F2 type_facts breadth, F3 overload discrimination, F4
normalized import facts, F5 Miller pin-bump/consumption slice, F6 Miller P4 history/trends.
The 2.9.0 release itself (tag/publish) requires explicit user approval per repo rules.
