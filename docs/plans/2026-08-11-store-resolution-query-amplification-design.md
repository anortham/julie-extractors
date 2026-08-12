# Store Resolution Query Amplification Design

## Problem

A scoped production resolve processed 199,123 rows for 47 names in 520,055 ms. The resolver process read about 98 GB through 24,145,680 read syscalls while retaining only about 26 MB RSS. This is database query amplification, not extraction, memory pressure, or scope selection.

The first hypothesis was that `prime_candidate_window`'s global `LIMIT window_size` leaves a high-cardinality cutoff name unprimed and causes every identifier to repeat top-level candidate pages. A deterministic repeated-name/high-fanout fixture disproved that hypothesis: PrimeWindow executed once for 300 rows and TopLevelNamed executed no more than three times, including when source confidence varied to defeat the full outcome cache. The production amplification is in another query family or an interaction unique to the real scoped path.

A faithful scoped replay now identifies the production access pattern. On a verified reflink clone with a ready
392,526-identifier predecessor base, the one replay completed in 49.81 seconds. `run_resolution_session` took
24.848 seconds. `LocateIdentifier` executed 10,804 times—72.12% of all candidate statements and exactly the
pending-row count—because `materialized_relationship_covers` calls it inside its relationship-row loop. Both
locator indexes already exist; an index-only change cannot remove those round trips. `finish_exact` then consumed
about another 23.7 seconds and is a separate measured bottleneck.

## Goals

- Count candidate queries and rows so a slow resolve says which query family amplified.
- Make repeated equivalent candidate page reads reuse bounded results.
- Keep memory proportional to a small number of pages, not workspace or name cardinality.
- Preserve resolution rows and canonical digest exactly.
- Keep one-file incremental resolution under 5 seconds on the development corpus; keep a full real Miller resolution under 60 seconds.
- Do not repeat an operation over 60 seconds without new phase/query evidence.

## Options

### Group identifiers by name and cache one complete name

This improves locality but changes phase ordering and can still require workspace-sized memory for a common name. Rejected.

### Materialize all visible candidates into a scratch table

This removes source-store rereads but copies a large fraction of the corpus for narrow resolves and adds scratch lifecycle/schema work. It remains a fallback if bounded page reuse is insufficient.

### Bounded candidate-page reuse

Deferred pending telemetry. It remains a possible implementation only if the faithful replay proves a query family repeats byte-identical page keys. Adding it now would optimize a disproven fixture rather than the production bottleneck.

The faithful replay did not select this option. The dominant calls are relationship-coverage locators, not repeated
candidate pages.

### Batch relationship coverage

Selected. Replace the per-relationship `locate_identifier` call shape with a bounded SQL or scratch-materialized
coverage query that evaluates identifiers and reference-kind relationships in pages. Preserve the existing rule:
coverage is true only when the relationship target maps to exactly one identifier at the same version/name/span
or line. Keep view/generation visibility predicates and deterministic order. No cache may grow with the workspace.

### Exact finalization

Track separately from candidate resolution. Add timings/counters around `finish_exact` publication work, then
optimize its largest measured database/file phase after the relationship-coverage fix. Combining the two would
hide whether query amplification or publication improved.

## Design

Add internal counters to `StoreScratchResolutionSession`:

- candidate query executions
- candidate page cache hits
- candidate rows read
- counts by stable query family

Expose them through the existing test/diagnostic surface and carry them into the store resolution phase report. Labels are a fixed enum/string set, not SQL text or symbol names.

The first diagnostic fixture uses many identifiers sharing one name and more candidate symbols than `window_size`. It guards the disproven top-level theory and verifies fixed-family counters. Test-only diagnostics persist logarithmic live snapshots and fail closed unless the configured view is current, converging, and bound to a ready predecessor. Task 3 implements batched relationship coverage, verifies exact output and bounded work, then runs one faithful replay. A second task instruments and fixes exact finalization.

## Verification Budget

- RED/GREEN: the exact new fixture only.
- Affected gate: store resolution mechanism plus performance test files once.
- No real corpus replay until query counts are green.
- One real corpus replay after focused gates, with a 60-second hard timeout and captured counters.
- No three-run benchmark until the single bounded replay meets the budget.
