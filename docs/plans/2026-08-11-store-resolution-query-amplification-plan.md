# Store Resolution Query Amplification Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use razorback:subagent-driven-development when subagent delegation is available. Fall back to razorback:executing-plans for single-task, tightly-sequential, or no-delegation runs.

**Goal:** Identify and remove the resolver query family responsible for production SQLite read amplification, with telemetry that prevents another speculative fix.

**Architecture:** Add fixed-cardinality query counters inside `StoreScratchResolutionSession`, disprove or confirm each candidate family with bounded evidence, then implement the smallest optimization selected by one 60-second faithful replay. Do not preselect a cache or SQL rewrite before counters identify the production family.

**Tech Stack:** Rust, rusqlite, SQLite family store, cargo test, existing store-resolution performance fixtures.

**Architecture Quality:** High-risk data-path optimization. All caching stays private to one resolution session; eviction is correctness-neutral, and public artifact/store schemas remain unchanged.

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

**Worker gate invariant:** repeated high-fanout candidate requests preserve exact output while SQLite candidate executions scale with distinct pages, and counters accurately distinguish executions, hits, and rows.

**Lead affected-change scope:** affected mechanism and performance test binaries once, then `cargo fmt --all -- --check` and strict Clippy for the changed library/CLI target.

**Branch gate:** existing CLI affected contract group; no all-workspace gate until the performance fix is integrated with the release candidate.

**Security scope:** none declared.

**Replay/metric evidence:** Hard gates are canonical digest/row equality, candidate query execution bound, cache-capacity bound, and the 60-second real replay timeout. Query-family counts, rows read, CPU, RSS, and page faults are report-only diagnostics.

**Escalation triggers:** Query executions still proportional to identifiers, any digest/row mismatch, cache growth above its constant cap, or bounded real replay timeout requires renewed root-cause analysis before rerunning.

**Assigned verification failure:** Workers stop and report when assigned verification fails, unless this plan explicitly says to update that gate.

**Verification ledger:** Record invariant, command, scope label, commit SHA, result, and timestamp. Reuse green evidence for an unchanged tree.

## Parallel Execution Contract

| Task | Parallel batch | File ownership | Serialization required | Dependency reason |
|---|---|---|---|---|
| Task 1: Query telemetry and repeated-name RED | None - serial | `crates/julie-extract-cli/src/store/resolution_session.rs`; `crates/julie-extract-cli/src/store/report.rs` only if existing report wiring requires it; focused store resolution tests | Yes | Task 2 consumes the exact counters and failing fixture established here.
| Task 2: Bounded faithful query-family replay | None - serial | replay/report harness and evidence docs only unless telemetry wiring is incomplete | Yes | Must consume Task 1's counters and select, not implement, the next optimization.
| Task 3: Measured query-family fix | None - serial | exact resolver source/tests named by Task 2; design/plan evidence | Yes | Implementation depends on Task 2's measured dominant family and access pattern.

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
- [ ] Fixed query-family counters capture executions and candidate rows without unbounded labels.
- [ ] A deterministic fixture has repeated identifiers for one name and candidate fanout above `window_size`.
- [ ] The fixture records expected exact output and fails specifically because executions grow with identifiers.
- [ ] Existing bounded-cache/page diagnostics remain correct.
- [ ] Exact RED plus relevant existing focused tests complete within the worker ceiling.
- [ ] Commit only owned files with `serial-worker-commit`.

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
- [x] The design and Task 3 are updated to the measured access pattern before production behavior changes. (`LocateIdentifier` inside `materialized_relationship_covers` is the primary target; `finish_exact` is tracked separately.)
- [x] Commit only owned files with `serial-worker-commit`.

### Task 3: Measured query-family fix

**Files:**
- Modify: exact resolver source named by Task 2
- Test: exact focused regression reproducing Task 2's dominant family
- Modify: `docs/plans/2026-08-11-store-resolution-query-amplification-design.md`
- Modify: this plan

**Interfaces:**
- Consumes: Task 2's dominant query family, key distribution, rows, and timeout evidence.
- Produces: the smallest bounded optimization that removes those repeated reads without semantic change.

**Contract inputs:** The exact implementation shape is written into this task by Task 2 before dispatch. A fixed-capacity decoded page cache is permitted only when Task 2 proves repeated exact page keys; otherwise use the measured path's appropriate batched/indexed/scratch representation.

**File ownership:** resolver source and exact regression selected by Task 2, plus design/plan evidence

**Serialization required:** Yes.

**Dependency reason:** Must not optimize before Task 2 names the production bottleneck.

**Acceptance criteria:**
- [ ] Exact regression fails for the measured query-count/time invariant before implementation and passes after.
- [ ] Exact digest, counts, and resolution rows remain unchanged.
- [ ] Memory/cache state remains explicitly bounded.
- [ ] A single bounded real replay completes within 60 seconds or identifies one newly measured next bottleneck without an unchanged rerun.
- [ ] Commit only owned files with `serial-worker-commit`.
