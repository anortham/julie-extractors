# Dogfood Incremental Rescan Baseline Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use razorback:subagent-driven-development when subagent delegation is available. Fall back to razorback:executing-plans for single-task, tightly-sequential, or no-delegation runs.

**Goal:** Extend the dogfood gate so it records a real incremental no-change rescan baseline after the initial `julie-extract scan`.

**Architecture:** Keep dogfood as repo tooling under `xtask`; do not change the public `julie-extract` CLI, SQLite schema, JSONL schema, or report contract. The dogfood command should run the product CLI in this order: cold scan, immediate rescan against the same SQLite artifact, info, and JSONL export. The rescan report becomes hard evidence that incremental scan returns `no_change` while the timing remains report-only.

**Tech Stack:** Rust `xtask`, `julie-extract` CLI subprocesses, SQLite artifact validation, JSON report validation, JSONL export validation.

**Architecture Quality:** Low to medium risk. The change stays in repo tooling, but it tightens release evidence around an important product performance path.

---

## Source Documents

- `AGENTS.md`: product boundary, SQLite-first output, CLI-first integration, and test discipline.
- `RAZORBACK.md`: strategy-tier areas and verification ownership.
- `docs/testing-strategy.md`: dogfood is a non-default specialist gate.
- `docs/release.md`: release evidence rules.
- `docs/release-evidence/README.md`: evidence file expectations.
- `docs/release-evidence/v0.1.0-dogfood.md`: current dogfood evidence.
- `docs/plans/2026-06-01-incremental-scan-hash-skip.md`: previous slice that made no-change rescans skip parser work.

## Current Baseline

- `cargo xtask dogfood repo --root . --out-dir target/dogfood/julie-extractors` runs one cold `scan`, then `info`, then `export`.
- `metrics.json` records cold scan, info, export, artifact sizes, row totals, JSONL record counts, and rows per second.
- It does not run a second scan against the just-written artifact, so it does not capture the incremental `no_change` path that the hash-skip optimization improved.

## Architecture Quality

**Affected modules:** `xtask/src/dogfood.rs`, `xtask/tests/dogfood_contract.rs`, `docs/testing-strategy.md`, `docs/release-evidence/v0.1.0-dogfood.md`, and this plan.

**Caller-facing interface:** The public product interface remains `julie-extract` plus SQLite/JSONL/report artifacts. `cargo xtask dogfood repo` is repo tooling; adding one output file and metrics fields changes release-evidence shape, not downstream product APIs.

**Depth/locality check:** Keep subprocess orchestration and evidence validation in `xtask/src/dogfood.rs`. Do not push benchmarking logic into `julie-extract`, `julie-extract-artifact`, or parser crates.

**Test surface:** Prove behavior through `plan_repo_from_args` output paths and `validate_outputs` on fixture reports/artifacts. The integration evidence comes from running the actual dogfood command after implementation.

**Seams/adapters:** Reuse the existing `run_julie_extract`, `validate_report`, and metrics-writing path. Add a rescan report validator instead of a new benchmark framework.

**Rejected shortcuts:** Do not time incidental shell output. Do not add hard timing thresholds from a single machine run. Do not skip JSONL validation. Do not add old Julie integration behavior or watchers.

**Architecture risk:** Low to medium. The main risk is making dogfood slower; it remains a manual specialist gate and the added rescan should be much cheaper than cold scan/export.

## Verification Strategy

**Project source of truth:** `AGENTS.md`, `RAZORBACK.md`, `docs/testing-strategy.md`, and `docs/release.md`.

**Worker red/green scope:** `cargo test -p xtask --test dogfood_contract`

**Worker ceiling:** `cargo test -p xtask`

**Worker gate invariant:** Dogfood planning includes a rescan report path; validation accepts a cold scan `ok` report and requires a rescan `no_change` report with zero changed/deleted/failed files and positive unchanged files.

**Lead affected-change scope:** `cargo test -p xtask`, `cargo xtask dogfood repo --root . --out-dir target/dogfood/julie-extractors`, and the Python SQLite consumer readback against the generated artifact.

**Branch gate:** `cargo xtask test default` and `cargo xtask test contract` before merge, push, or PR.

**Replay/metric evidence:** Hard gates are dogfood command success, JSON reports with expected statuses, valid SQLite metadata, nonzero files/symbols, valid JSONL, and rescan `no_change`. Cold scan duration, rescan duration, info duration, export duration, artifact bytes, JSONL bytes, rows per second, and any ratio metrics are report-only.

**Escalation triggers:** Public CLI/report/schema changes, parser dependency changes, default-suite runtime growth, weak dogfood evidence, or any need for old Julie internals.

**Assigned verification failure:** Workers stop and report when assigned verification fails unless this plan explicitly says to update that gate.

**Verification ledger:** Record invariant, command, scope label, commit SHA, result, and timestamp in the final report. For dogfood, record hard-gate metrics and report-only metrics.

## Model Routing

**Project source of truth:** `RAZORBACK.md`.

**Strategy tier:** Planning, evidence interpretation, and performance acceptance.
- Harness mapping: inherit in this Codex session.

**Implementation tier:** Bounded edits to dogfood xtask command, tests, and docs after this plan fixes the evidence shape.
- Harness mapping: inherit.

**Mechanical tier:** Formatting and wording-only docs edits.
- Harness mapping: inherit.

**Gate-interpretation reviewer:** Review of failed dogfood validation, report mismatches, or performance evidence ambiguity.
- Harness mapping: inherit.

**Escalation tier:** Public artifact contract changes, parser dependency changes, weak tests, or repeated verification failures.
- Harness mapping: inherit.

**Worker eligibility:** Workers are eligible only when file ownership is narrow, the evidence shape is already decided, and they do not reinterpret public contracts.

**Mechanical exclusion:** Mechanical workers cannot own failing tests, replay evidence, metrics, or acceptance gates.

**Unsupported harness behavior:** If the harness cannot choose models per agent, use `inherit` and continue.

## File Structure

- Create: `docs/plans/2026-06-01-dogfood-incremental-rescan-baseline.md` - this plan and progress ledger.
- Modify: `xtask/src/dogfood.rs` - add rescan report path, run the second scan, validate `no_change`, and serialize rescan timing/summary metrics.
- Modify: `xtask/tests/dogfood_contract.rs` - add red/green tests for rescan path planning, rescan validation, and invalid rescan evidence.
- Modify: `docs/testing-strategy.md` - document that dogfood includes an immediate no-change rescan.
- Modify: `docs/release.md` - document that release dogfood evidence includes rescan evidence.
- Modify: `docs/release-evidence/v0.1.0-dogfood.md` - refresh evidence after running the updated dogfood gate on this branch.

## Open Decisions

- **Hard threshold:** Rejected for this slice. One branch run is not enough to set a reliable performance budget.
- **Multiple-run baseline command:** Rejected for this slice. First capture the actual incremental path in the existing gate; a later slice can add repeated-run statistics if this output is stable.
- **Export optimization:** Out of scope. Existing evidence shows JSONL export dominates runtime; this slice measures scan/rescan only.

## Progress

- [x] Task 0: Plan baseline
- [x] Task 1: Add red rescan evidence tests
- [x] Task 2: Run and validate dogfood rescan
- [x] Task 3: Update docs and evidence
- [x] Task 4: Verify focused and branch gates
- [x] Task 5: Harden rescan validation after review

## Tasks

### Task 0: Plan Baseline

**Files:**
- Create: `docs/plans/2026-06-01-dogfood-incremental-rescan-baseline.md`

**What to build:** Capture the design for adding incremental no-change rescan evidence to dogfood.

**Acceptance criteria:**
- [x] Plan uses the Razorback implementation-plan header.
- [x] Plan keeps public product contracts unchanged.
- [x] Plan records rejected shortcuts and report-only metrics.

### Task 1: Add Red Rescan Evidence Tests

**Files:**
- Modify: `xtask/tests/dogfood_contract.rs`

**What to build:** Tests that require dogfood output planning and validation to include a rescan report.

**Acceptance criteria:**
- [x] `repo_args_build_default_output_paths_and_binary` expects `rescan-report.json`.
- [x] Valid fixture evidence includes `rescan-report.json` with `status=no_change`.
- [x] Invalid fixture evidence fails when rescan status is not `no_change` or unchanged file count is zero.
- [x] The test fails before implementation for the expected missing field/validation reason.

### Task 2: Run And Validate Dogfood Rescan

**Files:**
- Modify: `xtask/src/dogfood.rs`

**What to build:** Run a second `julie-extract scan --json` against the same DB after the cold scan and before `info`/`export`.

**Acceptance criteria:**
- [x] Dogfood writes `rescan-report.json`.
- [x] `metrics.json` includes `rescan_duration_ms`.
- [x] Validation requires cold scan `status=ok` and rescan `status=no_change`.
- [x] Validation rejects rescan reports with changed, deleted, failed, or zero unchanged files.
- [x] `cargo test -p xtask --test dogfood_contract` passes.

### Task 3: Update Docs And Evidence

**Files:**
- Modify: `docs/testing-strategy.md`
- Modify: `docs/release.md`
- Modify: `docs/release-evidence/v0.1.0-dogfood.md`

**What to build:** Document the new rescan evidence shape and refresh dogfood evidence with current metrics.

**Acceptance criteria:**
- [x] Testing strategy says dogfood runs cold scan, immediate rescan, info, and JSONL export.
- [x] Evidence document records current commit, command, output paths including `rescan-report.json`, hard gate result, row totals, and report-only rescan timing.
- [x] Generated SQLite, JSONL, reports, and metrics remain under `target/` and are not committed.

### Task 4: Verify Focused And Branch Gates

**What to verify:**
- `cargo test -p xtask --test dogfood_contract`
- `cargo test -p xtask`
- `cargo xtask dogfood repo --root . --out-dir target/dogfood/julie-extractors`
- `python3 examples/python/sqlite_consumer.py target/dogfood/julie-extractors/artifact.sqlite`
- `cargo xtask test default`
- `cargo xtask test contract`

**Acceptance criteria:**
- [x] Focused dogfood contract tests pass.
- [x] Dogfood command writes valid rescan evidence.
- [x] Python consumer reads the generated SQLite artifact.
- [x] Default and contract branch gates pass before merge, push, or PR.

### Task 5: Harden Rescan Validation After Review

**Files:**
- Modify: `xtask/src/dogfood.rs`
- Modify: `xtask/tests/dogfood_contract.rs`
- Modify: `docs/release-evidence/v0.1.0-dogfood.md`

**What to build:** Make dogfood reject `no_change` rescan reports that still
create a revision or write rows.

**Acceptance criteria:**
- [x] Invalid rescan evidence with `revision.created_revision_id` set fails validation.
- [x] Invalid rescan evidence with nonzero `counts.rows_written` fails validation.
- [x] Evidence is refreshed against the commit containing the validation hardening.
