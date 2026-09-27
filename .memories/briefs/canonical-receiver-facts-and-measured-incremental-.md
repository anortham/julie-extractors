---
id: canonical-receiver-facts-and-measured-incremental-
title: Canonical receiver facts and measured incremental reporting
status: completed
created: 2026-09-27T15:35:44.604Z
updated: 2026-09-27T17:19:14.218Z
tags:
  - receiver-facts
  - all-languages
  - correctness
  - performance-measurement
---

## Outcome
Canonical extraction now owns receiver names and qualification; the CLI byte scanner is removed. Language-specific producers correct reference spans, duplicate C++/Rust calls, SQL qualifications and Rust macro receivers. Ruby setter declarations no longer produce call facts. Semantic extraction identity advanced; updated consumers need scan --force.

## Coverage and verification
42-language applicability matrix, 31 supplementary fixtures, 121 literal receiver expectations, and receiver-type preservation across extraction levels and APIs. Final comparison of 308 source-named golden fixtures preserved all valid old receivers and removed five false facts. Reviewed 205 golden changes. Linux default: 6,741 passed, 7 ignored, 38s. Golden update: 8 passed. Strict quality: zero silent cells and quality-bar debts. Windows on f65b2e68: 384 CLI/artifact and 36 focused extractor tests passed.

## Reporting decision
On 1.69M rows, 22 exact count queries took 10.472ms p95 against about 540ms incremental command time. Keep exact counts. No cache or reporting optimization was justified.

## Authority and integration
All languages matter; historical docs are revisable. No code-kb changes. Implementation commit f65b2e68 and measurement commit d20db26e are local. Final documentation records verified state. No push or release authorized or performed.
