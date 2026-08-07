# v4 Store Prerequisites (Ph2a) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use razorback:subagent-driven-development when subagent delegation is available. Fall back to razorback:executing-plans for single-task, tightly-sequential, or no-delegation runs.

**Goal:** land the v4 store contract's prerequisite fixes in the live artifact path — byte-deterministic `metadata_json`, the two-process determinism gate, the previous-vs-current extractor compatibility gate, and the V-1/V-5 purity surgeries — releasable as julie-extract 2.30.0 with a sequenced Miller reader migration.

**Architecture:** no new subsystems. One serialization chokepoint change, two new CI gates, one schema-v6 column drop with its lockstep writer sites removed, one writer-scope narrowing. The Miller companion task migrates 24 reader sites off the dropped column before the pin bump.

**Tech Stack:** Rust (rustup toolchain 1.97.1; repo floor ≥1.95), rusqlite, clap, xtask tiers, GitHub Actions; C#/.NET 10 for the Miller companion task.

**Architecture Quality:** No Architecture Impact — every change is confined to existing seams (the `json_string` chokepoint, `resolution_store` overlay primitives, `schema.rs` DDL, xtask tiers). The v4 store schema itself is Ph2b, not this plan.

**Program context:** this is the first slice of Ph2 in
[`miller:docs/plans/2026-08-06-index-store-views-program.md`](../../../miller/docs/plans/2026-08-06-index-store-views-program.md);
contract authority is the frozen v4 contract (miller repo,
`docs/plans/2026-08-07-index-store-v4-contract.md`) §2 (determinism requirement), §7/§16.8
(compatibility gate), §16.1 (determinism gate mechanics), §16.2 (purity surgeries). Ph2 slicing:
**Ph2a (this plan)** = prerequisites in the live artifact path; **Ph2b** = store schema +
`store import/update/delete` + `store_log` + chunked commits + crash matrix + the `resolve` verb +
bulk own-file shape (§16.4–16.6); **Ph2c** = resolution bases/deltas + coordinator + GC/purge +
`store export`/`--from-artifact` + equivalence gates + the G3b re-measure condition (§16.7, §14–15).
V-2/V-3/V-4 (`files` mutable-column moves) land with the store schema in Ph2b because their
destination (the view manifest) does not exist yet.

## Global Constraints

- Prefix every cargo command with `RUSTUP_TOOLCHAIN=1.97.1` (default toolchain 1.94 is below the repo floor).
- Cold builds can exceed xtask tier wall-clock budgets; re-run once warm before treating a budget trip as a failure.
- `serde_json` must stay WITHOUT the `preserve_order` feature — the canonicalization in Task 1 depends on `serde_json::Map` being BTreeMap-backed (verified: `Cargo.lock` serde_json 1.0.150, no `preserve_order` anywhere).
- The determinism gate MUST run its two scans in **separate processes** (`RandomState` reseeds per process; a same-process double scan passes while the defect is live) and MUST assert at least one multi-key `metadata_json` object (vacuity guard), on a multi-language fixture (v4 contract §16.1).
- `SQLITE_SCHEMA_VERSION` bumps 5 → 6 exactly once, in Task 3. `EXTRACT_CONTRACT_VERSION` (4) and `RESOLUTION_VERSION` (6) do not change in this plan.
- Release/tag/publish and the Miller pin bump require explicit user approval (standing rule). This plan ends at release-prep + approval request.
- Gate criteria are never tuned to pass. A red gate is reported, not adjusted.
- Miller repo rules apply to Task 5: fast suite via `scripts/test.sh`, scale via `scripts/test.sh scale`, 0-warning Release build.

## Verification Strategy

**Project source of truth:** `docs/release.md` (tiers + branch gates), `xtask/src/test_tiers.rs`.

**Worker red/green scope:** the targeted test file for the change (`cargo test -p <crate> --test <file>`), plus impacted unit tests in the crate.

**Worker ceiling:** one crate's test suite. Workers do not run `xtask test contract` or CI-wide gates on their own.

**Worker gate invariant:** Task 1 — two-process scans byte-agree on every `metadata_json`-carrying table (red before the fix, green after); Task 2 — the compat harness fails on an undeclared output diff and passes on a declared one; Task 3 — schema v6 artifacts carry no `identifiers.target_symbol_id` and the resolution suite stays green; Task 4 — writer contract suite green with the narrowed lookup; Task 5 — Miller fast+scale suites green against a lockstep v5 artifact.

**Lead affected-change scope:** `RUSTUP_TOOLCHAIN=1.97.1 cargo xtask test default` after each merged batch (src changed); `cargo xtask test contract` after Task 3 (schema/contract change).

**Branch gate (before release-prep/handoff):** `cargo fmt --check`, `cargo test -p xtask`, `cargo xtask test default`, `cargo xtask test contract` (docs/release.md branch gates), plus one dogfood scan (`cargo xtask dogfood repo --root . --out-dir target/dogfood/julie-extractors`).

**Escalation triggers:** any change to `crates/julie-extractors/Cargo.toml`, `Cargo.lock`, `language_spec/**`, or `registry*` adds the certification tier (docs/release.md changed-path rule). Golden-output changes require contract review per xtask `changed_plan`.

**Assigned verification failure:** workers stop and report; only Task 1's red-first step is an expected failure.

**Verification ledger:** record invariant, command, scope label, commit SHA, result, timestamp in `.razorback/sdd/progress.md`.

## Parallel Execution Contract

| Task | Parallel batch | File ownership | Serialization required | Dependency reason |
|---|---|---|---|---|
| Task 1: metadata_json determinism (gate + fix) | Batch A | Modify `crates/julie-extract-cli/src/extraction.rs`; Create `crates/julie-extract-cli/tests/determinism_contract.rs`; Modify `xtask/src/test_tiers.rs` (contract-tier entry only) | No | None - safe parallel batch. |
| Task 2: extractor compatibility gate | Batch A | Create `xtask/src/compat.rs`; Modify `xtask/src/main.rs`+`xtask/src/lib.rs` (new subcommand wiring), `.github/workflows/ci.yml` (new job); Create `docs/contracts/extraction-output-changes.md` | No | None - safe parallel batch. |
| Task 3: V-1 surgery (schema v6) | None - serial | Modify `crates/julie-extract-artifact/src/schema.rs`, `src/resolution_store.rs`, `tests/schema_contract.rs`, `tests/resolution_store_contract.rs`, `crates/julie-extract-cli/src/resolution.rs` (any `i.target_symbol_id` reads), `crates/julie-extract-cli/tests/operations_contract.rs` (affected assertions), `xtask/src/release.rs` (v6 doc pin); Create `docs/contracts/sqlite-schema-v6.md` + catalog sha256 | Yes | Overlaps Task 1's crate test surface and Task 2's xtask files; also the schema bump must land after both gates exist so the gates exercise it. |
| Task 4: V-5 SymbolLookup narrowing | Batch A | Modify `crates/julie-extract-artifact/src/writer/rows.rs` (or the writer file owning `SymbolLookup`), `crates/julie-extract-artifact/tests/writer_contract.rs` | No | None - safe parallel batch. |
| Task 5: Miller reader migration (cross-repo) | None - serial | Miller repo: Modify `src/Miller.Indexing/SqliteSymbolGraphIndex.cs`, `SymbolGraphReader.cs`, `ReferenceEvidenceReader.cs`, `ReferenceExportReader.cs` (24 `i.target_symbol_id` sites) + affected tests | Yes | Must land and prove green against a v5 lockstep artifact BEFORE the 2.30.0 pin bump; separate repo/worktree from Tasks 1–4. |
| Task 6: release prep 2.30.0 | None - serial | Modify `Cargo.toml` versions ×3, `Cargo.lock`, `docs/release.md`; Create `docs/release-notes/v2.30.0.md`; Modify `docs/contracts/extraction-output-changes.md` (declare the 2.30.0 canonicalization) | Yes | Requires every prior task merged and branch gates green. |

Commit mode: `parallel-lead-commit` for Batch A; serial tasks commit through the lead after inline review.

### Task 1: metadata_json determinism — two-process gate + chokepoint canonicalization

**Files:**
- Create: `crates/julie-extract-cli/tests/determinism_contract.rs`
- Modify: `crates/julie-extract-cli/src/extraction.rs:948-968` (`optional_json` / `json_string`)
- Modify: `xtask/src/test_tiers.rs:283` (`contract_plan()` — add the new test)

**Interfaces:**
- Consumes: `json_string` at `extraction.rs:958` (`serde_json::to_string(value)` — the sole nondeterministic serialization point); fixture root `fixtures/extraction/resolution_contract/` (9 language dirs); binary spawn pattern `Command::new(env!("CARGO_BIN_EXE_julie-extract"))` per `operations_contract.rs:41`.
- Produces: byte-deterministic `metadata_json` on every carrying table; the `determinism_contract` test name Task 2's declared-change doc and Task 6's release notes reference.

**Contract inputs:** v4 contract §16.1 mechanics (two separate processes; `(pk, metadata_json)` equality; ≥1 multi-key object; multi-language fixture). The `metadata_json`-carrying tables are enumerated from `schema.rs` (15 columns; assert over every table the scan populates on this fixture — at minimum `files`, `symbols`, `identifiers`, `relationships`, `source_regions`, `structural_facts`, `complexity_metrics`).

**File ownership:** Modify `crates/julie-extract-cli/src/extraction.rs`; Create `crates/julie-extract-cli/tests/determinism_contract.rs`; Modify `xtask/src/test_tiers.rs` (contract-tier entry only)

**Serialization required:** No

**Dependency reason:** None - safe parallel batch.

**What to build:** TDD in one lane. First the failing gate: a test that runs TWO separate `julie-extract scan` processes over the same fixture tree into two artifacts and asserts every `metadata_json`-carrying table's `(pk, metadata_json)` rowset is byte-identical, plus the vacuity guard (≥1 row whose `metadata_json` parses to an object with ≥2 keys). Verify it FAILS against the unfixed binary (61/118 symbols rows differed on a 3-file fixture in the Ph0 audit — this fixture is larger). Then the fix: route `json_string` through a canonicalizing round-trip (`serde_json::to_value` → serialize; `serde_json::Map` is BTreeMap-backed with `preserve_order` off, so nested object keys sort). Re-run: green. Wire the test into `contract_plan()`.

**Approach:** do NOT change the seven `Option<HashMap>` declarations in `crates/julie-extractors/src/base/types.rs` — the chokepoint fix covers all of them with one function and no 117-file ripple (v4 contract §2 explicitly permits "sorted-key serialization"). The hand-built identifiers map (`extraction.rs:463-501`) is already `serde_json::Map`-backed — leave it. Volatile columns (`indexed_at`, revision ids) are excluded from comparison; compare only `(pk, metadata_json)`.

**Acceptance criteria:**
- [ ] The gate fails against the pre-fix binary (recorded red run) and passes after the chokepoint change.
- [ ] Every `metadata_json`-carrying table populated by the fixture scan is compared; the multi-key vacuity guard is asserted.
- [ ] `cargo test -p julie-extract-cli --test determinism_contract` green; `contract_plan()` carries it.
- [ ] Worker-scope verification passes and the change is handed to the lead per commit mode.

### Task 2: extractor compatibility gate (previous vs current binary)

**Files:**
- Create: `xtask/src/compat.rs`
- Modify: `xtask/src/main.rs`, `xtask/src/lib.rs` (subcommand wiring per existing xtask module pattern)
- Modify: `.github/workflows/ci.yml` (new job, gated like Fast Gates)
- Create: `docs/contracts/extraction-output-changes.md` (the declared-changes ledger)

**Interfaces:**
- Consumes: previous release binary via `gh release download v<prev> --pattern '*x86_64-unknown-linux-gnu*'` in CI (local runs may point `--previous-binary <path>`); the fixture root `fixtures/extraction/resolution_contract/`; volatile-column knowledge from Task 1.
- Produces: `cargo xtask compat-check [--previous-binary <path>] [--declared <version>]` — exits nonzero when per-version extraction output differs from the previous release AND `docs/contracts/extraction-output-changes.md` has no entry for the current `Cargo.toml` version.

**Contract inputs:** v4 contract §7 ("same-epoch compatibility is a gated claim, never an assumption") and §16.8 (previous vs current binary, byte-equivalent per-version output, separate processes, same vacuity guard, multi-language fixture; any difference forces an epoch bump plus a compatible/incompatible classification — until the store exists, "epoch bump" is recorded as a ledger entry classifying the change).

**File ownership:** Create `xtask/src/compat.rs`; Modify `xtask/src/main.rs`+`xtask/src/lib.rs`, `.github/workflows/ci.yml`; Create `docs/contracts/extraction-output-changes.md`

**Serialization required:** No

**Dependency reason:** None - safe parallel batch.

**What to build:** an xtask command that scans the fixture with two binaries (previous release, current build) into two artifacts, dumps every extraction table (the per-version schema class — exclude `extraction_revisions`, `revision_file_changes`, `artifact_metadata`, resolution overlay tables, and volatile columns `indexed_at`/`last_revision_id`), and byte-compares the dumps. On difference: if the ledger declares the current version, pass with a notice naming the entry; else fail listing the first N differing rows per table. A CI job runs it on Linux against the previous GitHub release. The ledger's first entry is written by Task 6 (2.30.0 declares the metadata canonicalization as a compatible, byte-churning change).

**Approach:** dump via deterministic `SELECT * ORDER BY <pk>` per table to text, then diff — no schema-coupled row structs, so the harness survives schema bumps (a v5-vs-v6 table-set difference is itself a reportable diff the ledger must declare). CI job needs `contents: read` for the release download and must not block on the very first run after a declared change (the notice path). Keep the job out of the tag-triggered release workflow — it belongs to main/PR CI.

**Acceptance criteria:**
- [ ] `cargo xtask compat-check` with an identical binary pair passes; with a byte-differing pair and no ledger entry fails; with the entry passes-with-notice. All three proven by tests or a recorded local run.
- [ ] CI job added and green on this branch (against v2.29.0 — pre-canonicalization output equality is expected to FAIL only after Task 1 merges, which the branch run must show as the declared-entry notice path once Task 6 lands the ledger entry; the run order is recorded in the ledger doc).
- [ ] Worker-scope verification passes and the change is handed to the lead per commit mode.

### Task 3: V-1 purity surgery — drop `identifiers.target_symbol_id`, schema v6

**Files:**
- Modify: `crates/julie-extract-artifact/src/schema.rs` (`identifiers` DDL :227-251, `SCHEMA_INDEXES_SQL` :540+ — remove `idx_identifiers_target`; `SQLITE_SCHEMA_VERSION` :3 → 6)
- Modify: `crates/julie-extract-artifact/src/resolution_store.rs` (remove lockstep writes :296, :323, :582, :624 and their doc comments :4, :254, :314, :618; sweep worklist SQL for reads)
- Modify: `crates/julie-extract-artifact/tests/schema_contract.rs`, `tests/resolution_store_contract.rs`
- Modify: `crates/julie-extract-cli/src/resolution.rs` + `crates/julie-extract-cli/tests/operations_contract.rs` (sweep every `identifiers`-table `target_symbol_id` read/assertion)
- Create: `docs/contracts/sqlite-schema-v6.md` + `docs/contracts/sqlite-schema-v6.catalog.sha256`
- Modify: `xtask/src/release.rs:142-206` (pin the v6 doc paths)

**Interfaces:**
- Consumes: the lockstep invariant (resolution overlay is the source; `identifier_resolutions.target_symbol_id` carries every value the denormalized column had); the v5→v6 precedent — follow the v4→v5 bump choreography from git history (`git log --oneline -- docs/contracts/sqlite-schema-v5.md`).
- Produces: schema v6 artifacts with no `identifiers.target_symbol_id`; `schema_migration_required` (exit 3) against v5 artifacts per the existing `artifact_access.rs:488-530` gates — Miller's `ExtractorUpgrade` rescan machinery consumes that refusal and rebuilds.

**Contract inputs:** v4 contract §2 V-1 ("resolution outcomes live only in the resolution layer"), §16.2 (cross-repo sequencing: Miller's reads must survive one extractor version or land together — this plan chooses "Miller migrates first", Task 5). `idx_identifiers_target` deletion reclaims 1.26% of artifact bytes.

**File ownership:** as listed above (both crates' schema/resolution surfaces + contract docs + release pins)

**Serialization required:** Yes

**Dependency reason:** Overlaps Task 1's crate test surface and Task 2's xtask files; the schema bump must land after both gates exist so the branch run exercises them against it.

**What to build:** remove the column from the DDL, delete the four lockstep UPDATE sites, bump the version constant, write the v6 contract doc (delta from v5: one column + one index removed, rationale = v4 purity V-1), regenerate the catalog sha256, update the release-pinned doc paths, and sweep both crates' SQL and test assertions for the dropped column. `resolution_scope_equivalence`, `resolution_shadow`, and `resolution_contract` must stay green — they prove the overlay-only path was already authoritative.

**Approach:** there is no migration engine and none is added — a v6 binary refuses a v5 artifact (`schema_migration_required`), which is the existing, Miller-consumed upgrade path. Do not write ALTER TABLE migration code. The compat gate (Task 2) will report the v6 table-shape diff — the Task 6 ledger entry covers it.

**Acceptance criteria:**
- [ ] `schema_contract.rs` asserts v6 and the column/index absence; grep proves zero remaining `identifiers`-table `target_symbol_id` references in src (`git grep -n "target_symbol_id" -- crates | grep -v identifier_resolutions | grep -v pending_resolutions` returns only doc/history lines).
- [ ] Full resolution suite green: `resolution_store_contract`, `resolution_contract`, `resolution_scope_equivalence`, `resolution_shadow`, `operations_contract`.
- [ ] v6 contract doc + catalog sha256 exist and `xtask/src/release.rs` pins them; `cargo test -p xtask` green.
- [ ] Worker-scope verification passes and the change is committed by the lead after inline review.

### Task 4: V-5 — narrow the writer's `SymbolLookup` to file-own symbols

**Files:**
- Modify: `crates/julie-extract-artifact/src/writer/rows.rs` (locate `SymbolLookup`; if it lives elsewhere in the writer, the implementer corrects the path and reports the mismatch)
- Modify: `crates/julie-extract-artifact/tests/writer_contract.rs`

**Interfaces:**
- Consumes: the empirical baseline — 0 cross-file links over 703k rows (Ph0 purity audit V-5), so the narrowing is behavior-preserving today.
- Produces: extraction-table writes whose symbol lookups are structurally file-scoped — the property the store's isolated per-file extraction (Ph2b) depends on.

**Contract inputs:** v4 contract §2 V-5: "a store extracting files in isolation must not depend on it staying inert."

**File ownership:** Modify `crates/julie-extract-artifact/src/writer/rows.rs` (+ actual `SymbolLookup` home), `tests/writer_contract.rs`

**Serialization required:** No

**Dependency reason:** None - safe parallel batch.

**What to build:** scope the lookup used by extraction-table row writes to the file's own symbols, and add a writer-contract test that a cross-file symbol id in extraction-row input does NOT resolve through the lookup (the narrowed behavior), while same-file parents still do.

**Acceptance criteria:**
- [ ] `writer_contract.rs` green including the new narrowing test; `writer_performance.rs` unchanged and green.
- [ ] Worker-scope verification passes and the change is handed to the lead per commit mode.

### Task 5: Miller reader migration off the denormalized column (cross-repo)

**Files (miller repo, own worktree):**
- Modify: `src/Miller.Indexing/SqliteSymbolGraphIndex.cs`, `src/Miller.Indexing/SymbolGraphReader.cs`, `src/Miller.Indexing/ReferenceEvidenceReader.cs`, `src/Miller.Indexing/ReferenceExportReader.cs` — all 24 `i.target_symbol_id` sites
- Test: affected suites under `tests/Miller.Tests/`

**Interfaces:**
- Consumes: the lockstep invariant — on every v5 artifact, `identifier_resolutions.target_symbol_id` equals the denormalized value wherever a row exists, and `COALESCE(i.…, ir.…)` never yields a value `ir` lacks (the resolution pass writes both in one batch: `resolution_store.rs:254`).
- Produces: a Miller that reads resolution targets exclusively from `identifier_resolutions` — correct against v5 (lockstep) AND v6 (column gone), which is what lets the 2.30.0 pin bump land without a dual-schema reader.

**Contract inputs:** v4 contract §2 V-1 cross-repo sequencing; Miller CLAUDE.md test discipline (fast suite every change, scale before commit).

**File ownership:** the four Miller.Indexing readers + their tests; no julie files.

**Serialization required:** Yes

**Dependency reason:** Runs in the miller repo after Tasks 1–4 merge in julie; must be committed and green on miller main BEFORE the 2.30.0 pin bump (which needs user approval and is outside this plan).

**What to build:** replace every `COALESCE(i.target_symbol_id, ir.target_symbol_id)` with `ir.target_symbol_id` and every bare `i.target_symbol_id` read with the `ir` join equivalent (all 24 sites already join `identifier_resolutions` or can). Update test fixture DDL that still declares the column only where the fixture claims to be v6; v5-shaped fixtures keep it (fixtures must stay contract-faithful to the version they model).

**Acceptance criteria:**
- [ ] Zero `i.target_symbol_id` references remain in Miller `src/`.
- [ ] `scripts/test.sh` and `scripts/test.sh scale` green; `dotnet build Miller.slnx -c Release` 0/0.
- [ ] Committed on miller main (or its worktree merged) before any 2.30.0 pin-bump work starts.

### Task 6: release prep 2.30.0

**Files:**
- Modify: `crates/julie-extractors/Cargo.toml`, `crates/julie-extract-artifact/Cargo.toml`, `crates/julie-extract-cli/Cargo.toml` (2.29.0 → 2.30.0), `Cargo.lock`
- Create: `docs/release-notes/v2.30.0.md`
- Modify: `docs/release.md` (pointer + notes list), `docs/contracts/extraction-output-changes.md` (declare the canonicalization + schema v6 as this release's classified changes)

**Interfaces:**
- Consumes: every prior task merged; branch gates green; Task 2's ledger format.
- Produces: a release-preppable branch. The actual tag/publish and the Miller pin bump remain USER-APPROVAL-GATED and are not part of this plan's execution.

**Contract inputs:** docs/release.md release checklist; `cargo xtask release preflight --version 2.30.0` must pass.

**File ownership:** version manifests + release docs only.

**Serialization required:** Yes

**Dependency reason:** Requires all prior tasks merged and the branch gate green.

**What to build:** version bumps, release notes (headline: v4 prerequisites — deterministic `metadata_json` with a one-time byte churn on multi-key metadata; schema v6 dropping the denormalized resolution column with the Miller-side migration sequence; the two new CI gates), ledger entries classifying both changes, preflight run.

**Acceptance criteria:**
- [ ] Branch gates green at the release-prep commit (ledger rows recorded).
- [ ] `cargo xtask release preflight --version 2.30.0` passes.
- [ ] Release-prep commit exists; the approval request for tag/publish + pin bump is surfaced to the user with the evidence.
