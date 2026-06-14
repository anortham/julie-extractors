# Per-File Extraction Cost Attribution Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use razorback:subagent-driven-development when subagent delegation is available. Fall back to razorback:executing-plans for single-task, tightly-sequential, or no-delegation runs.

**Goal:** Add machine-readable per-file artifact row attribution to JSON reports so consumers can identify files that dominate extraction output.

**Architecture:** Keep attribution as a read-side report view over existing SQLite tables. Add stable `counts.file_rows` and `counts.file_rows_truncated` fields to report JSON, populate them from grouped SQLite counts for successful scan/info reports, and leave writer internals and SQLite schema unchanged.

**Tech Stack:** Rust, `julie-extract-artifact` report contract types, `julie-extract-cli`, SQLite via `rusqlite`, existing report and operations contract tests.

**Architecture Quality:** Strategy-tier public report contract change. The approved shape is `counts.file_rows[]` entries containing `path`, `language`, `status`, `total_rows`, and exhaustive `rows` domain counts, plus `counts.file_rows_truncated` for bounded summaries. Main risk is count drift from `counts.totals`; SQL attribution must avoid join fanout and include only rows attributable to a specific file.

---

## File Structure

- Modify: `crates/julie-extract-artifact/src/reports.rs`
  Add the serializable per-file report entry type and include it in `ReportCounts`.
- Modify: `crates/julie-extract-cli/src/artifact_access.rs`
  Add SQL helpers that compute per-file row attribution from existing artifact tables.
- Modify: `crates/julie-extract-cli/src/commands.rs`
  Populate `counts.file_rows` for successful artifact-backed reports.
- Modify: `crates/julie-extract-artifact/tests/report_contract.rs`
  Lock the JSON report shape and exhaustive per-file row domain keys.
- Modify: `crates/julie-extract-cli/tests/operations_contract.rs`
  Prove `info --json` exposes full persisted attribution and does not mutate the artifact.
- Modify: `docs/contracts/reports.md`
  Document the new report field and command requirements.
- Modify: `TODO.md`
  Mark the architecture-quality item complete with verification evidence.

## Task 1: Lock the Report JSON Shape

**Files:**
- Modify: `crates/julie-extract-artifact/tests/report_contract.rs`
- Modify: `crates/julie-extract-artifact/src/reports.rs`

**What to build:** Add a failing report contract test for `counts.file_rows[]` and `counts.file_rows_truncated`. The entry shape is stable and uses exhaustive SQLite row-domain keys under `rows`.

**Approach:** Add `ReportFileRows` next to `ReportCounts`. Keep `RowDomainCounts` as the nested type so report consumers see the same row-domain vocabulary in `rows_written`, `totals`, and per-file attribution.

**Acceptance criteria:**
- [x] RED test fails before the report type exists.
- [x] `counts.file_rows[0].path`, `language`, `status`, `total_rows`, and `rows.*` serialize in snake-case JSON.
- [x] Per-file `rows` contains every SQLite schema v3 domain with zeroes for non-file-attributable domains.
- [x] `counts.file_rows_truncated` serializes as a boolean.

## Task 2: Compute Attribution From SQLite

**Files:**
- Modify: `crates/julie-extract-cli/src/artifact_access.rs`
- Modify: `crates/julie-extract-cli/src/commands.rs`
- Modify: `crates/julie-extract-cli/tests/operations_contract.rs`

**What to build:** Add a read-side helper that returns per-file row counts sorted by descending `total_rows`, then path. Populate successful scan reports with the largest files up to a small cap and `info --json` reports with the full persisted breakdown.

**Approach:** Use one grouped SQL query over `files` with correlated subqueries for each row domain. Count direct `file_id` tables directly. Count symbol-owned rows through `symbols.file_id`, and count `type_arguments` through `type_argument_usages.file_id`. Do not attribute artifact metadata, parser inventory, language capabilities, extraction revisions, or revision file changes to individual files.

**Acceptance criteria:**
- [x] `info --json` includes one `counts.file_rows` entry per persisted file.
- [x] Per-file sums match the file-attributable portion of `counts.totals` for the tested artifact.
- [x] The helper does not mutate artifact metadata or revisions.
- [x] Failed/no-artifact reports keep `file_rows` empty through the default report shape.

## Verification Strategy

**Project source of truth:** `AGENTS.md`, `RAZORBACK.md`, `docs/testing-strategy.md`, `docs/contracts/reports.md`, and existing CLI/artifact contract tests.

**Worker red/green scope:** `cargo test -p julie-extract-artifact --test report_contract` and `cargo test -p julie-extract-cli --test operations_contract <new_test_name>`.

**Worker ceiling:** `cargo test -p julie-extract-artifact --test report_contract` and `cargo test -p julie-extract-cli --test operations_contract`.

**Worker gate invariant:** The report shape is stable, per-file rows use exhaustive row-domain keys, and persisted `info --json` attribution matches existing SQLite rows without mutating the artifact.

**Lead affected-change scope:** `cargo test -p julie-extract-artifact --test report_contract`, `cargo test -p julie-extract-cli --test operations_contract`, `cargo fmt --all --check`, and `cargo clippy --workspace --all-targets --all-features --no-deps -- -D warnings`.

**Branch gate:** `cargo xtask test default`. Run `cargo xtask test contract` because this changes the public report contract.

**Replay/metric evidence:** No replay metric is required. Row-count agreement between per-file attribution and `counts.totals` is a hard gate in the focused CLI test.

**Escalation triggers:** Any SQLite schema change, writer behavior change, row-domain rename, report schema version debate, or unexpected default-suite runtime growth requires strategy-tier review.

**Assigned verification failure:** Workers stop and report when assigned verification fails, unless the failure is the expected RED test before implementation.

**Verification ledger:** Record invariant, command, scope label, commit SHA, result, and timestamp in the final checkpoint.

## Model Routing

**Project source of truth:** `RAZORBACK.md`.

**Strategy tier:** Public report contract shape and count interpretation.
- Harness mapping: inherit.

**Implementation tier:** Bounded SQL helper and report wiring after the contract shape is decided.
- Harness mapping: inherit.

**Mechanical tier:** Docs, imports, formatting, and TODO updates after tests pass.
- Harness mapping: inherit.

**Gate-interpretation reviewer:** Strategy tier for any mismatch between per-file sums and artifact totals.
- Harness mapping: inherit.

**Escalation tier:** Schema/API/report/release/capability/parser dependency changes or repeated verification failures.
- Harness mapping: inherit.

**Worker eligibility:** Workers are not used for this run because this is a single tightly coupled public contract slice.

**Escalation triggers:** Same as `RAZORBACK.md`: public schema/report changes, weak evidence, old Julie coupling, or unexpected default-suite runtime growth.

**Mechanical exclusion:** Mechanical workers cannot own failing tests, report-contract evidence, or acceptance gates.

**Unsupported harness behavior:** This session does not require per-agent model routing; inherit the active harness defaults.
