# Versioned Index Store Architecture

The versioned store is a separate persistence boundary inside `julie-extract-artifact`. The legacy v3 `ArtifactWriter` continues to own standalone schema-6 artifacts; it does not open or mutate the family store.

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

## Resolution model

Ph2c stores exact resolution output in immutable SQLite base files under the family generation.
Each base is keyed by manifest hash and resolver-output epoch, roots every source version it names,
and becomes reusable only after its file identity and semantic row counts are durable. A view binds
to the nearest ready base, computes outside the writer lease, and publishes one cumulative delta in
a short fenced `store.db` transaction. The transaction copies replacement/tombstone rows, advances
the view to `exact`, and appends the `resolution_exact_published` effect atomically.

Reader and resolver pins hold one coherent `(view, manifest, base, delta)` tuple. Manifest movement
invalidates exactness but does not mutate old bases or deltas. A crash before the exact publication
commit leaves no visible delta; a crash after it is reconciled from the store log without repeating
the effect. Private scratch databases are request-owned and may be removed by the successor of that
same durable request.

## Compatibility adapters

`store export` pins one exact generation and builds a standalone v3 artifact through the legacy
writer contract. `store import --from-artifact` validates a complete current v3 artifact before it
creates a family or enqueues work, then imports extraction rows, manifest state, and exact resolution
through resumable store transactions. Neither adapter copies a SQLite database file, and ordinary
imports still run extraction from source.

Ph2d owns retention, garbage collection, repair, and generation promotion. In particular it must
respect historical manifests, ready-base version roots, live pins, current view bindings, and active
claims before deleting versions, bases, deltas, scratch files, or log rows.
