# SQLite Resolution Scratch Delta Schema v1

> **Retired 2026-08-18.** julie-extract no longer writes resolution deltas or
> scratch files. Writer open reaps leftover scratch families. This page stays
> as historical authority. See
> [2026-08-18-resolution-write-path-retirement.md](../decisions/2026-08-18-resolution-write-path-retirement.md).

Status: retired. Historical Ph2c-a scratch-file authority only.

The production DDL is `julie_extract_artifact::store::RESOLUTION_SCRATCH_SQL`.
Scratch files use `PRAGMA user_version = 1`, are caller-owned contained paths,
and are never accepted while incomplete. Catalog hashes use the same normalized
`sqlite_master` SHA-256 algorithm as the base and store catalogs.

```text
resolution-scratch-catalog-sha256: fc6008182618ade70633393118e2d26cf8596ed9737564f841a101d1eaf25f32
```

`delta_meta(key TEXT PRIMARY KEY CHECK(length(key) > 0), value TEXT NOT NULL)`
records format/catalog identity, manifest hash, resolver output epoch, exact
replacement/tombstone counts, and `completed`.

`identifier_replacements` has the same natural key and payload checks as the
base `identifier_resolutions` table, including positive source versions and
non-empty local IDs and target symbols. `pending_replacements` has the same
key and required payload as `pending_resolutions`; tombstones require positive
source versions and non-empty relationship IDs. A pending replacement and
tombstone for the same key are rejected in one scratch file.

Named indexes provide target lookup and deterministic export order:

```text
idx_read_resolution_identifier_replacements_target
idx_export_resolution_identifier_replacements_order
idx_read_resolution_pending_replacements_target
idx_export_resolution_pending_replacements_order
idx_export_resolution_pending_tombstones_order
```

Rows are sorted by `(version_id, local_id)` before persistence. A reader opens
the file read-only, reruns SQLite integrity checks, requires completed metadata,
verifies catalog and row counts, and rejects escaped, symlinked, malformed, or
incomplete paths.
