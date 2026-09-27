---
id: evaluate-architecture-from-current-needs-across-al
title: Evaluate architecture from current needs across all supported languages
status: active
created: 2026-09-27T13:54:01.995Z
updated: 2026-09-27T14:26:52.895Z
tags:
  - architecture
  - all-languages
  - first-principles
  - code-kb
  - user-direction
---

## Goal
Fix the verified data correctness issues in julie-extractors: preserve distinct reference occurrences and prevent incremental writes mixing extraction generations.

## User direction
The user explicitly authorized correctness fixes after the code-kb session improved slow queries with indexes. Do not modify code-kb or pursue performance cleanup here. Historical docs and decisions can be reversed. Support all applicable languages equally; do not limit shared improvements to TypeScript/Python/Rust.

## Implementation direction
Keep occurrence identity independent of whether a span identifies the exact target token. Preserve existing exact-site joins and do not invent target coordinates. Enforce producer freshness for partial writes with a safe full-force recovery path; preserve prior data on failed generation changes. Prefer existing contract/version/fingerprint mechanisms over a new subsystem.

## Worktree
All implementation runs in /home/murphy/source/julie-extractors/.worktrees/fix-data-correctness on fix/data-correctness, from 57bffdad. The earlier owned documentation correction was committed before creation. Main is clean; no push/release is authorized.
