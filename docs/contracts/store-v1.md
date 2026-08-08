# Versioned Index Store v1

Status: frozen Ph2c contract. The exact physical catalog is
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

The writer atomically binds `family_id`, `extraction_identity_epoch`, `min_reader_version`, `min_writer_version`, `created_by_version`, and monotonic `binary_version` when it creates or adopts a family.

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

`store_log.sequence` is the sole monotonic `AUTOINCREMENT` allocator. Each row has a non-empty request and event kind, optional view/generation/version/level coordinates, a checked terminal flag, a JSON payload, and canonical creation time. A partial unique index permits one terminal row per request.

`request_chunks` records global non-negative chunk indexes and the unique store-log sequence owned by each chunk. It deliberately has no FK to the prunable log, versions, or manifests. Ph2b does not prune `store_log`.

## Coordinator

`coord.db` is independently creatable and contains only `requests` and the optional singleton `writer_lease`.

- Request kinds are `import`, `update`, `delete`, `resolve`, `export`, or `from_artifact`.
- Request states are `queued`, `claimed`, `committed`, `acknowledged`, or `failed`.
- Only claimed requests may carry claim owner and heartbeat fields.
- Committed and acknowledged requests require a terminal log sequence and result with no error.
- Failed requests require an error, prohibit a result, and may lack a terminal sequence.
- The idempotency key is unique through its named classified index.
- A partial unique index permits at most one claimed `resolve` request per family coordinator.
- Coordinator clocks are Unix-millisecond integers.
- The lease resource is exactly `store-writer`; holder identity/version are non-empty, PID and fencing token are positive, and release deletes the row.

`store.db` also catalogs immutable resolution bases, rooted source versions, cumulative per-view
deltas, and bounded reader/resolve pins. Semantic result rows remain in immutable base and delta
files; they are not copied into general Store tables.
