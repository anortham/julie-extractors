# Store Resolution Query Amplification Design

## Problem

A scoped production resolve processed 199,123 rows for 47 names in 520,055 ms. The resolver process read about 98 GB through 24,145,680 read syscalls while retaining only about 26 MB RSS. This is database query amplification, not extraction, memory pressure, or scope selection.

The first hypothesis was that `prime_candidate_window`'s global `LIMIT window_size` leaves a high-cardinality cutoff name unprimed and causes every identifier to repeat top-level candidate pages. A deterministic repeated-name/high-fanout fixture disproved that hypothesis: PrimeWindow executed once for 300 rows and TopLevelNamed executed no more than three times, including when source confidence varied to defeat the full outcome cache. The production amplification is in another query family or an interaction unique to the real scoped path.

A faithful scoped replay now identifies the production access pattern. On a verified reflink clone with a ready
392,526-identifier predecessor base, the one replay completed in 49.81 seconds. `run_resolution_session` took
24.848 seconds. `LocateIdentifier` executed 10,804 times—72.12% of all candidate statements and exactly the
pending-row count. A first exact regression attributed those calls to `materialized_relationship_covers` and
batched that path, but the one faithful replay disproved the attribution: wall time remained 49.63 seconds,
`run_resolution_session` remained 24.740 seconds, `LocateIdentifier` remained 10,804, and the new relationship
coverage family executed zero times. The uncommitted wrong-path slice was removed.

The real replay has zero relationship-hydration queries. The matching production path is
`recheck_resolved_pending_items`: it rechecks the prior pending worklist and calls `locate_identifier` for each
demoted co-located identifier. `load_resolved_pending_page` already hydrates that worklist in bounded pages but
does not carry the exact co-located identifier result. Both locator indexes already exist; an index-only change
cannot remove 10,804 round trips. `finish_exact` then consumes about another 23.7 seconds and remains a separate
measured bottleneck.

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

### Batch materialized relationship coverage

Rejected after replay. The exact synthetic test proved the implementation's semantics and batching, but the
faithful path executed no relationship-hydration work and retained all 10,804 locator calls. Do not retain or
recreate this optimization as a fix for the measured incident.

### Hydrate co-located identifiers in resolved-pending pages

Rejected after replay. The bounded page implementation passed its real scoped RED/GREEN, but the faithful run
executed this family zero times and retained all 10,804 locator calls. This second uncommitted wrong-path slice was
removed.

### Attribute locator calls by resolution phase

Completed as test-only diagnostics at `5089c3a2`. `SnapshottingSession` records fixed locator buckets for
ResolvedPending, Pending, Relationships, and Other/Unset in live and final JSON. The one new-evidence replay
reported Pending=10,804 and every other bucket=0; its earliest retained snapshot already showed Pending=8,109 at
1.047 seconds. `resolve_pending_items` is therefore the exact production caller.

### Hydrate co-located identifiers in pending pages

Rejected after replay. The exact implementation removed all 10,804 Pending locator statements and reduced total
candidate statements from 14,980 to 4,176 with unchanged digest/rows. The one faithful replay was still slower:
50.46 seconds wall / 25.418 seconds resolver versus 49.88 seconds / 24.813 seconds baseline. The enriched bounded
page query used both locator indexes but added temporary GROUP BY/ORDER BY B-trees. The uncommitted slice was
removed because statement-count reduction did not reduce wall time.

The rejected design added `SessionPendingWorkItem` with a `PendingWorkItem` plus the same exact-result shape used by
`SessionRelationship`: `located_identifier_id: Option<String>` plus `identifier_lookup_complete: bool`.
`StoreScratchResolutionSession::load_pending_page` computes the result in the existing bounded key page, using the
current span/line and exactly-one-match rules. `resolve_pending_items` consumes the hydrated result and falls back
to `locate_identifier` only for session adapters whose lookup is not complete. `load_resolved_pending_page` may
unwrap the same pending row without changing resolved-pending behavior.

Its architecture remained correct, but a correct internal interface is not enough reason to ship a measured
performance regression.

### Exact finalization

Completed independently from candidate resolution. Fixed cumulative timings around eight `finish_exact`
boundaries measured identifier-row insertion as the largest exclusive phase: 14.304 seconds of a 24.947-second
finish. `ResolutionBaseWriter::push_identifier_resolution` prepared the same insert for every row. A one-entry
rusqlite prepared-statement cache behind the unchanged streaming writer API reduced a 100,000-row exact contract
from 3.415 seconds to 1.715 seconds without buffering or a new public page API. The single post-fix replay reduced
identifier-row insertion to 7.547 seconds and total process wall from 49.98 to 43.10 seconds. Exact file SHA-256,
392,526 identifier rows, 10,804 pending rows, 1,538 source versions, and SQLite integrity remained unchanged.

## Design

Add internal counters to `StoreScratchResolutionSession`:

- candidate query executions
- candidate page cache hits
- candidate rows read
- counts by stable query family

Expose them through the existing test/diagnostic surface and carry them into the store resolution phase report. Labels are a fixed enum/string set, not SQL text or symbol names.

The first diagnostic fixture uses many identifiers sharing one name and more candidate symbols than `window_size`.
It guards the disproven top-level theory and verifies fixed-family counters. Test-only diagnostics persist
logarithmic live snapshots and fail closed unless the configured view is current, converging, and bound to a ready
predecessor. Task 3 closed with a rejected no-win optimization and preserved caller telemetry. Task 4 measured
exact finalization before changing behavior and shipped only the selected identifier-row statement reuse.

## Verification Budget

- RED/GREEN: the exact new fixture only.
- Affected gate: store resolution mechanism plus performance test files once.
- No real corpus replay until query counts are green.
- One real corpus replay after focused gates, with a 60-second hard timeout and captured counters.
- No three-run benchmark until the single bounded replay meets the budget.
