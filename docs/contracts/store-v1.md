# Versioned Index Store v1

Status: frozen Ph2d lifecycle contract. The exact physical catalog is
[`sqlite-store-schema-v2.md`](sqlite-store-schema-v2.md).

This contract defines the target-owned family store used after the legacy v3 artifact boundary. It does not change the v3 `ArtifactWriter`, SQLite schema version 6, or the standalone extraction artifact.

## Identity and compatibility

- `STORE_SQLITE_SCHEMA_VERSION = 2` identifies the physical `store.db` and `coord.db` catalogs.
- `STORE_FORMAT_EPOCH = 1` identifies the store generation format.
- `EXTRACTION_IDENTITY_EPOCH = 1` participates in file-version identity.
- File-version identity is `(path, content_hash, extraction_epoch)`; extracted output bytes are not identity inputs.
- Byte-identical extractor output may remain in the same extraction epoch. Any output difference requires a strictly newer epoch and a classified extraction-output ledger entry.
- Both databases record `PRAGMA user_version = 2`. An uninitialized database may be created at version 2. Schema-v1 files are not migrated in place and return a typed older-schema refusal before mutation; a newer version also returns a typed refusal.

## Store metadata

`store_meta` accepts unrestricted non-empty text keys with non-null text values. Schema creation seeds only:

| Key | Value |
|---|---:|
| `store_sqlite_schema_version` | `2` |
| `store_format_epoch` | `1` |
| `retention_window_days` | `7` |
| `retention_byte_target` | `1.20` |
| `retention_byte_ceiling` | `1.25` |
| `retention_path_cap` | `24` |
| `generation_state` | `serving` |

The writer atomically binds `family_id`, `extraction_identity_epoch`, `min_reader_version`, `min_writer_version`, `created_by_version`, and monotonic `binary_version` when it creates or adopts a family.

Keys with the reserved prefix `maintenance_tmp_` are temporary intent-mirror metadata written only
on a frozen source generation while maintenance is live. They must never remain on a published
destination generation. Promotion copies restore `min_writer_version` to the pre-maintenance value
recorded on `maintenance_intent.source_min_writer_version` rather than the temporary raised source
floor.

## Immutable versions

`file_versions` allocates a never-reused integer `version_id` and stores `path`, `content_hash`, `extraction_epoch`, `language`, `content_bytes`, nullable `line_count`, nullable `metadata_json`, and nullable `complete_l1`, `complete_l2`, and `complete_l3` log stamps.

- Counts are non-negative.
- A completeness stamp is null or positive.
- L2 completeness requires L1 completeness; L3 requires L2.
- All 14 extraction child tables key rows by `(version_id, local_id)`.
- Every child directly cascades from `file_versions(version_id)`.
- Every child-to-child and self reference is version-qualified, deferred, and `ON DELETE NO ACTION`.
- Whole-version purge is the only cascading physical delete path.
- Fingerprint-global rows are keyed by extraction epoch.

`reference_sites.level` is `1` or `2`. The version-qualified identity guard keeps the first payload for a duplicate `(version_id, reference_site_id)` and ignores a later conflicting payload. Its comparison includes all legacy compared fields plus `level`.

## Views and manifests

Each `views` row names a non-empty `view_id` and root, an optional current manifest generation,
canonical creation/update times, and a coherent resolution state. `unbound` has no binding;
`converging` binds a ready base and cumulative delta without an exact generation; `exact` requires
`resolution_exact_at = current_generation`.

`manifests` is immutable and keyed by `(view_id, generation)`. `manifest_entries` is keyed by `(view_id, generation, path)`, records a required language that participates in manifest hash v2, and records one of:

- `indexed`: a version is present and error fields are null.
- `failed_preserved`: a prior version is present and both error fields are present.
- `failed`: no version is present and both error fields are present.

A manifest entry's nullable version FK is `ON DELETE RESTRICT`; live and historical manifests are GC roots. Publication inserts the manifest before changing `views.current_generation` in one transaction.

## Durable log and progress

`store_log.sequence` is the store catalog's monotonic `AUTOINCREMENT` log allocator. Each row has a non-empty request and event kind, optional view/generation/version/level coordinates, a checked terminal flag, a JSON payload, and canonical creation time. A partial unique index permits one terminal row per request. Root-owned family allocator marks prevent file-version, log, per-view manifest, and per-view resolution-delta identities from restarting after promotion or forward rollback.

`request_chunks` records global non-negative chunk indexes and the unique store-log sequence owned by each chunk. It deliberately has no FK to the prunable log, versions, or manifests. Ph2b does not prune `store_log`.

## Coordinator

`coord.db` is independently creatable and contains live `requests`, the optional singleton
`writer_lease`, immutable `request_receipts`, durable `consumer_cursors`, the optional singleton
`maintenance_intent`, and scoped `family_allocator_marks`.

- Request kinds are `import`, `update`, `delete`, `resolve`, `export`, or `from_artifact`.
- Request states are `queued`, `claimed`, `committed`, `acknowledged`, or `failed`.
- Only claimed requests may carry claim owner and heartbeat fields.
- Committed and acknowledged requests require a terminal log sequence and result with no error.
- Failed requests require an error, prohibit a result, and may lack a terminal sequence.
- The idempotency key is unique through its named classified index.
- A partial unique index permits at most one claimed `resolve` request per family coordinator.
- Coordinator clocks are Unix-millisecond integers.
- The lease resource is exactly `store-writer`; holder identity/version are non-empty, PID and fencing token are positive, and release deletes the row.
- Ordinary writer lease acquire refuses while a foreign live maintenance intent exists, even when no
  `writer_lease` row is present. Maintenance ownership is explicit
  (`run_id` + `owner_id` + `owner_pid` + `fencing_token`) and is not inferred from holder id/PID alone.
- Enqueue of a new request, resolve claim, consumer cursor advance, and cursor release recheck
  foreign live intent inside the same IMMEDIATE coordinator transaction that would mutate. Idempotent
  enqueue replay of an existing request may return without insert under live intent.
- A terminal request may age into one immutable receipt that independently reserves its request ID
  and idempotency key and preserves the original terminal result and generation identity.
- Consumer cursor sequence/time and family allocator high-water/time values cannot regress.
- The maintenance intent is resource `store-maintenance`, carries one coherent owner/heartbeat/fence,
  and blocks ordinary writer acquisition while live.

`store.db` also catalogs immutable resolution bases, rooted source versions, cumulative per-view
deltas, and bounded reader/resolve pins. Semantic result rows remain in immutable base and delta
files; they are not copied into general Store tables.

## Resolution publication

- `store resolve` claims through `coord.db` without holding the store-writer lease during semantic
  computation.
- Durable resolve writes to `store.db` (exact publish and terminal log append) use a generation-fenced
  writer; unfenced raw opens are not a production path.
- The input manifest, resolver-output epoch, ready base, request claim, writer holder PID, and
  fencing token are revalidated before and inside the final publication transaction.
- Exact publication heartbeats the writer lease once immediately before `BEGIN IMMEDIATE`, revalidates
  lease ownership against wall clock, inserts one cumulative delta, all replacement/tombstone rows,
  the view binding, and one `resolution_exact_published` log effect in the same `store.db` transaction.
- A CAS or fence loser publishes none of those rows. A store-committed/coordinator-uncommitted tear
  reconciles from the terminal store fact without re-executing semantic work.
- Resolve pins release on success and best-effort on failure while ownership still allows release.
  Expired pins are not base-protection roots; only unexpired pins or existing delta rows protect a
  ready base.
- Import materializes resolution bases with catalog `building` before file publish and CAS
  `building → ready` only after file identity and semantic counts match.
- Only the dedicated resolver may claim `resolve` requests. Generic import/update/delete backlog
  draining leaves them queued while another resolve is claimed.
- Request-private exact/base scratch files are not catalog data. A successor that owns the same
  durable request removes an abandoned private `.work` file before retrying.

## Public compatibility adapters

- `store export --store <family> --view <id> --out <artifact.db>` requires an exact current view,
  holds a reader pin through validation and atomic rename, and emits the current standalone v3
  artifact contract. It has no coordinator request controls and never mutates store state.
- `store import --from-artifact <artifact.db>` is an import mode, not a database copy. It rejects
  scan/extraction controls, validates the artifact schema/root/hash/epoch/completeness before store
  creation, enqueues one bounded typed plan, and resumes by request-global chunk index.
- Both adapters preserve natural extraction and resolution keys. Idempotency replay returns the
  original request-specific result even when the source or artifact path later disappears.

## Retention boundary

Ph2d may reclaim only objects outside every current/historical manifest, ready-base version root,
identifier and pending delta source/target root, unexpired resolution pin, current base/delta binding,
active request/claim, scratch owner, consumer cursor window, and retained-generation safety window.
Terminal request rows become durable receipts before coordinator deletion; orphan store logs are
pruned only afterward and below every safe cursor. General maintenance behavior is specified by the
Ph2d lifecycle design and is implemented behind `store maintain`.

## Lifecycle maintenance interface

The unreleased CLI exposes lifecycle work only under `store maintain`. `inspect` is read-only.
`gc`, `repair`, `promote`, and consumer-cursor mutations require `--apply`; without it they return a
pure plan and do not modify `store.db`, `coord.db`, generation files, or `CURRENT`.

Every mutation validates the inspected plan fingerprint, store and coordinator root fingerprints,
family, serving generation, capacity, maintenance intent, writer lease, and fencing token before
its first write. Apply re-probes live free bytes immediately before the first mutative GC cohort,
scratch purge, and generation staging create rather than freezing plan-time free space for the whole
run. A concurrent root change is `stale_plan`, not an implicit replan. Insufficient promotion
headroom is `capacity_insufficient` before intent or lease acquisition. A live writer or
maintenance owner is `busy`.

GC preserves every root named by this contract and uses receipts before request/log pruning.
Promotion builds a sibling generation, validates catalogs and identities, fsyncs the staged files,
and replaces `CURRENT` only after the generation is ready. Repair may checkpoint a valid current
generation, select one unambiguous valid named generation after a torn publication, or rebuild
under the frozen generation policy. Ambiguous or unselectable state is reported honestly as
`recovery_required` or `repair_unavailable`; repair never fabricates catalog state.

Consumer cursor advance is monotonic, cannot pass the durable log high-water mark, and remains
bound to the serving generation. Release removes only the exact consumer row. Consumer IDs are
validated identifiers and are never interpolated into paths.

Maintenance has its own JSON/human report schema, `StoreMaintenanceReport` version 1. It is not a
request and does not add request IDs, view IDs, or request-state fields to the request-oriented
StoreReport. JSON always uses one stdout line, including failures; human failure uses stderr. Exit
codes are 0 completed/no-change, 1 operational refusal, 2 usage, and 3 incompatible store.
