# JSONL Export Performance Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use razorback:subagent-driven-development when subagent delegation is available. Fall back to razorback:executing-plans for single-task, tightly-sequential, or no-delegation runs.

**Goal:** Reduce JSONL export runtime by fixing the first verified bottleneck in the export path while preserving JSONL v1, SQLite v1, CLI, and report contracts.

**Architecture:** Keep SQLite as the canonical artifact and JSONL as a deterministic export derived from SQLite rows. The first implementation target is the JSONL writer boundary because current evidence points to unbuffered output writes, not SQLite reads, as the dominant cost. Do not change record order, payload keys, report counts, schema versions, or artifact table/index contracts.

**Tech Stack:** Rust, `rusqlite`, `serde_json`, `std::io::BufWriter`, `julie-extract` CLI, SQLite artifact fixtures, `cargo xtask` dogfood evidence.

**Architecture Quality:** Low to medium risk. The public product contracts stay unchanged, but the exporter is public crate behavior and a CLI path, so tests must prove byte-for-byte contract stability and bounded write behavior through caller-facing APIs.

---

## Source Documents

- `AGENTS.md`: product boundary, SQLite-primary output, JSONL-secondary output, CLI-first integration, and default-suite discipline.
- `RAZORBACK.md`: strategy-tier areas, worker eligibility, escalation triggers, and verification ownership.
- `docs/contracts/jsonl-v1.md`: JSONL envelope, record order, and payload schemas.
- `docs/contracts/reports.md`: `export` report shape and stdout/stderr rules.
- `docs/contracts/sqlite-schema-v1.md`: canonical SQLite row domains and required indexes.
- `docs/testing-strategy.md`: default, contract, dogfood, and release evidence gates.
- `docs/plans/2026-06-01-product-completion-tracker.md`: slice order and acceptance criteria.
- `docs/release-evidence/2026-06-01-release-binary-dogfood.md`: release-binary dogfood timings that triggered this plan.

## Current Baseline

- Release-binary dogfood evidence at `a3038ee` recorded cold scan `7607ms`, immediate no-change rescan `52ms`, and JSONL export `68983ms`.
- Debug-profile dogfood evidence recorded JSONL export `76175ms`.
- `crates/julie-extract-artifact/src/jsonl.rs:106-135` exports one deterministic record-kind group at a time from SQLite.
- `crates/julie-extract-artifact/src/jsonl.rs:1277-1301` builds a `serde_json::Value` envelope for every row and calls `serde_json::to_writer` directly against the supplied writer.
- `crates/julie-extract-cli/src/commands.rs:555-596` opens the SQLite artifact read-only and writes export output directly to `File` or stdout.

## Inspection Evidence

Read-only timing against the existing local dogfood artifact
`/Users/murphy/source/julie-extractors/target/dogfood/julie-extractors/artifact.sqlite`:

- SQLite table counts across every JSONL row domain: `0.158s`.
- Fetching every row from every exported table: `213232` rows in `0.763s`.
- Python JSON sample for `30000` large-row records: `22.9MB` in `0.252s`.
- Release binary export to `/dev/null`:
  `real 20.79s`, `user 3.83s`, `sys 15.66s`.

Conclusion: the first verified bottleneck is output write behavior, most likely
many small writes from `serde_json::to_writer` into an unbuffered `File` or
stdout sink. SQLite reads are not the first bottleneck. JSON serialization may
still matter after buffering, but it is not the first implementation target.

## Architecture Quality

**Affected modules:** `crates/julie-extract-artifact/src/jsonl.rs`,
`crates/julie-extract-artifact/tests/jsonl_contract.rs`,
`crates/julie-extract-cli/tests/operations_contract.rs`, release evidence docs,
and this tracker plan.

**Caller-facing interface:** `export_jsonl`, `export_jsonl_to_path`, and
`julie-extract export --format jsonl` keep the same signatures, command flags,
record order, record payloads, report counts, and error/status behavior.

**Depth/locality check:** Keep the optimization inside the artifact JSONL
exporter before changing SQL queries, schema indexes, CLI report fields, or
dogfood tooling. The writer boundary is smaller than the behavior it unlocks
and does not require old Julie internals.

**Test surface:** Prove behavior through the same interfaces callers use:
`export_jsonl`, `export_jsonl_to_path`, and the CLI `export` integration test.
Use a counting writer for the hard write-call invariant instead of timing-based
unit tests.

**Seams/adapters:** No new abstraction is needed for the first implementation.
Use `BufWriter` at the existing writer boundary. If buffering alone does not
materially improve release evidence, the next candidate is a reusable
per-record line buffer that serializes one complete line before one `write_all`.

**Rejected shortcuts:** Do not change JSONL v1 payloads, omit record kinds,
compress output, add an alternate format, move export state into SQLite helper
tables, make dogfood timings a hard budget, or copy old Julie export code.

**Architecture risk:** Low to medium. The implementation is local, but any
export bug affects every non-Rust consumer that reads JSONL.

## Verification Strategy

**Project source of truth:** `AGENTS.md`, `RAZORBACK.md`,
`docs/testing-strategy.md`, and the contracts under `docs/contracts/`.

**Worker red/green scope:** `cargo test -p julie-extract-artifact --test jsonl_contract buffered_export_uses_bounded_write_calls`

**Worker ceiling:** `cargo test -p julie-extract-artifact --test jsonl_contract`
and `cargo test -p julie-extract-cli --test operations_contract export_jsonl_emits_valid_jsonl_records_from_scanned_artifact`.

**Worker gate invariant:** JSONL export keeps exact v1 records and order while
using a bounded number of downstream writer calls for a multi-record export.

**Lead affected-change scope:** `cargo test -p julie-extract-artifact --test jsonl_contract`,
`cargo test -p julie-extract-cli --test operations_contract export_jsonl_emits_valid_jsonl_records_from_scanned_artifact`,
and `cargo build --release -p julie-extract-cli --bin julie-extract`.

**Branch gate:** `cargo xtask test default` and `cargo xtask test contract`
before merge, push, or PR.

**Replay/metric evidence:** Hard gates are JSONL contract tests, CLI export
report counts, valid JSONL parseability, and the bounded-write regression test.
Report-only metrics are release export time to `/dev/null`, release dogfood
export duration, artifact bytes, JSONL bytes, and rows per second. Do not set a
hard wall-clock budget until the repeatable performance baseline slice records
variance.

**Escalation triggers:** Public JSONL/SQLite/report/CLI contract changes,
parser dependency changes, default-suite runtime growth, weak dogfood evidence,
or a buffering implementation that fails to materially reduce release export
time.

**Assigned verification failure:** Workers stop and report when assigned
verification fails unless this plan explicitly says to update that gate.

**Verification ledger:** Record invariant, command, scope label, commit SHA,
result, and timestamp. For replay or metric evidence, record hard-gate metrics
and report-only metrics. If the same HEAD already has a passing ledger entry for
the required scope, reuse that evidence instead of rerunning the same expensive
gate.

## Model Routing

**Project source of truth:** `RAZORBACK.md`.

**Strategy tier:** Planning, architecture, performance acceptance, public
contract interpretation, and review finding triage.
- Harness mapping: inherit in this Codex session.

**Implementation tier:** Bounded edits to JSONL export buffering and contract
tests after the public interface stays unchanged.
- Harness mapping: inherit.

**Mechanical tier:** Formatting and wording-only documentation edits.
- Harness mapping: inherit.

**Gate-interpretation reviewer:** Lead interprets failed contract,
bounded-write, dogfood, or performance evidence.
- Harness mapping: inherit.

**Escalation tier:** Public artifact contract changes, CLI status/count changes,
weak tests, repeated verification failures, or metric evidence that contradicts
the chosen bottleneck.
- Harness mapping: inherit.

**Worker eligibility:** Workers are eligible only when file ownership is narrow,
the public interface is already decided, and the verification ceiling is
explicit.

**Escalation triggers:** Any change to public artifact schema, CLI status, exit
code, error code, language capability claim, parser dependency version, or
default-suite runtime.

**Mechanical exclusion:** Mechanical workers cannot own failing tests, replay
evidence, metrics, or acceptance gates.

**Unsupported harness behavior:** If the harness cannot choose models per
agent, use `inherit` and continue.

## File Structure

- Create: `docs/plans/2026-06-01-jsonl-export-performance.md` - this plan and progress ledger.
- Modify: `crates/julie-extract-artifact/src/jsonl.rs:106-160` - buffer JSONL export writes without changing record construction or output shape.
- Modify: `crates/julie-extract-artifact/tests/jsonl_contract.rs:13-386` - add a bounded-write regression test and keep exact contract tests green.
- Modify if needed: `crates/julie-extract-cli/src/commands.rs:555-596` - only if artifact-level buffering does not cover CLI file and stdout exports.
- Modify if needed: `crates/julie-extract-cli/tests/operations_contract.rs:346-388` - keep CLI export behavior and reports stable.
- Create: `docs/release-evidence/2026-06-01-jsonl-export-buffering.md` - record before/after report-only export evidence when the implementation lands.
- Modify: `docs/plans/2026-06-01-product-completion-tracker.md` - update current slice status and next-slice order.

## Open Decisions

- **Buffer size:** Start with a named internal constant, `64 * 1024` bytes. This
  is large enough to collapse small serde writes without making memory scale
  with artifact size.
- **Fallback if `BufWriter` is not enough:** Use a reusable per-record `Vec<u8>`
  line buffer and write one newline-terminated record at a time. Do not start
  there unless report-only metrics show buffering alone is insufficient.
- **CLI path output atomicity:** `export_jsonl_to_path` already writes through a
  temporary path and rename; the CLI currently writes directly to the requested
  path. Do not change CLI atomicity in this performance slice unless a test
  exposes a data-loss bug on the path already being edited.
- **Hard timing budgets:** Rejected for this slice. The repeatable baseline
  slice owns min/median/max data and budget decisions.

## Progress

- [x] Task 0: Plan baseline and bottleneck identification
- [ ] Task 1: Add bounded-write red test
- [ ] Task 2: Buffer JSONL export writes
- [ ] Task 3: Verify CLI export contract and release-profile metric evidence
- [ ] Task 4: Update evidence and tracker

## Tasks

### Task 0: Plan Baseline And Bottleneck Identification

**Files:**
- Create: `docs/plans/2026-06-01-jsonl-export-performance.md`
- Modify: `docs/plans/2026-06-01-product-completion-tracker.md`
- Modify: `.memories/briefs/julie-extractors-product-completion-focus.md`

**What to build:** Capture the inspected JSONL export path and the evidence that
write behavior is the first bottleneck to attack.

**Approach:** Use Julie code intelligence before writing the plan. Record source
locations, local read-only timing evidence, rejected shortcuts, and exact
verification scopes. Keep this as a plan-only slice; implementation starts after
this plan merges.

**Acceptance criteria:**
- [x] Plan identifies SQLite reads, JSON serialization, file writes, and process
  overhead separately.
- [x] Plan states the first implementation target and why.
- [x] Plan keeps JSONL, SQLite, CLI, and report contracts unchanged.
- [x] Plan separates hard gates from report-only metrics.

### Task 1: Add Bounded-Write Red Test

**Files:**
- Modify: `crates/julie-extract-artifact/tests/jsonl_contract.rs:13-386`

**What to build:** Add a failing regression test proving JSONL export does not
write tiny chunks directly to the downstream sink.

**Approach:** Add a `CountingWriter` test helper that records `write` call count
and bytes written. Export a multi-record SQLite fixture through `export_jsonl`
and assert the resulting JSONL is valid, record order is unchanged, and writer
calls are bounded by buffer-sized chunks rather than serde token writes.

**Acceptance criteria:**
- [ ] The test fails before buffering because downstream write calls are too
  high.
- [ ] The test asserts JSONL parseability and record count, not only write-call
  count.
- [ ] The threshold is based on output bytes and the chosen buffer size, not a
  wall-clock duration.

### Task 2: Buffer JSONL Export Writes

**Files:**
- Modify: `crates/julie-extract-artifact/src/jsonl.rs:106-160`

**What to build:** Buffer JSONL export writes at the artifact exporter boundary
so all callers benefit without changing the public API.

**Approach:** Wrap the supplied writer in `BufWriter::with_capacity` inside
`export_jsonl`, keep the existing export helper call order, and flush explicitly
before returning the summary. Keep `write_record` focused on envelope
serialization and summary updates.

**Acceptance criteria:**
- [ ] `export_jsonl` output remains byte-for-byte valid JSONL v1.
- [ ] `export_jsonl_to_path` still removes incomplete temporary output on export
  failure.
- [ ] `export --out - --json` still writes JSONL to stdout and the report to
  stderr.
- [ ] No public schema, report, CLI, or crate API changes are introduced.

### Task 3: Verify CLI Export Contract And Release-Profile Metrics

**Files:**
- Modify if needed: `crates/julie-extract-cli/src/commands.rs:555-596`
- Modify if needed: `crates/julie-extract-cli/tests/operations_contract.rs:346-388`

**What to build:** Prove the CLI export surface still behaves as documented and
capture report-only release-profile timing after buffering.

**Approach:** Run the focused artifact and CLI export tests first. Build the
release binary and run one release export to `/dev/null` against the existing
dogfood artifact for before/after comparison. If the implementation changes the
CLI path, run dogfood through the release binary and Python SQLite consumer.

**Acceptance criteria:**
- [ ] CLI export report `status`, `operation`, `mode`, row counts, and
  stdout/stderr behavior remain stable.
- [ ] Release export to `/dev/null` is recorded as report-only evidence.
- [ ] If dogfood is rerun, generated SQLite, JSONL, reports, and metrics remain
  under `target/`.

### Task 4: Update Evidence And Tracker

**Files:**
- Create: `docs/release-evidence/2026-06-01-jsonl-export-buffering.md`
- Modify: `docs/plans/2026-06-01-product-completion-tracker.md`
- Modify: `.memories/briefs/julie-extractors-product-completion-focus.md`

**What to build:** Preserve the performance evidence and keep the project
tracker pointed at the correct next slice.

**Approach:** Record hard-gate results separately from timings. If buffering
reduces release export `real` and `sys` time against the same artifact, the
next slice remains repeatable performance baseline. If it does not, update the
tracker to run the per-record line-buffer candidate before baseline work.

**Acceptance criteria:**
- [ ] Evidence records commit, binary path/profile, command, artifact row
  counts, JSONL bytes, and report-only export timings.
- [ ] Tracker records whether the next slice is repeatable baseline or the
  fallback line-buffer candidate.
- [ ] Default and contract branch gates pass before PR.
