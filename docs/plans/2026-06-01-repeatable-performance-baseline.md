# Repeatable Performance Baseline Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use razorback:subagent-driven-development when subagent delegation is available. Fall back to razorback:executing-plans for single-task, tightly-sequential, or no-delegation runs.

**Goal:** Add a repeatable release-profile performance baseline for the standalone extraction product without turning timing into a default-suite gate.

**Architecture:** Keep the existing dogfood gate as the single-run hard validator for public `julie-extract` behavior. Add repo tooling that runs that gate repeatedly into isolated output directories, then writes an aggregate JSON summary with min, median, and max for the report-only performance fields. The public CLI, SQLite schema, JSONL schema, report contract, and Rust crate APIs remain unchanged.

**Tech Stack:** Rust `xtask`, existing dogfood runner, `serde_json`, release `julie-extract` binary, SQLite and JSONL dogfood artifacts, release evidence markdown.

**Architecture Quality:** Low to medium risk. The change is release-evidence tooling, not product behavior, but it owns performance evidence interpretation, so the command must separate hard dogfood validity from report-only timing metrics and must not leak into the default suite.

---

## Source Documents

- `AGENTS.md`: product boundary, SQLite-primary output, JSONL-secondary output, CLI-first integration, and default-suite discipline.
- `RAZORBACK.md`: strategy-tier performance evidence ownership, worker eligibility, and escalation triggers.
- `docs/testing-strategy.md`: default, contract, dogfood, release evidence, and CI tier boundaries.
- `docs/plans/2026-06-01-product-completion-tracker.md`: Slice 4 acceptance criteria.
- `docs/release-evidence/2026-06-01-release-binary-dogfood.md`: release-profile single-run baseline before JSONL buffering.
- `docs/release-evidence/2026-06-01-jsonl-export-buffering.md`: post-buffering report-only export evidence.

## Current Baseline

- PR #6 merged at `bac074a` and CI Fast Gates passed before merge.
- Existing single-run dogfood command:
  `cargo xtask dogfood repo --root . --out-dir target/dogfood/julie-extractors --binary target/release/julie-extract`.
- Existing dogfood metrics already include cold scan, immediate no-change rescan, info, export, row totals, JSONL record totals, artifact sizes, and rows per second.
- Post-buffering release export to `/dev/null` is `2.43s` real and `0.21s` sys, but that is one run against one existing artifact. It is evidence, not a baseline.

## Architecture Quality

**Affected modules:** `xtask/src/performance.rs`, `xtask/src/commands.rs`, `xtask/src/lib.rs`, `xtask/tests/performance_baseline_contract.rs`, release evidence docs, the product tracker, and the active brief.

**Caller-facing interface:** Add repo tooling only:

```bash
cargo xtask performance baseline \
  --root . \
  --out-dir target/performance/julie-extractors-baseline \
  --binary target/release/julie-extract \
  --runs 3
```

The command runs the existing dogfood gate once per run under
`<out-dir>/run-001`, `<out-dir>/run-002`, and so on, then writes
`<out-dir>/baseline-summary.json`.

**Hard gates:** Every run must pass the dogfood validator: scan report `ok`,
rescan report `no_change`, no rescan rows written, valid SQLite artifact,
valid JSONL export, expected schema versions, nonzero files, and nonzero
symbols.

**Report-only metrics:** Durations, rows per second, artifact bytes, row totals,
JSONL record counts, min, median, and max remain report-only. This slice does
not set a wall-clock pass/fail threshold.

**Rejected shortcuts:** Do not hand-write one-off shell loops as the only
repeat mechanism, do not commit generated artifacts, do not run dogfood from
the default suite, do not change public artifact contracts, and do not promote
one-machine timings into release budgets.

## Verification Strategy

**Project source of truth:** `AGENTS.md`, `RAZORBACK.md`,
`docs/testing-strategy.md`, and `xtask` contract tests.

**Worker red/green scope:** `cargo test -p xtask --test performance_baseline_contract`.

**Worker ceiling:** `cargo test -p xtask`.

**Worker gate invariant:** The performance baseline command requires at least
three runs, creates deterministic per-run directories, aggregates min/median/max
correctly, and serializes enough evidence to compare cold scan, rescan, export,
artifact sizes, row counts, and JSONL counts.

**Lead affected-change scope:** `cargo test -p xtask`, `cargo build --release -p julie-extract-cli --bin julie-extract`, and one release-profile baseline command under `target/performance/`.

**Branch gate:** `cargo xtask test default` and `cargo xtask test contract`
before push or PR.

**Replay/metric evidence:** Hard gates are the xtask contract tests and every
dogfood run passing validation. Report-only metrics are the aggregate summary
values and the evidence markdown tables. Record min, median, and max; do not set
thresholds in this slice.

**Escalation triggers:** Public CLI/report/schema changes, default-suite runtime
growth, dogfood evidence failures, inconsistent row counts across runs, or
variance high enough to make the baseline misleading without explanation.

**Assigned verification failure:** Workers stop and report when assigned
verification fails unless this plan explicitly says to update that gate.

**Verification ledger:** Record invariant, command, scope label, commit SHA,
result, and timestamp. For the release-profile baseline, record run count,
binary SHA-256, output directory, hard-gate result, and report-only min/median/max.

## Model Routing

**Project source of truth:** `RAZORBACK.md`.

**Strategy tier:** Planning, architecture, performance acceptance, release
evidence interpretation, and review finding triage.
- Harness mapping: inherit in this Codex session.

**Implementation tier:** Bounded `xtask` command and contract tests after the
public product interface stays unchanged.
- Harness mapping: inherit.

**Mechanical tier:** Formatting and wording-only documentation edits.
- Harness mapping: inherit.

**Gate-interpretation reviewer:** Lead interprets failed dogfood, baseline, or
contract evidence.
- Harness mapping: inherit.

**Escalation tier:** Public artifact contract changes, CLI/report status
changes, weak performance evidence, repeated verification failures, or default
suite runtime growth.
- Harness mapping: inherit.

**Worker eligibility:** Workers are eligible only when file ownership is narrow,
the public interface is already decided, and the verification ceiling is
explicit.

**Mechanical exclusion:** Mechanical workers cannot own failing tests, replay
evidence, metrics, or acceptance gates.

**Unsupported harness behavior:** If the harness cannot choose models per
agent, use `inherit` and continue.

## File Structure

- Create: `xtask/src/performance.rs` - parse and run `performance baseline`,
  call the existing dogfood runner repeatedly, aggregate metrics, and write
  `baseline-summary.json`.
- Modify: `xtask/src/commands.rs` - route `cargo xtask performance baseline`.
- Modify: `xtask/src/lib.rs` - expose the new `performance` module.
- Create: `xtask/tests/performance_baseline_contract.rs` - test argument
  parsing, run directory planning, minimum run count, aggregate math, and JSON
  summary shape without running real dogfood.
- Create: `docs/release-evidence/2026-06-01-repeatable-performance-baseline.md`
  - record the actual release-profile baseline output.
- Modify: `docs/plans/2026-06-01-product-completion-tracker.md` - mark Slice 4
  evidence and keep Slice 5 next.
- Modify: `.memories/briefs/julie-extractors-product-completion-focus.md` -
  update current status and next slice.

## Open Decisions

- **Run count:** Require at least `3` runs. That is enough to compute
  min/median/max and small enough for a same-session release evidence run.
- **Median definition:** Sort numeric samples and use the middle value for odd
  run counts. For even run counts, average the two middle values.
- **Output shape:** Keep the JSON summary intentionally boring: root path,
  binary path, run count, per-run metrics, and aggregate metrics. The evidence
  markdown is the human release note; the JSON file is the machine-readable
  source.
- **Thresholds:** Do not add thresholds here. Slice 5 may choose budgets only
  after reading this baseline.

## Tasks

### Task 1: Add Baseline Command Contract Tests

**Files:**
- Create: `xtask/tests/performance_baseline_contract.rs`

**What to build:** Tests that define the public repo-tooling behavior before
implementation.

**Acceptance criteria:**
- [ ] `performance baseline` requires `--root`, `--out-dir`, `--binary`, and
  `--runs`.
- [ ] `--runs 1` and `--runs 2` fail with a clear error.
- [ ] Per-run output directories are planned as `run-001`, `run-002`, etc.
- [ ] Aggregate summaries compute min, median, and max from sample metrics.
- [ ] Serialized summary contains per-run metrics and aggregate metrics.

### Task 2: Implement Repeatable Baseline Runner

**Files:**
- Create: `xtask/src/performance.rs`
- Modify: `xtask/src/commands.rs`
- Modify: `xtask/src/lib.rs`

**What to build:** A repo-tooling command that repeatedly calls the existing
dogfood runner and writes `baseline-summary.json`.

**Acceptance criteria:**
- [ ] Uses existing `dogfood::run_repo` validation instead of duplicating hard
  evidence checks.
- [ ] Writes generated artifacts only under the requested `--out-dir`.
- [ ] Does not change public product CLI, schema, JSONL, or report contracts.
- [ ] Keeps timing and rows-per-second metrics report-only.

### Task 3: Run Release-Profile Baseline And Record Evidence

**Files:**
- Create: `docs/release-evidence/2026-06-01-repeatable-performance-baseline.md`
- Modify: `docs/plans/2026-06-01-product-completion-tracker.md`
- Modify: `.memories/briefs/julie-extractors-product-completion-focus.md`

**What to build:** Execute the baseline against the release binary and record
the hard-gate result plus report-only min/median/max.

**Acceptance criteria:**
- [ ] Evidence records commit, timestamp, command, binary SHA-256, binary size,
  output directory, and generated summary path.
- [ ] Evidence records cold scan, no-change rescan, export, artifact sizes, row
  counts, and JSONL record counts across repeated runs.
- [ ] Evidence explicitly says timings are report-only and no threshold was
  introduced.
- [ ] Tracker marks Slice 4 complete and leaves Slice 5 as the next active
  slice.

### Task 4: Verify And Finish Branch

**Files:**
- `.memories/autonomous-run-2026-06-01-repeatable-performance-baseline.md`

**What to build:** Final run ledger and PR.

**Acceptance criteria:**
- [ ] `cargo fmt --all -- --check` passes.
- [ ] `cargo test -p xtask` passes.
- [ ] `cargo xtask test default` passes.
- [ ] `cargo xtask test contract` passes.
- [ ] PR Fast Gates pass on the final branch head.

## Progress

- [x] Task 0: Plan Slice 4 shape from existing dogfood and release evidence.
- [ ] Task 1: Add baseline command contract tests.
- [ ] Task 2: Implement repeatable baseline runner.
- [ ] Task 3: Run release-profile baseline and record evidence.
- [ ] Task 4: Verify and finish branch.
