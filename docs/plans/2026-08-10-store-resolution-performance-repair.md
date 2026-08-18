> **Superseded 2026-08-18.** The resolution write path is retired. See [2026-08-18-resolution-write-path-retirement.md](../decisions/2026-08-18-resolution-write-path-retirement.md).

# Store Resolution Performance Repair Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use razorback:subagent-driven-development when subagent delegation is available. Fall back to razorback:executing-plans for single-task, tightly-sequential, or no-delegation runs.

**Goal:** Make family-store resolution converge at production cardinality without repeated validated opens, per-edge disk bookkeeping, or broad candidate scans.

**Architecture:** Keep the public store-resolution and schema contracts unchanged. Reuse one validated reader and prepared statement for exact target validation. During resolution, retain the store session's bounded 300-row design: cache only the current candidate-name window, push language/kind/file filters into SQLite, reuse bounded cross-window summaries, hydrate relationship identifiers in bounded batches, and spill tier-candidate deduplication to scratch only after the in-memory window fills.

**Tech Stack:** Rust, rusqlite, SQLite, Cargo feature-gated performance tests.

**Architecture Quality:** Resolver policy stays storage-agnostic through `CandidateLookup`; the store implementation overrides only filtered retrieval and bounded caching. Peak memory remains a function of the configured window, not corpus rows. The whole-corpus `WorkspaceCandidateIndex` alternative was rejected because it violated the store mechanism's bounded-memory contract. Target validation, artifact identity, CLI output, resolver epoch, and schema remain unchanged.

## Global Constraints

- Preserve exact resolution semantics and target validation.
- Keep the default test suite fast; production-cardinality timing stays under the existing `test-perf` feature.
- Do not change store schemas, resolver epochs, CLI output, or the public resolution artifact contract.
- The performance fixture must model many distinct target pairs, not only many identifiers sharing one target.

---

## Verification Strategy

**Project source of truth:** `AGENTS.md`, `crates/julie-extract-cli/Cargo.toml`, and `xtask` test tiers.

**Worker red/green scope:** `cargo test -p julie-extract-cli --features test-store-resolution-contract --test store_resolution_performance target_validation_finishes_with_high_distinct_target_cardinality -- --exact --nocapture`.

**Worker ceiling:** The focused CLI performance integration test and directly affected store-resolution mechanism tests.

**Worker gate invariant:** High-cardinality exact-base creation must preserve results while completing within the fixed gate that fails the current per-target connection-open implementation.

**Lead affected-change scope:** `cargo test -p julie-extract-cli --features test-store-resolution-contract --test store_resolution_mechanism -- --nocapture` and the complete `store_resolution_performance` test target.

**Branch gate:** `cargo xtask test default`, `cargo xtask test contract`, `cargo fmt --all -- --check`, and `cargo clippy --workspace --all-targets --all-features --no-deps -- -D warnings`.

**Security scope:** none declared.

**Replay/metric evidence:** Exact output equivalence and target-validation failures are hard gates. Real Miller-corpus wall time, logical bytes read, target-pair count, and sidecar convergence time are hard usability evidence for this repair.

**Escalation triggers:** Any schema, resolver-epoch, result-byte, or CLI-contract change requires the full contract tier and a new architecture decision. Any remaining material hotspot gets its own evidence-backed follow-up slice rather than being hidden in this fix.

**Assigned verification failure:** Workers stop and report when assigned verification fails, unless this plan explicitly says to update that gate.

**Verification ledger:** Record invariant, command, scope label, commit SHA, result, and timestamp. For replay or metric evidence, also record hard-gate metrics and report-only metrics. If the same HEAD already has a passing ledger entry for the required scope, reuse that evidence instead of rerunning the same expensive gate.

## Parallel Execution Contract

| Task | Parallel batch | File ownership | Serialization required | Dependency reason |
|---|---|---|---|---|
| Task 1: Production-cardinality regression | None - serial | `crates/julie-extract-cli/tests/store_resolution_performance.rs` | Yes | The failing gate defines the implementation contract. |
| Task 2: Reuse one validated target reader and candidate index | None - serial | `crates/julie-extract-cli/src/store/resolution_session.rs`; `crates/julie-extract-cli/src/resolution.rs` | Yes | Requires Task 1's observed failure and live replay evidence. |
| Task 3: Real-corpus verification and performance finding | None - serial | `docs/findings/2026-08-10-store-resolution-performance-repair.md` | Yes | Requires the repaired binary and live before/after evidence. |

### Task 1: Production-cardinality regression

**Files:**
- Modify: `crates/julie-extract-cli/tests/store_resolution_performance.rs`

**Interfaces:**
- Consumes: `StoreScratchResolutionSession::finish_exact` and the real versioned store fixture.
- Produces: A feature-gated regression whose distinct-target cardinality makes connection-open amplification observable.

**Contract inputs:** Existing Miller-scale row counts and fixed performance-gate conventions.

**File ownership:** `crates/julie-extract-cli/tests/store_resolution_performance.rs`

**Serialization required:** Yes.

**Dependency reason:** The failing gate defines the implementation contract.

**What to build:** Add a bounded high-distinct-target case that builds an exact resolution base through the real session and asserts both semantic counts and a conservative fixed completion ceiling. Verify it fails on current `main` for the observed repeated-reader cost.

**Approach:** Keep it behind `test-perf`; use enough unique targets to fail deterministically without making the gate itself unusably slow. Derive expected row and target counts directly from literals in the fixture.

**Acceptance criteria:**
- [x] The new test fails against `c26b04f` for elapsed target-validation time, not setup or correctness.
- [x] The fixture contains many distinct valid target pairs and verifies the emitted exact counts.
- [x] Worker-scope verification passes after Task 2 and the change is handed to the lead per `parallel-lead-commit`.

### Task 2: Reuse one validated target reader and candidate index

**Files:**
- Modify: `crates/julie-extract-cli/src/store/resolution_session.rs:374-458`
- Modify: `crates/julie-extract-cli/src/resolution.rs:439-563`

**Interfaces:**
- Consumes: `StoreConnectionFactory::open_reader` and `ResolutionBaseWriter::finish_with_target_lookup`.
- Produces: The unchanged `finish_exact() -> Result<ResolutionFileIdentity, StoreResolutionError>` contract with one reader and one prepared statement per exact-base validation.

**Contract inputs:** Task 1's regression and the existing indexed `(version_id, symbol_id)` plus manifest-version lookup.

**File ownership:** `crates/julie-extract-cli/src/store/resolution_session.rs`; `crates/julie-extract-cli/src/resolution.rs`

**Serialization required:** Yes.

**Dependency reason:** Requires Task 1's observed failure.

**What to build:** Open the store reader and prepare the target-existence statement once before calling `finish_with_target_lookup`. Replace per-edge store work with bounded per-window candidate caches, filtered summaries, reader reuse, and a window-sized tier accumulator that spills to scratch only for high collisions.

**Approach:** Preserve all validation and error mapping. Extend the shared candidate-lookup interface with default filtered operations so the in-memory resolver keeps identical semantics while the store implementation can use selective SQL. Never hold a whole-corpus snapshot.

**Acceptance criteria:**
- [x] One exact-base finish performs one validated store-reader open regardless of target count.
- [x] High-distinct-name candidate resolution stays under its fixed gate and no longer performs per-edge broad store reads.
- [x] Missing targets still fail with the same typed validation error and incomplete artifacts remain unpublished.
- [x] Existing mechanism/equivalence tests and the new performance regression pass.
- [x] Worker-scope verification passes and the change is handed to the lead per `parallel-lead-commit`.

### Task 3: Real-corpus verification and performance finding

**Files:**
- Create: `docs/findings/2026-08-10-store-resolution-performance-repair.md`

**Interfaces:**
- Consumes: The repaired `julie-extract` binary, Miller's live family store, store logs, process I/O counters, and Miller sidecar status.
- Produces: Reproducible before/after evidence and a ranked disposition for every remaining material convergence hotspot.

**Contract inputs:** Before evidence: 6m20s producer resolve, 56,433 distinct target pairs, at least 45.9 GB logical reads, and about 1.5m subsequent sidecar convergence.

**File ownership:** `docs/findings/2026-08-10-store-resolution-performance-repair.md`

**Serialization required:** Yes.

**Dependency reason:** Requires the repaired binary and live before/after evidence.

**What to build:** Re-run the exact Miller corpus path with the repaired producer, record phase timings and I/O, and inspect the remaining producer and Miller consumer path for other avoidable full passes or N+1 work. Implement any confirmed material producer issue as another TDD slice; route consumer issues into the corresponding Miller repair plan.

**Approach:** Treat warm query latency and background convergence separately. Do not call the storage mechanism usable until the real corpus is exact/current within a practical bound and repeated no-op/small-delta behavior is measured.

**Acceptance criteria:**
- [x] Real Miller producer resolve improves materially from 6m20s and no longer exhibits per-target connection/read amplification.
- [x] Exact resolution counts and output identity remain correct.
- [x] Remaining material consumer hotspots are carried into an active Miller implementation slice with measured evidence.
- [x] Branch verification passes and the evidence is recorded.
