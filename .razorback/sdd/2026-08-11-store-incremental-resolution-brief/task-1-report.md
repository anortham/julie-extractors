# Task 1 Report: Scope lifecycle schema and manifest capture

## Status

Complete. The additive schema-v2 resolution-scope journal, typed read/validation API, shared manifest-publication capture seam, legacy writer upgrade, and promotion compatibility are implemented and verified.

## Implementation

- Added `store/scope.rs` with `ResolutionScopeState`, `ResolutionScopeBatch`, `ResolutionScopeChange`, `ResolutionScopeError`, feature-version reads, atomic feature installation, typed batch/state reads, batch validation, and private transition capture.
- Added three strict tables: `resolution_scope_batches`, `resolution_scope_changes`, and `resolution_scope_states`; immutable-update triggers; classified indexes; RFC 3339 timestamp validation; and `store_meta.resolution_scope_journal_version=1`.
- Kept `PRAGMA user_version=2`. Read-only legacy schema-v2 stores tolerate absent feature metadata. Schema creation, writer open, and publication install the feature atomically.
- Routed every non-no-op manifest flip through private `ManifestStore::publish_transaction`; same-generation reuse returns before capture and writes no batch.
- Used `AUTOINCREMENT transition_id` as transition identity. Manifest generations remain reusable payload values and `previous_transition_id` preserves view-local order.
- Captured the first exact manifest/base/delta/resolver-epoch tuple before binding invalidation and preserved it across later unbound transitions.
- Canonically sorted touched paths by UTF-8 bytes, stored nullable old/new version IDs, and persisted exact child count plus `sha256` hash under `julie-resolution-scope-changes-v1`.
- Set the fixed journal bound to 512 changes, matching the existing generation-copy default window. Missing history/state, a journal/head gap, or 513+ changes writes a validated scope-unusable header with no children.
- Kept generation/layout policy unchanged: `GenerationLifecycle::acquire` reaches the upgraded writer-open seam before build, `initialize_store_database` reaches upgraded schema creation, and existing `copy_store_metadata` copies the feature key. Focused promotion tests prove the ordering and catalog compatibility.
- Updated both store contracts and the authoritative store catalog hash to `28654ace146042ef1ea8dcf85703dcc7f87b958eb055eff6633b830e1bded163`.

## TDD red/green evidence

| Behavior | RED evidence | GREEN evidence |
|---|---|---|
| Additive schema-v2 feature | 2026-08-11T16:56:40Z: `resolution_scope_journal_is_an_additive_schema_v2_feature` failed with `QueryReturnedNoRows` for the absent metadata key | 2026-08-11T16:57:40Z: same exact test passed |
| Legacy read/first writer upgrade | 2026-08-11T16:58:02Z: writer-open test failed with `QueryReturnedNoRows` after read-only legacy open | 2026-08-11T16:58:09Z: same exact test passed |
| Shared transition chain and generation reuse | 2026-08-11T16:58:40Z: expected 3 batches, observed 0 | 2026-08-11T17:02:48Z: same exact test passed |
| Immutable batch header | 2026-08-11T17:05:27Z: header update unexpectedly succeeded | 2026-08-11T17:05:38Z: same exact test passed after immutable-update trigger |
| Canonical scope timestamp | 2026-08-11T17:05:53Z: invalid timestamp insert unexpectedly succeeded | 2026-08-11T17:06:07Z: same exact test passed after canonical timestamp check |
| Discontinuous usable chain validation | 2026-08-11T17:09:28Z: validator accepted a usable batch whose predecessor target did not match its source | 2026-08-11T17:09:52Z: same exact test passed after identity/chain validation |
| Unjournaled-writer fallback validation | 2026-08-11T17:10:24Z: validator rejected the intentionally discontinuous scope-unusable fallback header | 2026-08-11T17:10:40Z: same exact test passed while usable discontinuities remain rejected |

Additional focused evidence passed for direct legacy publication fallback, the 512/513 bound, public in-transaction publication, scope-row promotion, metadata copying, and catalog-compatible legacy promotion.

## Verification ledger

| UTC | Scope | Command | Invariant | Result |
|---|---|---|---|---|
| 2026-08-11T17:12:25Z | worker-red-green / schema | `cargo test -p julie-extract-artifact --test store_schema_contract -- --test-threads=1` | Strict schema-v2 catalog, metadata, lifecycle columns, timestamp constraints, and authority hash | PASS, 18/18 |
| 2026-08-11T17:12:26Z | worker-red-green / connection | `cargo test -p julie-extract-artifact --test store_connection_contract -- --test-threads=1` | Legacy v2 remains read-only compatible and first writer atomically installs the feature | PASS, 27/27 |
| 2026-08-11T17:12:27Z | worker-red-green / manifest-scope | `cargo test -p julie-extract-artifact --test store_manifest_contract -- --test-threads=1` | Shared seam, no-op rule, transition chain, exact-state preservation, fallbacks, bound, immutability, and validation | PASS, 24/24 |
| 2026-08-11T17:12:28Z | worker-red-green / generation | `cargo test -p julie-extract-artifact --test store_generation_contract -- --test-threads=1` | Legacy feature upgrade precedes promotion catalog comparison | PASS, 9/9 |
| 2026-08-11T17:12:29Z | worker-red-green / generation equivalence | `cargo test -p julie-extract-artifact --test store_generation_equivalence -- --test-threads=1` | Journal rows, metadata, and AUTOINCREMENT high-water survive logical copy | PASS, 12/12 |
| 2026-08-11T17:12:31Z | worker-ceiling / formatting | `cargo fmt --all -- --check` | Repository Rust formatting is clean | PASS |
| 2026-08-11T17:12:34Z | worker-ceiling / lint | `cargo clippy -p julie-extract-artifact --all-targets --all-features --no-deps -- -D warnings` | Owned production/tests compile across targets/features with no warnings | PASS |

No full/default/contract/Miller replay gates were run, per the worker ceiling.

## Files changed

- `crates/julie-extract-artifact/src/store/scope.rs` (new)
- `crates/julie-extract-artifact/src/store/schema.rs`
- `crates/julie-extract-artifact/src/store/connection.rs`
- `crates/julie-extract-artifact/src/store/manifest.rs`
- `crates/julie-extract-artifact/src/store/mod.rs`
- `crates/julie-extract-artifact/tests/store_schema_contract.rs`
- `crates/julie-extract-artifact/tests/store_connection_contract.rs`
- `crates/julie-extract-artifact/tests/store_manifest_contract.rs`
- `crates/julie-extract-artifact/tests/store_generation_contract.rs`
- `crates/julie-extract-artifact/tests/store_generation_equivalence.rs`
- `docs/contracts/store-v1.md`
- `docs/contracts/sqlite-store-schema-v2.md`

`store/layout.rs` and `store/generation.rs` required no production edit because their existing paths already reach the new schema/writer upgrade and copy every `store_meta` key; the owned promotion tests prove that behavior.

## Miller calls and evidence

- `workspace(onboarding)` and `workspace(status)` both remained in bootstrap.
- Required `context(query="scope lifecycle schema manifest publication capture incremental resolution artifact store")` failed with: `resolution_input_incomplete: reference_resolution_status must be complete, found partial`.
- Required retry used `workspace(refresh)` once; the same context call returned the same exact failure.
- File `inspect` attempts for schema, connection, layout, manifest, generation, and module exports all returned the same failure.
- Pre-change `impact` attempts for schema, connection, manifest, and generation all returned the same failure.
- Full `inspect`/`trace` attempts for `publish_transaction` and post-change full inspections for the public scope functions, `publish_transaction`, and `StoreConnectionFactory::open_writer`, plus post-change working-tree `impact`, all returned the same failure.
- Per the task brief's explicit outage fallback, exploration then used only targeted source regions and exact symbol searches. No API shape was invented from memory.

## API-shape evidence

- `ManifestStore::publish` and `publish_in_transaction` converge on private `publish_transaction` (`store/manifest.rs:462`, `:526`, `:637`).
- Capture is invoked once inside that seam before resolution invalidation and the head CAS (`store/manifest.rs:732`).
- CLI import/update and delete call `publish_in_transaction` at `store/executor.rs:838` and `:871`; `from_artifact` calls it at `:1028`.
- `StoreConnectionFactory::open_reader` stays validation-only while `open_writer` installs the feature after lease/fence/pragmas (`store/connection.rs:172`, `:186`, `:253`).
- `initialize_store_database` calls `create_store_schema` (`store/layout.rs:618`), so new generations receive the feature.
- Promotion uses `GenerationLifecycle::build_and_publish` and `copy_store_metadata` (`store/generation.rs:177`, `:727`); the latter selects every durable `store_meta` key except documented generation/temporary exceptions.

## Architecture quality self-review

- **Affected modules:** artifact schema/connection/manifest plus the new private-policy scope module.
- **Caller-facing interface:** additive typed state/batch/change records and read/validate/feature functions; existing manifest and CLI signatures are unchanged.
- **Depth/locality:** schema upgrade, lifecycle state, canonical hashing, transition capture, and validation are local to `store::scope`; callers do not learn journal SQL or ordering rules.
- **Test surface:** behavior is exercised through public manifest publication, public writer open, typed validation, and public generation promotion.
- **Seam:** the existing private `ManifestStore::publish_transaction` remains the only content-publication writer seam.
- **Rejected shortcuts:** generation-as-transition identity, executor-specific journal writes, partial children beyond the bound, eager read-only migration, and compatibility-mode schema version changes.
- **Architecture risk:** medium because this adds durable schema and lifecycle state, contained by strict constraints, immutable headers, typed validation, authority hashing, and focused promotion tests.

Canonical checklist:

- Complexity stays local in `store::scope`.
- The caller-facing API is smaller than the lifecycle behavior it unlocks.
- Tests use caller-facing manifest/writer/promotion interfaces.
- The new module earns its seam by owning DDL, capture, hashing, reads, and validation together.
- No public trait or speculative adapter was added.
- The structural cause is fixed at the shared publication seam, not repeated in CLI executors.

## Judgment calls

- `store/scope.rs:11`: chose 512 as the conservative fixed bound because it matches the existing `DEFAULT_COPY_WINDOW=512`; 513+ writes header-only fallback.
- `store/scope.rs:104`: feature installation is atomic and transaction-aware; it starts `BEGIN IMMEDIATE` only when no caller transaction exists.
- `store/scope.rs:214`: validation recomputes child count/hash, manifest identities, usable-chain continuity, exact touched rows, and the immutable predecessor delta tuple.
- `store/scope.rs:378`: capture is `pub(crate)`, not public, to preserve the approved rule that private manifest publication is the only writer seam; public callers receive typed read/validate operations.
- `store/scope.rs:406`: an existing manifest with no prior transition is treated as legacy/untrusted even if the view currently appears exact; this implements the missing-metadata full-fallback rule.
- `store/scope.rs:634`: used three tables so immutable history/children are separate from the mutable per-view lifecycle head.
- `store/connection.rs:253`: writer-open upgrade is after lease/fence/pragmas, so schema mutation never occurs through reader open or before write authorization.
- `store/manifest.rs:732`: capture occurs after target manifest materialization but before binding invalidation/head advance, all in the same transaction.
- `store/generation.rs`: left production unchanged because adding a second promotion-specific upgrader would duplicate policy; acquisition already opens the writer and the focused legacy-promotion test proves catalog-compatible ordering.

## Worktree state before checkpoint/commit

- Path: `/home/murphy/source/julie-extractors`
- Branch: `feature/store-incremental-resolution`
- Base/current commit before worker commit: `692ef4eb08dafcf55dbc08e7d1e17cc8d3f11c29`
- Related task files: modified/untracked as listed above; report added.
- Concurrent unrelated state left untouched and unstaged: `TODO.md`, `.memories/briefs/store-resolution-performance-repair.md`, three older untracked memories, plan Markdown/HTML.
- Required parent-created memory to include: `.memories/2026-08-11/171012_8e12.md`.
- Other worktree: `/home/murphy/source/julie-extractors/.worktrees/user-relief-2026-08-11`, branch `bugfix/user-relief-2026-08-11`, clean at `692ef4e`.

## Concerns

- Miller code-intelligence evidence is unavailable because its bootstrap repeatedly failed with the exact `resolution_input_incomplete` error above; targeted source reads, focused executable tests, catalog authority, formatting, and clippy provide the replacement evidence.
- Task 2 still owns lifecycle GC/rooting, rollback/promotion bypass semantics, and exact-publication clearing; Task 1 intentionally does not implement those policies.
