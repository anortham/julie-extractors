# Versioned Index Store v1

Status: frozen Ph2d lifecycle contract. The exact physical catalog is
[`sqlite-store-schema-v2.md`](sqlite-store-schema-v2.md).

This contract defines the target-owned family store used after the legacy v3 artifact boundary. It does not change the v3 `ArtifactWriter`, SQLite schema version 7, or the standalone extraction artifact.

> **2026-08-18 retirement:** julie-extract no longer writes workspace-global
> reference resolution. The former resolve verb, bases, deltas, pins, and
> scope journal are gone. Writer open drops those objects in place. View
> `resolution_*` columns stay for migrated stores; this product does not bind
> them. Miller computes resolution at query time. See
> [2026-08-18-resolution-write-path-retirement.md](../decisions/2026-08-18-resolution-write-path-retirement.md).

## Identity and compatibility

- `STORE_SQLITE_SCHEMA_VERSION = 2` identifies the physical `store.db` and `coord.db` catalogs.
- `STORE_FORMAT_EPOCH = 1` identifies the store generation format.
- `EXTRACTION_IDENTITY_EPOCH = 8` participates in file-version identity.
- File-version identity is `(path, content_hash, extraction_epoch)`; extracted output bytes are not identity inputs.
- Byte-identical extractor output may remain in the same extraction epoch. Any output difference requires a strictly newer epoch and a classified extraction-output ledger entry.
- Both databases record `PRAGMA user_version = 2`. An uninitialized database may be created at version 2. Schema-v1 files are not migrated in place and return a typed older-schema refusal before mutation; a newer version also returns a typed refusal.
- Reader registration requires writer version `2.40.0` or newer. Maintenance permanently raises the serving generation's `min_writer_version` under its existing coordinator fence before reader admission can succeed. Version 2.39.0 and older refuse mutating maintenance with `incompatible_store` before changing coordinator or store state.

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

`coord.db.reader_registrations` records one immutable owner and manifest snapshot per live reader.
The row references the existing `store.db` manifest root; no file-version roots are copied into the
coordinator. Only `heartbeat_at` and `expires_at` may change, and heartbeat cannot regress. `owner_birth_identity` stays internal
to the producer and is not part of the CLI report.

Reader-floor activation uses the maintenance intent's `source_min_writer_version` as the permanent
floor while the fence is held. It installs reader objects only when that floor is below `2.40.0`,
the entire reader catalog is absent, and no registration row exists. The catalog installation and
permanent-floor update commit together. Once the permanent floor is reader-capable, missing,
partial, or malformed reader objects fail closed and are never recreated as an empty catalog.

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
and canonical creation/update times. The `resolution_state`, `resolution_base_id`,
`resolution_delta_generation`, and `resolution_exact_at` columns remain so a migrated
store keeps its prior values. New views stay `unbound`. This product does not bind
or consume those columns.

`manifests` is immutable and keyed by `(view_id, generation)`. `manifest_entries` is keyed by `(view_id, generation, path)`, records a required language that participates in manifest hash v2, and records one of:

- `indexed`: a version is present and error fields are null.
- `failed_preserved`: a prior version is present and both error fields are present.
- `failed`: no version is present and both error fields are present.

A manifest entry's nullable version FK is `ON DELETE RESTRICT`; live and historical manifests are GC roots. Publication inserts the manifest before changing `views.current_generation` in one transaction.

A view ends its life only when a caller names it. `store maintain retire-view --view <id> --apply`
deletes that view's manifest entries, its manifests, and its `views` row in one store transaction,
so no orphan manifest can survive to fail a later maintenance root check. Retirement is never
inferred from a missing root: the root of a dead workspace is already gone, so its absence proves
nothing. The verb keeps every family allocator mark, store-log row, request receipt, and consumer
cursor, which is what stops a later view from reusing a retired identity. The versions the view
released are not deleted here; they become ordinary GC candidates for a later `gc --apply` run.

Re-publishing the current generation is a no-op. The retired scope journal is no
longer written; writer open drops leftover journal tables.

## Durable log and progress

`store_log.sequence` is the store catalog's monotonic `AUTOINCREMENT` log allocator. Each row has a non-empty request and event kind, optional view/generation/version/level coordinates, a checked terminal flag, a JSON payload, and canonical creation time. A partial unique index permits one terminal row per request. Root-owned family allocator marks prevent file-version, log, and per-view manifest identities from restarting after promotion or forward rollback.

`request_chunks` records global non-negative chunk indexes and the unique store-log sequence owned by each chunk. It deliberately has no FK to the prunable log, versions, or manifests. Ph2b does not prune `store_log`.

## Coordinator

`coord.db` is independently creatable and contains live `requests`, the optional singleton
`writer_lease`, immutable `request_receipts`, durable `consumer_cursors`, the optional singleton
`maintenance_intent`, and scoped `family_allocator_marks`.

- Request kinds that can be enqueued are `import`, `update`, `delete`, `export`, or `from_artifact`.
- Historical `resolve` rows still parse. The coordinator cannot enqueue or claim them.
  A writer-open reaper moves leftover queued or claimed resolve rows to typed `failed`.
- Request states are `queued`, `claimed`, `committed`, `acknowledged`, or `failed`.
- Only claimed requests may carry claim owner and heartbeat fields.
- Committed and acknowledged requests require a terminal log sequence and result with no error.
- Failed requests require an error, prohibit a result, and may lack a terminal sequence.
- A requester identity of the form `cli-<pid>` names the requesting process, and a claim owner of
  the same form names the claiming process. A drain reaps, right after it takes the writer lease,
  every queued row whose requester process is dead and every claimed row whose requester and claim
  owner are both dead, and only after the row's `requester_deadline` has expired. Crash resume is
  a designed path: a successor process may adopt and complete a queued request whose submitter died
  while the submitter's window remains, so a dead pid alone never reaps a row, and a NULL deadline
  never expires. Reaped rows become typed `failed` with the `coordinator_requester_dead`
  error token. Maintenance runs the same reap before it refuses a run as `busy`. A live claim owner
  is executing the request and is never reaped; an identity without a `cli-<pid>` pid is never
  probed.
- A request row counts its quantum overruns in `quantum_overruns`. A kind that may not renew its
  writer lease is requeued on an overrun for the first two, and failed with the typed
  `coordinator_quantum` error on the third, so one request whose work can never fit the quantum
  stops starving every request queued behind it.
- The idempotency key is unique through its named classified index.
- Coordinator clocks are Unix-millisecond integers.
- The lease resource is exactly `store-writer`; holder identity/version are non-empty, PID and fencing token are positive, and release deletes the row.
- Ordinary writer lease acquire refuses while a foreign live maintenance intent exists, even when no
  `writer_lease` row is present. Maintenance ownership is explicit
  (`run_id` + `owner_id` + `owner_pid` + `fencing_token`) and is not inferred from holder id/PID alone.
- Enqueue of a new request, consumer cursor advance, and cursor release recheck
  foreign live intent inside the same IMMEDIATE coordinator transaction that would mutate. Idempotent
  enqueue replay of an existing request may return without insert under live intent.
- A terminal request may age into one immutable receipt that independently reserves its request ID
  and idempotency key and preserves the original terminal result and generation identity.
- Consumer cursor sequence/time and family allocator high-water/time values cannot regress.
- The maintenance intent is resource `store-maintenance`, carries one coherent owner/heartbeat/fence,
  and blocks ordinary writer acquisition while live.

`store.db` does not catalog resolution bases, deltas, or pins. Writer open
drops leftover `bases/` files and both scratch families.

## Public compatibility adapters

- `store export --store <family> --view <id> --out <artifact.db>` exports the
  current view's fact tables into a standalone schema-v7 artifact. It does not
  require a resolution binding. It holds one read transaction from manifest
  selection through every fact-table copy, has no coordinator request controls,
  and never mutates store state.
- `store import --from-artifact <artifact.db>` is an import mode, not a database copy. It rejects
  scan/extraction controls, validates the artifact schema/root/hash/epoch/completeness before store
  creation, enqueues one bounded typed plan, and resumes by request-global chunk index. It does not
  materialize resolution state.
- Both adapters preserve natural extraction keys. Idempotency replay returns the
  original request-specific result even when the source or artifact path later disappears.

## Retention boundary

Ph2d may reclaim only objects outside every current/historical manifest,
active request/claim, scratch owner, consumer cursor window, registered reader root, and
retained-generation safety window. A reader registration in `coord.db` binds one immutable
generation/view/manifest snapshot. While retained, it protects the exact manifest hash, every
completed extraction level reachable from that manifest, failed entries, its physical generation,
and log rows at or above `min_retained_store_log_sequence`. The reader log floor is inclusive;
consumer cursors retain their existing acknowledged-watermark semantics.
`store maintain retire-view` is the one path that removes manifest roots themselves; it removes
them for one named view and leaves the versions they held to ordinary reclaim. Retirement refuses
while any retained reader names that view, including readers bound to non-current generations.
Terminal request rows become durable receipts before coordinator deletion; orphan store logs are
pruned only afterward and below every safe cursor. Committed and acknowledged rows older than the
request safety window are archived up to the durable log high-water mark, not up to the safe
consumer cursor, so a lagging consumer cannot pin them in `requests` forever. Failed rows of that
age carry no result to preserve and are deleted outright, in the same bounded batch size as
receipts; the apply report counts them. Queued and claimed rows are never pruned. General maintenance behavior is specified by the
Ph2d lifecycle design and is implemented behind `store maintain`.

## Lifecycle maintenance interface

The published CLI exposes lifecycle work only under `store maintain`. `inspect` is read-only.
`gc`, `repair`, `promote`, `retire-view`, and consumer-cursor mutations require `--apply`; without it
they return a pure plan and do not modify `store.db`, `coord.db`, generation files, or `CURRENT`.

Every mutation validates the inspected plan fingerprint, store and coordinator root fingerprints,
family, serving generation, capacity, maintenance intent, writer lease, and fencing token before
its first write. Apply re-probes live free bytes immediately before the first mutative GC cohort,
scratch purge, and generation staging create rather than freezing plan-time free space for the whole
run. A concurrent root change is `stale_plan`, not an implicit replan. Insufficient promotion
headroom is `capacity_insufficient` before intent or lease acquisition. A live writer or
maintenance owner is `busy`.

Reader expiry only triggers process-instance qualification. Unexpired, alive, paused, or
identity-unknown owners remain protected; only definitive same-domain process death permits
maintenance to remove a registration. Reader rows are revalidated under the existing maintenance
intent before destructive work. Whole-generation cleanup completes before that intent is released,
and every SQLite handle into a candidate generation is closed before filesystem deletion.

GC preserves every root named by this contract and uses receipts before request/log pruning.
Promotion builds a sibling generation, validates catalogs and identities, fsyncs the staged files,
and replaces `CURRENT` only after the generation is ready. Repair may checkpoint a valid current
generation, select one unambiguous valid named generation after a torn publication, or rebuild
under the frozen generation policy. Ambiguous or unselectable state is reported honestly as
`recovery_required` or `repair_unavailable`; repair never fabricates catalog state. Retire-view
takes the same fence and the same plan validation, then commits its three deletes together. A view
the store does not hold is `invalid_arguments` with code `view_not_found`; a queued or claimed
request that names the view is `busy`.

Consumer cursor advance is monotonic, cannot pass the durable log high-water mark, and remains
bound to the serving generation. Release removes only the exact consumer row. Consumer IDs are
validated identifiers and are never interpolated into paths.

Maintenance has its own JSON/human report schema, `StoreMaintenanceReport` version 1. It is not a
request and does not add request IDs, view IDs, or request-state fields to the request-oriented
StoreReport. JSON always uses one stdout line, including failures; human failure uses stderr. Exit
codes are 0 completed/no-change, 1 operational refusal, 2 usage, and 3 incompatible store.

The retired scope journal is not an additive feature. Writer open deletes
`resolution_scope_journal_version` and drops leftover journal tables. A store
without those objects is the current catalog.
