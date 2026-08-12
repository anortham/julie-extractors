# Task 4 Report: Exact Finalization Telemetry and Fix

## Status

Complete. Fixed-cardinality `finish_exact` telemetry measured identifier-row insertion as the largest exclusive phase. The minimum one-entry prepared-statement cache reduced that phase by 47.2% on the faithful corpus and reduced process wall from 49.98 seconds to 43.10 seconds with byte-identical exact output.

## Implementation

### Task 4A: fixed finalization boundaries

`StoreScratchResolutionSession` now has an internal eight-boundary finish implementation and a test-feature-only observer:

1. `prior_overlay`
2. `identifier_totality`
3. `writer_init`
4. `source_versions`
5. `identifier_rows`
6. `pending_rows`
7. `writer_finish`
8. `scratch_cleanup`

The production `finish_exact() -> Result<ResolutionFileIdentity, StoreResolutionError>` signature and result are unchanged. Cumulative timing starts at method entry. Observer persistence time is subtracted from cumulative samples. The pre-existing diagnostic `elapsed_ms` retains its prior meaning: resolver elapsed in final JSON and total elapsed in live snapshots.

The existing diagnostic worker persists every completed finish phase atomically to live JSON, then writes the complete fixed array to final JSON. A timeout therefore retains the last completed boundary. Samples contain only a fixed phase enum and cumulative microseconds.

### Task 4B: measured identifier-row fix

The first replay measured `identifier_rows` at 14.304006 seconds, 57.3% of the 24.946871-second finish. `ResolutionBaseWriter::push_identifier_resolution` issued the same SQL through `Connection::execute` for each of 392,526 rows.

The writer now gives its existing connection a one-entry prepared-statement cache and uses `prepare_cached` inside the unchanged streaming method. Validation, ordering, one-row-at-a-time memory, transaction boundaries, schema, and caller API are unchanged. No page API, buffering, cross-schema attach, or CLI knowledge of artifact tables was added.

## Strict TDD Evidence

All timestamps are UTC on 2026-08-12.

### Fixed observer contract

Command:

`cargo test -p julie-extract-cli --features test-store-resolution-contract --test store_resolution_performance finish_exact_observer_retains_every_cumulative_phase_and_preserves_identity -- --exact --nocapture`

- RED: exit 101 for missing `FinishExactPhase`, `FinishExactPhaseSample`, and `finish_exact_observing`.
- GREEN: 1 passed, 15 filtered; test 0.12s, command 6.20s.
- Mutation caught: omitting, reordering, duplicating, or dynamically naming a boundary; changing finish identity or rows.

### Timeout retention contract

Command used the exact test `finish_exact_live_diagnostic_retains_last_completed_phase_when_observer_stops`.

- RED: missing `CandidateQuerySnapshot.finish_exact` and atomic persistence seam.
- GREEN: 1 passed, 16 filtered; test 0.06s.
- The real observer stopped after `writer_init`; live JSON retained exactly the first three cumulative phases.

### Final/live serialization contract

Command used the exact test `finish_exact_diagnostic_publishes_complete_fixed_samples_to_final_and_live_json`.

- RED: missing final finish array and shared finish helper.
- GREEN: 1 passed, 17 filtered; test 0.06s, command 0.96s.
- Mutation caught: a phase present only in memory or only in one diagnostic output.

### Observer overhead contract

Command used the exact test `finish_exact_cumulative_timing_excludes_observer_persistence_work`.

- RED: final cumulative 85,981us versus 96,357us wall after eight 10ms observer sleeps, proving callback work contaminated later samples.
- GREEN: 1 passed, 18 filtered; test 0.15s, command 2.16s.
- Mutation caught: including test-harness persistence latency in local finish timings.

### High-cardinality writer contract

Command:

`cargo test -p julie-extract-cli --features test-store-resolution-contract --test store_resolution_performance streaming_identifier_writer_reuses_one_statement_at_high_cardinality -- --exact --nocapture`

- RED: exit 101 after all identity/count/order/integrity assertions passed: `streaming 100000 identifier rows took 3.415186674s, expected at most 2.5s`.
- One initial GREEN compile attempt failed because the cache-capacity constant was placed in a function scope; it did not run the behavior. The declaration was corrected before the same exact test was rerun.
- GREEN: 1 passed, 19 filtered; 100,000 inserts 1,715ms; test 3.47s, command 6.33s.
- Improvement: 1.700s, 49.8% on the focused insertion interval.
- Mutation caught: replacing cached preparation with per-row preparation while preserving superficially correct rows.

Instrumentation itself followed RED before implementation. The corrected constant placement was a compile/setup correction, not a behavioral RED.

## First Faithful Replay: Phase Selection

Immutable source:

`/home/murphy/source/julie-extractors/.worktrees/fix-store-resolution-scope/target/performance/store-resolution-scope-fix-clean-final/run-001/fixture-miller-mutated/family`

Fresh reflink-only clone:

`target/performance/query-task4-finalization-replay-20260812/family`

Output:

`/tmp/julie-query-task4-finalization-result-20260812.json`

The clone passed schema-hash, store/base hash, integrity, foreign-key, current-generation, view-state, predecessor readiness, and exact-base absence checks. The source remained exact and unchanged.

One 60-second TERM / 5-second KILL replay completed once; it was not retried:

- process wall 49.98s; test 49.86s
- user 48.77s; system 1.09s; CPU 99%; max RSS 57,304KB
- logical file input 0; output 1,264 filesystem blocks
- resolver `elapsed_ms` 24,896ms
- `finish_exact` 24.946871s

| Phase | Cumulative | Exclusive |
|---|---:|---:|
| prior_overlay | 0 | 0 |
| identifier_totality | 5.802430s | 5.802430s |
| writer_init | 5.803749s | 1.319ms |
| source_versions | 5.826608s | 22.859ms |
| identifier_rows | 20.130614s | 14.304006s |
| pending_rows | 20.398288s | 267.674ms |
| writer_finish | 24.935041s | 4.536753s |
| scratch_cleanup | 24.946871s | 11.830ms |

Identifier rows were the largest phase. This disproved `writer_finish` as the next opaque boundary and selected only statement reuse for identifier insertion.

## Post-fix Faithful Replay

Fresh reflink-only clone:

`target/performance/query-task4-cached-writer-replay-20260812/family`

Output:

`/tmp/julie-query-task4-cached-writer-result-20260812.json`

Pre-run proof at 05:54:04Z:

- `cp -a --reflink=always` succeeded; no copy fallback.
- source store SHA-256 remained `fdecb118f030ac379ac3c6ef4d20e23963171ceb27e0ae31dafbf67411c2aa4b`.
- source and clone schema SHA-256 matched: `368706f91ef59409d5af4d1a7641aa5d111a5396b8a5b61467756e1fbff4f6eb`.
- clone integrity `ok`; foreign-key rows 0.
- generation-2 view was `converging` through zero-row delta 2 on ready predecessor `base-dcae...`.
- predecessor had 392,526 identifiers, 10,804 pending rows, and 1,538 versions.
- predecessor source/clone SHA-256 matched: `8e4cfdb8977817da058c11e7d17c55ca8c927157431f2cd433718fcd40983b3c`.
- generation-2 `base-c5ab...` metadata and files were absent only in the clone; source remained exact and integrity `ok`.
- no Cargo, rustc, extractor, or diagnostic process was active; load was 0.75/0.76/0.73 with 51.86GB available. Three ambient Miller hosts used about 0.96GB, 0.99GB, and 1.13GB RSS; this is report-only contamination.

Clone-only reset transaction rebound the view from exact delta 3/base `c5ab...` to the existing generation-2 zero-row delta 2 on ready base `dcae...`, deleted delta 3, then deleted only `c5ab...` base-version/base metadata. Only clone `c5ab...` DB/WAL/SHM files were removed. Source was never mutated.

Exact one-shot command:

`/usr/bin/time -v timeout --signal=TERM --kill-after=5s 60s env JULIE_STORE_RESOLUTION_QUERY_STORE=<fresh-clone> JULIE_STORE_RESOLUTION_QUERY_VIEW=replay-miller-mutated-1 JULIE_STORE_RESOLUTION_QUERY_GENERATION=2 JULIE_STORE_RESOLUTION_QUERY_OUTPUT=/tmp/julie-query-task4-cached-writer-result-20260812.json cargo test -p julie-extract-cli --features test-store-resolution-contract --test store_resolution_performance store_resolution_query_diagnostic_worker -- --exact --nocapture`

Result: PASS once, no retry.

- process wall 43.10s; test 42.89s
- user 42.01s; system 0.93s; CPU 99%; max RSS 56,536KB
- major faults 0; minor faults 20,100; file input 0; output 2,112 blocks
- resolver `elapsed_ms` 24,710ms
- `finish_exact` 18.160769s
- exact DB SHA-256 `b8833220fd1e24b78586e2ddd1c0f8d17b77c37a1fb58c9df175617c5d46c28b`, unchanged from baseline
- 392,526 identifier rows; 10,804 pending rows; 1,538 source versions; integrity `ok`
- final and live phase arrays were identical

| Phase | Cumulative | Exclusive | Baseline exclusive | Change |
|---|---:|---:|---:|---:|
| identifier_totality | 5.791301s | 5.791301s | 5.802430s | -0.011129s |
| writer_init | 5.792579s | 1.278ms | 1.319ms | -0.041ms |
| source_versions | 5.814772s | 22.193ms | 22.859ms | -0.666ms |
| identifier_rows | 13.361977s | 7.547205s | 14.304006s | -6.756801s (-47.2%) |
| pending_rows | 13.627848s | 265.871ms | 267.674ms | -1.803ms |
| writer_finish | 18.151414s | 4.523566s | 4.536753s | -13.187ms |
| scratch_cleanup | 18.160769s | 9.355ms | 11.830ms | -2.475ms |

Process wall improved 6.88s, 13.8%. The improvement is confined to the selected identifier-row phase within noise of the other boundaries.

## Affected Verification

- artifact schema `streaming_base_writer`: 4 passed, 14 filtered.
- artifact base publication exact: 1 passed, 8 filtered, 0.09s. An initial command ran zero because the target's `test-store-resolution` feature was omitted; the corrected exact feature-gated command passed.
- resolution-base crash publication: 1 passed, 12 filtered, six transaction/file boundaries, 1.32s.
- finish observer filter: 4 passed, 16 filtered, 0.26s. The caught simulated observer panic is expected.
- strict artifact Clippy with `test-store-resolution,test-store-crash`: PASS.
- strict CLI library/performance Clippy with `test-store-resolution-contract`: PASS.
- `cargo fmt --all -- --check`: PASS.
- `git diff --check`: PASS.

No broad suite, second optimization, or unchanged replay ran.

## Gate Invariants

- PASS: eight fixed cumulative phases, exactly once and in hand-derived order.
- PASS: production `finish_exact` interface and identity unchanged.
- PASS: live output retains the last completed phase on observer interruption.
- PASS: observer persistence overhead excluded from finish cumulative timing.
- PASS: exact file SHA, row counts, ordering, integrity, crash recovery, and publication identity unchanged.
- PASS: cache bounded to one statement; no corpus buffering, page API, public CLI/report/schema/trait change.

Hard metrics are exact identity, SHA, row counts, phase order, query-family counters, and integrity. Wall timing, CPU, RSS, page faults, and filesystem counters are report-only comparisons from single same-machine runs.

## Miller Calls and API-shape Evidence

All Miller calls used workspace `/home/murphy/source/julie-extractors` with `ensure_fresh=false`; this task worktree was not registered or opened.

- `context` on `finish_exact`: proved the indexed eight-step ordering and pivots to prior overlay, totality validation, and `ResolutionBaseWriter`.
- `inspect StoreScratchResolutionSession::finish_exact`, including continuation: proved `pub fn finish_exact(mut self) -> Result<ResolutionFileIdentity, StoreResolutionError>` and the exact stream/finish/cleanup sequence.
- `trace refs finish_exact`: returned 16 fallback references including production resolve and focused mechanism/performance callers.
- `impact finish_exact`: medium blast radius and the affected scoped predecessor, collision, phase-window, pinned-fixture, visible-root, and high-cardinality performance contracts.
- `inspect ResolutionBaseWriter::push_identifier_resolution`: proved the public streaming row method validates sorted unique keys and previously executed the same insert per call.
- `inspect ResolutionBaseWriter` and `new`: proved one owned SQLite connection, ordering state, Bulk pragmas, schema creation, and one `BEGIN IMMEDIATE` transaction.
- `impact` and `trace` for `push_identifier_resolution`: references were incomplete/ambiguous in the indexed main workspace; bounded source reads selected exact contracts.
- `search mode=source` for the writer: found schema, crash, binding, base, and CLI call sites.
- `inspect` of the other same-named builder method: proved it was an in-memory builder and not the production streaming seam.
- `inspect StatementPreparationCounter` and `prepare_cached`: proved the repository already uses a fixed preparation-count wrapper over transaction `prepare_cached`; this supported reuse of rusqlite's bounded statement cache instead of new telemetry or a public page API.

Miller lacked the branch-only diagnostic worker/instrumentation shape because the registered workspace indexed the main checkout. That limitation was recorded and bounded local reads were used only for branch-only code and exact test selection.

## Files Changed

- `crates/julie-extract-cli/src/store/resolution_session.rs`
- `crates/julie-extract-cli/tests/store_resolution_performance.rs`
- `crates/julie-extract-artifact/src/store/resolution.rs`
- `docs/plans/2026-08-11-store-resolution-query-amplification-design.md`
- `docs/plans/2026-08-11-store-resolution-query-amplification-plan.md`
- `.razorback/sdd/2026-08-11-store-resolution-query-amplification-plan/task-4-brief.md`
- `.razorback/sdd/2026-08-11-store-resolution-query-amplification-plan/task-4-report.md`
- `.razorback/sdd/2026-08-11-store-resolution-query-amplification-plan/progress.md`
- one new Goldfish checkpoint created immediately before commit

## Self-review and Judgment Calls

- `crates/julie-extract-cli/src/store/resolution_session.rs:595` - kept the production method signature and delegated to one inner implementation so instrumentation cannot fork behavior.
- `crates/julie-extract-cli/src/store/resolution_session.rs:600` - exposed the observer only under the existing test feature and subtracted callback wall to keep timing local to finish work.
- `crates/julie-extract-cli/src/store/resolution_session.rs:626` - emitted a sample only after a successful boundary so timeout state never claims incomplete work.
- `crates/julie-extract-cli/tests/store_resolution_performance.rs:244` - persisted live samples through the existing atomic diagnostic path rather than a public reporter or CLI field.
- `crates/julie-extract-cli/tests/store_resolution_performance.rs:991` - used 100,000 real streamed rows with a generous 2.5-second same-machine invariant; semantic assertions precede the time assertion so RED proves performance, not setup.
- `crates/julie-extract-artifact/src/store/resolution.rs:3217` - bounded the connection cache to exactly one statement because only the measured identifier insert needs reuse.
- `crates/julie-extract-artifact/src/store/resolution.rs:3293` - retained per-row validation and immediate execution behind the unchanged API; rejected a public bounded-page API because it added surface without measured benefit.
- `crates/julie-extract-artifact/src/store/resolution.rs:3309` - used the repository's established `prepare_cached` pattern; rejected CLI attach/copy because it would leak artifact schema and policy across the seam.

Concern: resolver time remains about 24.7 seconds, identifier totality about 5.8 seconds, and writer finish about 4.5 seconds. Task 4 deliberately stops after the single selected fix; another optimization requires a new measured task rather than stacking behavior here.
