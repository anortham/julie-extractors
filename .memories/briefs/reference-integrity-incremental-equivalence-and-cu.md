---
id: reference-integrity-incremental-equivalence-and-cu
title: Reference integrity, incremental equivalence, and current coverage gaps
status: completed
created: 2026-09-27T18:47:57.058Z
updated: 2026-09-27T21:05:48.260Z
tags: []
---

## Completed direction
The user approved cross-language reference-target correctness, incremental/fresh-scan equivalence, and revalidation of all 21 capability gaps. Implementation commit 14f4510cdb0cb55ab5d71929c92f7d433ff5094c is verified and integrated into local main.

## Results
Scoped binding and receiver fixes retain unresolved occurrences without false concrete targets. Incremental add/edit/malformed-recovery/rename/delete/no-op sequences match fresh artifacts. Ten original capability gaps close with 26 new fixture groups; eleven context/composition gaps and one separately discovered PCRE2 branch-reset gap have named closure tasks in docs/plans/2026-09-27-reference-integrity.md.

## Evidence
Linux default: 6,804 passed in 29s. Normal golden tier: 8/8. Strict quality: 42 languages, no silent cells or quality-bar debts. Native Windows default passes on the implementation SHA (190s including rebuild), with all four equivalence tests passing. Final report: .memories/autonomous-run-2026-09-27-reference-integrity.md.

## Preserved constraints
Historical docs remain evidence, not guardrails. All applicable languages matter. The product remains source-tree-to-SQLite extraction; no workspace-global resolver and no changes to code-kb. Source facts preserve uncertainty. No push or release. The extraction contract changed, so consumers need an upgraded extractor and forced re-extraction.
