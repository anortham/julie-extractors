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

## Upgrading from v6

There is no in-place artifact migration. Rebuild the workspace to produce a
v7 catalog.
