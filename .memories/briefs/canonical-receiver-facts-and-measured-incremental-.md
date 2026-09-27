---
id: canonical-receiver-facts-and-measured-incremental-
title: Canonical receiver facts and measured incremental reporting
status: active
created: 2026-09-27T15:35:44.604Z
updated: 2026-09-27T17:07:21.515Z
tags:
  - receiver-facts
  - all-languages
  - correctness
  - performance-measurement
---

## Goal
Move receiver detection out of CLI artifact mapping into canonical language-aware extraction. Verify receiver names/types and precise reference locations across every applicable language. Measure whole-artifact counting during incremental commands and make only evidence-backed changes.

## Authority and scope
User approved these three next items with "ok do it". Implementation and local commits are authorized; no push or release. All languages matter. Historical docs may be overturned. Do not modify code-kb.

## Worktree
/home/murphy/source/julie-extractors/.worktrees/refactor-receiver-facts on refactor/receiver-facts, base e11fe30a. Main and prior correctness worktree are clean.

## Approach and evidence
AST enrichment belongs at the registry boundary. Rust macro trees preserve original byte positions; SQL call producers use existing parsed qualification. Preserve authoritative receiver metadata/null suppression/types. CLI serializes facts; no consumer syntax guessing or new schema/resolver. The 42-language matrix plus 31 supplementary fixtures passes. Producer fixes correct broad spans in R/C++/HTML/regex/C#, duplicate C++/Rust member calls, and false Ruby setter references. Golden review, Linux branch gate and Windows verification remain.

## Performance decision
Immutable baseline measurement on 1.69M rows: 22 exact count queries took 10.472ms p95 against ~540ms incremental command time. Keep exact totals; no cache or count optimization justified.
