---
id: reference-integrity-incremental-equivalence-and-cu
title: Reference integrity, incremental equivalence, and current coverage gaps
status: active
created: 2026-09-27T18:47:57.058Z
updated: 2026-09-27T18:47:57.058Z
tags: []
---

## Approved work
The user approved all three next priorities: audit reference target accuracy across languages and fix defects; prove updates/deletes/renames match fresh extraction and fix stale facts; revalidate all 21 declared capability gaps against current code and needs, implementing actionable missing extraction.

## Constraints
Historical docs are evidence, not guardrails. Support every applicable language. Keep changes within source-tree-to-SQLite extraction; do not modify code-kb or add workspace-global reference resolution. Prefer existing helpers and source-backed facts. Preserve uncertainty rather than guessing. No push or release.

## Workspace
/home/murphy/source/julie-extractors/.worktrees/fix-reference-integrity, branch fix/reference-integrity, baseline 981e16cc. Main and earlier task worktrees are clean and merged. All cargo and scratch CLI commands use CARGO_TARGET_DIR=/home/murphy/source/julie-extractors/target and TMPDIR=/home/murphy/source/julie-extractors/target/reference-integrity-tmp because /tmp quota is exhausted.

## Completion
Source-backed dispositions for every declared gap; all actionable defects found within the three approved areas fixed with public-API/CLI checks; no hidden coverage weakening. Lead reviews delegated diffs, runs one final default/golden/strict-quality gate and relevant Windows checks, commits and integrates locally.
