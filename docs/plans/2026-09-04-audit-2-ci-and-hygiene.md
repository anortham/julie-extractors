# Audit Wave 2: CI Gates and Repository Hygiene Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use razorback:subagent-driven-development when subagent delegation is available. Fall back to razorback:executing-plans for single-task, tightly-sequential, or no-delegation runs.

**Goal:** Make CI enforce the gates the agent guidelines promise, and remove stale worktrees, tombstone tests, and ignore-rule conflicts.

**Architecture:** Configuration and documentation changes only, plus deletion of one dead fixture and its keeper test. No product code changes.

**Tech Stack:** GitHub Actions, Node (for the existing `.mjs` report), git.

**Architecture Quality:** No Architecture Impact: every task is a CI step, a doc reconciliation, a rename, or a deletion with no behavior change in the product.

Source: `docs/findings/2026-09-04-architecture-and-performance-audit.md`, items T2, T3, T4, T6, T7, T10, T11. T1 is refuted and has no task.

## Global Constraints

- `AGENTS.md` and `CLAUDE.md` stay byte-for-byte equivalent. Run `scripts/check-agent-doc-sync.sh` before every commit that touches either.
- Test pass/fail must not depend on wall-clock time (`docs/testing-strategy.md:294`). No CI timer that fails a build.
- Worktree removal and branch deletion are destructive. Task 4 stops at an approval gate and lists exactly what it will remove. It never removes a worktree with untracked or modified files.
- Do not rewrite git history. The `.razorback/` and `.memories/` histories stay.

## Verification Strategy

**Project source of truth:** `docs/testing-strategy.md`, `.github/workflows/ci.yml`, `scripts/check-agent-doc-sync.sh`.

**Worker red/green scope:** the command the task changes: `node scripts/language-data-quality-report.mjs --strict` for Task 1; `cargo test -p julie-extract-cli --test test_tiers` for Task 3; `cargo test -p julie-extract-artifact --test test_tiers` for Task 5; `scripts/check-agent-doc-sync.sh` for Task 2.

**Worker ceiling:** `cargo xtask test default`.

**Worker gate invariant:** the changed gate runs and passes locally with the exact command CI will run.

**Lead affected-change scope:** `cargo test -p xtask` plus the two `test_tiers` targets.

**Branch gate:** `cargo xtask test default`, `cargo xtask test contract`, `scripts/check-agent-doc-sync.sh`, `cargo fmt --check`.

**Security scope:** none declared.

**Replay/metric evidence:** Task 6 records binary size, release build time, and extraction baseline for two profiles. Report-only; no gate.

**Escalation triggers:** a CI workflow change must be proven by a green run on a push to a branch before merge.

**Assigned verification failure:** Workers stop and report when assigned verification fails, unless this plan explicitly says to update that gate.

**Verification ledger:** Record invariant, command, scope label, commit SHA, result, and timestamp.

## Parallel Execution Contract

| Task | Parallel batch | File ownership | Serialization required | Dependency reason |
|---|---|---|---|---|
| Task 1: Data-quality report in CI | Batch A | Modify `.github/workflows/ci.yml` (one new step in the fast-gates job) | No | None - safe parallel batch. |
| Task 2: Reconcile the wall-clock tripwire policy | Batch A | Modify `AGENTS.md`, `CLAUDE.md` (line 69), `docs/testing-strategy.md` (near line 294), `xtask/src/test_tiers.rs` (report-only timing line) | No | None - safe parallel batch. |
| Task 3: Delete the legacy resolution fixture and keeper test | Batch A | Delete `fixtures/store-resolution/legacy-v3/**`; modify `crates/julie-extract-cli/tests/test_tiers.rs` | No | None - safe parallel batch. |
| Task 4: Remove merged worktrees and branches | None - serial | git worktree and branch state only; modify `.gitignore` | Yes | Approval gate: destructive. |
| Task 5: Untrack `.razorback/` reports and rename performance test files | Batch A | `git rm --cached .razorback/**`; rename `crates/julie-extract-artifact/tests/writer_performance.rs` and `store_writer_performance.rs`; modify `crates/julie-extract-artifact/tests/test_tiers.rs`, `crates/julie-extract-artifact/Cargo.toml` if test targets are named | No | None - safe parallel batch. |
| Task 6: Measure a release profile | None - serial | Create `docs/evidence/2026-09-release-profile-measurement.md`; modify `Cargo.toml` only if the measurement wins | Yes | Needs a quiet machine; run alone. |

Commit mode: `parallel-lead-commit` for Batch A; `serial-worker-commit` for Tasks 4 and 6.

---

## Task 1: Run the data-quality report in CI

**Files:** Modify `.github/workflows/ci.yml`: add `actions/setup-node@v4` with Node 22 and one step `node scripts/language-data-quality-report.mjs --strict` in the fast-gates job, after the agent-guidance sync check at line 41.

**What to build:** A CI step that fails when `silent_cells` or `quality_bar_debts` is non-zero, which is what `--strict` already does (`process.exitCode = 1` at script line 601).

**Approach:** Run the script locally first and confirm exit code 0 on the current tree. Confirm the script has no npm dependencies (it uses only Node built-ins; check the import lines). Push the branch and confirm the step is green in Actions before merging.

**Acceptance criteria:**
- [x] Local run exits 0.
- [x] CI fast-gates job has the step and passed on the branch.
- [x] `docs/testing-strategy.md` lists the command in the CI section.

## Task 2: Reconcile the wall-clock tripwire policy

**Files:** Modify `AGENTS.md:69`, `CLAUDE.md:69`, `docs/testing-strategy.md` (add a short paragraph near line 294), `xtask/src/test_tiers.rs`.

**What to build:** One consistent policy. Decision recorded here: the default tier reports its wall clock as an informational line, and the agent guidelines say so. No pass/fail timer.

**Approach:**
1. Replace the CLAUDE.md and AGENTS.md line 69 with: `The default tier prints its wall clock. Treat growth past 3 minutes warm as a defect to fix, not a gate.`
2. In `xtask/src/test_tiers.rs`, wrap the default tier run in an `Instant` and print `default tier wall clock: <seconds>s` at the end. Report-only; never affects the exit code. This is xtask code, not a test file, so the existing `Instant::now()` guard in the artifact crate does not apply.
3. Add the paragraph to `docs/testing-strategy.md` naming the informational line and the 3-minute expectation.
4. Run `scripts/check-agent-doc-sync.sh`.

**Acceptance criteria:**
- [x] Both agent files carry the same new line; sync script passes.
- [x] `cargo xtask test default` prints the wall clock line.
- [x] `cargo test -p xtask` passes.

## Task 3: Delete the legacy resolution fixture and its keeper test

**Files:** Delete `fixtures/store-resolution/legacy-v3/` (11 files). Modify `crates/julie-extract-cli/tests/test_tiers.rs`: delete `legacy_resolution_fixture_and_oracle_are_checked_in_together` (line 34). Keep `legacy_resolution_oracle_is_feature_gated_out_of_default_suite` (line 5) and `store_lifecycle_process_and_scale_contracts_are_feature_gated` (line 45).

**What to build:** Nothing. Remove the fixture nothing reads and the test that only asserts it exists.

**Approach:** Confirm with `grep -rn store-resolution crates xtask scripts .github` that the only code reference is the keeper test. Confirm the feature-gated oracle test at line 5 does not read the fixture. If it does, stop and report.

**Acceptance criteria:**
- [x] `fixtures/store-resolution/` is gone.
- [x] `cargo test -p julie-extract-cli --test test_tiers` passes.
- [x] `cargo xtask test contract` passes.

## Task 4: Remove merged worktrees and branches (approval gate)

**Files:** Modify `.gitignore`: add `/.claude/worktrees/`. No source files.

**What to build:** A clean worktree inventory.

**Approach:**
1. For every worktree in `git worktree list` except the main checkout, run `git -C <path> status --short --branch` and record the result.
2. Confirm each branch has zero commits ahead of main: `git log main..<branch> --oneline` must be empty.
3. Present the list to the user: 29 clean trees to remove, plus `ct-language-audit-plan` which holds two untracked docs and must stay until the user reconciles those files.
4. **Stop for approval.** Removing worktrees and deleting branches is destructive.
5. After approval: `git worktree remove <path>` for each clean tree, `git branch -d <branch>` for each merged branch, `git worktree prune`.
6. Add the ignore rule and commit it.

**Acceptance criteria:**
- [x] User approved the exact removal list.
- [x] `git worktree list` shows main plus any tree the user chose to keep.
- [x] `.gitignore` ignores `/.claude/worktrees/`.
- [x] The dirty `ct-language-audit-plan` tree was not removed unless the user reconciled it first.

## Task 5: Untrack `.razorback/` reports and rename performance test files

**Files:**
- `git rm --cached -r .razorback/` (13 files). The ignore rule at `.gitignore:16` then applies.
- Rename `crates/julie-extract-artifact/tests/writer_performance.rs` to `writer_batching_contract.rs` and `store_writer_performance.rs` to `store_writer_batching_contract.rs`. Keep `writer_perf.rs` (the gated, timed one).
- Modify `crates/julie-extract-artifact/tests/test_tiers.rs` and `Cargo.toml` if they name the test targets.

**What to build:** Names that say what the tests check.

**Approach:** Grep the docs for the old file names (`docs/testing-strategy.md`, `docs/plans`) and update them. Run the artifact crate's `test_tiers` and the two renamed tests.

**Acceptance criteria:**
- [x] `git ls-files .razorback` is empty.
- [x] Both renamed tests run and pass by their new names.
- [x] No doc references the old names.

## Task 6: Measure a release profile

**Files:** Create `docs/evidence/2026-09-release-profile-measurement.md`. Modify root `Cargo.toml` only if the measurement wins.

**What to build:** Evidence for or against `[profile.release] lto = "thin"` and `codegen-units = 1`.

**Approach:**
1. Record with the default profile: release build wall clock from clean, `julie-extract` binary size, and `cargo xtask performance baseline --root . --out-dir target/performance/profile-default --binary target/release/julie-extract --runs 3`.
2. Add `[profile.release]` with `lto = "thin"` and `codegen-units = 1` to the root `Cargo.toml`. Repeat all three measurements.
3. Adopt the profile only if extraction median improves by more than 5 percent or binary size drops by more than 15 percent, and release build time grows by less than 2x. Otherwise revert and record the numbers.

**Acceptance criteria:**
- [x] Evidence file has both rows with all three numbers.
- [x] `Cargo.toml` either has the profile with a one-line pointer to the evidence, or is unchanged and the evidence says why.
