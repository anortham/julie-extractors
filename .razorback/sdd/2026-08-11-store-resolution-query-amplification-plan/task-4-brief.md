### Task 4: Exact finalization telemetry and fix

**Files:**
- Modify: exact `StoreScratchResolutionSession::finish_exact` source selected by the diagnostic
- Modify: `crates/julie-extract-artifact/src/store/resolution.rs` after the first replay selected identifier-row insertion
- Test: focused store-resolution mechanism or performance regression for the selected phase
- Modify: `docs/plans/2026-08-11-store-resolution-query-amplification-design.md`
- Modify: this plan

**Interfaces:**
- Consumes: Task 3's one bounded replay and the measured roughly 23.7-second post-resolver interval.
- Produces: fixed-cardinality finalization phase timings/counters and one bounded optimization for the largest measured phase.

**Contract inputs:** Instrument before changing behavior. Preserve crash consistency, manifest/base identity, canonical digest, and row equality. Do not combine unrelated finalization phases in one fix.

**Measured baseline:** The accepted caller-attribution replay is 49.88 seconds wall, 24,813ms inside `run_resolution_session`, and approximately 24.98 seconds after the resolver. The rejected Pending batch replay is not a new baseline: it was 50.46 seconds wall / 25,418ms resolver and was removed at `b8abd489`.

**Instrumentation contract:** Add fixed-cardinality cumulative timings around the existing `finish_exact` boundaries, without a public CLI/report/store schema change. The fixed phases are `prior_overlay`, `identifier_totality`, `writer_init`, `source_versions`, `identifier_rows`, `pending_rows`, `writer_finish`, and `scratch_cleanup`, plus total elapsed time if useful. A test-only observer must publish a cumulative snapshot after every completed phase so a hard timeout retains the last completed boundary. Never use paths, SQL, IDs, symbol names, or dynamic labels.

**Caller contract:** Keep the existing `finish_exact() -> Result<ResolutionFileIdentity, StoreResolutionError>` interface unchanged. The diagnostic-only interface may return or observe the fixed sample under `test-store-resolution-contract`; production resolution must not learn timing/report policy.

**Diagnostic contract:** Extend the existing `store_resolution_query_diagnostic_worker` JSON/live snapshot rather than adding another standalone harness. Prove the fixed phase serialization/order in one exact test before the replay. Use one fresh reflink-only reset clone, verify integrity/readiness and source immutability, then run exactly one 60-second TERM / 5-second KILL replay. Do not retry the same replay unchanged.

**Selection contract:** Instrumentation is Task 4A. Only after the one replay names the largest phase may Task 4B add a production optimization. Write one exact behavioral RED for that phase's work invariant, then the minimum GREEN. If no single phase is large enough or the observed cost is in `writer_finish`, deepen telemetry only inside the measured boundary before changing behavior. Do not guess or bundle two phases.

## Architecture Quality

**Affected modules:** Store scratch resolution finalization, the existing test-only performance diagnostic, and the artifact resolution writer after the first replay measured identifier-row insertion as dominant and lead review expanded ownership.

**Caller-facing interface:** Existing `finish_exact` remains unchanged. The new seam is test-feature-only, fixed-cardinality, and smaller than the finalization behavior it observes.

**Depth/locality check:** Timing policy stays inside `StoreScratchResolutionSession`; JSON persistence stays inside the test harness. Any eventual fix stays inside the single measured phase.

**Test surface:** An exact real-session finish test proves observer order/completeness and exact identity; the faithful diagnostic proves phase attribution; the selected phase then gets its own caller-facing behavioral RED/GREEN.

**Seams/adapters:** Reuse the existing diagnostic worker and its live JSON path. Do not add a public reporter, schema, trait method, or workspace-sized cache.

**Rejected shortcuts:** Inferring phase cost from source shape; optimizing `validate_identifier_totality` before measurement; timing only the whole method; rerunning the 50-second replay after a no-evidence change.

**Architecture risk:** Medium. Crash consistency and publication identity are load-bearing, but the instrumentation seam is test-only and behavior-local.

**File ownership:** exact finalization source/tests selected by the diagnostic; `crates/julie-extract-artifact/src/store/resolution.rs` and exact artifact/performance contracts for the measured identifier-row path; plus design/plan evidence

**Serialization required:** Yes.

**Dependency reason:** Resolver and finalization improvements must be measured independently.

**Acceptance criteria:**
- [x] One bounded diagnostic divided `finish_exact` into fixed-cardinality phases and named identifier-row insertion at 14.304 seconds as the largest exclusive phase.
- [x] Exact RED failed at 3.415 seconds for 100,000 streamed identifier rows and passed at 1.715 seconds after the minimal cached-statement fix.
- [x] Canonical file SHA-256, rows, crash recovery, publication identity, and integrity remained exact.
- [x] The single post-fix bounded replay completed in 43.10 seconds, down from 49.98 seconds, and identifier-row insertion fell to 7.547 seconds.
- [x] Commit only owned files with `serial-worker-commit`.

**Worker red/green:** One exact `store_resolution_performance` test for fixed phase observation/serialization. Do not run the real replay until that is green.

**Worker ceiling:** Exact instrumentation test plus the existing exact artifact-finalization performance tests selected by Miller impact; formatting/diff check and strict affected-target Clippy. The worker owns the single replay only after reporting the fully verified clone, command, and quiet compile lane.

**Gate invariants:** Phase samples are fixed and cumulative; every successful boundary is reported exactly once in order; the normal `finish_exact` result and identity are unchanged; a killed diagnostic retains the last completed phase; final replay digest/rows/integrity match the accepted baseline.
