# Index Store Ph2c — Resolution and Adapters Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use razorback:subagent-driven-development when
> subagent delegation is available. Fall back to razorback:executing-plans for single-task,
> tightly-sequential, or no-delegation runs.

**Goal:** Add manifest-scoped reference resolution to the versioned family store, prove its
determinism/exactness/performance before exposing durable state, and ship real `store resolve`,
`store export`, and `store import --from-artifact` commands without changing legacy v3 artifact
output.

**Architecture:** Ph2c is two hard-gated slices. Ph2c-a first refactors the existing resolver behind
one storage-aware `ResolutionSession`, preserves the v3 writer as a pinned oracle, and proves the
real base/delta mechanism against production-owned SQLite schemas. Ph2c-b begins only after every
G1–G5 gate passes. It advances both store catalogs to schema v2, publishes immutable bases through a
two-phase filesystem/catalog state machine, computes cumulative per-view deltas off the writer
lease under a heartbeat claim, CAS-publishes exact bindings under a short writer lease, and adds
the three public adapters. The resolver engine never names legacy or store physical identities;
each session maps them to semantic version-qualified keys.

**Approved design:**
[`2026-08-08-index-store-ph2c-resolution-design.md`](2026-08-08-index-store-ph2c-resolution-design.md)
is authoritative. The design was approved after a three-cycle architecture doubt pass. This plan
refines names, file ownership, exact schema identities, red/green order, and branch gates; it does
not reopen the approved two-slice cut or thresholds.

**Tech stack:** Rust 1.97.1, rusqlite 0.40 with bundled SQLite, clap 4, serde/serde_json, sha2,
blake3, existing store WAL/FULL pragma helpers, and existing xtask test tiers.

## Architecture Quality

- **Affected modules:** legacy resolver policy remains in `julie-extract-cli`; the new session seam
  sits beside it. Store catalog/file lifecycle belongs to `julie-extract-artifact::store`.
  Public store commands stay thin and reuse the durable coordinator/report machinery.
- **Deep interface:** `ResolutionSession` is intentionally one cohesive interface. Candidate loads,
  worklist anti-joins, same-pass overlay visibility, and semantic writes are coupled by resolver
  invariants; splitting them into independent read/write ports would leak storage joins back into
  policy.
- **Dependency direction:** resolver policy depends on semantic session types; legacy/store
  adapters depend on physical schemas; artifact storage never depends on CLI resolver policy.
- **No long snapshot:** store resolution uses bounded immutable reads plus a separate scratch
  writer. It never `ATTACH`es `store.db` and never keeps one family-store read transaction open for
  the whole resolve.
- **Publication boundary:** off-lease work may create only scratch/final base files and in-memory or
  scratch delta state. Durable view/base/delta/log changes happen under the writer lease and one
  store transaction.
- **Architecture risk:** high until Ph2c-a closes G1–G5. Ph2c-b is forbidden while any hard
  mechanism gate is red.

## Scope Boundary

**In scope:**

- One `ResolutionSession` contract with legacy v3 and store scratch implementations.
- Byte/row-stable legacy resolution output and deterministic relationship propagation order.
- Production base and scratch-delta SQLite schemas shared by tests and runtime.
- Streaming semantic diff, replacements, pending tombstones, exact gap enumeration, and target
  integrity checks.
- Store/coord schema v2 with manifest-entry language, base/delta catalogs, roots, pins, coherent
  view binding states, and resolve/export/from-artifact request kinds.
- Two-phase immutable-base publication and torn-state recovery.
- Off-writer-lease resolve claims with heartbeat/loss fencing and short writer-lease CAS publish.
- Public `store resolve`, `store export`, and `store import --from-artifact` commands.
- G1–G5, actual-store G3b, crash, concurrency, equivalence, and dogfood evidence.

**Out of scope:**

- General retention, capacity eviction, vacuum/reindex escalation, generation promotion, or schema
  migration. Ph2d owns those and must honor `resolution_base_versions`, pins, and live claims.
- Miller registry/read-session wiring, sidecars, status/health/dashboard, and rollout. Ph3 owns it.
- Resolver-policy changes, new tiers, language-specific heuristics, or extraction changes.
- Any migration from store schema v1. Schema-v1 catalogs receive the existing typed `OlderSchema`
  refusal before mutation.

## Global Constraints

- Legacy artifact schema remains v6 and legacy output remains byte/row compatible. Legacy hook
  failures stay nonfatal `ResolutionHookError`s.
- Store resolution errors are fatal typed request errors. They never become a successful terminal
  request with partial exactness.
- Resolver semantic output excludes legacy `resolved_at_revision`. The new
  `RESOLVER_OUTPUT_EPOCH` is the compatibility identity for base reuse.
- A store session is bound to one `(family_id, view_id, manifest_generation, manifest_hash)`.
- Visible extraction rows come only from `indexed`/`failed_preserved` manifest entries with a
  version. Every manifest entry contributes `(path, language)` existence, including `failed`.
- Every visible version must have `complete_l2`; otherwise resolution returns
  `resolution_input_incomplete` before creating base, delta, exactness, or terminal success.
- Semantic identity is `(version_id, local_id)`. The engine receives semantic identity types and
  never physical `file_id`/table aliases/pathnames.
- Candidate, relationship, worklist, diff, and export order is deterministic. Stable order is part
  of the contract, not an implementation convenience.
- A resolved target pair must exist in the manifest-visible symbol set. Separate base files rely on
  an explicit streaming integrity pass because cross-database FKs are impossible.
- Base identity is `(manifest_hash, resolver_output_epoch)` only after L2 completeness is proven.
- One family may have at most one claimed resolve request. A claim heartbeat uses a dedicated
  coordinator connection and claim loss aborts before publication.
- Resolve computation does not hold the writer lease. Base registration/read roots and final CAS
  publication do.
- Existing import/update/delete behavior and report schema stay compatible. Content-changing
  manifest publication invalidates exactness in the same transaction; identical reuse may retain
  it.
- Tests that spawn a real CLI or measure Miller-scale data stay feature-gated and out of the default
  tier.
- No production crash hook appears outside exact test features; normal debug/release binaries must
  not contain hook environment keys or boundary names.
- No new MCP/API/network surface and no new dependency are planned.

## Exact Schema-v2 Contract

Task 6 writes these tables and named indexes exactly into the checked-in contract before runtime
code uses them. All timestamps in `store.db` remain canonical RFC3339 UTC text; coordinator times
remain injected Unix milliseconds.

### Store catalog

- `manifest_entries` adds `language TEXT NOT NULL CHECK(length(language) > 0)`. Manifest hash v2
  includes language in its length-delimited semantic tuple. Existing path/status/version identity
  fields remain unchanged.
- `resolution_bases(base_id TEXT PRIMARY KEY, manifest_hash TEXT NOT NULL,
  resolver_output_epoch INTEGER NOT NULL CHECK > 0, state TEXT building|ready,
  relative_path TEXT NOT NULL, identifier_count INTEGER NOT NULL CHECK >= 0,
  pending_count INTEGER NOT NULL CHECK >= 0, file_bytes INTEGER,
  file_sha256 TEXT, request_id TEXT NOT NULL, created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL)`; unique index
  `uidx_read_resolution_bases_identity(manifest_hash,resolver_output_epoch)`. The CHECK requires
  `file_bytes > 0` and a nonempty `file_sha256` only in `ready`; both are NULL in `building`.
- `resolution_base_versions(base_id TEXT, version_id INTEGER,
  PRIMARY KEY(base_id,version_id))` with base `ON DELETE CASCADE` and version `ON DELETE RESTRICT`.
- `resolution_deltas(view_id TEXT, delta_generation INTEGER CHECK > 0, base_id TEXT,
  manifest_generation INTEGER CHECK > 0, manifest_hash TEXT NOT NULL,
  resolver_output_epoch INTEGER CHECK > 0, identifier_replacements INTEGER CHECK >= 0,
  pending_replacements INTEGER CHECK >= 0, pending_tombstones INTEGER CHECK >= 0,
  exact_gap_rows INTEGER CHECK >= 0, exact_gap_files INTEGER CHECK >= 0,
  exact_gap_json TEXT NOT NULL, request_id TEXT NOT NULL, created_at TEXT NOT NULL,
  PRIMARY KEY(view_id,delta_generation))`. It references the view, ready base, and immutable
  `(view_id,manifest_generation)`. `exact_gap_json` is canonical, carries deterministic row/file
  facts, and its counts must match the scalar columns.
- `resolution_identifier_deltas(view_id,delta_generation,version_id,identifier_id,
  target_version_id,target_symbol_id,tier,confidence,method,outcome,candidates)` keyed by
  `(view_id,delta_generation,version_id,identifier_id)`. `outcome='resolved'` iff both target
  columns are non-NULL; other outcomes require them NULL. Target existence is additionally checked
  against the pinned visible symbol set at publish/read validation.
- `resolution_pending_deltas(view_id,delta_generation,version_id,pending_relationship_id,
  operation,target_version_id,target_symbol_id,tier,confidence,method)` keyed by
  `(view_id,delta_generation,version_id,pending_relationship_id)`. `operation` is
  `replace|tombstone`; replacement requires the target/tier/confidence/method payload and tombstone
  requires every payload field NULL.
- `resolution_pins(pin_id TEXT PRIMARY KEY, owner_kind TEXT reader|resolve, owner_id TEXT NOT NULL,
  view_id TEXT NOT NULL, manifest_generation INTEGER NOT NULL, base_id TEXT NOT NULL,
  delta_generation INTEGER, expires_at TEXT NOT NULL, created_at TEXT NOT NULL)`. The optional delta
  must belong to the same view/base/manifest tuple. Named read indexes cover owner expiry and bound
  tuple lookup.
- `views.resolution_state` expands to `unbound|converging|exact`. `unbound` requires every binding
  field NULL; `converging` requires base and delta generation with `resolution_exact_at` NULL;
  `exact` requires base, delta generation, and `resolution_exact_at=current_generation`. Composite
  deferred FKs point to the selected ready base and view delta.

### Coordinator catalog

- `requests.kind` accepts `import|update|delete|resolve|export|from_artifact`.
- Partial unique index `uidx_coord_one_claimed_resolve` on `requests(kind)` where
  `kind='resolve' AND state='claimed'` enforces one claimed resolve per family coordinator.
- Existing claim owner/heartbeat columns are the resolve heartbeat/fencing identity; no second
  lease table or retry timer is added.
- Import/update/delete retain existing writer-lease scheduling. Resolve uses a dedicated claim path
  and acquires the writer lease only for registration/recovery and final publication. Export is
  read/pin only. From-artifact uses ordinary batch writer scheduling.

### Resolution base file

- `base_meta(key TEXT PRIMARY KEY,value TEXT NOT NULL)` records format version, catalog SHA-256,
  manifest hash, resolver epoch, row counts, and completed state.
- `identifier_resolutions(version_id,identifier_id,target_version_id,target_symbol_id,tier,
  confidence,method,outcome,candidates,PRIMARY KEY(version_id,identifier_id))` with the same resolved
  target CHECK as the store delta table.
- `pending_resolutions(version_id,pending_relationship_id,target_version_id,target_symbol_id,tier,
  confidence,method,PRIMARY KEY(version_id,pending_relationship_id))`.
- Read-aligned indexes cover target pair and deterministic export order. The production creator,
  validator, and catalog-hash function are the only DDL authority for both Ph2c-a and Ph2c-b.

## ResolutionSession Interface

`crates/julie-extract-cli/src/resolution_session.rs` owns semantic types and this single trait. The
method names may change only through an explicit plan mismatch; responsibilities may not be split.

```rust
trait ResolutionSession {
    type Error;

    fn corpus_identity(&self) -> Result<ResolutionCorpusIdentity, Self::Error>;
    fn prior_resolution_state(&mut self) -> Result<Option<SessionResolutionState>, Self::Error>;
    fn current_revision(&mut self) -> Result<i64, Self::Error>;
    fn load_candidate_index(&mut self) -> Result<WorkspaceCandidateIndex, Self::Error>;
    fn select_worklists(
        &mut self,
        request: &ResolutionPassRequest,
        index: &WorkspaceCandidateIndex,
    ) -> Result<ResolutionWorklists, Self::Error>;
    fn load_identifier_locator(
        &mut self,
        scope: &ResolutionWorklistScope,
    ) -> Result<IdentifierLocator, Self::Error>;
    fn load_covered_identifiers(
        &mut self,
        index: &WorkspaceCandidateIndex,
        locator: &IdentifierLocator,
        scope: &ResolutionWorklistScope,
    ) -> Result<HashSet<SemanticIdentifierId>, Self::Error>;
    fn read_current_overlay(
        &mut self,
        worklists: &ResolutionWorklists,
    ) -> Result<CurrentResolutionOverlay, Self::Error>;
    fn flush(
        &mut self,
        writes: ResolutionWriteBatch,
    ) -> Result<ResolutionCounts, Self::Error>;
    fn aggregate_report(&mut self) -> Result<Vec<ResolutionReportRow>, Self::Error>;
}
```

`ResolutionCorpusIdentity` distinguishes legacy revision identity from store
family/view/manifest identity. Semantic row IDs carry version identity explicitly. The engine owns
tier policy and phase boundaries; sessions own physical joins, ordering, anti-joins, and flush
visibility.

## Verification Strategy

**Project source of truth:** `docs/testing-strategy.md`, `docs/release.md`, and the exact command
vectors in `xtask/src/test_tiers.rs`.

**Worker red/green scope:** every behavior change starts with the named focused test in its task.
Record the first failing assertion/compile error and first green run in the task report.

**Worker ceiling:** the touched package's default tests plus `cargo clippy -p <crate> --all-targets
-- -D warnings`, `cargo fmt --all -- --check`, and `git diff --check`. Feature-gated performance or
crash tasks run their owned feature command as well.

**Ph2c-a hard gate:** Tasks 1–5 end with three full G3 runs. Every pair/run must pass G1, G2, G3a,
G3b, G3c, G4, and G5. A failure stops execution and reopens the design; Tasks 6–12 do not begin.

**Lead batch gate:** `RUSTUP_TOOLCHAIN=1.97.1 cargo xtask test default` after each merged batch.

**Branch gate:**

```bash
RUSTUP_TOOLCHAIN=1.97.1 cargo fmt --all -- --check
RUSTUP_TOOLCHAIN=1.97.1 cargo test -p xtask
RUSTUP_TOOLCHAIN=1.97.1 cargo xtask test default
RUSTUP_TOOLCHAIN=1.97.1 cargo xtask test contract
RUSTUP_TOOLCHAIN=1.97.1 cargo test -p julie-extract-artifact --features test-store-resolution --test store_resolution_contract -- --test-threads=1
RUSTUP_TOOLCHAIN=1.97.1 cargo test -p julie-extract-cli --features test-store-resolution-contract --test store_resolution_contract -- --test-threads=1
RUSTUP_TOOLCHAIN=1.97.1 cargo test -p julie-extract-cli --features test-store-resolution-contract --test store_resolution_adapters -- --test-threads=1
RUSTUP_TOOLCHAIN=1.97.1 cargo xtask performance store-resolution --runs 3
RUSTUP_TOOLCHAIN=1.97.1 cargo clippy -p julie-extract-artifact --all-targets --all-features -- -D warnings
RUSTUP_TOOLCHAIN=1.97.1 cargo clippy -p julie-extract-cli --all-targets --all-features -- -D warnings
cargo deny check
git diff --check
```

**Security scope:**

- `security-deps`: `cargo deny check`; no dependency is planned, but schema/feature work must not
  weaken the existing advisory/license/source policy.
- `security-secrets`: none declared by the repository. Public fixtures contain no private source.
- `security-inputs`: validate every CLI path/identifier, scratch/final path containment, manifest
  identity, base hash, SQLite integrity, and imported-artifact schema before mutation.
- `security-concurrency`: claim, pin, writer-lease, and publish CAS tests are mandatory because
  stale ownership can corrupt durable state.
- `security-process`: crash hooks remain exact-feature-only and absent from normal binaries.
- The repository declares no external-model policy; the approved design was reviewed by Anthropic
  under Razorback's default policy and contained no secrets or private customer data.

**Evidence:** task reports live under
`.razorback/sdd/2026-08-08-index-store-ph2c-resolution-plan/`; generated SQLite databases, timing
JSON, RSS samples, and raw logs stay under `target/ph2c/`. Reuse passing evidence only at the exact
same HEAD.

## Parallel Execution Contract

| Task | Batch | File ownership | Serialization | Dependency |
|---|---|---|---|---|
| 1. Legacy oracle | A | CLI resolution contract tests/fixtures, CLI Cargo feature, xtask tier vector | No | None |
| 2. Session seam | serial | CLI `resolution.rs`, new `resolution_session.rs`, legacy tests | Yes | Task 1 oracle must catch drift |
| 3. Base/scratch schema | B | artifact new resolution file modules/tests | No | Task 2 semantic types fixed |
| 4. Streaming diff/exactness | serial | artifact diff module + CLI store session adapter/tests | Yes | Tasks 2–3 |
| 5. G1–G5 harness | serial hard stop | xtask performance command + feature harness/evidence | Yes | Tasks 1–4 |
| 6. Schema v2 | C | artifact store schema/manifest/coordinator contracts + docs | No | Ph2c-a all green |
| 7. Base lifecycle | C | artifact base files/layout/recovery/tests | No | Ph2c-a all green; consumes Task 3 |
| 8. Bind/delta/pins | serial | artifact binding/pin/delta runtime/tests | Yes | Tasks 6–7 |
| 9. Resolve request/CLI | serial | coordinator + CLI args/report/resolve/executor/tests | Yes | Task 8 |
| 10. Export adapter | D | CLI export module/tests | No | Task 8 pin/read API |
| 11. From-artifact adapter | D | CLI import adapter/tests | No | Tasks 7–9 writer/request APIs |
| 12. Final convergence | serial | xtask tiers, crash/equivalence/dogfood/docs | Yes | Tasks 9–11 |

Tasks in a shared batch may run in parallel only when their listed files remain disjoint. The lead
reviews and commits combined batch changes. Core session/schema/coordinator tasks remain serial.
No worker edits another worker's files, reverts shared changes, or commits unrelated state.

---

## Task 1: Pin the Legacy Resolution Oracle and Feature Gates

**Files:**

- Create: `crates/julie-extract-cli/tests/resolution_session_contract.rs`
- Create: `fixtures/store-resolution/legacy-v3/` deterministic multi-language fixture + expected
  semantic dump
- Modify: `crates/julie-extract-cli/Cargo.toml`
- Modify: `crates/julie-extract-artifact/Cargo.toml`
- Modify: `xtask/src/test_tiers.rs`
- Modify: `xtask/tests/test_tiers.rs`
- Modify: `crates/julie-extract-cli/tests/test_tiers.rs`
- Modify: `crates/julie-extract-artifact/tests/test_tiers.rs`

**Interfaces:**

- Add artifact feature `test-store-resolution` and CLI feature
  `test-store-resolution-contract = ["julie-extract-artifact/test-store-resolution"]`.
- Pin a semantic dump of both overlay tables, metadata, aggregate report rows, and deterministic
  relationship propagation order from the current v3 implementation.
- Fixture must include resolved/ambiguous/missing/no-context identifiers, resolved/unresolved
  pending edges, same-name collisions, module paths, failed/failed-preserved path-existence cases,
  and at least two languages.

**TDD steps:**

1. Add a contract that runs the existing resolver twice from fresh v3 artifacts and compares the
   full semantic dump byte-for-byte. Confirm it passes before refactoring.
2. Add a vacuity guard for every required outcome/table and a relationship-order collision case.
3. Add tier convention tests proving these real-CLI/store contracts are absent from default and
   present in contract routing with exact features and `--test-threads=1`.
4. Commit the generated expected dump only after a key-by-key audit against the v3 tables.

**Acceptance:**

- [x] Existing resolver produces the pinned dump twice with zero differences.
- [x] Oracle covers both resolution tables, metadata, aggregate rows, all outcomes, and ordering.
- [x] Default tier remains fast; contract tier invokes the exact feature harnesses.
- [x] No production behavior changes in this task.

## Task 2: Extract the ResolutionSession and Legacy Adapter

**Files:**

- Create: `crates/julie-extract-cli/src/resolution_session.rs`
- Modify: `crates/julie-extract-cli/src/lib.rs`
- Modify: `crates/julie-extract-cli/src/resolution.rs`
- Modify: `crates/julie-extract-artifact/src/resolution_store.rs` only for storage-neutral semantic
  value types required by both adapters
- Test: `crates/julie-extract-cli/tests/resolution_session_contract.rs`
- Test: existing `resolution_contract.rs`, `resolution_scope_equivalence.rs`,
  `resolution_report_scope.rs`, `resolution_shadow.rs`, and `resolution_perf.rs`

**Interfaces:**

- Implement the plan's `ResolutionSession` trait and semantic version-qualified IDs.
- `LegacyResolutionSession<'tx>` wraps the existing v3 transaction, maps `file_id` to the semantic
  adapter identity, and delegates physical worklists/writes to `resolution_store`.
- Refactor `run_resolution`, `resolve_full`, `resolve_delta`, co-location propagation, and report
  aggregation to use only the session interface.
- Preserve legacy failure mapping and phase timing exactly.

**TDD steps:**

1. Add a compile-time test-only fake session that proves resolver policy can run without a
   rusqlite connection.
2. Move one phase at a time behind the trait: corpus/prior state, candidate load, worklists,
   locator/covered set, current overlay, batched writes, aggregate report.
3. After each move run the pinned oracle and the narrow legacy resolution contracts.
4. Remove direct SQL/table names from the engine region; source-scan only as a supporting guard,
   never as the behavior proof.
5. Run the existing v3 performance harness and record no material regression before Task 3.

**Acceptance:**

- [x] Pinned legacy semantic dump remains byte-identical.
- [x] Existing scan/update/delete reports and nonfatal hook semantics remain unchanged.
- [x] Engine code names no physical v3/store tables, file IDs, connections, or paths.
- [x] Fake session exercises the resolver without SQLite.

## Task 3: Build the Production Base and Scratch-Delta Schemas

**Files:**

- Create: `crates/julie-extract-artifact/src/store/resolution.rs`
- Create: `crates/julie-extract-artifact/src/store/resolution_diff.rs`
- Modify: `crates/julie-extract-artifact/src/store/mod.rs`
- Create: `crates/julie-extract-artifact/tests/store_resolution_schema_contract.rs`
- Create: `docs/contracts/sqlite-resolution-base-schema-v1.md`
- Create: `docs/contracts/sqlite-resolution-delta-schema-v1.md`

**Interfaces:**

- `ResolutionBaseBuilder`, `ResolutionBaseReader`, `ResolutionScratchDelta`,
  `ResolutionSemanticCounts`, `ResolutionFileIdentity`, and typed validation errors.
- One production DDL constant and catalog-hash function per file type. Tests and runtime call the
  same creators; no test-owned approximation.
- Base writer accepts sorted semantic batches and stamps completed metadata only after all rows,
  indexes, target checks, FK/integrity checks, checkpoint, and durable close succeed.
- Scratch paths are caller-provided contained paths; final paths are never opened through symlinks.

**TDD steps:**

1. RED exact-catalog tests for tables, columns, CHECKs, indexes, user_version, and catalog SHA-256.
2. Implement base/scratch creators using existing store pragma profiles.
3. RED deterministic two-build test and input-order shuffle test.
4. Implement sorted batching and canonical metadata.
5. RED corrupt metadata/hash/row count/target-pair tests; implement validators.
6. Verify readers use read-only connections and reject incomplete scratch files.

**Acceptance:**

- [x] Production-owned base and scratch DDL exactly match checked-in contracts/hashes.
- [x] Two from-scratch builds are semantically and byte-order deterministic.
- [x] Invalid, incomplete, escaped, or symlinked files receive typed refusal.
- [x] Every base target pair is proven present in the visible symbol set before completion.

## Task 4: Implement StoreScratchResolutionSession and Streaming Exact Diff

**Files:**

- Modify: `crates/julie-extract-cli/src/resolution_session.rs`
- Create: `crates/julie-extract-cli/src/store/resolution_session.rs`
- Modify: `crates/julie-extract-cli/src/store/mod.rs`
- Modify: `crates/julie-extract-artifact/src/store/resolution_diff.rs`
- Create: `crates/julie-extract-cli/tests/store_resolution_mechanism.rs`
- Extend: `crates/julie-extract-artifact/tests/store_resolution_schema_contract.rs`

**Interfaces:**

- `StoreScratchResolutionSession` receives immutable family/view/manifest identity, bounded reader
  factory, scratch base builder, and `RESOLVER_OUTPUT_EPOCH`.
- Manifest visibility query yields ordered entries with path/language/status/version, rejects any
  visible version missing L2, and exposes failed path existence without extraction rows.
- `stream_resolution_diff(base, exact, scratch_delta)` performs ordered merge for identifier total
  rows and pending partial rows, emitting replacements and pending tombstones plus exact gap facts.
- `apply_base_delta` is a streaming reader used by G2/export; it never materializes the corpus.

**TDD steps:**

1. RED manifest scoping tests: excluded retained versions, failed path existence, language, path
   normalization, and incomplete-L2 refusal with zero scratch output.
2. Implement bounded read windows and semantic key mapping.
3. RED parity test against `LegacyResolutionSession` for the pinned fixture; implement store adapter
   phase methods until semantic dumps match.
4. RED diff matrix: add/replace/delete/multi-delete/path reuse/failed/failed-preserved/collision.
5. Implement ordered merge and persisted scratch delta.
6. RED apply roundtrip and gap enumeration; implement streamed base+delta view.
7. Measure peak RSS on a synthetic large fixture and prove memory stays bounded by configured
   windows rather than row count.

**Acceptance:**

- [x] G1 determinism and G2 exactness pass for both semantic tables.
- [x] Gap enumeration is exact and in-band with the streaming diff.
- [x] Store and legacy sessions produce the same semantic result on the pinned oracle.
- [x] No ATTACH, long store snapshot, or whole-corpus materialization exists.

## Task 5: Freeze and Pass the G1–G5 Measurement Gate

**Files:**

- Create: `crates/julie-extract-cli/tests/store_resolution_performance.rs`
- Create: `xtask/src/resolution_performance.rs`
- Modify: `xtask/src/lib.rs`
- Modify: `xtask/src/commands.rs`
- Modify: `xtask/src/main.rs`
- Create: `xtask/tests/resolution_performance_contract.rs`
- Create: `docs/findings/2026-08-08-index-store-ph2c-mechanism-gate.md`

**Interfaces:**

- `cargo xtask performance store-resolution --runs 3 [--out-dir <path>]` runs the fixed
  Miller-scale pair matrix and writes per-run JSON plus a verdict summary.
- Metrics are exactly `resolution_compute_ms`, `store_fresh_ms`, `diff_ms`, `delta_write_ms`,
  `publish_ms`, `time_to_exact_ms`, row counts, peak RSS, base/delta bytes, and integrity time.
- Timers use injected phase markers in the real session/diff builders; tests reject missing,
  overlapping, or widened intervals.

**TDD steps:**

1. RED parser/summary tests for three-run minimum, per-pair rows, no averaging, and every threshold.
2. Implement the xtask route and deterministic JSON schema.
3. RED metric-boundary unit tests; wire phase markers around actual reads/flush/diff/write/publish.
4. Run one diagnostic pass, fix correctness/performance causes without changing thresholds or
   denominator.
5. Run three full passes and write the exact machine/harness/result ledger.

**Hard-stop acceptance:**

- [x] G1 zero semantic differences for every pair/run.
- [x] G2 persisted base+delta equals fresh exact output for every pair/run.
- [x] G3a is at least 50,000 identifier rows/sec for every pair/run.
- [x] G3b is at most 0.50 for every pair/run using the approved denominator.
- [x] G3c is at most 30 seconds for every pair/run.
- [x] G4 exact gaps pass every run; G5 performs zero foreground identifier work and the
  store-real background pipeline beats the frozen 24,390 ms refuted-bind control on the
  equivalent Miller corpus every run. Foreground milliseconds are recorded, but no new
  post-measurement latency threshold replaces the frozen criterion.
- [x] Finding records all metrics and peak RSS without averages hiding a failure.
- [x] If any checkbox is false, stop. Do not begin Task 6.

## Task 6: Advance Store and Coordinator Catalogs to Schema v2

**Files:**

- Modify: `crates/julie-extract-artifact/src/store/schema.rs`
- Modify: `crates/julie-extract-artifact/src/store/manifest.rs`
- Modify: `crates/julie-extract-artifact/src/store/coordinator.rs`
- Modify: `crates/julie-extract-artifact/src/store/model.rs`
- Modify: `crates/julie-extract-artifact/src/store/mod.rs`
- Modify: `crates/julie-extract-artifact/tests/store_schema_contract.rs`
- Modify: `crates/julie-extract-artifact/tests/store_manifest_contract.rs`
- Modify: `crates/julie-extract-artifact/tests/store_coordinator_contract.rs`
- Modify: `crates/julie-extract-artifact/tests/store_connection_contract.rs`
- Create: `docs/contracts/sqlite-store-schema-v2.md`
- Modify: `docs/contracts/store-v1.md`

**Interfaces:**

- Set `STORE_SQLITE_SCHEMA_VERSION = 2`; create exact tables/checks/indexes from this plan.
- Extend `ManifestEntry` with language and manifest hash v2.
- Extend `RequestKind` with Resolve/Export/FromArtifact; keep stable parse/report strings.
- Add typed store resolution catalog models without exposing raw SQL to CLI code.

**TDD steps:**

1. RED exact schema/catalog-hash tests for store.db and coord.db v2.
2. RED schema-v1 open through reader and writer factories; require typed `OlderSchema` before any
   metadata/table mutation.
3. Implement DDL and connection validation.
4. RED manifest hash language sensitivity and cross-view reuse; update builder/entries.
5. RED view binding coherence and FK tests for every state.
6. RED coordinator request-kind/unique-claimed-resolve tests; implement parse/DDL.
7. Run all Ph2b schema/connection/manifest/coordinator regressions.

**Acceptance:**

- [ ] Exact v2 DDL and catalog hashes are frozen and documented.
- [ ] V1 catalogs refuse cleanly; no migration or partial mutation occurs.
- [ ] Manifest language is required, hashed, and roundtrips through every producer.
- [ ] Invalid view/base/delta/pin/request states cannot commit.
- [ ] Existing import/update/delete behavior remains green on newly created v2 stores.

## Task 7: Implement Immutable Base Publication and Torn-State Recovery

**Files:**

- Modify: `crates/julie-extract-artifact/src/store/resolution.rs`
- Modify: `crates/julie-extract-artifact/src/store/layout.rs`
- Modify: `crates/julie-extract-artifact/src/store/connection.rs`
- Create: `crates/julie-extract-artifact/tests/store_resolution_base_contract.rs`
- Extend: `crates/julie-extract-artifact/tests/store_crash_contract.rs`

**Interfaces:**

- `ResolutionBaseCatalog::{begin_build,recover,mark_ready,find_ready}`.
- Begin transaction inserts building row + `resolution_base_versions`; build occurs after lease
  release; final rename precedes ready CAS.
- Scratch/final names derive from validated request/manifest identities and remain generation-local.
- Recovery classifies row/file combinations exactly as the design specifies.

**TDD steps:**

1. RED state matrix for absent/building/ready rows crossed with missing/valid/invalid scratch/final
   files and live/dead owner.
2. Implement begin-build and rooted path allocation under writer lease.
3. Implement off-lease build, fsync/close, atomic rename, and ready CAS.
4. Implement recovery with no deletion until ownership/pin proof.
5. Add crash subprocesses at row insert, root insert, scratch close, rename, ready CAS, and
   post-ready boundaries; retry must produce one ready base identity.

**Acceptance:**

- [ ] Every torn state converges deterministically without orphaning a live file.
- [ ] Ready bases always pass identity/hash/count/integrity/target checks.
- [ ] Version roots exist before off-lease build and protect every source version.
- [ ] Concurrent identical builders produce one ready identity and safe loser cleanup.

## Task 8: Implement Cumulative Deltas, Exact Bindings, and Pins

**Files:**

- Modify: `crates/julie-extract-artifact/src/store/resolution.rs`
- Modify: `crates/julie-extract-artifact/src/store/resolution_diff.rs`
- Modify: `crates/julie-extract-artifact/src/store/manifest.rs`
- Create: `crates/julie-extract-artifact/tests/store_resolution_binding_contract.rs`
- Extend: `crates/julie-extract-artifact/tests/store_manifest_contract.rs`

**Interfaces:**

- `ResolutionBindingStore::{bind_base,begin_convergence,publish_exact,open_pin,renew_pin,release_pin}`.
- Foreground base selection is O(manifest), writes `converging`, and performs zero identifier
  resolution.
- Exact publish CAS predicates view, manifest generation/hash, previous base/delta head, request
  claim, and writer fencing token. Delta rows, delta catalog, view exactness, gap facts, and one log
  effect share a transaction.
- Pinned reader applies a delta only when its manifest equals the delta exact generation.

**TDD steps:**

1. RED first-view, identical-base reuse, nearest-base selection, and zero-resolution bind tests.
2. RED replacements/tombstones cumulative apply test; implement delta catalog/write/read.
3. RED CAS-loss at each predicate; assert zero partial rows/log/binding mutation.
4. RED content-changing manifest invalidation and identical-manifest retention tests.
5. RED pin expiry/renewal/release and superseded-delta cleanup tests.
6. RED base-only fallback for a pin/delta exactness mismatch.

**Acceptance:**

- [ ] Exact binding is one coherent transaction with one effect record.
- [ ] CAS losers publish nothing and leave scratch cleanup recoverable.
- [ ] Pinned sessions never combine mismatched manifest/base/delta state.
- [ ] Content changes leave exactness behind atomically; identical manifests retain it.
- [ ] Cleanup removes only unpinned superseded deltas; general GC remains absent.

## Task 9: Add Off-Lease Resolve Requests and the Public Resolve Command

**Files:**

- Modify: `crates/julie-extract-artifact/src/store/coordinator.rs`
- Modify: `crates/julie-extract-artifact/tests/store_coordinator_contract.rs`
- Modify: `crates/julie-extract-cli/src/store/args.rs`
- Modify: `crates/julie-extract-cli/src/store/mod.rs`
- Modify: `crates/julie-extract-cli/src/store/report.rs`
- Create: `crates/julie-extract-cli/src/store/resolve.rs`
- Modify: `crates/julie-extract-cli/src/store/executor.rs`
- Create: `crates/julie-extract-cli/tests/store_resolution_contract.rs`

**Interfaces:**

- Add exact CLI syntax from the approved design.
- `StoreCoordinator` gains resolve-specific claim/heartbeat/fail/commit methods that do not acquire
  the writer lease. A dedicated heartbeat connection pumps during compute.
- Resolve executor validates one manifest/L2-complete corpus, reuses/builds a base, computes exact
  scratch output, diffs, reacquires writer lease, and calls Task 8 publish.
- Report state expands to `unbound|converging|exact`, names base/delta/exact generation and gap
  lower bound/exact gap, and adds stable failures
  `resolution_input_incomplete|resolution_failed|resolution_not_exact`. `StoreOperation` adds
  `Resolve|Export|FromArtifact`; `StoreRequestedLevel` adds `NotApplicable` for resolve/export so
  existing import/update/delete JSON fields and spellings remain byte-compatible.

**TDD steps:**

1. RED parser/report snapshots and unknown/future verb rejection.
2. RED incomplete-L2 public request; assert no base/delta/exact terminal success.
3. RED claim heartbeat/loss tests with fake clock; claim loss must stop before writer lease/publish.
4. RED proof that compute proceeds while another short writer-lease transaction completes.
5. Implement thin command, idempotency lookup before preflight, enqueue, claim, heartbeat, compute,
   publish, terminal/reconcile.
6. Add hard-kill tests before/after base rename, delta rows, exact CAS, terminal store effect, and
   coord convergence.

**Acceptance:**

- [ ] Public resolve is real, idempotent, resumable, and never extracts source.
- [ ] At most one resolve claim exists per family; stale/dead claimant takeover is fenced.
- [ ] Long compute holds no writer lease and heartbeat loss prevents publication.
- [ ] Reports truthfully distinguish unbound/converging/exact and request-specific generations.
- [ ] Existing import/update/delete scheduler and takeover contracts remain green.

## Task 10: Add Atomic Pinned Store Export

**Files:**

- Create: `crates/julie-extract-cli/src/store/export.rs`
- Modify: `crates/julie-extract-cli/src/store/args.rs`
- Modify: `crates/julie-extract-cli/src/store/mod.rs`
- Modify: `crates/julie-extract-cli/src/store/report.rs`
- Modify: `crates/julie-extract-cli/src/artifact_access.rs` only for reusable v3 output validation
- Create: `crates/julie-extract-cli/tests/store_resolution_adapters.rs`

**Interfaces:**

- Export requires an exact view or returns `resolution_not_exact` without output.
- It opens a pin, writes `<out>.partial`, materializes the current manifest extraction rows plus
  base/delta semantic overlay into current v3 schema, validates schema/FKs/integrity/metadata, then
  atomically renames.
- Export never writes family-store catalog/log rows except pin lifecycle.
- Export mints its report request identity because the approved CLI has no request controls. If a
  crash leaves the final output present, retry validates its embedded family/view/manifest/base/
  delta identity and returns `reused`; a nonmatching existing output is a typed refusal and is never
  overwritten.

**TDD steps:**

1. RED parser/output collision/symlink/partial cleanup and non-exact refusal tests.
2. RED full natural-key equivalence against a fresh legacy v3 artifact over all extraction and
   resolution tables.
3. Implement streaming version-to-v3 ID mapping and semantic overlay application.
4. RED concurrent manifest flip/pin test; exported artifact must remain one coherent generation.
5. Crash before/after validation/rename; retry leaves one valid output and no live stale pin.

**Acceptance:**

- [ ] Exported artifact is a valid current v3 artifact with exact pinned semantics.
- [ ] No partial output is published and existing output is not overwritten implicitly.
- [ ] Concurrent family changes cannot mix generations.
- [ ] Store mutation is limited to bounded pin lifecycle.

## Task 11: Add Resumable Import From a Legacy Artifact

**Files:**

- Create: `crates/julie-extract-cli/src/store/from_artifact.rs`
- Modify: `crates/julie-extract-cli/src/store/args.rs`
- Modify: `crates/julie-extract-cli/src/store/mod.rs`
- Modify: `crates/julie-extract-cli/src/store/report.rs`
- Modify: `crates/julie-extract-cli/src/store/executor.rs`
- Extend: `crates/julie-extract-cli/tests/store_resolution_adapters.rs`

**Interfaces:**

- `store import --from-artifact` validates current v3 schema, family/root/view identity, extraction
  and resolver epochs, resolution completeness, hashes, and canonical paths before enqueue.
- `StoreImportArgs` adds optional `--from-artifact`; clap/parser validation makes it mutually
  exclusive with `--level` and every scan control. Store/family/root/view/request controls remain
  required exactly as the approved syntax states. Ordinary import parsing is unchanged.
- Durable payload carries source artifact identity and bounded transformation plan, not raw SQL.
- Executor transforms file rows into immutable versions/levels, publishes one manifest, builds one
  ready base from semantic overlay rows, and binds exact. It never copies the v3 file wholesale.
- Request chunks and terminal state follow existing batch resume rules.

**TDD steps:**

1. RED parser/idempotency/schema/root/family/path/epoch/incomplete-resolution refusals.
2. RED natural-key transformation over every extraction family and both resolution tables.
3. Implement chunked version transformation using existing writer transaction seam.
4. Implement manifest/base/exact binding reuse and terminal snapshot.
5. Crash matrix across version stamps, manifest flip, base rename/ready, exact CAS, terminal/reconcile.
6. Retry same idempotency key after source artifact deletion; observe the original terminal request
   without preflight side effects.

**Acceptance:**

- [ ] Transformation is row-equivalent, resumable, and creates no duplicate versions/effects.
- [ ] Imported view is exact at its published generation with a validated ready base.
- [ ] Invalid/incomplete v3 input mutates neither store nor coordinator.
- [ ] Replay reports the original durable result even when the source artifact is gone.

## Task 12: Close Actual-Store G3b, Crash/Concurrency, Dogfood, and Docs

**Files:**

- Modify: `crates/julie-extract-cli/tests/store_resolution_contract.rs`
- Modify: `crates/julie-extract-cli/tests/store_resolution_adapters.rs`
- Modify: `crates/julie-extract-artifact/tests/store_crash_contract.rs`
- Modify: `xtask/src/test_tiers.rs`
- Modify: `xtask/tests/test_tiers.rs`
- Modify: `docs/contracts/store-v1.md`
- Modify: `docs/contracts/sqlite-store-schema-v2.md`
- Create: `docs/release-evidence/2026-08-08-index-store-ph2c/README.md`
- Modify: `docs/architecture/versioned-index-store.md`
- Modify: `docs/README.md` only if the active documentation map requires the new contracts/evidence

**What to prove:**

- Repeat G3b with `delta_write_ms` measured against the real store transaction and the same
  approved `resolution_compute_ms` denominator.
- Concurrent resolve vs import/update/delete, same-view resolve races, stale claim, CAS loss, pin
  lifetime, reader/base/delta coherence, and terminal reconciliation.
- Full public roundtrip: legacy v3 → from-artifact exact store → export v3, plus independently
  extracted store → resolve → export, with normalized natural-key equality.
- Real family dogfood with two views, shared versions/base reuse, changed content, multi-delete,
  failed-preserved, path reuse, crash/takeover, exactness invalidation/reconvergence, WAL/RSS/timing,
  quick/integrity/FK checks, and zero duplicate terminal/effect/chunk rows.

**TDD and closeout:**

1. Add the actual-store G3b assertion before wiring the real publication timer; capture RED.
2. Add public concurrency/crash/adapter roundtrip cases; close each independently.
3. Register exact feature harnesses in xtask contract vectors and convention tests.
4. Run the complete branch gate at one clean HEAD.
5. Run dogfood only on disposable source copies and record exact commands, SHAs, timings, row
   counts, WAL/RSS, integrity, failures, and recovery facts.
6. Audit docs against live schemas/CLI help/reports; do not claim release or Miller adoption.
7. Run impact review on the final diff, self-review every acceptance item, and fix confirmed gaps.

**Acceptance:**

- [ ] Actual-store G3b passes every pair/run with the same denominator and ≤0.50 threshold.
- [ ] All crash/concurrency/claim/pin/CAS/reconciliation tests pass with no duplicate effects.
- [ ] Both adapter roundtrips are natural-key equivalent and non-vacuous.
- [ ] Dogfood reaches exact state after each mutation/recovery and all four DB integrity checks pass.
- [ ] Default/contract/specialist/clippy/fmt/deny/diff gates pass at final clean HEAD.
- [ ] Evidence names what shipped, what remains Ph2d/Ph3, and makes no push/release claim.

## Final Acceptance Checklist

- [ ] Legacy v3 artifacts and reports remain byte/row compatible.
- [ ] Both session adapters satisfy the same resolver contract and semantic oracle.
- [ ] G1/G2/G3a/G3b/G3c/G4/G5 pass three runs with no averaging.
- [ ] Store/coord schema v2 is exact and schema-v1 stores refuse before mutation.
- [ ] Base, delta, pin, claim, manifest, and view states survive every tested crash boundary.
- [ ] Resolve computes off the writer lease and publishes through a fenced short CAS transaction.
- [ ] Export and from-artifact are real atomic/resumable adapters, not file-copy shortcuts.
- [ ] Reports are request-specific, truthful, stable, and preserve existing schema-v1 consumers.
- [ ] Ph2d GC obligations for base roots, pins, and claims are documented and executable in tests.
- [ ] Final worktree, branch, commit, status, and all related worktrees are reconciled before merge.
