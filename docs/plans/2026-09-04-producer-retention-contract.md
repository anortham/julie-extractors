# Producer Retention Contract Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use razorback:subagent-driven-development when subagent delegation is available. Fall back to razorback:executing-plans for single-task, tightly-sequential, or no-delegation runs.

**Goal:** Add a producer-owned durable reader-registration contract so Miller can hold an exact family-store generation safely while Julie maintenance performs GC, promotion, rollback, or view retirement.

**Architecture:** Add manifest-root reader registrations to `coord.db`, separate from the existing `consumer_cursors` watermark and from retired `resolution_pins`. A registration records one immutable view/manifest/generation snapshot, the required log retention point, and an opaque owner nonce; it references the existing manifest root rather than copying every file version into coordinator rows. Julie owns registration writes and maintenance qualification; Miller invokes the public CLI and opens the generation only after atomic admission succeeds.

**Tech Stack:** Rust 2024, rusqlite, SQLite WAL, clap, serde, filesystem generation layout, platform process identity APIs.

**Architecture Quality:** This is a new producer/consumer safety contract. The producer must make admission and maintenance-root capture atomic under the existing coordinator maintenance-intent fence. Heartbeat timeout is diagnostic only: GC may release a reader registration automatically only after definitive same-process-instance death (PID plus captured platform birth identity), or after explicit release. If process identity cannot be proved, the registration remains protected and the report names the retained registration. Risk is high because an incorrect race decision can delete data still read by Miller.

## Global Constraints

- Do not reuse `consumer_cursors`; cursors are monotonic log watermarks and do not represent a live reader lease.
- Do not restore or extend retired `resolution_pins`; `store.db` does not catalog resolution pins and writer open removes those objects (`docs/contracts/store-v1.md:145-148`).
- Acquire captures one exact generation/view/manifest snapshot and its manifest root before Miller opens the generation. The registration cannot retarget to another generation or view.
- `coord.db` owns reader registrations. `store.db` remains generation-local; existing `views` and `manifest_entries` are the producer-owned roots and no reader registration table is added there.
- Registration identity is immutable after acquire: `(pin_id, owner_nonce, owner_pid, owner_birth_identity, view_id, manifest_generation, generation_name)` cannot be updated.
- Renew and release authenticate with the opaque owner nonce; release is idempotent for an already released registration and rejects an incorrect nonce.
- A heartbeat timeout never authorizes GC by itself. Automatic cleanup requires a definitive dead process instance; unknown birth identity fails closed and retains the registration with a reportable warning.
- Existing maintenance intent and fencing rules remain authoritative. Reader acquire, renew, and release must not bypass a live foreign maintenance intent.
- Existing cursor semantics remain available and are verified separately; the new reader contract does not replace cursors.
- Existing Julie `store maintain` commands remain compatible. The new reader CLI uses report schema version 1 and must be additive to the existing maintenance report.
- The old v4 plan's time-only expiry behavior is deliberately amended: expiry is a liveness observation and warning, never a deletion authorization. The cleanup cost is an additional coordinator probe and a retained-registration report when process identity is unavailable.
- Legacy or mixed-version writers must prove they cannot ignore active reader registrations before the feature is enabled. If the existing writer-version floor cannot guarantee that behavior, coordinator schema/floor migration is mandatory before publishing reader commands.
- No Miller source is modified by this producer plan. Consumer integration is cross-linked at `../../../miller/docs/plans/2026-09-04-reader-retention-integration.md`.
- The architecture program is cross-linked at `../../../miller/docs/plans/2026-09-04-architecture-review-program.md`.

## Existing Contract Evidence

Julie already provides the primitives that this plan must extend without pretending they are reader leases:

- `coord.db.consumer_cursors` has `consumer_id`, `generation_name`, `store_log_sequence`, and `updated_at` only (`crates/julie-extract-artifact/src/store/schema.rs:1338-1346`).
- `StoreCoordinator::advance_consumer_cursor` validates the generation, rejects a sequence above the global `store_log` allocator high-water mark, rejects generation changes and regressions, and rechecks foreign maintenance intent inside its write transaction (`crates/julie-extract-artifact/src/store/coordinator.rs:2089`).
- `StoreCoordinator::release_consumer_cursor` deletes one named cursor after the same foreign-intent check (`crates/julie-extract-artifact/src/store/coordinator.rs:2172`).
- `MaintenancePlan` already reports protected and eligible bases, deltas, pins, requests, scratch, cursors, and generations (`crates/julie-extract-artifact/src/store/maintenance.rs:309-330`).
- `MaintenanceRootKind` already has `ConsumerCursor`, `CurrentGeneration`, `RollbackGeneration`, and historical `Pin` variants (`crates/julie-extract-artifact/src/store/maintenance.rs:70-88`).
- The store contract requires retention to respect manifests, active requests, scratch owners, cursor windows, and retained-generation safety windows (`docs/contracts/store-v1.md:157-165`).
- `store maintain gc|repair|promote`, `cursor advance`, and `cursor release` are public commands; mutation requires `--apply`, and apply revalidates the plan fingerprint (`docs/contracts/cli.md:56-61,501-532`).
- Current source references to `resolution_pins` are migration cleanup only (`crates/julie-extract-artifact/src/store/schema.rs:472`, `crates/julie-extract-artifact/src/store/generation.rs:1610-1613`). They are not a reader API.
- Julie's lifecycle architecture says Ph2d is complete and Miller owns production reader integration, sidecars, cursors/pins, and the pin bump (`docs/plans/2026-08-07-index-store-ph2b-store-kernel-plan.md:61,443`, `docs/plans/2026-08-08-index-store-ph2d-lifecycle-design.md:347,560`).

## Proposed Wire Contract

The following names and fields are the shared proposal for Julie and Miller. They are new typed APIs, not claims about existing symbols.

### Reader acquire

```text
julie-extract store reader acquire \
  --store <family-dir> \
  --family <uuid> \
  --view <view-id> \
  --generation <generation-name> \
  --owner <opaque-owner-label> \
  --owner-pid <positive-pid> \
  --nonce <opaque-owner-nonce> \
  --lease-ms <positive-duration> \
  [--json]
```

The command refuses a missing family, view, generation, manifest, or allocator state. It validates the requested manifest metadata and captures one manifest-root registration, then inserts one registration row in one `BEGIN IMMEDIATE` coordinator transaction after checking the current maintenance intent. The response names the exact snapshot Miller must open:

```json
{
  "report_schema_version": 1,
  "operation": "reader_acquire",
  "state": "acquired",
  "family_id": "family-uuid",
  "view_id": "view-id",
  "manifest_generation": 42,
  "generation_name": "gen-000042",
  "pin_id": "reader-00000000000000000000000000000001",
  "owner_nonce": "0123456789abcdef0123456789abcdef",
  "owner_pid": 1234,
  "store_instance_id": "family-uuid:gen-000042",
  "manifest_hash": "<producer-manifest-hash>",
  "extraction_identity_epoch": 9,
  "served_store_log_sequence": 800,
  "min_retained_store_log_sequence": 700,
  "snapshot_fingerprint": "<snapshot-fingerprint>",
  "protected_manifest_count": 1,
  "expires_at": 1788550000000,
  "warning": null
}
```

The JSON above is a test-only shape example, not runtime evidence; angle-bracket values are placeholders and must be replaced by verified values in an actual report. `owner_birth_identity` is producer-internal registration state and is not returned in the consumer wire response; Julie captures and compares it locally for renew/death qualification.

The response is bounded: the manifest root and its entries remain producer-only and are represented by `protected_manifest_count: 1` plus the `snapshot_fingerprint`; file versions are enumerated only by maintenance during root marking/apply. `served_store_log_sequence` is the sequence at which the immutable read snapshot was captured; `min_retained_store_log_sequence` is the lowest log sequence Julie must retain for the registration. They are distinct and both are required. Miller computes its local `index_level` (`symbols` or `full`) and level stamps after registration under its read snapshot; those derived values are not producer fields and are not compared during acquire. `store_instance_id` is derived exactly as Miller's existing `StoreInstanceId(FamilyId, generationName)`, namely `<family_id>:<generation_name>`; it is not a random UUID. `manifest_hash` is copied verbatim from the producer manifest, with no added prefix.

The requested `--nonce` is the client idempotency nonce and the immutable owner nonce. Acquire with the same nonce and identical family/view/generation/owner request returns the existing registration and `state: acquired` without inserting a duplicate. The same nonce with any differing requested owner, family, view, or generation returns `reader_owner_mismatch` or `stale_snapshot`. Julie captures the owner birth identity from the live PID while holding the admission transaction; it never trusts a caller-supplied birth-identity string.

### Reader renew

```text
julie-extract store reader renew \
  --store <family-dir> \
  --family <uuid> \
  --pin <pin-id> \
  --nonce <opaque-owner-nonce> \
  --owner-pid <positive-pid> \
  --lease-ms <positive-duration> \
  [--json]
```

Renew authenticates the immutable identity tuple and updates only liveness fields (`heartbeat_at`, `expires_at`). Julie re-probes the supplied PID and compares the captured birth identity stored at acquire; it refuses a changed process instance, a changed owner nonce, an expired registration whose owner cannot be proven alive, or a live foreign maintenance intent. It returns `state: renewed` with the same snapshot fields and `pin_id`.

### Reader release

```text
julie-extract store reader release \
  --store <family-dir> \
  --family <uuid> \
  --pin <pin-id> \
  --nonce <opaque-owner-nonce> \
  [--json]
```

Release authenticates the nonce and deletes the registration/root rows in one transaction after checking maintenance intent. An absent row returns `state: released`, `released: false`, and no error regardless of a well-formed nonce; no tombstone is required. An existing row with a wrong nonce returns `reader_owner_mismatch` without revealing registration details.

### Report and failure contract

Every command emits exactly one JSON line with `report_schema_version: 1` when `--json` is selected. Stable fields are `operation`, `state`, `family_id`, `view_id`, `pin_id` when safe to expose, `generation_name`, `manifest_generation`, `owner_pid`, `expires_at`, `warning`, `failure_class`, and nullable `error`. Failure classes are `busy`, `stale_snapshot`, `invalid_arguments`, `incompatible_store`, `reader_not_found`, `reader_owner_mismatch`, `reader_identity_unknown`, `capacity_insufficient`, and `operational`. Human output follows existing stdout success/stderr failure rules. Exit codes follow the existing maintenance contract: 0 success/no-op, 1 operational refusal, 2 CLI usage, and 3 incompatible store.

## Proposed Coordinator Schema

Add these tables to `coord.db` in the coordinator schema. The names are intentionally distinct from `consumer_cursors` and retired `resolution_pins`.

```sql
CREATE TABLE IF NOT EXISTS reader_registrations (
  pin_id TEXT PRIMARY KEY CHECK (length(pin_id) BETWEEN 1 AND 128),
  owner_nonce TEXT NOT NULL UNIQUE CHECK (length(owner_nonce) BETWEEN 32 AND 512),
  owner_label TEXT NOT NULL CHECK (length(owner_label) BETWEEN 1 AND 128),
  family_id TEXT NOT NULL CHECK (length(family_id) BETWEEN 1 AND 128),
  view_id TEXT NOT NULL CHECK (length(view_id) BETWEEN 1 AND 128),
  manifest_generation INTEGER NOT NULL CHECK (manifest_generation > 0),
  generation_name TEXT NOT NULL CHECK (length(generation_name) BETWEEN 1 AND 128),
  owner_pid INTEGER NOT NULL CHECK (owner_pid > 0),
  owner_birth_identity TEXT NOT NULL CHECK (length(owner_birth_identity) BETWEEN 1 AND 512),
  store_instance_id TEXT NOT NULL CHECK (length(store_instance_id) BETWEEN 1 AND 512),
  manifest_hash TEXT NOT NULL CHECK (length(manifest_hash) BETWEEN 1 AND 512),
  extraction_identity_epoch INTEGER NOT NULL CHECK (extraction_identity_epoch > 0),
  served_store_log_sequence INTEGER NOT NULL CHECK (served_store_log_sequence >= 0),
  acquired_at INTEGER NOT NULL CHECK (acquired_at >= 0),
  heartbeat_at INTEGER NOT NULL CHECK (heartbeat_at >= acquired_at),
  expires_at INTEGER NOT NULL CHECK (expires_at > heartbeat_at),
  min_retained_store_log_sequence INTEGER NOT NULL CHECK (min_retained_store_log_sequence >= 0 AND min_retained_store_log_sequence <= served_store_log_sequence),
  snapshot_fingerprint TEXT NOT NULL CHECK (length(snapshot_fingerprint) > 0),
  UNIQUE (family_id, pin_id)
) STRICT;

```

The registration row is the immutable identity, snapshot, and time state. The existing `manifests(view_id,generation,manifest_hash)` and `manifest_entries(view_id,generation,path,version_id)` rows remain the authoritative manifest root; GC enumerates their reachable versions during mark/apply rather than copying them at acquire. Add indexes supporting `(family_id, generation_name)` and `(family_id, expires_at)`. The schema version/floor change must be recorded in the producer compatibility ledger and generated schema catalog.

## Cross-Database Admission Fence

The registration lives in `coord.db`, while the manifest, generation metadata, level stamps, and file-version rows are read from the selected generation's `store.db`. Acquire must not read one database, release the coordinator transaction, and then open an arbitrary generation. The producer sequence is:

1. Open the current coordinator and store connections using the existing `StoreConnectionFactory`/`StoreCoordinator` ordering.
2. Begin the coordinator `BEGIN IMMEDIATE` admission transaction and reject a foreign live `maintenance_intent`.
3. Open the selected generation's `store.db` query-only with `busy_timeout=0`; read only constant-size manifest metadata needed to validate the requested view/generation/manifest hash, store instance, extraction epoch, and served log sequence under the existing writer/maintenance serialization. Do not scan manifest entries, derive Miller index levels/stamps, call `open_writer`, create schema, acquire a writer lease, checkpoint, migrate, or mutate while the coordinator transaction is held.
4. Revalidate `CURRENT`/generation identity and the coordinator maintenance intent before inserting the one registration row. If store state changed or the read connection is busy, roll back and close before any bounded retry outside the transaction; return `stale_snapshot` or `busy` without registration rows.
5. Commit the registration row before returning `acquired`. Only then may Miller open any generation SQLite handle. A WAL read snapshot plus the maintenance-intent exclusion prevents GC/promotion from deleting or publishing the selected manifest between validation and commit.

The implementation must document the lock order and prove it does not deadlock with existing writer and maintenance paths. Existing writers open a store connection before taking the coordinator writer lease; reader admission takes the coordinator admission transaction before a query-only store connection and never waits while holding the coordinator lock. Therefore no inverse wait cycle is introduced. A coordinator transaction that cannot validate the store snapshot fails closed; it never inserts a registration with guessed identity fields.

The acquire idempotency lookup by `owner_nonce` occurs inside the same coordinator transaction. An existing identical request returns its stored snapshot; a mismatched request refuses before any new registration write. This makes a lost CLI reply safe to retry and prevents duplicate registration rows. A benchmark fixture with 1,000, 10,000, and 100,000 manifest entries must prove one registration row and constant manifest point-query count per acquire; elapsed time is report-only. Count rows visited as well as statements and inspect `EXPLAIN QUERY PLAN`: a single aggregate or hidden join scanning every manifest entry does not pass this gate. Admission must not query `manifest_entries` or enumerate `file_versions`; their traversal belongs to maintenance root marking.

## Verification Strategy

**Project source of truth:** `docs/testing-strategy.md`, `docs/contracts/store-v1.md`, `docs/contracts/cli.md`, and the coordinator/store contract tests.

**Worker red/green scope:** focused producer tests named in each task; no whole workspace suite during individual tasks.

**Worker ceiling:** `cargo test -p julie-extract-artifact --test store_reader_registration_contract`, `cargo test -p julie-extract-artifact --test store_coordinator_contract`, `cargo test -p julie-extract-artifact --test store_maintenance_contract`, and `cargo test -p julie-extract-cli --test store_maintenance_cli_contract` as applicable.

**Worker gate invariant:** a reader never opens an unregistered generation; every registered root survives GC, promotion, rollback, and view retirement until explicit release or definitive owner death; unknown process identity retains the root.

**Lead affected-change scope:** `cargo xtask test changed <touched paths>` plus the producer CLI contract targets and the reader registration tests.

**Branch gate:** `cargo xtask test default`, `cargo xtask test contract`, `cargo test -p julie-extract-artifact --features test-store-crash --test store_crash_contract`, `cargo fmt --check`, `cargo clippy --workspace --all-targets`, and `cargo doc --workspace --no-deps`.

**Security scope:** none declared; the nonce, PID/birth identity, and fail-closed cleanup rules are correctness gates rather than a secrets audit.

**Replay/metric evidence:** hard gates are zero unsafe deletions, zero registration identity mutations, and exact report/row parity. Report the extra acquire/renew/release coordinator writes and GC probe cost separately.

**Escalation triggers:** any schema/floor change, any maintenance race failure, any platform identity ambiguity, any generation swap failure, any changed CLI report field, or any failure in crash recovery requires the branch gate and Windows lifecycle coverage.

**Assigned verification failure:** Workers stop and report when assigned verification fails, unless this plan explicitly says to update that gate.

**Verification ledger:** Record invariant, command, scope label, commit SHA, result, and timestamp. Record race matrix outcomes and whether cleanup was explicit release, definitive death, or fail-closed retention.

## Parallel Execution Contract

| Task | Parallel batch | File ownership | Serialization required | Dependency reason |
|---|---|---|---|---|
| Task 1: Lock schema, compatibility floor, and typed models | None - serial | `crates/julie-extract-artifact/src/store/schema.rs`, new `src/store/reader.rs`, new coordinator contract test, schema/catalog docs | Yes | The schema and typed identity must be frozen before admission, GC, or CLI work. |
| Task 2: Implement atomic acquire/renew/release | Batch A | `crates/julie-extract-artifact/src/store/coordinator.rs`, `src/store/reader.rs`, `tests/store_reader_registration_contract.rs` | Yes | Depends on Task 1's schema and typed model; owns the coordinator write protocol. |
| Task 3: Add process-instance death qualification | Model-freeze batch | new `src/store/reader_liveness.rs`, platform modules/tests, `tests/store_reader_liveness_contract.rs`; transferred `src/store/mod.rs` and artifact platform dependency declaration | No | Owner PID/birth-string models and root exports are frozen by Task 1. May run beside Task 1's remaining floor tests and then Task 2; do not edit reader.rs/coordinator.rs/maintenance.rs. |
| Task 4: Integrate reader roots into GC, promotion, rollback, and retire-view | Batch B | `src/store/maintenance.rs`, `src/store/generation.rs`, `src/store/manifest.rs`, related maintenance tests | Yes | Depends on acquire rows and liveness qualification; must land before CLI exposes the contract. |
| Task 5: Add public reader CLI and JSON report | Batch C | `crates/julie-extract-cli/src/store/args.rs`, new `crates/julie-extract-cli/src/store/reader.rs`, `crates/julie-extract-cli/src/store/mod.rs`, `tests/store_reader_cli_contract.rs`, `docs/contracts/cli.md` | Yes | Depends on stable producer API and report schema; Plan 4 Task 6 must merge first because it narrows store exports. |
| Task 6: Verify consumer cursor qualification and mixed-version behavior | Batch D | `tests/store_coordinator_contract.rs`, `tests/store_maintenance_contract.rs`, compatibility evidence, `docs/evidence/` | Yes | Depends on Tasks 1–5; proves cursors remain watermarks and old writers cannot ignore active registrations. |

## Task 1: Lock schema, compatibility floor, and typed models

**Execution decision (2026-09-05):** Audit Plan 4 is merged at `bb93a721`. Use the existing `store_meta.min_writer_version` compatibility gate with reader-capable development version `2.40.0`; this does not authorize release, tags, pushes, or Miller pins. Keep store schema v2 so existing families remain usable. Before reader admission is enabled, permanently raise the CURRENT store's writer floor under the existing maintenance fence, outside the admission transaction. A coordinator-only schema addition cannot exclude old maintenance binaries. Reader admission validates the floor using query-only metadata reads and refuses any unsafe floor; it never migrates or writes store.db while holding the coordinator admission transaction. Existing maintenance copies the source floor to newly published generations. Task 4 must verify every publication/recovery path preserves the reader floor, including already-built successors. Retained non-CURRENT generations remain write-fenced. The actual v2.39.0 binary must refuse all mutating maintenance before coordinator/store mutation. Do not silently limit rollout to fresh stores.

**Additional owned files:** `src/store/mod.rs`, `src/store/connection.rs`, `src/store/layout.rs`, bounded floor-activation support in `src/store/maintenance.rs`, their narrow compatibility tests, workspace Cargo version metadata/lockfile, and the existing schema catalog generator/output as required. Preserve private store modules and explicitly re-export only caller-facing reader types. Select development version `2.40.0` without release metadata, notes, tags, or pins. Task 1 defines and proves the permanent floor gate; Tasks 2/4 consume it before admission and preserve it across publication.

**Model clarifications:** Persist the diagnostic owner label as `owner_label` (bounded 1..128 characters) so idempotency can reject changed owner labels; it is not authentication. The caller supplies the unpredictable nonce; the producer generates the unpredictable pin ID because acquire has no `--pin` input. Release removes the single registration row, not nonexistent child/version-root rows. Add the cross-field constraint `min_retained_store_log_sequence <= served_store_log_sequence`. Freeze these models before Tasks 2 and 3.

**Files:** `crates/julie-extract-artifact/src/store/schema.rs`, new `crates/julie-extract-artifact/src/store/reader.rs`, schema catalog/compatibility documentation, new `crates/julie-extract-artifact/tests/store_reader_registration_contract.rs`.

**What to build:** Add the proposed `coord.db.reader_registrations` table, indexes, schema catalog entry, and typed Rust models for immutable reader identity, manifest snapshot, acquire/renew/release requests, and report facts. Keep registration rows in `coord.db`; use existing `store.db` manifest roots rather than adding a copied version-root table.

**Interfaces:** New producer-scoped reader request/response types in `crates/julie-extract-artifact/src/store/reader.rs`; new `reader_registrations` schema owned by `coord.db`; no public `consumer_cursors` signature change.

**Focused red/green:** `cargo test -p julie-extract-artifact --test store_reader_registration_contract schema_rejects_invalid_reader_identity` (red before schema/models, green after); `cargo test -p julie-extract-artifact --test store_schema_contract`.

**Approach:** Start from `create_coordinator_schema` and the existing strict tables. Use bounded IDs and positive integer checks matching `consumer_cursors`, `writer_lease`, and `maintenance_intent`. Store `snapshot_fingerprint` as a deterministic digest over the immutable family, store instance, view, manifest generation/hash, generation name, extraction epoch, served sequence, and minimum retained sequence; do not hash an enumerated version list at acquire. Add tests that create the schema twice, inspect exact columns/constraints, reject empty/oversized identity fields, and prove one registration row is sufficient for a manifest with 1,000, 10,000, and 100,000 entries.

**Compatibility proof:** The current v2.39.0 writer is the old-writer baseline. Add a fixture opened by the v2.39.0 binary and require `incompatible_store` (exit 3) before any mutation for `store maintain gc --apply`, `repair --apply`, `promote --apply`, and `retire-view --apply` when reader registrations are present. The old writer is not treated as reader-aware. If the existing writer-version floor cannot enforce that refusal, add a mandatory coordinator schema/floor migration and keep reader acquire disabled until the old writer is excluded by that floor. Do not use an in-place schema change that an old writer auto-accepts while ignoring registrations; no compatibility fallback may silently proceed.

**Acceptance criteria:**

- [x] `coord.db` contains one `reader_registrations` table with the exact proposed identity/snapshot fields and no copied version-root table.
- [x] No reader registration table is created in `store.db`.
- [x] Identity fields are immutable in the database and models expose no retarget operation.
- [x] Schema/version compatibility either proves old writers are safe or refuses them before reader enablement.
- [x] The focused schema contract passes.

## Task 2: Implement atomic acquire, renew, and release

**Files:** `crates/julie-extract-artifact/src/store/coordinator.rs`, `src/store/reader.rs`, `tests/store_reader_registration_contract.rs`.

**What to build:** Add producer-scoped typed methods for reader acquire, renew, release, and inspection. Acquire must validate the current view and manifest metadata against the requested generation and insert exactly one registration row in one `BEGIN IMMEDIATE` transaction. It must recheck foreign live `maintenance_intent` in that same transaction before insertion.

**Interfaces:** `ReaderAcquireRequest`, `ReaderAcquireResult`, `ReaderRenewRequest`, `ReaderReleaseRequest`, and `ReaderRegistration` in `crates/julie-extract-artifact/src/store/reader.rs`; coordinator methods on `StoreCoordinator`; exact cross-database admission helper described above.

**Focused red/green:** `cargo test -p julie-extract-artifact --test store_reader_registration_contract acquire_is_idempotent_by_nonce` and `cargo test -p julie-extract-artifact --test store_coordinator_contract`.

**Approach:** Follow `advance_consumer_cursor`'s validation and transaction pattern, but bind the registration to an immutable snapshot fingerprint. Re-read the current view/manifest rows inside the transaction; reject a changed generation as `stale_snapshot`. Renew updates only heartbeat/expiry after matching pin ID, nonce, PID, and producer-captured birth identity. Release deletes the parent and children atomically; an absent row is an idempotent no-op regardless of nonce, while an existing row requires the exact nonce. Never return the nonce to a caller that did not present it.

**Race tests:** Use barriers around the coordinator transaction to race acquire against an ordinary writer and maintenance intent creation, generation publication, view retirement, and GC planning. Assert either acquire commits before the maintenance fence and becomes a protected manifest root, or it refuses without a registration row. Race renew/release against GC apply and assert no root is deleted while the authenticated mutation is committed.

**Acceptance criteria:**

- [ ] Acquire returns one exact generation/view/manifest snapshot and a bounded protected-manifest count; file versions remain producer-side roots.
- [ ] Acquire cannot commit under a foreign live maintenance intent.
- [ ] Renew cannot change snapshot identity and rejects nonce/PID/birth-identity mismatches.
- [ ] Release is idempotent for the exact nonce and opaque for a wrong nonce.
- [ ] Admission/maintenance race tests prove no partial registration or unsafe deletion.

## Task 3: Implement definitive process-instance death qualification

**Files:** new `crates/julie-extract-artifact/src/store/reader_liveness.rs`, platform-specific process identity modules, `tests/store_reader_liveness_contract.rs`.

**What to build:** Capture a platform birth identity at acquire and compare `(PID, birth identity)` when deciding whether an expired registration belongs to a definitively dead process instance. Provide a test seam for clock and process inspection. Linux must use a stable process-start identity; Windows must use a process creation identity/handle-based check; other platforms must return unknown unless their existing platform support proves an equivalent identity.

**Interfaces:** `ProcessInstanceIdentity`, `ProcessIdentityProbe`, and `DeathQualification` in `crates/julie-extract-artifact/src/store/reader_liveness.rs`; the probe captures identity from the live PID and is never populated from a CLI string.

**Focused red/green:** `cargo test -p julie-extract-artifact --test store_reader_liveness_contract expired_paused_reader_is_retained` and `cargo test -p julie-extract-artifact --test store_reader_liveness_contract pid_reuse_is_unknown`.

**Approach:** Keep timeout as the trigger for a liveness probe, not as the verdict. A matching live PID and birth identity retains the registration. A dead PID with a previously captured matching birth identity permits cleanup. PID reuse, missing identity, probe error, access denial, and unsupported platform all return `Unknown` and retain the registration with warning code `reader_identity_unknown`. Explicit release remains available when a platform cannot prove death.

**Acceptance criteria:**

- [ ] A paused process past `expires_at` is retained when its process instance is still alive.
- [ ] A terminated process is eligible for cleanup only with definitive PID plus birth-identity evidence.
- [ ] PID reuse, unknown birth identity, probe failure, and unsupported platform all fail closed.
- [ ] Tests cover clock expiry, pause, crash, PID reuse, and Windows real process identity paths.
- [ ] Liveness results carry a reportable reason and never masquerade as explicit release.

## Task 4: Qualify reader roots in GC, promotion, rollback, and view retirement

**Files:** `crates/julie-extract-artifact/src/store/maintenance.rs`, `src/store/generation.rs`, `src/store/manifest.rs`, `tests/store_maintenance_contract.rs`, `tests/store_generation_equivalence.rs`, `tests/store_manifest_contract.rs`.

**What to build:** Load unexpired reader registrations and their manifest roots into `MaintenancePlan`, enumerate reachable versions only during maintenance marking/apply, include generation/view/manifest/log roots in fingerprints, and revalidate them at apply before the first destructive operation. Expired registrations are cleaned only through Task 3's definitive-death result; unknown registrations remain protected and add a warning.

**Interfaces:** Extend `MaintenancePlan`/`MaintenanceRootKind` and the existing maintenance snapshot/planner functions; extend `GenerationApplyReport` and `StoreMaintenanceReport` with bounded reader protection/unknown-identity facts. Keep the existing `store maintain` command grammar.

**Focused red/green:** `cargo test -p julie-extract-artifact --test store_reader_registration_contract gc_keeps_registered_manifest_roots`; `cargo test -p julie-extract-artifact --test store_maintenance_contract`; `cargo test -p julie-extract-artifact --test store_generation_equivalence`.

**Approach:** Extend the existing protected cursor/root collection beside `protected_cursors`, `protected_generations`, and `protected_failed_paths`. GC must retain the registered manifest, its entries, every reachable version, and required log sequence. Promotion and rollback must retain the source generation until all registrations are released or definitively dead. `retire-view` must refuse a live registration for that view and must not delete its registration as a side effect. Apply reopens `coord.db`, rechecks the registration fingerprint and maintenance fence, then rechecks the candidate generation immediately before each cohort.

**Race matrix:** Cover reader acquire versus GC plan/apply, renew versus GC apply, release versus GC apply, generation swap while a reader holds the old generation, rollback while a reader holds the selected historical generation, and view retirement while a reader is registered. Add crash injection after intent, before root scan, after root scan, and before the first delete; recovery must preserve the registration or produce a safe retained warning.

**Acceptance criteria:**

- [ ] GC plans show protected reader registrations, manifest roots, reachable versions, generations, and log floors.
- [ ] GC, promotion, rollback, and retire-view cannot delete a protected root.
- [ ] Apply revalidation catches registration changes after planning.
- [ ] A live paused reader remains protected past heartbeat timeout.
- [ ] Unknown process identity produces a warning and retains the root.
- [ ] Crash/restart tests preserve safety and keep maintenance reports internally consistent.

## Task 5: Add public reader CLI and report v1

**Files:** `crates/julie-extract-cli/src/store/args.rs`, new `crates/julie-extract-cli/src/store/reader.rs`, `crates/julie-extract-cli/src/store/mod.rs`, `tests/store_reader_cli_contract.rs`, `docs/contracts/cli.md`.

**What to build:** Add `store reader acquire|renew|release` using the exact arguments and JSON fields in the Proposed Wire Contract. The CLI is a thin producer client over the typed artifact-store API; it does not open a generation on behalf of Miller and does not write a second registration store.

**Interfaces:** `StoreCommand::Reader`, `StoreReaderCommand`, and typed argument structs in `store/args.rs`; dispatch functions and report serialization in `store/reader.rs`; no CLI writes outside the family store.

**Focused red/green:** `cargo test -p julie-extract-cli --test store_reader_cli_contract reader_help_is_stable`; `cargo test -p julie-extract-cli --test store_reader_cli_contract acquire_renew_release_json_is_one_line`.

**Approach:** Add clap parsing with the existing path, family, ID, and duration validators. Serialize one stable report line, route human success to stdout and failure to stderr, and preserve the existing maintenance exit codes. Validate nonce and owner identity lengths before opening the store. Keep `--json` output bounded: include `protected_manifest_count` and warning text, never file-version lists, full source paths, or arbitrary database payloads.

**Plan 4 dependency:** Land after `audit-4-dead-code-and-api-narrowing` merges and its Task 6 store export narrowing is reviewed. The producer API must be added to the final allowed store export list; do not reintroduce removed public symbols to make the CLI compile.

**Acceptance criteria:**

- [ ] Help exposes exactly `store reader acquire|renew|release` and the proposed arguments.
- [ ] JSON report v1 is one line, deterministic for the same snapshot, and stable on idempotent release.
- [ ] CLI tests cover wrong nonce, stale generation, unknown process identity, and incompatible writer floor.
- [ ] Existing `store maintain` and cursor command reports remain byte-compatible.
- [ ] CLI invokes producer registration methods and creates no Miller-owned files.

## Task 6: Verify cursor qualification and mixed-version behavior

**Files:** `crates/julie-extract-artifact/tests/store_coordinator_contract.rs`, `tests/store_maintenance_contract.rs`, `tests/store_maintenance_schema_contract.rs`, `crates/julie-extract-cli/tests/store_maintenance_cli_contract.rs`, new `docs/evidence/2026-09-producer-retention-contract.md`.

**What to build:** Preserve and explicitly verify the existing consumer cursor contract while adding reader registrations. A cursor remains a monotonic `(generation_name, store_log_sequence, updated_at)` watermark; it may protect log retention but it cannot authorize a generation open and cannot substitute for a reader registration.

**Interfaces:** No new cursor API. Extend existing `StoreCoordinator::advance_consumer_cursor`, `release_consumer_cursor`, maintenance root collection, and CLI contract fixtures only where needed to observe the new reader roots.

**Focused red/green:** `cargo test -p julie-extract-artifact --test store_coordinator_contract consumer_cursor_advance_is_monotonic_bounded_and_releasable`; `cargo test -p julie-extract-artifact --test store_maintenance_schema_contract consumer_cursors_and_allocator_marks_never_regress`; `cargo test -p julie-extract-cli --test store_maintenance_cli_contract cursor_advance_and_release_are_explicit_monotonic_mutations`.

**Approach:** Re-run the existing cursor tests with active reader registrations. Prove cursor advance/release still recheck foreign maintenance intent and reject generation/sequence regressions. Add mixed-version fixtures: an old writer, current writer, and newer writer attempt maintenance against a family with registrations. Record whether the old writer refuses by floor or understands the new root; any path that can proceed while ignoring registrations is a hard failure.

**Acceptance criteria:**

- [ ] Existing cursor monotonicity, generation binding, release, and foreign-intent tests remain green.
- [ ] GC retains log rows required by both cursor windows and reader registrations.
- [ ] A cursor-only client cannot open a generation after its cursor advances.
- [ ] The v2.39.0 old-writer fixture returns `incompatible_store` (exit 3) before mutation for every mutating maintenance command when registrations exist.
- [ ] Evidence records the race matrix, platform results, report warnings, and verification ledger entries.

## Windows and Branch Gate

Windows lifecycle verification is required because process birth identity, path spelling, and generation handles differ from Unix. Run the focused lifecycle tests through the repository's `win-test` workflow, including real process termination, PID reuse simulation, generation swap, view retirement, and unknown-identity fail-closed cases. The producer branch gate must include the commands in Verification Strategy and the Windows result in the evidence document before the CLI contract is published.

## Required State Transitions and SQL Assertions

The focused contract tests must exercise the following state transitions with a fresh coordinator and a real generation fixture. The SQL assertions are deliberately explicit so a future implementation cannot satisfy the report shape while omitting a retention root.

1. Create `gen-000001`, view `default`, manifest generation `1`, versions `101` and `102`, and log high-water `800`. Acquire a reader and assert one registration row, `protected_manifest_count = 1`, the requested generation, and `served_store_log_sequence <= 800` plus `min_retained_store_log_sequence <= served_store_log_sequence`.
2. Publish `gen-000002` for the same view after acquisition. Assert the registration still names `gen-000001` and its snapshot fingerprint is unchanged; no update to `generation_name` or `manifest_generation` is accepted.
3. Advance the cursor for a different consumer while the reader is held. Assert the cursor row changes monotonically while the reader registration row remains unchanged.
4. Run a GC plan and assert the plan includes the reader's `pin_id` under protected roots, its manifest under protected manifests, its generation under protected generations, each reachable version under protected versions, and the reader log floor under protected log roots.
5. Attempt `retire-view --view default --apply` while the reader is live. Assert `busy`, no manifest deletion, no view deletion, and no reader deletion.
6. Release with the wrong nonce. Assert `reader_owner_mismatch`, no row changes, and no registered snapshot, owner, or nonce details in the response. The specified absent-row success versus existing-row mismatch necessarily distinguishes those cases; do not claim membership secrecy the wire contract cannot provide.
7. Release with the correct nonce twice. Assert the first operation deletes exactly one registration row and the second is a successful idempotent no-op.
8. Reacquire on `gen-000002`, then attempt an apply using a plan fingerprint made before acquire. Assert `stale_plan` and no destructive SQL statement has run.

The test should query these exact facts after each operation:

```sql
SELECT pin_id,owner_nonce,view_id,manifest_generation,generation_name,
       owner_pid,owner_birth_identity,heartbeat_at,expires_at,
       store_instance_id,manifest_hash,extraction_identity_epoch,
       served_store_log_sequence,
       min_retained_store_log_sequence,snapshot_fingerprint
FROM reader_registrations ORDER BY pin_id;
```

The implementation must use parameterized SQL for all identity values. A registration may be physically marked released in an internal audit record if that is needed for diagnostics, but the live root tables must not retain an authenticated registration after release. The report must distinguish `released`, `definitively_dead`, and `retained_unknown_identity` so operators can tell why a root is still present.

## Operational Ownership and Recovery Rules

The caller that acquires a registration owns the renewal schedule. Julie does not start a background process for a reader and does not infer that a registration is obsolete from missing CLI traffic. Miller is responsible for renewing before expiry and for explicit release on session close. A crash leaves a registration until the next maintenance probe establishes definitive process-instance death.

Recovery must preserve the registration table across generation replacement because `coord.db` is outside the generation directories. A torn acquire transaction leaves no registration row. A torn release transaction leaves the complete live registration. A torn GC transaction cannot delete a referenced manifest, entry, version, or generation before the corresponding registration root is re-read and qualified. A coordinator database recovery that cannot read registration rows must fail maintenance closed rather than treating the root set as empty.

The caller generates the nonce and Julie generates the registration identifier, both with sufficient unpredictability; neither may be derived from a workspace path, PID alone, timestamp alone, or generation name. Julie stores them as opaque text and includes only the `pin_id` in diagnostic references. The nonce is accepted on an argument in the specified CLI contract; it must not be written to ordinary human reports or logs.

The owner label is diagnostic metadata and is not an authentication factor. PID and birth identity are the liveness proof. A renewal from a process with a matching PID but a different birth identity is an owner mismatch and cannot extend the registration. An explicit release remains the only cleanup path for an owner whose platform has no definitive birth identity support.

## Producer and Consumer Sequencing

The required consumer sequence is:

```text
1. Read only the current family/view binding and generation pointer needed to request admission; do not read manifest roots before acquire.
2. reader acquire (producer transaction validates and captures the manifest identity).
3. Open the returned generation and read only the returned snapshot.
4. Renew periodically while the read session is live.
5. On close, reader release; on failure, leave the registration for death qualification.
```

The producer must not return `acquired` until the registration row commits. The consumer must not open a generation from a pre-acquire path or from a later `CURRENT` pointer. If opening the returned generation fails after acquire, the consumer releases the registration; if release fails, it reports the `pin_id` so maintenance can retain and diagnose it.

The existing cursor sequence is orthogonal:

```text
reader registration -> protects an open generation and its file/log roots
consumer cursor     -> protects a consumer's acknowledged log watermark
```

Both roots participate in GC. Neither root may be silently converted into the other. Cursor release must not release a reader registration, and reader release must not advance a consumer cursor.

## Evidence Required Before Producer Handoff

The evidence document must include the Julie commit SHA, schema catalog digest, CLI help output, one successful acquire/renew/release JSON transcript with nonce values redacted, and SQL counts before and after each race test. It must record at least these cases:

- live reader, expired heartbeat, matching process identity: retained;
- paused live reader past timeout: retained;
- crashed reader, definitive death: eligible for cleanup;
- PID reused with different birth identity: retained;
- unknown birth identity or denied probe: retained with warning;
- acquire racing maintenance intent: one winner, no partial rows;
- renew racing GC: renewal or GC wins under the same fence, never deletion of a live registration;
- generation promotion and rollback while old generation is held: old manifest and physical generation retained;
- view retirement while a reader is held: refusal with no manifest/root deletion;
- old writer encountering reader schema/floor: refusal or proven reader-aware qualification.

For each case, record the final `reader_registrations` count, manifest-root protection facts, the maintenance disposition, and whether a destructive statement ran. A passing test that observes only CLI output is insufficient; the database root facts are the authority.

## Cross-Repository Handoff

After the producer branch is merged and a Julie release is published, Miller must consume the exact acquire/renew/release JSON contract in `../../../miller/docs/plans/2026-09-04-reader-retention-integration.md`. Miller should acquire before opening its read session, renew from the session owner, release on normal close, and report producer warnings without treating a timeout as permission to continue unpinned. The broader architectural review remains in `../../../miller/docs/plans/2026-09-04-architecture-review-program.md`.
