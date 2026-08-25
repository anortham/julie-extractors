# Store Update Gating And Coordinator Queue Hygiene Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use razorback:subagent-driven-development when subagent delegation is available. Fall back to razorback:executing-plans for single-task, tightly-sequential, or no-delegation runs.

**Goal:** Close TODO entries 19–23: gate `store update` behind scan's discovery decisions, make coordinator reports honest and the queue self-draining, reap dead-requester rows, and publish discovery limits in `languages --json`.

**Architecture:** Reuse the existing discovery policy (`FilePolicy::select_file`) at the update enqueue point; extend the coordinator's existing renewable-quantum and lease-takeover mechanisms rather than adding new ones; extend the existing `languages` report payload.

**Tech Stack:** Rust, SQLite store coordinator, julie-extract CLI report contracts.

**Architecture Quality:** No new modules. Each fix lands inside the subsystem that owns the defect. Risk: coordinator.rs is shared by Tasks B and C, so C serializes behind B.

## Global Constraints

- Terminal report states are API contracts: an update refused by discovery reports `unsupported`, never an error exit and never a queue row.
- The caller's report `state` reflects only the caller's own request; backlog outcomes travel in a warning field.
- Queue rows must reach a terminal state in bounded drains: overruns are counted, and after 3 overruns the row fails with `failure_class=coordinator_quantum`.
- `languages --json` additions are additive; bump the report schema only if the repo's contract docs require it for additive fields (check `docs/contracts/`, follow precedent).
- TODO.md entries 19–23 flip to `closed` with a one-line resolution each, in the same commit as their fix.
- Every fix carries a regression test that fails on the pre-fix behavior.

## Verification Strategy

**Project source of truth:** `AGENTS.md`, `docs/release.md`, `docs/contracts/`, `xtask`.

**Worker red/green scope:** Focused cargo tests for the owned subsystem (`cargo test -p julie-extract-cli <filter>`, `cargo test -p julie-extract-artifact <filter>`).

**Worker ceiling:** Owned-crate test targets. Workers do not run default/golden/branch gates.

**Worker gate invariant:** Each regression test reproduces the TODO's evidence scenario and passes only with the fix.

**Lead affected-change scope:** `cargo test -p julie-extract-cli` and `cargo test -p julie-extract-artifact` after each landed task.

**Branch gate:** `cargo fmt --check`, `cargo test -p xtask`, `cargo xtask test default`, `cargo xtask test contract`, strict data-quality report, doc-sync check, Windows default suite at the final SHA.

**Security scope:** `gitleaks git v2.36.0..HEAD`; `cargo audit`.

**Escalation triggers:** Any store schema change (none expected) escalates to contract review; any change to report shapes updates `docs/contracts/` in the same task.

**Assigned verification failure:** Workers stop and report when assigned verification fails.

**Verification ledger:** Maintained in the SDD workspace ledger.

## Parallel Execution Contract

| Task | Parallel batch | File ownership | Serialization required | Dependency reason |
|---|---|---|---|---|
| Task A: Gate `store update` (TODO 19) | Batch A | Modify `crates/julie-extract-cli/src/store/update.rs`, `crates/julie-extract-cli/src/store/executor.rs`; tests in `crates/julie-extract-cli/tests/store_cli_contract.rs` or a new store-update test file; TODO.md entry 19 | No | None - safe parallel batch. |
| Task B: Coordinator overrun counter + honest caller state (TODO 21, 20) | Batch A | Modify `crates/julie-extract-artifact/src/store/coordinator.rs`, `crates/julie-extract-cli/src/store/import.rs`, `crates/julie-extract-cli/src/store/update.rs` report path only if required (coordinate with Task A via lead); coordinator tests; TODO.md entries 20, 21 | No | None - safe parallel batch (update.rs report-path overlap resolved by lead at landing). |
| Task D: Discovery limits in `languages --json` (TODO 23) | Batch A | Modify `crates/julie-extract-cli/src/limits.rs`, `crates/julie-extract-cli/src/discovery.rs` (export constants), `crates/julie-extract-cli/src/commands.rs`; `docs/contracts/` report doc; TODO.md entry 23 | No | None - safe parallel batch. |
| Task C: Dead-requester reaping + `requests` pruning (TODO 22) | None - serial | Modify `crates/julie-extract-artifact/src/store/coordinator.rs`, `crates/julie-extract-artifact/src/store/maintenance.rs`; maintenance tests; TODO.md entry 22 | Yes | Shares `coordinator.rs` with Task B; lands after B. |

### Task A: Gate `store update` behind discovery (TODO 19)

**What to build:** Before reading or enqueuing, run the same `FilePolicy::select_file` decision `scan` uses. `Unsupported` (any reason: hard-excluded, ignored, oversized, unsupported extension) refuses the enqueue and reports terminal `unsupported` with the reason; no queue row is written; exit code follows the existing unsupported-file precedent in the CLI contract.

**Acceptance criteria:**
- [x] `store update` on an oversized file returns `unsupported` and leaves zero queue rows.
- [x] `store update` on a `.min.js` returns `unsupported` and leaves zero queue rows.
- [x] A supported file still updates end to end.
- [x] TODO entry 19 closed.

### Task B: Overrun counter and honest caller state (TODO 21, 20)

**What to build:** Count quantum overruns per request row; after 3, mark the row failed with `failure_class=coordinator_quantum` instead of requeuing. Decide `Update` renewable-vs-counter by matching the reasoning that added `Import` to `permits_renewable_quantum`, and record the choice in the TODO closure line. Separate the caller's own terminal state from backlog outcomes: the report's `state` is the caller's request state; backlog failures surface in a warning list.

**Acceptance criteria:**
- [x] An unschedulable row reaches a terminal failed state within 3 drains.
- [x] A committed caller request reports its own committed state even when a backlog request fails in the same drain.
- [x] TODO entries 20 and 21 closed.

### Task D: Publish discovery limits (TODO 23)

**What to build:** Add a `discovery_limits` block to `languages --json`: `max_source_file_bytes`, hard-exclude suffixes, hard-exclude directory names. Follow the repo's report-contract precedent for additive fields and update the report contract doc.

**Acceptance criteria:**
- [x] `languages --json` carries the limit and both hard-exclude sets, values sourced from the real constants.
- [x] Contract doc updated; TODO entry 23 closed.

### Task C: Reap dead-requester rows, prune `requests` (TODO 22)

**What to build:** Extend the claimed-row takeover rule to `queued`/`claimed` rows whose requester pid is dead (reuse the existing pid-liveness helper and its memoization — Windows pid probes cost ~100 ms). Add `requests`-table pruning of aged terminal rows to `store maintain`.

**Acceptance criteria:**
- [x] A queued or claimed row with a dead requester pid is reaped or aged out.
- [x] `store maintain` prunes terminal `requests` rows past the retention age.
- [x] TODO entry 22 closed.
