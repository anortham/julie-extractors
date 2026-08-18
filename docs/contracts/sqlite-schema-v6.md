# SQLite Schema v6

> **Retired 2026-08-18 by [SQLite Schema v7](sqlite-schema-v7.md).** v7 removes
> `pending_resolutions` and `identifier_resolutions`. This page stays as the
> historical v6 authority. A v7 binary writing an artifact uses schema 7. A
> v7 reader with `--strict-schema` refuses a leftover v6 artifact with
> `schema_migration_required`; recovery is a whole-workspace `scan`.

Schema version 6 was the current SQLite artifact contract through the resolution
write-path era. Extraction contract 4 is its matching producer contract. JSONL
moved to contract 5 independently.

The complete catalog authority is the normalized `sqlite_master` SHA-256 in
[`sqlite-schema-v6.catalog.sha256`](sqlite-schema-v6.catalog.sha256). The conformance test creates
the schema, orders catalog rows by `(type, name)`, compacts SQL whitespace, hashes the complete
catalog, and requires an exact match. This prevents handwritten DDL excerpts from becoming a
second schema.

v6 is a single-change delta over [v5](sqlite-schema-v5.md). Everything v5 defined — canonical
reference sites, the capability-gap status vocabulary, and every other table — is unchanged.

## Resolution outcomes live only in the resolution overlay

The denormalized `identifiers.target_symbol_id` column, its
`FOREIGN KEY (target_symbol_id) REFERENCES symbols(symbol_id) ON DELETE SET NULL`, and the
`idx_identifiers_target` index are **removed**.

```sql
CREATE TABLE identifiers (
    identifier_id TEXT PRIMARY KEY,
    reference_site_id TEXT NOT NULL,
    file_id TEXT NOT NULL,
    path TEXT NOT NULL,
    language TEXT NOT NULL,
    name TEXT NOT NULL,
    kind TEXT NOT NULL,
    containing_symbol_id TEXT,
    start_line INTEGER NOT NULL,
    start_column INTEGER NOT NULL,
    end_line INTEGER NOT NULL,
    end_column INTEGER NOT NULL,
    start_byte INTEGER NOT NULL,
    end_byte INTEGER NOT NULL,
    confidence REAL NOT NULL,
    code_context TEXT,
    metadata_json TEXT,
    FOREIGN KEY (reference_site_id) REFERENCES reference_sites(reference_site_id) ON DELETE CASCADE,
    FOREIGN KEY (file_id) REFERENCES files(file_id) ON DELETE CASCADE,
    FOREIGN KEY (containing_symbol_id) REFERENCES symbols(symbol_id) ON DELETE SET NULL
);
```

`identifier_resolutions.target_symbol_id` is now the **single source of truth** for an identifier's
resolution outcome. Under v4 and v5 the same value was stored twice and kept in lockstep by the
resolution storage primitives; two copies of one fact can drift, and only one of them carried the
provenance (tier, confidence, method, outcome, candidate count). The overlay carried every value the
denormalized column held, so removing the column loses no fact.

Consumers read a resolved identifier target by joining the overlay:

```sql
SELECT i.identifier_id, r.target_symbol_id, r.tier, r.confidence, r.method, r.outcome
FROM identifiers i
LEFT JOIN identifier_resolutions r ON r.identifier_id = i.identifier_id;
```

An identifier with no `identifier_resolutions` row was never attempted. A row whose
`target_symbol_id` is `NULL` was attempted and did not resolve — the
[outcome vocabulary](sqlite-schema-v4.md#outcome-vocabulary) distinguishes `ambiguous`, `missing`,
and `no_context`. The `CHECK ((outcome = 'resolved') = (target_symbol_id IS NOT NULL))` on the
overlay still enforces that coherence, and `ON DELETE CASCADE` on the target still reverts an
identifier to never-attempted when its target symbol dies.

The extraction pass writes no resolution at all: it binds `containing_symbol_id` for symbols the
written file owns and leaves every resolution outcome to the resolution pass. No resolution
provenance is written to `identifiers.metadata_json`.

The JSONL `identifier` record is unchanged: it still carries a `target_symbol_id` key with the same
values, now read from the overlay through the join above. JSONL contract 4 therefore does not bump.

## Upgrading from v5

There is no migration engine and none is planned. A v6 binary reading a v5 artifact under
`--strict-schema` is **rejected** at preflight with a `schema_migration_required` report code and
exit `3`; the recovery is a whole-workspace `julie-extract scan`, which writes a fresh v6 artifact.
Miller's extractor-upgrade rescan machinery already consumes that refusal.

Because a v5 artifact is never opened by a v6 binary for writing, `idx_identifiers_target` cannot
survive into a v6 artifact, and `drop_retired_secondary_indexes` deliberately does **not** list it.
That helper exists for indexes retired *without* a schema-version bump, where a live artifact has to
shed them on its next open.
