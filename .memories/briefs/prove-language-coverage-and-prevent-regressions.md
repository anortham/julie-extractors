---
id: prove-language-coverage-and-prevent-regressions
title: Prove language coverage and prevent regressions
status: active
created: 2026-11-04T00:15:04.316Z
updated: 2026-11-04T00:15:04.316Z
tags: []
---

Execute docs/plans/2026-11-03-language-coverage-goal.md from baseline 837b9796 (product f9a1b6cf). Prove all 42 languages across 15 observed domains, correct five unsupported body claims and 31 applicability cells, preserve eleven consumer/context boundaries, add a pinned offline 42-language corpus and controlled baseline/candidate correctness/performance checks. Completion requires every plan acceptance criterion, native Windows checks, controlled metrics and final gates on one committed source state. SQLite and julie-extract remain product APIs; no resolver, context guessing, release, push, or code-kb changes. Reuse .worktrees/language-coverage-plan on docs/language-coverage-plan; lead owns shared manifests/CLI routing/evidence/commits; workers use non-overlapping ownership.
