# Product Completion Tracker Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use razorback:subagent-driven-development when subagent delegation is available. Fall back to razorback:executing-plans for single-task, tightly-sequential, or no-delegation runs.

**Goal:** Keep the standalone extraction product work focused after the bootstrap and performance-baseline merges.

**Architecture:** Treat this as the current project tracker, not a replacement for the detailed slice plans. Keep the product boundary unchanged: source tree to versioned extraction artifact, SQLite primary, JSONL secondary, `julie-extract` CLI primary, Rust crate secondary.

**Tech Stack:** Rust workspace, `julie-extract`, SQLite, JSONL, `cargo xtask`, GitHub Actions, release evidence docs, Goldfish briefs.

**Architecture Quality:** No product architecture impact. This plan coordinates remaining slices and records which completed plans are now reference material.

---

## Current Status

- `main` is at `875ee0f` after PR #3.
- CI Fast Gates passed on `main` after the PR #3 fast-forward push.
- No active product implementation branch is open.
- All migration and post-bootstrap plans below are complete and should be treated as historical evidence, not active task queues.
- Julie code intelligence is available again for this repo. Local Julie state is workspace tooling, not product code.

## Completed Milestones

- **Standalone product contracts:** completed in `docs/contracts/` and architecture docs.
- **Repo bootstrap:** completed in `docs/plans/2026-05-31-repo-bootstrap-implementation-plan.md`.
- **Old Julie code migration:** completed in `docs/plans/2026-05-31-julie-code-migration-implementation-plan.md`.
- **Post-bootstrap release readiness:** completed in `docs/plans/2026-06-01-post-bootstrap-stabilization-release-readiness.md`.
- **Release binary workflow:** completed in `docs/plans/2026-06-01-release-binaries-workflow.md`.
- **Incremental scan hash skip:** completed in `docs/plans/2026-06-01-incremental-scan-hash-skip.md`.
- **Dogfood no-change rescan baseline:** completed in `docs/plans/2026-06-01-dogfood-incremental-rescan-baseline.md`.

## Current Evidence

- PR #3 dogfood evidence: `docs/release-evidence/v0.1.0-dogfood.md`.
- Latest dogfood hard evidence:
  - cold scan `status=ok`;
  - immediate rescan `status=no_change`;
  - `created_revision_id=null`;
  - every rescan `counts.rows_written` value is `0`;
  - report-only timing: cold scan `18189ms`, rescan `215ms`, export `76771ms`.
- Release binary workflow evidence: `docs/release-evidence/2026-06-01-release-binaries-workflow.md`.
- Autonomous run report for PR #3: `.memories/autonomous-run-2026-06-01-dogfood-rescan-baseline.md`.

## Non-Goals To Keep Out

- Do not add Julie MCP/server/daemon/search/embedding/watcher/dashboard/editing behavior.
- Do not back-port extractor product work into `/Users/murphy/source/julie` unless explicitly asked.
- Do not change SQLite, JSONL, report, or CLI contracts casually. Route those through a fresh strategy-tier plan.
- Do not turn dogfood, certification, real-world, or release package gates into the default suite.
- Do not promote one-machine dogfood timings into hard performance thresholds.

## Ordered Next Slices

### Slice 1: Release-Binary Dogfood Evidence

**Why now:** The current dogfood performance evidence uses the default debug binary. We need release-binary evidence before setting budgets or optimizing based on timings.

**Expected files:**
- Modify or create release evidence under `docs/release-evidence/`.
- Modify this tracker if the resulting evidence changes the next-slice order.

**Verification:**
- `cargo build --release -p julie-extract-cli --bin julie-extract`
- `cargo xtask dogfood repo --root . --out-dir target/dogfood/julie-extractors-release --binary target/release/julie-extract`
- `python3 examples/python/sqlite_consumer.py target/dogfood/julie-extractors-release/artifact.sqlite`
- Branch gate only when committing code or release docs: `cargo xtask test default` and `cargo xtask test contract`.

**Acceptance criteria:**
- [ ] Evidence records the release binary path, commit, timestamp, hard-gate statuses, row totals, artifact sizes, and report-only timings.
- [ ] Evidence makes clear whether timings are debug or release profile.
- [ ] Generated artifacts remain under `target/`.

### Slice 2: JSONL Export Performance Plan

**Why next:** Current evidence shows JSONL export dominates runtime. Do not optimize scan again until export has been inspected.

**Expected files:**
- Create a fresh plan under `docs/plans/`.
- Likely inspect `crates/julie-extract-artifact` export code and CLI export path before deciding implementation.

**Verification:**
- Start with Julie orientation and a plan. Do not start by running broad tests.
- Worker red/green scope depends on the chosen export bottleneck.
- Branch gate before PR: `cargo xtask test default` and `cargo xtask test contract`.

**Acceptance criteria:**
- [ ] Plan identifies whether bottleneck is SQLite reads, JSON serialization, file writes, or process/profile overhead.
- [ ] Plan separates hard contract invariants from report-only performance evidence.
- [ ] No JSONL contract changes unless explicitly approved.

### Slice 3: Repeatable Performance Baseline

**Why after release evidence/export inspection:** Thresholds need repeated same-machine runs and release-profile data, not one debug dogfood run.

**Expected files:**
- Plan under `docs/plans/`.
- Evidence under `docs/release-evidence/` or a dedicated performance evidence doc.

**Verification:**
- Use a repeatable command or documented script only after the evidence shape is planned.
- Keep metrics report-only until repeated data supports a threshold.

**Acceptance criteria:**
- [ ] Captures cold scan, no-change rescan, export, artifact sizes, and row counts across repeated runs.
- [ ] Records variance or at least min/median/max before proposing hard budgets.
- [ ] Keeps default tests fast.

### Slice 4: v0.1.0 Release Candidate Audit

**Why later:** This is meaningful after release-binary dogfood evidence and any export performance decision.

**Expected files:**
- Update release evidence, release notes, and this tracker.

**Verification:**
- `cargo xtask test default`
- `cargo xtask test contract`
- Specialist gates only where the audit touches their owned areas.

**Acceptance criteria:**
- [ ] Release package staging evidence is current.
- [ ] Release notes match actual shipped surfaces and known non-goals.
- [ ] No generated artifacts are committed.

## Verification Discipline

- Reuse same-HEAD passing evidence from plan ledgers instead of rerunning broad gates for status updates.
- Run focused tests after implementation changes.
- Run `cargo xtask test default` and `cargo xtask test contract` before merge, push, or PR.
- Run dogfood only for evidence-producing slices or dogfood-affecting changes.
- Do not run certification, real-world, dogfood, or release package staging as a reflex.

## Progress

- [x] Tracker created after PR #3 merge.
- [ ] Slice 1: Release-binary dogfood evidence.
- [ ] Slice 2: JSONL export performance plan.
- [ ] Slice 3: Repeatable performance baseline.
- [ ] Slice 4: v0.1.0 release candidate audit.
