> **Superseded 2026-08-18.** The resolution write path is retired. See [2026-08-18-resolution-write-path-retirement.md](../decisions/2026-08-18-resolution-write-path-retirement.md).

# Store Resolution Query Amplification Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use razorback:subagent-driven-development when subagent delegation is available. Fall back to razorback:executing-plans for single-task, tightly-sequential, or no-delegation runs.

**Goal:** Identify and remove the resolver query family responsible for production SQLite read amplification, with telemetry that prevents another speculative fix.

**Architecture:** Add fixed-cardinality query and caller-phase counters inside the test diagnostic, disprove or confirm each candidate family with bounded evidence, then move exact co-located identifier lookup into the existing bounded Pending hydration page. Keep generic resolution storage-agnostic and preserve a fallback for session adapters that cannot pre-hydrate the result. Instrument exact finalization only after the resolver query fix is measured.

**Tech Stack:** Rust, rusqlite, SQLite family store, cargo test, existing store-resolution performance fixtures.

**Architecture Quality:** Medium-risk deep-session change. A `SessionPendingWorkItem` wraps the artifact-owned `PendingWorkItem` with the same complete/optional identifier-result contract as `SessionRelationship`; the store adapter owns SQLite batching and the generic resolver owns only fallback policy. No public artifact/store schema or workspace-sized cache is added.

## Global Constraints

- Preserve canonical exact output and row counts byte-for-byte.
- Preserve manifest view/generation visibility predicates and deterministic ordering.
- Candidate caches have fixed capacity and never scale with workspace size.
- Telemetry labels have fixed cardinality and never contain SQL, paths, names, or symbol IDs.
- Do not cache errors or incomplete pages as complete results.
- One-file incremental resolution target is at most 5 seconds; full real Miller resolution target is at most 60 seconds.
- Never repeat an unchanged operation lasting more than 60 seconds without new phase/query evidence.
- Do not run a real corpus replay before the deterministic query-count regression is green.

---

## Verification Strategy

**Project source of truth:** repository `AGENTS.md`, existing `store_resolution_mechanism` and `store_resolution_performance` contracts.

**Worker red/green scope:** exact new tests with `cargo test -p julie-extract-cli --features test-store-resolution-contract --test store_resolution_performance <exact-test> -- --exact --nocapture` or the matching mechanism test command.

**Worker ceiling:** the two affected integration-test binaries only, narrowed to their relevant tests where possible. Do not run all targets, xtask multi-run benchmarks, or a real Miller corpus.

**Worker gate invariant:** resolved-pending demotions preserve exact output while co-located identifier lookup executions scale with bounded pages rather than pending rows; counters accurately distinguish executions and rows.

**Lead affected-change scope:** affected mechanism and performance test binaries once, then `cargo fmt --all -- --check` and strict Clippy for the changed library/CLI target.

**Branch gate:** existing CLI affected contract group; no all-workspace gate until the performance fix is integrated with the release candidate.

**Security scope:** none declared.

**Replay/metric evidence:** Hard gates are canonical digest/row equality, zero per-row store locator calls during resolved-pending recheck, bounded hydration-page executions, and the 60-second real replay timeout. Query-family counts, rows read, CPU, RSS, and page faults are report-only diagnostics.

**Escalation triggers:** Query executions still proportional to identifiers, any digest/row mismatch, cache growth above its constant cap, or bounded real replay timeout requires renewed root-cause analysis before rerunning.

**Assigned verification failure:** Workers stop and report when assigned verification fails, unless this plan explicitly says to update that gate.

**Verification ledger:** Record invariant, command, scope label, commit SHA, result, and timestamp. Reuse green evidence for an unchanged tree.

## Parallel Execution Contract

| Task | Parallel batch | File ownership | Serialization required | Dependency reason |
|---|---|---|---|---|
| Task 1: Query telemetry and repeated-name RED | None - serial | `crates/julie-extract-cli/src/store/resolution_session.rs`; `crates/julie-extract-cli/src/store/report.rs` only if existing report wiring requires it; focused store resolution tests | Yes | Task 2 consumes the exact counters and failing fixture established here.
| Task 2: Bounded faithful query-family replay | None - serial | replay/report harness and evidence docs only unless telemetry wiring is incomplete | Yes | Must consume Task 1's counters and select, not implement, the next optimization.
| Task 3: Measured query-family fix | None - serial | exact resolver source/tests named by Task 2; design/plan evidence | Yes | Implementation depends on Task 2's measured dominant family and access pattern.
| Task 4: Exact finalization telemetry and fix | None - serial | exact `finish_exact` implementation/tests, artifact resolution writer and exact contracts selected by one bounded diagnostic; design/plan evidence | Yes | Begins only after Task 3's resolver replay, so the two measured phases remain attributable.

### Task 1: Query telemetry and repeated-name RED

**Files:**
- Modify: `crates/julie-extract-cli/src/store/resolution_session.rs`
- Modify only if required by existing report flow: `crates/julie-extract-cli/src/store/report.rs`
- Test: `crates/julie-extract-cli/tests/store_resolution_performance.rs`
- Test only if lower-level fixture construction is materially simpler there: `crates/julie-extract-cli/tests/store_resolution_mechanism.rs`

**Interfaces:**
- Consumes: `StoreScratchResolutionSession`, `candidate_page`, `with_candidate_reader`, and existing diagnostic accessors.
- Produces: fixed-cardinality candidate query metrics and a repeated-name/high-fanout regression that fails on query-count amplification while exact output succeeds.

**Contract inputs:** Count only actual SQLite candidate statement executions as executions; cache work belongs to Task 2.

**File ownership:** `crates/julie-extract-cli/src/store/resolution_session.rs`; `crates/julie-extract-cli/src/store/report.rs` only if existing report wiring requires it; focused store resolution tests

**Serialization required:** Yes.

**Dependency reason:** Establishes the measurement contract and exact RED used by Task 2.

**Acceptance criteria:**
- [x] Fixed query-family counters capture executions and candidate rows without unbounded labels.
- [x] A deterministic fixture has repeated identifiers for one name and candidate fanout above `window_size`.
- [x] The fixture records expected exact output and disproves the initial top-level and filtered-summary cache hypotheses.
- [x] Existing bounded-cache/page diagnostics remain correct.
- [x] Exact telemetry plus relevant existing focused tests complete within the worker ceiling.
- [x] Commit only owned files with `serial-worker-commit`.

### Task 2: Bounded faithful query-family replay

**Files:**
- Modify only if required to expose existing counters through the current report: `crates/julie-extract-cli/src/store/report.rs`
- Modify only if required for a single-run bounded harness: `crates/julie-extract-cli/tests/store_resolution_performance.rs` or `xtask/src/resolution_performance.rs`
- Modify: `docs/plans/2026-08-11-store-resolution-query-amplification-design.md`
- Modify: this plan

**Interfaces:**
- Consumes: Task 1's query-family counters and the real Miller family store/view fixture.
- Produces: one bounded sample naming executions, rows, and elapsed time by query family plus a written selection of the next optimization.

**Contract inputs:** Hard timeout is 60 seconds. Preserve the stopped sample and counters. Do not immediately rerun a timeout and do not modify resolver behavior in this task.

**File ownership:** replay/report harness only if required, and query-amplification design/plan evidence

**Serialization required:** Yes.

**Dependency reason:** Requires Task 1's counters and prevents implementation against a disproven theory.

**Acceptance criteria:**
- [x] A single real replay is attempted only after telemetry gates, with a 60-second hard timeout and query counters captured. (A reflink clone reused a verified ready predecessor; one replay completed in 49.81 seconds.)
- [x] The report ranks query families by executions, rows, and available elapsed time.
- [x] If the replay exceeds 60 seconds, it stops once and the retained counters select the next bottleneck; it is not rerun unchanged. (The replay completed; the earlier setup timeout was not repeated unchanged.)
- [x] The initial design selected the measured `LocateIdentifier` family before production behavior changed; the first caller attribution was then explicitly rejected when its single faithful replay showed zero relationship-coverage executions.
- [x] Commit only owned files with `serial-worker-commit`.

### Task 3: Measured query-family fix

**Files:**
- Modify: `crates/julie-extract-cli/src/resolution_session.rs`
- Modify: `crates/julie-extract-cli/src/store/resolution_session.rs`
- Modify: `crates/julie-extract-cli/src/resolution.rs`
- Test: `crates/julie-extract-cli/tests/store_resolution_mechanism.rs`
- Test only if the fixed query-family sample changes: `crates/julie-extract-cli/tests/store_resolution_performance.rs`
- Modify: `docs/plans/2026-08-11-store-resolution-query-amplification-design.md`
- Modify: this plan

**Interfaces:**
- Consumes: Task 2's 10,804 `LocateIdentifier` executions, two disproven caller replays, caller-phase telemetry commit `5089c3a2`, and the existing `SessionRelationship` complete/optional hydration contract.
- Produces: `SessionPendingWorkItem { pending, located_identifier_id, identifier_lookup_complete }`, populated in bounded store Pending pages and consumed without per-row store locator calls.

**Contract inputs:** Exact caller evidence is Pending=10,804 and every other phase=0. Preserve exactly-one-match semantics for byte spans and line fallback, including ambiguity and no-match. Store hydration marks lookup complete even when no unique identifier exists. Other adapters may set it false and retain the existing locator fallback. Do not add a workspace-sized cache or store schema.

**File ownership:** the three exact resolver/session sources, mechanism/performance regression only as required, plus design/plan evidence

**Serialization required:** Yes.

**Dependency reason:** Must not optimize before Task 2 names the production bottleneck.

**Acceptance criteria:**
- [x] Exact RED exercised the real Pending phase and failed at 8 locator statements versus zero after semantic assertions.
- [x] GREEN covered byte span, line fallback, ambiguity, no match, and fallback while reducing locator statements to zero.
- [x] Existing `PendingHydration` remained one execution per bounded page; no query family was renamed or double-counted.
- [x] Exact digest, pending/identifier counts, and rows remained unchanged.
- [x] One faithful replay completed once under 60 seconds and showed the candidate was not wall-clock load-bearing: 50.46 seconds versus 49.88 seconds baseline.
- [x] The no-win production/test slice was removed; only caller-attribution telemetry remains committed at `5089c3a2`.

### Task 4: Exact finalization telemetry and fix

**Files:**
- Modify: exact `StoreScratchResolutionSession::finish_exact` source selected by the diagnostic
- Modify: `crates/julie-extract-artifact/src/store/resolution.rs` after the bounded diagnostic selected identifier-row insertion
- Test: focused store-resolution mechanism or performance regression for the selected phase
- Modify: `docs/plans/2026-08-11-store-resolution-query-amplification-design.md`
- Modify: this plan

**Interfaces:**
- Consumes: Task 3's one bounded replay and the measured roughly 23.7-second post-resolver interval.
- Produces: fixed-cardinality finalization phase timings/counters and one bounded optimization for the largest measured phase.

**Contract inputs:** Instrument before changing behavior. Preserve crash consistency, manifest/base identity, canonical digest, and row equality. Do not combine unrelated finalization phases in one fix.

**File ownership:** exact finalization source/tests selected by the diagnostic; artifact resolution writer and exact artifact/performance contracts selected by the measured identifier-row phase; plus design/plan evidence

**Serialization required:** Yes.

**Dependency reason:** Resolver and finalization improvements must be measured independently.

**Acceptance criteria:**
- [x] One bounded diagnostic divided `finish_exact` into eight fixed phases and measured identifier-row insertion as the largest exclusive phase at 14.304 seconds.
- [x] Exact RED failed at 3.415 seconds for 100,000 streamed identifier rows and passed at 1.715 seconds after one-entry prepared-statement reuse.
- [x] Canonical exact file SHA-256, 392,526 identifier rows, 10,804 pending rows, 1,538 source versions, crash recovery, publication identity, and integrity remained exact.
- [x] The one post-fix replay completed in 43.10 seconds versus the 49.98-second instrumentation baseline; identifier-row insertion fell to 7.547 seconds.
- [x] Commit only owned files with `serial-worker-commit`.
