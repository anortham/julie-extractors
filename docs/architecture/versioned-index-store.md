# Versioned Index Store Architecture

The versioned store is a separate persistence boundary inside `julie-extract-artifact`. The legacy v3 `ArtifactWriter` continues to own standalone schema-7 artifacts; it does not open or mutate the family store.

> **2026-08-18 retirement:** julie-extract no longer writes workspace-global
> reference resolution. Family stores stay schema v2 and drop leftover
> resolution objects on writer open. Miller computes resolution at query time.
> See [2026-08-18-resolution-write-path-retirement.md](../decisions/2026-08-18-resolution-write-path-retirement.md).

## Database split

- `store.db` owns immutable file versions, extraction evidence, views/manifests, the append log, and chunk progress.
- `coord.db` owns queued requests and the time-boxed store-writer lease.
- The databases are independently creatable and use separate WALs. No foreign key crosses the database boundary.

This split lets request ownership and recovery survive store-generation replacement without coupling coordinator heartbeats to the store writer's transaction.

## Write model

One coordinator holds the optional `store-writer` lease. A request is claimed with an owner and heartbeat, then writes idempotent chunks. Each chunk records the log sequence of its durable effect. The final store transaction writes the request's single terminal log row; a later coordinator transaction records that sequence and result on the request.

After a crash, a successor distinguishes three cases:

1. A terminal log row exists: reconcile the coordinator row without repeating the effect.
2. Only chunk rows exist: resume after the highest committed global chunk index.
3. No progress exists: execute from the beginning.

The log and progress tables intentionally do not reference retained versions or prunable log rows.

## Version and view model

A file version is immutable and identified by path, content hash, and extraction epoch. A never-reused integer `version_id` qualifies every local extraction ID. Completeness stamps publish L1, L2, and L3 in order.

A manifest generation maps each view path to a retained version or a classified failure. `views.current_generation` is the publication pointer. Readers therefore see one coherent generation, while historical manifests remain GC roots through restrictive version references.

## Delete model

Only deletion of a `file_versions` row cascades into extraction children. Child-to-child references never cascade; they are deferred so a whole version can be purged in one transaction without turning individual evidence deletion into a recursive erase path. Read-aligned indexes optimize candidate recall; GC-aligned indexes put `version_id` first for cohort deletion and later reclamation.

## Epoch boundary

The store format epoch and extraction identity epoch are independent. The initial value of each is 1. A same-epoch extractor comparison must be byte-identical. A changed extraction result is accepted only when the extraction epoch increases and the existing compatibility ledger classifies the change.

## Retired resolution model

Ph2c used to store exact resolution output in immutable SQLite base files. That
write path is retired. Writer open drops leftover base, delta, pin, and scope
objects and reaps `bases/` plus both scratch families. View `resolution_*`
columns stay for migrated stores and are not bound by this product.

## Compatibility adapters

`store export` builds a standalone schema-v7 artifact from the current view's
fact tables. It does not require a resolution binding. `store import --from-artifact`
validates a complete current artifact before it creates a family or enqueues work,
then imports extraction rows and manifest state through resumable store
transactions. Neither adapter copies a SQLite database file, and ordinary
imports still run extraction from source.

## Lifecycle and generations

Ph2d adds bounded lifecycle maintenance under one root-owned maintenance intent. The planner treats
current and historical manifests, active
requests/claims, receipts, and consumer cursors as explicit roots before deleting or demoting any
version, scratch file, or log row. L3 is demoted before L2; whole immutable versions are
purged only after every root is gone. Checkpoint, incremental vacuum, and truncate-checkpoint are
ordered and restartable.

Large repair, promotion, and rollback work builds a new `gen-NNN` directory, validates every catalog
and owned file, fsyncs it, then atomically replaces `CURRENT`. Readers remain generation-local and
retained generations stay valid until their pins and safety window expire. Rollback is forward-built:
it selects historical visible state while preserving the latest immutable identities, logs, receipts,
and cursors in a newly named generation.

`coord.db` remains outside generations. Its family allocator marks cover file-version and store-log
identities globally plus manifest generations per view. Ordinary request
progress and terminal reconciliation advance those marks monotonically; promotion and rollback scan
all named generations and receipts before raising destination allocators. No published identity is
reused after a generation transition.

The public lifecycle surface is `store maintain inspect|gc|repair|promote` plus `cursor
advance|release`. Inspection is read-only and every mutation requires `--apply`. Forward rollback is
an artifact API used by an orchestrator; it is deliberately not an end-user CLI verb.

v2.31.0 completed Julie's producer-side Ph2 store program. v2.31.1–v2.31.3 are patch releases for
physical maintenance, capacity safety, and concurrent multi-worktree fencing. Miller integration
remains Ph3 work and is not implied by those releases.

## Concurrent fencing (post-Ph2d hardening)

Concurrent import, update, delete, and maintain against one family store share `coord.db` and generation
local `store.db` files. Fencing rules below keep foreign writers off a frozen source and keep every
durable store mutation lease- and generation-checked.

### Maintenance intent as lease authority

A live foreign `maintenance_intent` blocks ordinary `store-writer` lease acquire even when no
`writer_lease` row exists. Promote and repair may release the writer lease for a long generation
build while the intent stays live; ordinary writers still refuse.

Maintenance ownership is explicit. Ordinary `try_acquire_or_takeover` never bypasses intent.
Maintenance uses `try_acquire_for_maintenance` with a full identity fence
(`run_id` + `owner_id` + `owner_pid` + `fencing_token`) that must match the live intent row.
Matching holder id or PID alone does not admit a writer under foreign intent. Expired intents and
dead-owner takeover still follow the existing PID and expiry policy.

Enqueue of a new request, consumer cursor advance, and cursor release recheck foreign
live intent inside the same `BEGIN IMMEDIATE` transaction that would write. Pre-transaction checks
alone are not enough. Idempotent enqueue replay of an existing request may return without insert.

### Temporary writer floor and intent mirrors

During acquire, maintenance writes `coord.db` intent and lease first (M1), then raises the serving
source `min_writer_version` and writes `maintenance_tmp_*` intent-mirror keys under a maintenance
fence (M2), then may release only the writer lease for the long build (M3).

Promotion materializes the destination with the pre-maintenance `min_writer_version` and never
copies temporary raised floors or `maintenance_tmp_*` keys as permanent destination state (M5).
Finish and abort restore a still-serving source floor and clear mirrors under the maintenance fence
before they delete lease and intent in `coord.db` (M7). Clearing intent first while a serving source
still holds a temporary raised floor is a defect. A retired source skips restore; the published
destination already holds the pre-maintenance floor from M5.

### Maintenance apply capacity

Maintenance apply re-probes live free bytes immediately before the first mutative GC cohort, scratch
purge batch, and generation staging create. Plan-time free-byte samples are not frozen for the whole
apply. Writer open reaps leftover `bases/` contents and retired resolve/scratch files.

### Follow-ups intentionally deferred

This
hardening does not claim cross-database atomicity between `coord.db` and `store.db`; recovery follows
the ordered multi-step state machines above.
