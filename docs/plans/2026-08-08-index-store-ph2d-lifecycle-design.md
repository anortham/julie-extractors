> **Superseded 2026-08-18.** The resolution write path is retired. See [2026-08-18-resolution-write-path-retirement.md](../decisions/2026-08-18-resolution-write-path-retirement.md).

# Index Store Ph2d Lifecycle Completion Design

**Status:** Approved 2026-08-08; ready for implementation planning.

**Goal:** Finish the unreleased Julie family-store lifecycle with safe retention, garbage collection,
repair, capacity control, immutable generation promotion, mixed-version guarantees, release
preparation, and downstream Miller pin validation.

**Architecture risk:** High. This phase deletes durable data, gates concurrent writers, changes the
served generation pointer, and establishes the recovery contract future Miller readers depend on.

## Context and Phase Boundary

Ph2b delivered the versioned extraction store, durable request coordinator, and import/update/delete
commands. Ph2c delivered manifest-scoped resolution, immutable resolution bases, cumulative deltas,
reader/resolve pins, v3 export, and import from a legacy artifact. Both slices are unreleased and
Miller does not use the family store yet.

Ph2d is the final Julie-owned lifecycle slice. It owns:

- retention planning, L3-before-L2 demotion, whole-version purge, base/delta/scratch cleanup, and
  safe log pruning;
- capacity preflight and typed refusal before an operation can strand the family without rollback
  space;
- integrity diagnosis, bounded repair escalation, and immutable store-generation promotion;
- full mixed-version, crash/recovery, reachability, disk-pressure, and real-repository gates;
- Julie release preparation and local Miller pin validation.

Ph3 remains Miller work: registry family resolution, admission/governor wiring, read sessions,
sidecars, status/health/dashboard, and rollback orchestration. Ph2d supplies the process and on-disk
contracts Ph3 consumes; it does not add Miller types or paths to Rust.

## Chosen Approach

Add one Julie-owned maintenance namespace:

```text
julie-extract store maintain inspect --store <family-dir> [--family <uuid>] [--json]
julie-extract store maintain gc      --store <family-dir> [--family <uuid>] [--apply] [--json]
julie-extract store maintain repair  --store <family-dir> [--family <uuid>] [--apply] [--json]
julie-extract store maintain promote --store <family-dir> [--family <uuid>] [--apply] [--json]
```

`inspect` is always read-only. The other actions plan and report by default; `--apply` is required
for mutation. Promotion may select a previously validated retained generation for explicit rollback,
but it never silently rolls back after an application-visible success.

The public CLI remains thin. `julie-extract-artifact::store` owns a pure planner, a fenced executor,
generation construction/publication, capacity accounting, integrity classification, and stable error
types. The CLI validates arguments and renders a separate maintenance report; it does not issue SQL,
delete files, or manipulate `CURRENT` directly.

## Rejected Approaches

### Direct maintenance SQL in CLI modules

Rejected because it duplicates root logic across commands, bypasses writer fencing, and makes a
future Miller caller depend on CLI implementation details rather than one durable contract.

### Automatic garbage collection during ordinary writes

Rejected because extraction latency would inherit unbounded cleanup work, a failed import could be
confused with capacity reclamation, and background deletion would be difficult to diagnose or replay.
Ordinary writes may run capacity preflight and return an actionable maintenance recommendation; they
do not silently launch general GC.

### In-place full `VACUUM`, migration, or repair of the serving generation

Rejected because a crash can strand the only serving copy, readers may hold the current inode, and
there is no atomic rollback boundary. Large rebuilds and schema transformations use generation
promotion. Only bounded, transactionally safe GC mutations and SQLite checkpoint/incremental-vacuum
steps may touch the current generation.

## Architecture Quality

### Modules and responsibilities

- `store/maintenance.rs` owns policy, reachability, typed plans, capacity facts, and bounded apply
  steps. The planner is deterministic and has no filesystem mutation side effects.
- `store/generation.rs` owns maintenance intent, generation naming, staged construction, validation,
  `CURRENT` publication, retained-generation discovery, and rollback selection.
- Existing `store/layout.rs` remains the path-containment authority and gains only the primitives
  needed to create/open a named staging generation and publish a validated name. Its creation-time
  scaffold reaper becomes ownership-aware and refuses to create an empty store when `CURRENT` is
  missing but a named generation already exists. Opening an existing generation becomes purely
  validating; schema creation and `user_version` writes run only on exclusive initialization.
- Existing `store/coordinator.rs` remains the writer-lease authority and refuses ordinary writer
  acquisition while a live maintenance intent exists. Its raw store transaction path uses the same
  generation and maintenance fence as every other writer.
- Existing `store/resolution.rs` supplies base, delta, binding, and pin roots through a bounded query
  interface; general policy does not live in the resolution module.
- `store/maintenance.rs` in the CLI owns nested arguments and report translation. Existing import,
  update, delete, resolve, and export modules do not gain cleanup branches.
- `store/maintenance_report.rs` owns a separate version-1 JSON/human report. Request-oriented
  `StoreReport` remains byte-compatible.

### Caller-facing interface

One `StoreMaintenance` facade consumes a `StoreConnectionFactory`, coordinator path, clock,
filesystem-capacity provider, and policy. Its public operations are `inspect`, `plan_gc`, `apply_gc`,
`plan_repair`, `apply_repair`, `plan_promotion`, and `apply_promotion`. Tests inject time, capacity,
PID liveness, and crash boundaries.

### Main architecture risk

The dangerous failure is a stale plan deleting an object that became reachable through another
database after planning. Read-only planning therefore uses a double-read coherence protocol across
`coord.db` and `store.db`; apply acquires maintenance ownership and the store-writer lease, rereads
all mutable roots, and rejects any plan whose root fingerprint changed. No cross-database atomicity is
claimed.

## Maintenance Ownership

Long generation construction cannot hold one SQLite transaction or rely on a five-second writer
lease. Ph2d uses a root-owned durable maintenance intent in `coord.db`, mirrored into unrestricted
`store_meta` keys in the source generation:

- run ID, action, source generation, owner PID, heartbeat, started time, and plan fingerprint;
- the current binary records the source compatibility floor, then raises only the frozen source's
  `min_writer_version` to the maintenance binary before releasing the initial lease;
- compatible writers check the root-owned intent and bound generation before work, while binaries
  too old to understand the intent are blocked from the frozen source by its raised floor;
- the owner heartbeats between bounded steps; a successor may resume only after expiry or proven
  process death;
- completion or explicit abort clears the intent in the serving generation; promotion copies the
  completed intent history but publishes the destination with no live intent.

Every store mutator, including base binding, pin lifecycle, superseded-delta cleanup, and ordinary
import/update/delete/resolve publication, checks maintenance ownership before beginning. Maintenance
batches additionally bind their plan to the maintenance run ID, the coordinator fencing token, the
largest observed `store_log.sequence`, and the coordinator request watermark. A batch rechecks those
facts immediately before commit. Once intent is published, coordinator enqueue and claim operations
refuse or wait; apply then rereads `coord.db` to close the check-before-intent race. A successor never
continues a stale owner's transaction.

`StoreCoordinator` records the generation identity resolved from `CURRENT` when constructed. Before
every raw store transaction it reopens through `StoreConnectionFactory` and verifies the generation
is still `serving`, still matches `CURRENT`, and has no foreign live maintenance intent. Claim,
progress, terminal, manifest, resolution, and cleanup paths cannot bypass this check. An already-open
handle to a `retired` generation may finish reads but cannot begin another write.

`StoreConnectionFactory::open_writer` is split so opening a connection is not itself a mutation.
Identity, floors, generation state, and maintenance intent are checked before write capability is
returned. Monotonic `binary_version` advancement becomes an explicit lease-held transaction and
refuses a live foreign maintenance intent. No caller can mutate the supposedly frozen source merely
by opening a pin, binding, or cleanup writer.

`StoreLayout::create` likewise separates initialization from existing-generation validation.
Validation opens query-only and never calls `create_store_schema`, changes `user_version`, or writes
page one. Only the no-`CURRENT`, no-generation, no-partial exclusive path initializes schema.

Acquisition order is fixed: coordinator writer lease, then store transaction, then root-owned
filesystem operations. No code takes those resources in reverse order. Read-only `inspect` takes no
lease and reports when its view raced rather than fabricating a stable plan. A live partial generation
contains an ownership record; `StoreLayout::create` and startup recovery reap it only after the
maintenance intent is absent or expired and its owner is dead.

`StoreLayout::create` may initialize `gen-001` only when no `CURRENT`, no named generation, and no
live partial generation exists. If `CURRENT` is missing while `gen-*` directories exist, it returns a
typed recovery-required error and never publishes a fresh empty store over the evidence.

## Reachability and Retention

### Protected roots

An object is protected while reachable from any of these:

- every current manifest entry, including `failed_preserved`, at every level already complete on its
  version;
- every retained historical manifest entry until that manifest is itself retired, protecting version
  identity and L1;
- every `resolution_base_versions` row belonging to a ready or live building base, protecting L1 and
  L2;
- every identifier-delta source version and target version, protecting L2 identifiers and their L1
  evidence;
- every pending-delta source version and target version, protecting L2 pending evidence plus the L1
  symbols, relationships, and reference sites needed to interpret it;
- every current view base/delta binding;
- every unexpired reader or resolve pin in its own generation;
- every coordinator request row in any state until request-retention removes it, including a
  store-terminal/coordinator-uncommitted tear;
- every live request-owned scratch/base file;
- every registered durable consumer cursor and its safety window;
- the current generation and any retained rollback generation still inside its safety window.

The planner emits a level-qualified reason set for every protected object. Apply treats an unknown or
malformed root as blocking, never reclaimable. Demotion uses those level roots; purge uses the union
of all level roots.

### Historical manifest and version policy

The existing defaults keep their exact meanings:

- `retention_window_days = 7`: manifest creation/index times are the age authority because
  `file_versions` deliberately has no timestamp. An unpinned historical manifest is not eligible
  before seven complete days have elapsed;
- `retention_path_cap = 24`: after protected roots, retain the newest 24 historical manifest-backed
  versions for one path. The seven-day safety window outranks the count cap, so more than 24 may be
  retained during rapid churn; the ceiling produces a typed pressure result rather than overriding
  the time window;
- `retention_byte_target = 1.20`: normal GC reclaims oldest eligible history until retained logical
  bytes are at or below 1.20 times the logical bytes required by protected current state;
- `retention_byte_ceiling = 1.25`: a projected write above 1.25 times protected current logical bytes
  must complete eligible GC or fail with a typed capacity result before the write begins.

Roots override all four values. If protected state alone exceeds the target or ceiling, maintenance
reports pressure and deletes nothing protected.

### Ordered reclamation

Each apply pass performs bounded, restartable steps in this order:

1. reap expired pins and request-private scratch whose owner is terminal or provably dead;
2. remove superseded unpinned deltas;
3. retire eligible unbound bases and their `resolution_base_versions` roots, then remove their
   verified files;
4. retire eligible historical manifests;
5. demote retained but deep-unprotected versions by deleting L3 before L2 in one deferred-FK
   transaction and clearing completion stamps coherently. One cohort contains at most 100 versions
   and at most 64 MiB of conservatively estimated dirty pages; apply stops at whichever bound is
   reached first, checkpoints, and resumes from the durable version cursor;
6. delete wholly unrooted `file_versions`, relying only on the version-owned cascades;
7. delete coordinator requests that are terminal, beyond their safety window, below every consumer
   watermark, and no longer needed for idempotency observation; commit and fsync `coord.db`;
8. delete now-orphaned `request_chunks` and `store_log` rows whose request IDs are absent from the
   live `requests` table, have immutable receipts, are below the safe consumer watermark, and are
   outside the reconciliation safety window;
9. checkpoint, run bounded `PRAGMA incremental_vacuum(N)` only when freelist thresholds justify it,
   then run `wal_checkpoint(TRUNCATE)`. Capacity planning includes the vacuum step's WAL growth. Full
   in-place `VACUUM` remains forbidden.

Filesystem deletion follows catalog commit only for files whose absence is recoverable. Catalog
deletion follows file removal only where a catalog row would otherwise falsely promise a ready file.
Every asymmetric boundary has a recovery rule and crash test.

## Durable Receipts and Consumer Watermarks

Ph2d amends the unreleased coordinator catalog with two generation-independent tables:

- compact immutable request receipts preserve unique request ID, idempotency key, kind, canonical
  payload identity, terminal result, terminal generation identity, terminal sequence, and completion
  time after a full request row ages out;
- durable consumer cursors preserve bounded consumer ID, generation identity, last consumed
  `store_log.sequence`, and update time.

Idempotency lookup checks live requests first and receipts second. A replay of a pruned request still
returns its original result, while a conflicting kind or payload still returns the stable conflict.
Enqueue also rejects a request ID already present in receipts, so an orphan terminal row can never
commit an unrelated reused ID.
Receipt creation and terminal-request deletion share one coordinator transaction and fsync before
the corresponding store log can be pruned.

The maintenance CLI exposes narrow cursor advance and release operations for Ph3's process-level
integration. Sequence values are monotonic and never move backward. A malformed cursor, missing
generation identity, or cursor ahead of the family allocator watermark blocks pruning. With no
registered consumers, log pruning still respects coordinator reconciliation, durable idempotency
receipts, and the configured safety window. Cross-database pruning is deliberately ordered
coordinator-first, then store: a crash may leave harmless orphan log rows, never a coordinator row
whose terminal fact has already disappeared.

Rollback is a forward operation, not a direct `CURRENT` flip to an older database. It builds a new
generation from the latest serving generation's immutable request/log history and the selected
retained generation's visible manifests, bases, bindings, and completion state. Consequently every
committed request still has its terminal row, durable cursors remain meaningful, and sequence space
never moves backward. Receipts store the complete immutable terminal result as well as its original
store-generation identity; replay reports that historical result even when it is no longer current.

Older Julie binaries do not prune logs and therefore cannot violate this contract. Both tables live
in root-owned `coord.db`, outside generation promotion and rollback.

## Capacity Preflight

Capacity is measured from filesystem facts, SQLite page/freelist/WAL facts, base and scratch file
sizes, and conservative per-operation estimates. Logical retention ratios and physical free-space
requirements are reported separately.

Promotion requires enough free bytes to hold the staged destination plus its WAL/checkpoint headroom
while the source generation remains intact for rollback. GC requires transaction/WAL headroom before
deleting. If the conservative requirement is not met, the command returns `capacity_insufficient`
before creating a partial generation or deleting catalog rows.

Writer connections set and read back a bounded `journal_size_limit` selected by the capacity policy;
the default limit is 256 MiB. Checkpoint results and remaining WAL bytes are part of the plan, so
`secure_delete=ON` bulk-delete amplification cannot be omitted from the estimate.

`journal_size_limit` is post-checkpoint retention policy, not a transaction-growth bound. Capacity
preflight separately reserves the worst-case WAL bytes for one 100-version/64-MiB demotion cohort,
SQLite page overhead, and checkpoint headroom. Apply refuses before opening the cohort transaction
when that physical space is unavailable.

No estimate is reported as exact. The report includes measured bytes, conservative required bytes,
free bytes, protected bytes, eligible bytes, target bytes, and ceiling bytes.

## Repair and Escalation

Inspection classifies failures into:

- catalog compatibility or family identity refusal;
- SQLite quick/integrity/foreign-key failure;
- manifest hash, current-pointer, completion-stamp, or log/chunk inconsistency;
- base/delta/pin tuple inconsistency;
- base file absence, identity mismatch, or orphan;
- recoverable WAL/index/free-page pressure;
- unrecoverable loss of immutable evidence.

Repair escalates conservatively:

1. bounded WAL checkpoint and readback;
2. recovery of already-defined torn base/scratch states;
3. bounded GC and incremental vacuum when catalog integrity is already valid;
4. generation rebuild for index reconstruction, compaction, schema transformation, or any repair
   that would otherwise rewrite the serving generation;
5. typed `repair_unavailable` when immutable source evidence cannot produce a valid replacement.

Ph2d never invents missing extraction or resolution rows. A repair either proves a derived rebuild
from retained immutable facts or refuses.

## Generation Promotion

Promotion is an off-lease build with a fenced short publish:

1. acquire the writer lease, validate quiescent mutable roots, record the source compatibility floor,
   raise only the frozen source's writer floor, and record the maintenance intent and source identity;
2. release the lease; compatible writers refuse while the intent is live and older writers are below
   the floor;
3. create the next `.gen-NNN.partial` under the family root, initialize the target catalog, and stream
   source rows in deterministic primary-key windows without a long-lived read transaction. Copy
   `store_meta` forward rather than reseeding it: family identity and retention values are preserved.
   The destination receives the pre-maintenance compatibility writer floor, not the temporary source
   fence; reader/writer floors, binary version, and extraction epoch move upward only for a genuine
   compatibility or schema requirement;
4. copy or rebuild base files into the destination, verifying every recorded size and SHA-256;
   generation-local pins are not copied—old readers renew in the old generation and new readers pin
   the new one;
5. run catalog fingerprint, exact/monotonic metadata validation, `quick_check`, `integrity_check`,
   `foreign_key_check`, manifest, resolution target, row-count, and file-identity validation;
6. checkpoint, close, fsync files and directories, then rename the partial directory to `gen-NNN`;
7. reacquire the writer lease, revalidate source generation and maintenance ownership, mark the
   source generation `retired`, atomically replace `CURRENT.partial` with `CURRENT`, fsync the family
   directory, mark the destination `serving`, clear the root-owned live intent, and release. Recovery
   treats a crash between those durable states as an unfinished promotion, never as permission for
   ordinary writes;
8. retain the previous generation until the pins recorded in that generation and its rollback safety
   window expire. Retained-generation cleanup opens every candidate generation and reads its own pin
   table; the new generation is never treated as authority for old-reader pins.

`coord.db` (including receipts and cursor registry), spool, and request-private scratch remain outside
generations. Generation names are allocated monotonically with checked overflow and are never reused.
Explicit row IDs are preserved during logical copy. Root-owned coordinator metadata records a typed
family allocator map for `file_versions.version_id`, `store_log.sequence`, per-view
`manifests.generation`, and per-view `resolution_deltas.delta_generation`. Allocation updates the
applicable mark transactionally rather than recomputing only `MAX()+1` from the serving generation.
Before every promotion or rollback, marks advance to the maxima across all named generations and
durable receipts; destination `sqlite_sequence` values and explicit per-view allocator rows are raised
to those family marks before publication. These identities therefore never restart or collide. Startup reaps only
unpublished partial scaffolding owned by a dead or expired maintenance run; it never removes a named
generation merely because `CURRENT` does not reference it.

Explicit rollback validates the retained generation with the current reader contract and verifies no
active writer/claim conflicts, but never republishes that database directly. It constructs a new
monotonically named generation by preserving the latest serving generation's immutable identities,
request logs, receipts, and allocator state while selecting the retained generation's visible state.
The result passes the same validation and publication protocol as promotion. It is a forward-built
logical rollback and reports both the selected historical generation and the newly serving one.

## Reports and Exit Behavior

`StoreMaintenanceReport` is a separate version-1 report so existing request-oriented `StoreReport`
JSON and human output remain byte-compatible. It includes:

- action, plan/apply mode, run ID, family ID, source/destination generation, and disposition;
- plan fingerprint and root fingerprint;
- protected/eligible/retired/demoted/purged counts by object type;
- logical retention and physical capacity facts;
- integrity checks, escalation selected, recovery actions, and crash-resume disposition;
- stable failure class and error object.

JSON success and failure remain one line on stdout with diagnostics absent from stderr. Human success
uses stdout; human failure uses stderr. Exit meanings remain 0 success/no-change, 1 operational
failure, 2 usage, and 3 incompatible store.

## Mixed-Version Behavior

- A binary below `min_reader_version` cannot inspect or read.
- A binary below `min_writer_version` cannot acquire the writer lease or mutate.
- The downgrade escape may bypass only the monotonic binary-version displacement rule; it never
  bypasses reader/writer floors, schema, format epoch, family, catalog hash, integrity, or maintenance
  ownership.
- Promotion raises only the frozen source's writer floor before off-lease construction so an older
  process cannot mutate it. The destination keeps the pre-maintenance compatibility floor unless its
  actual schema or format requires a higher one, so maintenance does not permanently evict otherwise
  compatible Miller installations.
- A newer compatible reader may continue using an already-open old generation; deletion waits for
  pins/safety and remains cross-platform safe.
- A same-version competing maintainer observes the durable intent and either resumes after stale/dead
  ownership or reports busy. It never starts another promotion for the same source generation.
- Pin creation and renewal are capped at one hour, matching the existing resolve/export policy. A
  caller cannot create an arbitrarily long retention root. The artifact API enforces the cap rather
  than relying on CLI convention, and SQLite `now` inside the deletion transaction is the single pin
  expiry authority.

## Pre-Implementation Catalog Amendment

The unreleased schema-v2 catalogs are amended once before maintenance behavior lands:

- add `version_id`-leading GC indexes for `resolution_identifier_deltas` and
  `resolution_pending_deltas`; without them, one whole-version delete scans both full delta tables;
- add coordinator request receipts and consumer cursors with exact state, monotonicity, and
  idempotency constraints;
- add coordinator family-allocator rows for never-reused version/log marks and per-view
  manifest/delta generation marks;
- add any maintenance guard triggers required by the final executor shape.

The same amendment freezes `generation_state = serving|retired` metadata semantics, the one-hour pin
TTL, and the `journal_size_limit` writer pragma/readback contract.

Regenerate both catalog hashes once and update checked-in schema and query-plan contracts together.
Existing Ph2c dogfood catalogs are unreleased fixtures and remain typed-incompatible; production does
not perform an in-place coordinator migration. Do not drip catalog changes across later tasks.

## Security and Path Safety

All generation, base, scratch, cursor, and partial paths are resolved beneath the canonical family
root. Symlinks, traversal, wrong file types, hard-link collisions with live databases, NUL/colon path
forms, and non-owned existing targets are rejected before mutation. Consumer IDs never become raw
filenames. Reports do not expose source content, environment secrets, or arbitrary SQLite errors.

No new dependency is required by this design. Repository instructions declare no external-model
policy; the design doubt pass may send these repository contracts to Anthropic and records that fact.

## Verification Strategy

### Fast contracts

- pure randomized reachability model compared with SQLite planner output;
- exact root-reason and stale-plan fingerprint tests;
- retention window/target/ceiling/path-cap boundaries;
- L3-before-L2 demotion and whole-version-only purge;
- manifest/base/delta/pin/request/claim/cursor root matrices;
- capacity estimation and no-mutation refusals;
- maintenance ownership, takeover, floor, and lock-order tests;
- direct coordinator transaction, existing-layout validation, retired-handle, and old-binary fencing
  tests proving there is no bypass around maintenance ownership;
- report JSON/human snapshots and existing store-report byte compatibility.

### Crash and filesystem contracts

- hard kill before/after every catalog/file boundary for GC, base cleanup, staged generation,
  directory rename, `CURRENT` replacement, intent clearing, and rollback;
- reopen validates both databases and every retained generation after each boundary;
- no duplicate generation, request terminal, cursor regression, or lost current pointer;
- Windows/macOS/Linux path, rename, open-reader, and retained-generation behavior.

### Scale and release gates

- multi-language family with two views, shared versions, failed and failed-preserved entries, ready
  bases, cumulative deltas, pins, and historical churn;
- bounded memory and transaction/WAL growth while reclaiming and promoting a Miller-scale store;
- exact 100-version/64-MiB cohort boundaries and capacity refusal before an oversized WAL transaction;
- promotion natural-row equivalence across the current manifests, 14 child tables, fingerprint-global
  tables, resolution bases/deltas, logs, and coordinator convergence;
- full mixed-version matrix using real old/current binaries;
- default, contract, crash, scale, Clippy, formatting, diff, secrets, and dependency gates declared
  by the implementation plan;
- disposable-repository dogfood, release package preflight, and downloaded-asset verification.

## Release Boundary

Ph2d prepares the next Julie Extractors release, expected to be v2.31.0 unless live release metadata
changes before execution. Version changes, release notes, package manifests, compatibility fixtures,
and local Miller pin validation are part of the implementation plan.

Pushing commits, tagging, publishing GitHub assets, and changing Miller's live pin remain separate
approval boundaries. Ph3 production wiring begins only after the Julie release assets and checksums
are independently verified.

## Acceptance Criteria

- [ ] Read-only inspection names every root, reclaimable object, capacity fact, and integrity issue
      without mutation.
- [ ] Apply revalidates roots under maintenance ownership and refuses a stale plan.
- [ ] No current/historical manifest, ready-base root, live pin/binding, active request/claim, live
      scratch owner, consumer cursor, or retained rollback generation is accidentally reclaimed.
- [ ] Root reasons are level-qualified; base, identifier-delta, and pending-delta source/target roots
      prevent L2 demotion as well as whole-version purge.
- [ ] Demotion is L3 before L2; physical extraction deletion occurs only through whole-version purge.
- [ ] Retention defaults have the exact frozen meanings defined above and roots always override them.
- [ ] Log pruning is below every durable consumer watermark and outside request reconciliation safety.
- [ ] Capacity refusal occurs before mutation and promotion always leaves a validated rollback copy.
- [ ] Repair never fabricates immutable evidence or rewrites the only serving generation in place.
- [ ] Promotion and rollback publish only through an fsynced atomic `CURRENT` replacement.
- [ ] Promotion and forward-built rollback preserve monotonic metadata plus family-wide version/log
      and per-view manifest/delta allocator marks; retired generations refuse writers and
      generation-local pins protect their own files.
- [ ] Request receipts prevent both idempotency-key and request-ID reuse after request/log pruning.
- [ ] Missing `CURRENT` with an existing named generation refuses recovery rather than creating an
      empty `gen-001`.
- [ ] Crash recovery, mixed-version, bounded-memory/WAL, equivalence, and multi-language gates pass.
- [ ] Existing v3 artifact commands and store import/update/delete/resolve/export contracts remain
      compatible.
- [ ] Release preparation is complete, but no push, tag, publication, or live Miller pin change occurs
      without explicit approval.

## Doubt-Pass Reconciliation

The design received a bounded, read-only adversarial review from Claude because its architecture risk
is high. The repository declares no external-model policy; repository contracts were sent to
Anthropic under Razorback's default policy and contained no secrets or customer data.

Verified objections incorporated into this design:

- the existing scaffold reaper deletes any matching partial generation without checking ownership;
- the writer lease alone does not fence every store mutator, and a stale plan needs store/log/request
  watermarks plus a durable maintenance intent;
- every coordinator request row roots terminal reconciliation until it is replaced by a durable
  idempotency receipt, so request pruning must commit before log pruning;
- file versions lack timestamps, making manifest/index times the retention age authority and requiring
  an explicit seven-day-over-count-cap precedence rule;
- both resolution delta tables need `version_id`-leading GC indexes before bounded cohort deletion;
- pins are generation-local and retained-generation cleanup must inspect the old generation rather
  than trusting the new one;
- logical generation copy must preserve SQLite allocator high-water marks;
- incremental vacuum is the intended bounded in-place reclamation mechanism and should precede a
  generation rebuild;
- pin TTL, caller clock, WAL size, and missing-`CURRENT` recovery require explicit bounds/refusals.

The second cycle verified those corrections and found six additional load-bearing gaps, also folded
into the design: receipts must reserve request IDs as well as idempotency keys; rollback must use
family-wide allocator marks; promotion must copy compatibility metadata monotonically; opening a
writer must stop mutating `binary_version` before ownership checks; resolution roots must protect
specific extraction levels; and serving/retired generation state must fence processes that opened the
old generation before `CURRENT` moved. It also corrected pin-clock authority, incremental-vacuum WAL
ordering, the `.gen-NNN.partial` spelling, and the exact missing-`CURRENT` creation path.

The final cycle challenged rollback and the actual coordinator write path. It found that directly
republishing an old generation would strand committed coordinator rows, receipts, and monotonic
consumer cursors; that manifest and resolution-delta generations also need family allocator marks;
that the coordinator bypasses the ordinary writer-opening path; that existing-layout validation
currently writes schema state; that a maintenance fence must not permanently raise the destination's
compatibility floor; that pending-delta roots were missing; and that one demotion transaction lacked
an explicit WAL bound. The design now makes rollback a forward generation build, covers all four
allocator families, fences coordinator transactions and retired handles, makes existing opens
query-only, separates the source maintenance floor from destination compatibility, adds pending-delta
roots, and caps each demotion cohort at 100 versions or 64 MiB of estimated dirty pages.

A bounded confirmation pass then returned clean on those seven corrections: terminal history remains
available across rollback, all allocator families are monotonic, raw coordinator writes and existing
layout opens are fenced, the destination floor remains compatible, pending roots are complete, prune
ordering is unambiguous, and demotion transactions have an explicit physical bound.

Two reviewer preferences did not change the design. Flat `store gc|repair|promote|inspect` verbs were
rejected because the approved `store maintain` namespace keeps one lifecycle boundary and avoids
further top-level command growth. Reusing or bumping request-oriented `StoreReport` was rejected
because a separate maintenance report preserves its byte contract and avoids fake view/request data.

The review called consumer cursors speculative, but the frozen Ph2b contract explicitly requires all
sidecar cursors plus a safety window to pass before terminal-log pruning. Ph2d therefore freezes a
generic coordinator cursor contract; Ph3 owns the Miller sidecar that advances it.
