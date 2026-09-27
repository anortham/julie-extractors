---
id: canonical-receiver-facts-and-measured-incremental-
title: Canonical receiver facts and measured incremental reporting
status: active
created: 2026-09-27T15:35:44.604Z
updated: 2026-09-27T15:35:44.604Z
tags:
  - receiver-facts
  - all-languages
  - correctness
  - performance-measurement
---

## Goal
Move receiver detection out of CLI artifact mapping into canonical language-aware extraction. Verify receiver names/types and precise reference locations across every applicable language. Measure the remaining whole-artifact counting cost of incremental commands and make only evidence-backed changes.

## Authority and scope
The user said "ok do it" after these three proposed next items. Implementation and local commits are authorized; no push or release. All languages matter. Historical docs can be overturned when current evidence supports improvement. Do not modify code-kb.

## Worktree
Use /home/murphy/source/julie-extractors/.worktrees/refactor-receiver-facts on refactor/receiver-facts, starting at e11fe30a. Main and the completed correctness worktree are clean and merged.

## Approach
Keep existing identifier/reference-site contracts and exact spans. Prefer AST evidence to source-text guesses; preserve authoritative language metadata and explicit suppression. Verify the public extraction API and SQLite output agree. Avoid new configuration, resolvers, schema caches, or frameworks without concrete need. Capture fixed-workload performance baselines before optimization.
