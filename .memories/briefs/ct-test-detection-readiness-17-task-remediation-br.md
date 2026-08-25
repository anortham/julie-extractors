---
id: ct-test-detection-readiness-17-task-remediation-br
title: "CT test-detection readiness: 17-task remediation branch"
status: active
created: 2026-08-25T16:25:07.194Z
updated: 2026-08-25T16:25:07.194Z
tags:
  - ct
  - test-detection
  - sdd
  - execution
---

## Direction

Close every extractor-side CT gap from the 2026-08-25 audit (checkpoint_ffbce0e9) so Miller CT gets honest test-role facts for 10 languages plus the shared contract.

## Where

- Worktree: `.claude/worktrees/ct-test-detection-readiness`, branch `worktree-ct-test-detection-readiness`, base main@8d7f37c6.
- Plan: `docs/plans/2026-08-25-ct-test-detection-readiness.md` (approved 2026-08-25).
- SDD ledger: `.razorback/sdd/2026-08-25-ct-test-detection-readiness/progress.md`.

## Execution model (user-directed)

- Workflow tool dispatches, Opus implementers, lead reviews every task inline (reviewer_choice: none).
- Serial tasks (1, 2, 15, 16, 17): single-agent workflow in the session worktree, serial-worker-commit.
- Batch tasks (A: 3,4,5,6,7; B: 8,9,10,11; C: 12,13,14): agents in isolated worktrees commit on their own branch and report SHAs; lead reviews `git diff base..sha`, lands via cherry-pick/am, then records the lead commit (parallel-lead-commit semantics).

## Constraints that bind everything

- One EXTRACTION_IDENTITY_EPOCH bump (Task 1 only, 5→6). test_role strings must match Miller's classifier exactly. Strict quality report stays at silent_cells=0, quality_bar_debts=0. Windows gate (win-test) required because Task 2 touches is_test_path.

