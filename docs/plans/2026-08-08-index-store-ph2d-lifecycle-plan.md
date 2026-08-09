# Index Store Ph2d Lifecycle Completion Implementation Plan

> Execute this plan in `/Users/murphy/source/julie-extractors/.claude/worktrees/index-store-ph2d`
> on branch `codex/index-store-ph2d`. Keep every task strict RED → GREEN, preserve unrelated work,
> and do not push, tag, publish, or change Miller's live pin without explicit approval.

**Goal:** Complete Julie's unreleased family-store lifecycle with safe inspection, retention, garbage
collection, repair, forward generation promotion/rollback, capacity control, release evidence, and a
locally verified Miller adoption path.

**Architecture:** A pure maintenance planner computes level-qualified reachability and physical
capacity from bounded database readers. A fenced executor applies restartable cohorts under the
root-owned maintenance intent and writer lease. Large repair, compaction, schema, and rollback work
builds a validated new generation and publishes it through the atomic `CURRENT` protocol. Public CLI
work stays under `julie-extract store maintain` and uses a separate report schema.

**Technology:** Rust 2024, rusqlite/SQLite WAL, clap, serde, filesystem fsync/atomic rename, existing
test subprocess crash hooks, Cargo/xtask, and Miller impact analysis.

**Approved design:**
[2026-08-08-index-store-ph2d-lifecycle-design.md](2026-08-08-index-store-ph2d-lifecycle-design.md)

## Global execution rules

- Use `RUSTUP_TOOLCHAIN=1.97.1` unless the workspace toolchain file selects a newer compatible
  installed toolchain.
- Run Miller search/inspect before source reads and impact before and after each production slice.
- Write the smallest failing contract first; record the exact RED and first GREEN in the task report.
- Keep store, coordinator, resolution, and request-report compatibility tests green after every task.
- No whole-corpus `Vec` materialization, cross-database transaction claim, `ATTACH`, long read
  snapshot, in-place full `VACUUM`, or direct write to a retired generation.
- A task is complete only after focused tests, relevant regressions, strict Clippy, format, diff, and
  clean-state evidence. Commit each task locally; never push during implementation.

## Progress

- [x] Task 1 — final unreleased catalogs
- [x] Task 2 — universal generation and maintenance fencing
- [x] Task 3 — deterministic inspection and planning
- [x] Task 4 — receipts, cursors, and bounded GC
- [x] Task 5 — promotion, repair, and forward rollback
- [x] Task 6 — maintenance CLI and reports
- [x] Task 7 — crash, mixed-version, scale, and language gates
- [ ] Task 8 — dogfood, docs, and release preparation
- [ ] Task 9 — pre-merge review and integration boundary

## Task 1: Freeze the final unreleased catalogs

**Files**

- Modify: `crates/julie-extract-artifact/src/store/schema.rs`
- Modify: `crates/julie-extract-artifact/src/store/model.rs`
- Modify: `crates/julie-extract-artifact/src/store/mod.rs`
- Modify: `crates/julie-extract-artifact/tests/store_schema_contract.rs`
- Modify: `docs/contracts/sqlite-store-schema-v2.md`
- Modify: `docs/contracts/store-v1.md`
- Create: `crates/julie-extract-artifact/tests/store_maintenance_schema_contract.rs`

### RED

Add schema contracts proving:

- `resolution_identifier_deltas` and `resolution_pending_deltas` have `version_id`-leading indexes;
- root-owned coordinator tables exist for immutable request receipts, consumer cursors, maintenance
  intent, and typed family allocator marks;
- allocator keys cover file versions, store log, per-view manifest generation, and per-view
  resolution-delta generation;
- receipt request IDs and idempotency keys are independently unique;
- cursor sequence/time/generation fields and maintenance owner/heartbeat fields are checked;
- store metadata accepts only `generation_state=serving|retired` where that key is present;
- existing schema creation remains strict and idempotent.

Run:

```bash
RUSTUP_TOOLCHAIN=1.97.1 cargo test -p julie-extract-artifact \
  --test store_maintenance_schema_contract -- --test-threads=1
```

Expected RED: missing tables/indexes/types or catalog mismatch.

### GREEN

Add the DDL and typed row/state models in one catalog amendment. Keep foreign-key direction explicit;
use coordinator receipts as the durable idempotency authority after live request deletion. Regenerate
the checked-in catalog text/hash evidence once.

Verify:

```bash
RUSTUP_TOOLCHAIN=1.97.1 cargo test -p julie-extract-artifact \
  --test store_maintenance_schema_contract -- --test-threads=1
RUSTUP_TOOLCHAIN=1.97.1 cargo test -p julie-extract-artifact \
  --test store_schema_contract --test store_resolution_schema_contract -- --test-threads=1
```

Commit: `feat(store): freeze lifecycle catalogs`

## Task 2: Make generation and maintenance fencing universal

**Files**

- Modify: `crates/julie-extract-artifact/src/store/layout.rs`
- Modify: `crates/julie-extract-artifact/src/store/connection.rs`
- Modify: `crates/julie-extract-artifact/src/store/coordinator.rs`
- Modify: `crates/julie-extract-artifact/src/store/pragmas.rs`
- Modify: `crates/julie-extract-artifact/tests/store_connection_contract.rs`
- Modify: `crates/julie-extract-artifact/tests/store_coordinator_contract.rs`
- Create: `crates/julie-extract-artifact/tests/store_generation_contract.rs`

### RED

Add contracts for:

- opening an existing layout performs no write and never calls schema initialization;
- missing `CURRENT` beside any named generation refuses instead of creating `gen-001`;
- partial generation cleanup requires matching dead/expired ownership;
- `open_writer`, coordinator raw store transactions, manifest, resolution, pin, and cleanup paths all
  verify `CURRENT`, `generation_state`, maintenance intent, and fencing token;
- an old open handle can read a retired generation but cannot start another write;
- the source maintenance floor blocks old binaries while the destination preserves the original
  compatibility floor;
- `binary_version` advancement happens only in an explicit lease-held write.

Run:

```bash
RUSTUP_TOOLCHAIN=1.97.1 cargo test -p julie-extract-artifact \
  --test store_generation_contract --test store_connection_contract \
  --test store_coordinator_contract -- --test-threads=1
```

### GREEN

Split layout initialization from query-only validation. Add a generation fence object bound to the
family root, generation name, intent run ID, owner, and coordinator token. Route coordinator store
transactions through `StoreConnectionFactory`; remove raw bypasses. Set and read back the 256 MiB
`journal_size_limit` writer pragma.

Commit: `feat(store): fence generation lifecycle writes`

## Task 3: Add deterministic maintenance inspection and planning

**Files**

- Create: `crates/julie-extract-artifact/src/store/maintenance.rs`
- Modify: `crates/julie-extract-artifact/src/store/resolution.rs`
- Modify: `crates/julie-extract-artifact/src/store/mod.rs`
- Create: `crates/julie-extract-artifact/tests/store_maintenance_contract.rs`
- Create: `crates/julie-extract-artifact/tests/store_maintenance_property.rs`

### RED

Build a pure reference model and SQLite fixture matrix covering current/historical manifests,
`failed_preserved`, bases, both delta tables, bindings, pins, requests/claims, scratch, cursors, and
retained generations. Assert exact level-qualified reasons, seven-day-over-24-path precedence,
1.20/1.25 ratios, unknown-root refusal, and deterministic plan fingerprints under shuffled inserts.

Add capacity fixtures for filesystem free bytes, SQLite pages/freelist/WAL, base/scratch sizes, staged
generation headroom, and one bounded demotion cohort.

### GREEN

Implement bounded keyset readers and a pure `MaintenancePlan`. The planner reports protected,
eligible, and pressure-only objects without mutating. Use injected clock/capacity providers. Bind the
plan to store/coord root fingerprints, current generation, store-log maximum, request watermark, and
allocator marks.

Verify:

```bash
RUSTUP_TOOLCHAIN=1.97.1 cargo test -p julie-extract-artifact \
  --test store_maintenance_contract --test store_maintenance_property -- --test-threads=1
```

Commit: `feat(store): plan lifecycle maintenance`

## Task 4: Implement receipts, cursors, and ordered bounded GC

**Files**

- Modify: `crates/julie-extract-artifact/src/store/maintenance.rs`
- Modify: `crates/julie-extract-artifact/src/store/coordinator.rs`
- Modify: `crates/julie-extract-artifact/src/store/log.rs`
- Modify: `crates/julie-extract-artifact/src/store/resolution.rs`
- Modify: `crates/julie-extract-artifact/tests/store_maintenance_contract.rs`
- Modify: `crates/julie-extract-artifact/tests/store_coordinator_contract.rs`
- Create: `crates/julie-extract-artifact/tests/store_maintenance_crash_contract.rs`

### RED

Add exact tests for:

- request receipt creation and live-request deletion in one coordinator transaction;
- replay and conflicts after pruning, including request-ID reuse refusal;
- monotonic cursor advance/release and malformed/ahead cursor blocking;
- coordinator-first then store-log pruning with crash between databases;
- L3-before-L2 demotion, completion-stamp clearing, and whole-version-only purge;
- both delta source/target roots preventing L2 demotion and purge;
- 100-version or 64-MiB cohort cutoff, durable cursor resume, and no duplicate/skipped version;
- checkpoint → incremental vacuum → truncate-checkpoint order and no full `VACUUM`;
- kill before/after every catalog/filesystem boundary.

### GREEN

Implement restartable GC steps in the approved order. Revalidate the plan under maintenance
ownership immediately before each commit. Keep filesystem and catalog asymmetries recoverable and
surface typed pressure/capacity outcomes instead of deleting protected data.

Verify:

```bash
RUSTUP_TOOLCHAIN=1.97.1 cargo test -p julie-extract-artifact \
  --test store_maintenance_contract --test store_coordinator_contract -- --test-threads=1
RUSTUP_TOOLCHAIN=1.97.1 cargo test -p julie-extract-artifact --features test-store-crash \
  --test store_maintenance_crash_contract -- --test-threads=1
```

Commit: `feat(store): apply bounded lifecycle gc`

## Task 5: Build generation promotion, repair, and forward rollback

**Files**

- Create: `crates/julie-extract-artifact/src/store/generation.rs`
- Modify: `crates/julie-extract-artifact/src/store/maintenance.rs`
- Modify: `crates/julie-extract-artifact/src/store/layout.rs`
- Modify: `crates/julie-extract-artifact/src/store/manifest.rs`
- Modify: `crates/julie-extract-artifact/src/store/resolution.rs`
- Modify: `crates/julie-extract-artifact/src/store/test_hooks.rs`
- Create: `crates/julie-extract-artifact/tests/store_generation_crash_contract.rs`
- Create: `crates/julie-extract-artifact/tests/store_generation_equivalence.rs`

### RED

Add contracts for deterministic primary-key streaming copy, base-file identity, validation refusal,
fsync/rename/`CURRENT` boundaries, serving/retired state, generation-local pin retention, and cleanup.
Assert all four family allocator marks advance across every named generation and receipt.

For forward rollback, create requests and cursors in the latest generation, select an older visible
state, and prove the new generation preserves terminal logs/receipts/cursor meaning while exposing the
selected manifests/bases/bindings. Assert no old manifest/delta generation identity is reused.

### GREEN

Implement `.gen-NNN.partial` ownership, bounded logical copy, validation, atomic publication, retained
generation cleanup, and recovery. Repair escalates from checkpoint/torn-state recovery to bounded GC,
then new-generation rebuild. Never rewrite the only serving generation or republish an old database
directly.

Verify:

```bash
RUSTUP_TOOLCHAIN=1.97.1 cargo test -p julie-extract-artifact \
  --test store_generation_equivalence -- --test-threads=1
RUSTUP_TOOLCHAIN=1.97.1 cargo test -p julie-extract-artifact --features test-store-crash \
  --test store_generation_crash_contract -- --test-threads=1
```

Commit: `feat(store): promote and repair generations`

## Task 6: Expose the maintenance CLI and report contract

**Files**

- Modify: `crates/julie-extract-cli/src/store/args.rs`
- Modify: `crates/julie-extract-cli/src/store/mod.rs`
- Create: `crates/julie-extract-cli/src/store/maintenance.rs`
- Create: `crates/julie-extract-cli/src/store/maintenance_report.rs`
- Modify: `crates/julie-extract-cli/src/lib.rs`
- Create: `crates/julie-extract-cli/tests/store_maintenance_cli_contract.rs`
- Modify: `crates/julie-extract-cli/tests/store_cli_contract.rs`
- Modify: `docs/contracts/cli.md`
- Modify: `docs/contracts/store-v1.md`

### RED

Add public subprocess tests for:

- `store maintain inspect|gc|repair|promote` nested grammar;
- mutation requiring `--apply` and inspect remaining read-only;
- separate maintenance report v1 exact JSON/human snapshots and exit codes;
- stable failure classes for busy, stale plan, capacity, incompatible, recovery-required, integrity,
  and unavailable repair;
- cursor advance/release without exposing filesystem names;
- legacy command help and request-oriented `StoreReport` byte compatibility.

### GREEN

Wire thin CLI modules into the artifact facade. Do not put planner/executor policy in clap/dispatch.
Keep JSON one-line stdout purity and human failure stderr behavior.

Commit: `feat(cli): expose store lifecycle maintenance`

## Task 7: Close mixed-version, crash, scale, and language gates

**Files**

- Modify: `crates/julie-extract-artifact/Cargo.toml`
- Modify: `crates/julie-extract-cli/Cargo.toml`
- Modify: `crates/julie-extract-artifact/tests/test_tiers.rs`
- Modify: `crates/julie-extract-cli/tests/test_tiers.rs`
- Modify: `xtask/src/test_tiers.rs`
- Modify: `xtask/tests/test_tiers.rs`
- Create: `crates/julie-extract-cli/tests/store_maintenance_equivalence.rs`
- Create: `crates/julie-extract-cli/tests/store_maintenance_mixed_version.rs`
- Create: `crates/julie-extract-cli/tests/store_maintenance_performance.rs`

### RED/GREEN matrix

- Old/current real binaries: source fence, destination compatibility, downgrade escape limits,
  retained-reader behavior, and allocator monotonicity.
- Crash every coordinator, transaction, file, rename, CURRENT, state, intent, receipt, cursor, vacuum,
  and rollback boundary; reopen both databases and every named generation.
- Randomized reachability against the pure model.
- Miller-scale bounded RSS, SQLite page windows, WAL per cohort, and physical capacity refusal.
- Full natural-row equivalence across manifests, 14 extraction child tables, global fingerprint tables,
  resolution bases/deltas, logs, receipts, and coordinator state.
- Real fixtures for every supported language; group by language/kind and fail on silent absence.

Register exact commands in the contract tier without moving subprocess/scale cases into the default
fast tier.

Commit: `test(store): prove lifecycle recovery and scale`

## Task 8: Dogfood, documentation, and release preparation

**Files**

- Modify: `README.md`
- Modify: `docs/README.md`
- Modify: `docs/architecture/versioned-index-store.md`
- Modify: `docs/plans/2026-08-07-index-store-ph2b-store-kernel-plan.md`
- Modify: `docs/plans/2026-08-08-index-store-ph2c-resolution-design.md`
- Create: `docs/findings/2026-08-08-index-store-ph2d-dogfood.md`
- Create or modify: release-note/version/package files selected from live release metadata

Run disposable-repository dogfood with two views, shared versions, churn, failed/failed-preserved
entries, bases, deltas, pins, receipts, cursors, GC, repair, promotion, forward rollback, kill/retry,
and fresh-store equivalence. Record timings, peak RSS/WAL, retained/current bytes, integrity/FK facts,
terminal uniqueness, allocator marks, and zero mismatches.

Run the final branch gate:

```bash
cargo fmt --all -- --check
cargo test -p xtask
cargo xtask test default
cargo xtask test contract
cargo test -p julie-extract-artifact --features test-store-crash --test store_crash_contract
cargo test -p julie-extract-cli --features test-store-contract --test store_equivalence
cargo clippy -p julie-extract-artifact -p julie-extract-cli --all-targets --all-features -- -D warnings
git diff --check
```

Use live GitHub release metadata to choose the next version. Prepare version files, release notes,
package contracts, and checksums locally. Build the candidate archives and verify downloaded/local
assets. Do not push, tag, publish, or update Miller's live pin.

Commit: `docs: complete index store ph2d lifecycle`

## Task 9: Pre-merge review and integration boundary

Run a complete exact-range review from `a0f10b54b986249da5e35c4e7229706e0dc60c0f` to branch HEAD.
Fix confirmed findings with strict RED/GREEN cycles, rerun affected gates, refresh Miller, inspect
post-edit impact, and verify every Julie worktree is clean and intentional.

Then report:

- base, HEAD, worktree, branch, and clean status;
- exact gate results and dogfood evidence;
- prepared release version and assets;
- whether any work remains outside the branch;
- the separate approval actions: merge, push main, tag/publish Julie, then update/validate Miller pin.

No integration or release action occurs until the user approves that exact verified state.
