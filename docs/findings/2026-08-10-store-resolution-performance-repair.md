# Store Resolution Performance Repair

## Result

The Miller corpus exact-resolution replay fell from 6:20.55 to 2:24.54 while preserving the exact 37,965 gap rows across 98 files. The repaired run processed the same 416,361 visible identifiers with 90 MB peak RSS.

## Root causes

1. Exact-base target validation reopened and revalidated `store.db` for every distinct resolved target.
2. Candidate resolution issued broad name queries and disk-backed tier-accumulator writes per edge.
3. Type facts, imports, module selection, and relationship propagation bypassed the chunk reader and repeated validated opens.
4. The performance fixture concentrated repeated names and targets in one file, so its caches hid production cardinality.

## Repair

- Exact validation now reuses one validated reader and one prepared target-existence statement.
- Candidate lookup pushes language, kind, and optional source-version filters into SQLite.
- The candidate-name window and module lookup cache have explicit high-water bounds; high-cardinality children, type facts, and imports stream without retention.
- Global filtered summaries use a bounded cross-window cache.
- Tier candidate deduplication stays in memory up to the configured window, then spills in bounded scratch transactions.
- Phase membership freezes use one validated reader, page hydration reuses the candidate reader, and relationship identifiers hydrate in one bounded batch.

The whole-corpus in-memory candidate index was rejected because the store-resolution contract requires peak memory to remain bounded by the configured window rather than corpus size.

## Evidence

| Run | Wall time | User time | System time | Peak RSS |
|---|---:|---:|---:|---:|
| Original repaired-corpus baseline | 6:20.55 | not retained | not retained | not retained |
| Target-reader-only repair | 5:08.55 | 295.04s | 13.00s | 90,128 KB |
| Rejected speculative source-file snapshots | 1:54.02 | 87.91s | 9.79s | 89,304 KB |
| Final bounded repair | 2:24.54 | 134.31s | 8.70s | 90,216 KB |

The faster experimental result was rejected because it issued up to one speculative query per source version and discarded full pages. The final result is the mergeable measurement.

Focused gates:

- `store_resolution_performance`: 7 passed.
- `store_resolution_mechanism`: 11 passed, including legacy/store parity, high collision, RSS, and 300-row bounds.
- High-distinct-target validation: original 4.03s, repaired below the fixed 2s ceiling.
- High-distinct-name resolution: original 5.55s, repaired below the fixed 3.5s ceiling.

## Remaining work

- Resolution is still full-corpus for every changed manifest. Safe incremental resolution needs a dependency closure and full-resolution oracle comparison; it is a separate architecture slice.
- Miller search and content sidecars rebuild on zero-change store sequence advances. That consumer defect is being repaired separately; vector convergence already uses deltas.
