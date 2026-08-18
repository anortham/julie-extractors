> **Superseded 2026-08-18.** The resolution write path is retired. See [2026-08-18-resolution-write-path-retirement.md](../decisions/2026-08-18-resolution-write-path-retirement.md).

# Index Store Ph2c Resolution Design

**Status:** Completed 2026-08-08. Ph2d lifecycle completion is included in the v2.31.0 release
candidate; Ph3 Miller integration remains separate.

## Goal

Add resolution-only execution and exact-generation resolution binding to the unreleased versioned
store without changing the legacy artifact's resolution output, blocking store writers for a
whole-corpus pass, or weakening the frozen G3b acceptance condition.

This design is governed by:

- Miller's frozen `docs/plans/2026-08-07-index-store-v4-contract.md`, especially sections 14–16.
- Miller's `docs/findings/2026-08-07-index-store-binding-proof.md` and its carried Ph2 gates.
- Julie Extractors' `docs/plans/2026-08-07-index-store-ph2b-store-kernel-plan.md`.
- The shipped legacy resolver and the merged Ph2b store contracts at commit `5577497`.

## Program Cut

Ph2c remains one program on one feature branch, with two separately reviewable and revertible
slices. The first slice proves the real mechanism. The second exposes it through durable store
state and public commands.

### Ph2c-a — resolver session and mechanism proof

1. Freeze the current legacy overlay output with a pinned multi-language oracle.
2. Refactor the resolver behind one manifest/corpus-scoped `ResolutionSession` interface.
3. Add the store scratch session, production resolution-file schemas, streaming diff, and persisted
   replacement/tombstone output.
4. Run the predeclared G1/G2/G3/G4/G5 gates. Any hard-gate failure blocks Ph2c-b and reopens the
   binding mechanism.

### Ph2c-b — durable binding and adapters

1. Bump the unreleased store and coordinator catalogs to schema version 2.
2. Add base publication/recovery, view deltas, exact-generation binding, pins, and resolve jobs.
3. Add `store resolve`, `store export`, and `store import --from-artifact`.
4. Re-run G3b against actual `store.db` delta publication and run the full state-machine,
   crash/recovery, equivalence, and concurrency gates.

Ph2d subsequently completed general retention, version/log reclamation, staged vacuum/reindex,
capacity preflight, store-generation promotion, repair, forward rollback, and release preparation.
Ph3 still owns Miller's production reader integration and the post-release pin bump.

## Architecture Quality

**Affected modules:** the legacy CLI resolver, artifact resolution storage, artifact store schema
and coordinator, store CLI parser/executor/reporting, and new resolution base/delta modules.

**Caller-facing interface:** three public commands—`store resolve`, `store export`, and
`store import --from-artifact`—plus the existing import/update/delete commands' resolution-state
reports. Callers do not manage scratch files, base identity, delta generations, pins, or CAS retries.

**Depth/locality check:** one `ResolutionSession` owns every coupled resolver obligation: visible
corpus selection, worklists, same-pass overlay readback, flush boundaries, deterministic ordering,
identity translation, and storage-specific errors. The resolver engine depends on that interface
instead of SQL table names. Base lifecycle and view binding live behind separate artifact-store
modules; CLI modules only validate, enqueue, wait, and render.

**Test surface:** behavior is proved through the legacy scan/update interface, public store commands,
and artifact-store contract APIs. Private helpers are not the primary test surface.

**Seams/adapters:** `ResolutionSession` has two required implementations from the start: the legacy
artifact session and the manifest-scoped store scratch session. The seam is therefore not
speculative. Base/delta schemas are production code in Ph2c-a and reused unchanged by Ph2c-b.

**Rejected shortcuts:** a monolithic Ph2c change; materializing a temporary v3 artifact; independent
read/write ports that cannot express the resolver's anti-joins; attaching the live store to a bulk
scratch connection; running whole-corpus resolution while holding the store-writer lease; and
claiming G3b from a proxy diff or widened denominator.

**Architecture risk:** high. The current resolver embeds legacy identities and SQL on both sides of
its worklists. The design controls that risk with a pinned oracle, a revertible refactor unit, a
mechanism gate before schema exposure, and a second G3b gate on the real store write path.

## Resolution Session Contract

The resolver engine consumes one session abstraction. The implementation plan may refine method
names, but it must preserve these responsibilities as one cohesive interface rather than leaking
storage joins back into the engine.

### Corpus identity and visibility

- A store session is bound to exactly `(family_id, view_id, manifest_generation, manifest_hash)`.
- Visible extraction rows come only from versions referenced by that manifest generation.
- `indexed` and `failed_preserved` entries with `version_id` contribute immutable extraction rows.
- Every manifest entry contributes a visible path for module-existence checks, including `failed`
  entries with no version. Schema v2 records the entry language so module selection has the same
  `(path, language)` input as the legacy `files` table.
- Paths are canonical root-relative slash paths. Store/v3 path normalization parity is a hard gate.
- The store must not resolve until every visible indexed or failed-preserved version has a committed
  L2 stamp. Otherwise `store resolve` returns typed `resolution_input_incomplete` and writes no base,
  delta, exactness state, or terminal success.
- L2 rows are immutable after their stamp. A base remains keyed by
  `(manifest_hash, resolver_output_epoch)` only because L2 completeness is a precondition, never a
  mutable part of a ready base.

### Deterministic resolver behavior

- Session worklists expose stable semantic keys and explicit ordering.
- Legacy local identifiers retain their legacy ordering at the adapter boundary.
- Store rows order by `(version_id, local_id)` only after the session has mapped them to semantic
  identities. Candidate and relationship propagation order must be deterministic and covered by
  collision fixtures.
- Same-pass writes become visible only at the existing resolver phase boundaries. The session owns
  the flush/readback rule.
- The engine never names `files`, `file_id`, `store.db`, `version_id`, ATTACH aliases, or scratch
  paths.

### Session implementations

`LegacyResolutionSession` uses the current artifact transaction and v3 SQL shape. Its failures map
to the existing non-fatal `ResolutionHookError`; scan/update commits remain byte-compatible.

`StoreScratchResolutionSession` uses:

- bounded read transactions over immutable manifest/version rows;
- a separate scratch output connection, never `ATTACH store.db`;
- streaming ordered merges for source/overlay anti-joins and base/delta comparison;
- the actual base and delta semantic schemas;
- typed SQLite, filesystem, integrity, and identity errors that become fatal store-request results.

The store session does not hold a single long `store.db` read snapshot. Reopening bounded reads is
safe because manifest rows and complete file-version rows are immutable. A publish CAS revalidates
the manifest generation and delta head before any durable binding change.

## Semantic Resolution Rows

Store resolution rows exclude `resolved_at_revision`; it is legacy artifact bookkeeping, not a
semantic resolution value. Base metadata records `resolver_output_epoch`.

### Identifier resolution

Natural key: `(version_id, identifier_id)`.

Payload:

- nullable `(target_version_id, target_symbol_id)`;
- nullable `tier`, `confidence`, and `method`;
- non-null `outcome`;
- nullable candidate count.

The resolved outcome requires a target; non-resolved outcomes require no target. The store gate
proves one semantic row for every visible identifier.

### Pending resolution

Natural key: `(version_id, pending_relationship_id)`.

Payload:

- non-null `(target_version_id, target_symbol_id)`;
- `tier`, `confidence`, and `method`.

Pending resolution is a partial relation. Delta tombstones therefore represent a base row that is
absent in the exact view result.

### Base target integrity

A separate base file cannot enforce foreign keys into `store.db`. Before a base is eligible for
rename, a streaming integrity pass verifies every target pair against the manifest-visible symbol
set. The base catalog also records its source versions in `resolution_base_versions`, which roots
them independently of manifest retention.

## Production Resolution Files

Ph2c-a adds production-owned DDL and catalog hashes for:

- immutable base databases containing metadata, identifier resolutions, and pending resolutions;
- scratch delta databases containing identifier replacements, pending replacements, and pending
  tombstones.

These functions are exercised only by feature-gated contracts in Ph2c-a. No public command or
store catalog row references them until Ph2c-b. This is shared production infrastructure, not a
success stub.

Base files are built in a temporary root for Ph2c-a. Ph2c-b uses
`<generation>/scratch/resolution-<request_id>-<nonce>.db` and publishes ready bases as
`<generation>/bases/base-<manifest_hash>-<resolver_output_epoch>.db`.

## G3 Measurement Contract

The measurement policy is frozen before the first Ph2c run. Three full runs are required. There is
no averaging: every measured Miller-scale pair in every run must pass each hard threshold.

Authoritative row counts come from tables, never report counters.

### Comparable resolution phase

`resolution_compute_ms` begins before the resolver's first candidate/worklist read and ends after
the final semantic resolution-row flush. It includes manifest-scoped source reads and candidate
index construction because they are part of the Rust resolver. It excludes secondary-index build,
foreign-key/integrity validation, checkpoint/close, diff, and delta persistence. This is the primary
denominator comparable to the accepted binding proof.

`store_fresh_ms` is a secondary end-to-end measure that adds base indexes, target-integrity checks,
SQLite integrity checks, and durable close. It cannot replace the primary denominator.

`diff_ms` measures the streaming semantic merge across both resolution tables.

`delta_write_ms` measures production-profile semantic delta writes and readback. Ph2c-a uses the
planned FULL/WAL/FK/index profile in scratch; Ph2c-b repeats the measurement against actual
`store.db` publication.

`time_to_exact_ms = store_fresh_ms + diff_ms + delta_write_ms + publish_ms`.

### Hard gates

- **G1 determinism:** zero semantic differences across two from-scratch builds for both tables.
- **G2 exactness:** persisted base plus persisted delta equals a fresh exact result for every pair,
  including synthetic deletion, multi-delete, path reuse, failed, and failed-preserved fixtures.
- **G3a rate:** `identifier_resolutions` table rows divided by `resolution_compute_ms` is at least
  50,000 rows/second for every measured pair.
- **G3b overhead:** `(diff_ms + delta_write_ms) / resolution_compute_ms <= 0.50` for every pair in
  every run. No secondary end-to-end ratio may substitute for this result.
- **G3c absolute:** every Miller-scale pair reaches exact state within 30 seconds.
- **G4 gap enumeration:** exact gap enumeration stays in-band with the streaming diff and preserves
  the frozen lower-bound/exact reporting semantics.
- **G5 bind:** foreground binding remains O(manifest) with zero identifier resolution work and
  satisfies the frozen millisecond-scale threshold.

The ledger also records peak RSS, base/delta bytes, row counts per table, integrity-check time, and
the secondary store-shaped ratio as report-only evidence.

## Ph2c-b Store Schema Version 2

Schema v2 changes both `store.db` and `coord.db`. Existing schema-v1 catalogs receive the existing
typed `OlderSchema` refusal. There is no in-place migration: Ph2b is unreleased, and Ph2d owns store
generation promotion. Tests prove the refusal occurs at open, before any mutation.

### Store catalog additions

- `resolution_bases`: identity, state (`building|ready`), relative filename, semantic row counts,
  bytes, source manifest hash, resolver epoch, and creation/update timestamps.
- `resolution_base_versions`: `(base_id, version_id)` roots every version named by the base.
- `resolution_identifier_deltas`: cumulative per-view replacements keyed by
  `(view_id, delta_generation, version_id, identifier_id)`.
- `resolution_pending_deltas`: cumulative replacements and tombstones keyed by
  `(view_id, delta_generation, version_id, pending_relationship_id)`.
- `resolution_pins`: generation/base/delta pins used by store readers and in-progress resolve jobs.
- `manifest_entries.language`: required module-resolution input and part of manifest-hash v2.
- `views` binding checks that permit only coherent unbound/converging/exact states.

The exact DDL, checks, foreign keys, indexes, and catalog hashes are frozen in the implementation
plan before code changes.

### Coordinator additions

- Request kinds add `resolve`, `export`, and `from_artifact`.
- At most one resolve request per family may remain claimed for off-lease computation.
- A resolve claimant heartbeats its request from a dedicated connection while computing. Losing
  the claim stops work before publication.
- The store-writer lease is acquired only for short binding/publication transactions.
- Import/update/delete retain their existing bounded quantum scheduling.

## Base and Delta State Machine

### Base publication

1. Under the store-writer lease, insert a `building` base row and its version roots.
2. Release the store-writer lease.
3. Build and validate the scratch database while heartbeating the resolve claim.
4. Atomically rename the scratch file to its final base path.
5. Reacquire the store-writer lease and CAS the base row to `ready` after verifying the file,
   recorded identity, and current request claim.

The filesystem is authoritative for a completed rename. Recovery handles every torn state:

- building row plus valid final file → verify and mark ready;
- building row plus invalid final file → delete file and rebuild;
- ready row plus missing file → reset to building and rebuild;
- unowned base file → delete only after proving no live request/pin owns it;
- live scratch file → never reaped by another claimant.

### View binding and convergence

- First family view: validated scratch output becomes the base; delta is empty; bind exact.
- Identical `(manifest_hash, resolver_output_epoch)` views reuse one ready base with an empty delta.
- Foreground bind selects the nearest ready same-epoch base in O(manifest) time and sets
  `exact_at = NULL`.
- Background convergence builds a fresh exact result off-lease, diffs it against the selected base,
  and prepares one cumulative delta.
- Publish CASes `(view_id, manifest_generation, previous_delta_generation, resolve_claim)`.
- CAS success writes delta rows, exact gap facts, the new delta head, `resolution_exact_at`, and one
  store-log effect in the same transaction.
- CAS loss writes no partial delta/log/binding state and discards the scratch result.
- Content-changing import/update/delete leaves `resolution_exact_at` behind in the same manifest
  transaction. An identical manifest may retain exactness.
- A session may apply a delta only when its pin's manifest generation equals the delta's `exact_at`.
  Otherwise it serves the pinned base alone and reports convergence honestly.
- Superseded delta rows are removed only when no pin references them. This pin-aware state cleanup
  is required for correctness; general retention remains Ph2d.

Ph2d must not add version/base GC without honoring `resolution_base_versions`, resolve claims, and
pins. That cross-phase invariant is part of the Ph2d entry contract.

## Public Commands

```text
julie-extract store resolve --store <family-dir> [--family <uuid>] --view <id> [request controls] [--json]
julie-extract store export --store <family-dir> [--family <uuid>] --view <id> --out <file> [--json]
julie-extract store import --from-artifact <symbols.db> --store <family-dir> --family <uuid> --root <dir> --view <id> [request controls] [--json]
```

`store resolve` enqueues or observes an idempotent resolve request. It never extracts source files.
It reports unbound, converging with a lower-bound gap, or exact at a named manifest generation.

`store export` materializes the pinned view and exact resolution generation into `<out>.partial`,
validates it, then renames atomically. It never mutates the family store.

`store import --from-artifact` performs a resumable transformation into immutable versions,
manifest state, a ready base, and exact binding. It does not copy the v3 file wholesale and does not
mix the legacy artifact schema into store tables.

The report remains schema v1 unless its existing shape cannot represent a required fact. New stable
failure classes include `resolution_input_incomplete`, `resolution_failed`, and
`resolution_not_exact`; schema/family/root/idempotency failures retain their existing classes.

## Verification and Gate Routing

Fast unit and contract tests remain in the default tier. Slow multi-process, Miller-scale, crash,
and measurement gates are feature-gated from their first commit and registered explicitly in
`xtask`.

Planned feature gates:

- artifact: `test-store-resolution` for base/delta schema, persisted roundtrip, crash, and recovery;
- CLI: `test-store-resolution-contract` for public commands, legacy oracle, multi-language
  equivalence, and G1/G2;
- performance: a non-default resolution-measurement command for the three-run G3 suite.

Ph2c-b cannot be called complete until these pass:

- pinned pre-refactor legacy overlay oracle, including both resolution tables and revision stamps;
- two-view family isolation with shared versions and divergent manifests;
- L2-incomplete typed refusal and non-vacuous identifier totality;
- base building/rename/ready crash matrix and filesystem reconciliation;
- manifest movement invalidates scratch output;
- first-view base promotion and identical-manifest base sharing;
- exactness invalidation on import/update/delete and carry-forward only on identical manifests;
- delta precedence, pending tombstones, stale-delta exclusion, and pin-held old delta;
- CAS loser leaves no partial rows, binding, or store-log effect;
- resolve claim heartbeat, dead claimant takeover, interactive fairness, and terminal reconciliation;
- schema-v1 typed refusal before mutation;
- incremental-converged equals fresh exact store, and exported v3 artifact equals legacy extraction on
  semantic rows across every supported language;
- final G3b measurement against the actual store delta transaction.

## Doubt-Pass Reconciliation

The design received three read-only adversarial cycles from Claude. No external-model policy is
declared in the repository; repo content was sent to Anthropic under Razorback's default policy.

Surviving objections changed the design:

- the first cut lacked the real G3b numerator, so Ph2c-a now includes streaming diff and persisted
  delta/tombstone output;
- independent read/write ports could not express SQL anti-joins, so one deep `ResolutionSession`
  owns corpus and overlay semantics;
- off-lease computation, exact manifest membership, L2 completeness, status/path/language parity,
  target integrity, and deterministic order are explicit;
- the primary G3b denominator remains comparable to the original resolution phase;
- schema-v2 refusal, base version roots, heartbeat pumping, reader/resolve pins, and Ph2c-b's own
  crash/CAS gates are explicit.

The final cycle found no reason to return to a monolith or temporary-v3 adapter. Its remaining
findings are represented as hard requirements above.

## Out of Scope

- General GC, retention policies, demotion, capacity preflight, vacuum/reindex escalation, and
  generation promotion: Ph2d.
- Miller registry family selection, read sessions, sidecars, status/health/dashboard, governor
  wiring, and rollback orchestration: Ph3.
- Row-level incremental resolution scoping: separately justified resolver work; Ph2c convergence is
  whole-corpus and CAS-safe.
- New extraction semantics or language-specific resolver behavior.

## Acceptance Criteria

- [x] Ph2c-a lands as a revertible legacy refactor followed by a separately revertible scratch
  mechanism proof.
- [x] Legacy artifact resolution output remains pinned-oracle equivalent.
- [x] G1/G2 cover both resolution tables and persisted roundtrip, including disappearing rows.
- [x] Every G3a/G3b/G3c pair in all three runs passes the predeclared thresholds.
- [x] Ph2c-b re-runs G3b against actual store publication and passes.
- [x] Schema-v2 base/delta/pin state and recovery satisfy the frozen section-14 state machine.
- [x] Public resolve/export/from-artifact commands are real, idempotent, crash-safe, and reported.
- [x] Multi-language incremental, fresh, exported, and legacy semantic rows agree.
- [x] Ph2d and Ph3 boundaries remain intact.
