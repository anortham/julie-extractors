# SQLite Schema v7

Schema version 7 is the current SQLite artifact contract. Extraction contract 4
and JSONL contract 5 are its matching producer contracts.

The complete catalog authority is the normalized `sqlite_master` SHA-256 in
[`sqlite-schema-v7.catalog.sha256`](sqlite-schema-v7.catalog.sha256). The
conformance test creates the schema, orders catalog rows by `(type, name)`,
compacts SQL whitespace, hashes the complete catalog, and requires an exact
match.

v7 is a single-change delta over [v6](sqlite-schema-v6.md). Everything v6
defined except the resolution overlay is unchanged.

## Resolution overlay tables are removed

`pending_resolutions` and `identifier_resolutions`, plus
`idx_pending_resolutions_target` and `idx_identifier_resolutions_target`, are
**removed**. New artifacts do not create those objects. Identifier
`target_symbol_id` is not stored on the artifact; JSONL still emits the key
with a null value.

A leftover v6 artifact that still carries overlay rows opens for read unless
`--strict-schema` is set. Write access and `--strict-schema` refuse it with
`schema_migration_required`. Recovery is a whole-workspace `julie-extract scan`.

## `files` rows for unsupported paths

No DDL changes, so the catalog hash is unchanged. What changes is which rows a
scan writes: `files.status = 'unsupported'` is no longer reserved for evidence
that stale rows were removed. A scan now writes one such row per file the
discovery walk reached and dropped for an unsupported extension, carrying
`language = 'unsupported'`, the content hash, the byte count, and a null
`line_count`. Those files own no `symbols` rows or other fact rows.

`revision_file_changes` gains the matching coverage: `unsupported` on the first
scan that sees the path and on every later content change, `deleted` when the
path disappears. Ignored paths, hard-excluded paths, oversized source files,
and the artifact's own `-wal`/`-shm`/`-journal` companions stay out of the
artifact.

## Upgrading from v6

There is no in-place artifact migration. Rebuild the workspace to produce a
v7 catalog.
