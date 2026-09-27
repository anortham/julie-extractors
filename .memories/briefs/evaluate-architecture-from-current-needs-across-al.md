---
id: evaluate-architecture-from-current-needs-across-al
title: Evaluate architecture from current needs across all supported languages
status: completed
created: 2026-09-27T13:54:01.995Z
updated: 2026-09-27T15:17:24.723Z
tags:
  - architecture
  - all-languages
  - first-principles
  - code-kb
  - user-direction
---

## Result
The authorized correctness fixes are implemented and verified: distinct reference occurrences survive mapping, SQL emits available context spans, and partial writes cannot mix producer generations. Force replaces compatible artifacts transactionally and preserves the old generation on source or write failures. Existing artifacts need one force scan with the updated binary.

## User direction preserved
Historical docs are evidence, not constraints. Correct bad decisions when necessary. Support all applicable languages; shared correctness work must not favor TypeScript/Python. The other session owns code-kb performance work.

## Verification
Linux default:6,708 passed (7 ignored),49s. Windows CLI/artifact:390 passed. Reference-site corpus:3 passed. SQL:110 passed. All-language goldens:8 passed. Strict quality:42 languages,0 silent cells,0 quality debts. Formatting passed.

## Delivery
Implementation commits:5f138c56 (occurrences),4dc333e3 (freshness),4cc16974 (Windows contract test). Task worktree: /home/murphy/source/julie-extractors/.worktrees/fix-data-correctness, branch fix/data-correctness. Finish with local fast-forward of clean main; no push or release authorized. Full evidence and scope are in docs/plans/2026-09-27-data-correctness.md.
