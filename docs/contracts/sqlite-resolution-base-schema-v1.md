# SQLite Resolution Base Schema v1

> **Retired 2026-08-18.** julie-extract no longer writes resolution bases.
> Writer open reaps leftover `bases/` files. This page stays as historical
> authority. See
> [2026-08-18-resolution-write-path-retirement.md](../decisions/2026-08-18-resolution-write-path-retirement.md).

Status: retired. Historical Ph2c-a base-file authority only.

The production DDL is `julie_extract_artifact::store::RESOLUTION_BASE_SQL`. Base
files are immutable after completion and use `PRAGMA user_version = 1`. The
catalog authority hash uses the store convention: each non-internal
`sqlite_master` row is normalized as `type|name|tbl_name|compact_whitespace(sql)`,
ordered by `(type,name)`, joined with newlines, and hashed with SHA-256.

```text
resolution-base-catalog-sha256: 64ff476f9b72fd7e2ab2a642c26e223ffe8b47ea308c1ce4af6d51403c4b49f0
```

## Tables

`base_meta(key TEXT PRIMARY KEY CHECK(length(key) > 0), value TEXT NOT NULL)`
records `format_version`, `catalog_sha256`, `manifest_hash`,
`resolver_output_epoch`, sorted source/target `source_versions`, exact semantic row counts,
and `completed` (`0` while building, `1` only after validation).

`resolution_base_versions(version_id INTEGER PRIMARY KEY CHECK(version_id > 0))`
stores every caller-supplied source/target version named by a row in ascending semantic order;
builders refuse any persisted target whose version is absent from this root set.

`identifier_resolutions` is keyed by `(version_id, identifier_id)` and contains:

```text
version_id INTEGER NOT NULL
identifier_id TEXT NOT NULL
target_version_id INTEGER
target_symbol_id TEXT
tier INTEGER
confidence REAL
method TEXT
outcome TEXT NOT NULL
candidates INTEGER
```

`resolved` rows require both target columns; all other outcomes require both
target columns to be null. Target versions, tiers, candidates, and confidence
are positive or in their documented ranges; source and local IDs are positive
or non-empty; optional methods and non-null target symbols are non-empty.

Both source and nullable target version columns have deferred `NO ACTION`
foreign keys to `resolution_base_versions(version_id)`. Pending source and
target versions use the same deferred roots.

`pending_resolutions` is keyed by `(version_id, pending_relationship_id)` and
requires a positive target version, non-empty target symbol and method, positive
tier, and confidence in `[0,1]`.

Named indexes provide target lookup and deterministic export order:

```text
idx_read_resolution_identifiers_target
idx_export_resolution_identifiers_order
idx_read_resolution_pending_target
idx_export_resolution_pending_order
```

Builders sort both semantic row families by `(version_id, local_id)`, validate
all target pairs against the caller's manifest-visible symbol set, run foreign-key
and integrity checks, and readers rerun those checks before exposing rows.
Builders checkpoint WAL, and only then expose a completed reader.
