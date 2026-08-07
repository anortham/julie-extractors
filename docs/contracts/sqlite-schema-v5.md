# SQLite Schema v5

> **Superseded by [SQLite Schema v6](sqlite-schema-v6.md).** v6 removes the denormalized
> `identifiers.target_symbol_id` column and the `idx_identifiers_target` index; everything else on
> this page still holds. A v6 binary refuses a v5 artifact with `schema_migration_required`.

Schema version 5 was the current SQLite artifact contract through julie-extract 2.29.0. Extraction
contract 4 and JSONL contract 4 are its matching producer contracts.

The complete catalog authority is the normalized `sqlite_master` SHA-256 in
[`sqlite-schema-v5.catalog.sha256`](sqlite-schema-v5.catalog.sha256). The conformance test creates
the schema, orders catalog rows by `(type, name)`, compacts SQL whitespace, hashes the complete
catalog, and requires an exact match. This prevents handwritten DDL excerpts from becoming a
second schema.

## Canonical reference sites

```sql
CREATE TABLE reference_sites (
    reference_site_id TEXT PRIMARY KEY,
    file_id TEXT NOT NULL,
    path TEXT NOT NULL,
    language TEXT NOT NULL,
    containing_symbol_id TEXT,
    start_line INTEGER,
    start_column INTEGER,
    end_line INTEGER,
    end_column INTEGER,
    start_byte INTEGER,
    end_byte INTEGER,
    is_exact INTEGER NOT NULL,
    provenance TEXT NOT NULL,
    FOREIGN KEY (file_id) REFERENCES files(file_id) ON DELETE CASCADE,
    FOREIGN KEY (containing_symbol_id) REFERENCES symbols(symbol_id) ON DELETE SET NULL,
    CHECK (length(reference_site_id) > 0),
    CHECK (is_exact IN (0, 1)),
    CHECK (
        (is_exact = 1
         AND start_line IS NOT NULL AND start_column IS NOT NULL
         AND end_line IS NOT NULL AND end_column IS NOT NULL
         AND start_byte IS NOT NULL AND end_byte IS NOT NULL)
        OR
        (is_exact = 0
         AND start_line IS NULL AND start_column IS NULL
         AND end_line IS NULL AND end_column IS NULL
         AND start_byte IS NULL AND end_byte IS NULL)
    ),
    CHECK (
        (is_exact = 1 AND provenance = 'target_token')
        OR (is_exact = 0 AND provenance = 'spanless')
    )
);
```

`identifiers`, `relationships`, and `pending_relationships` each have a required
`reference_site_id` foreign key to this table. An exact site ID is the stable hash of
`(file_id, start_byte, end_byte)`. A spanless site ID is the stable hash of
`(file_id, row_specific_id)` and is explicitly non-exact.

Producer attestation is required for `target_token`. A call expression, type expression, root node,
line number, nearest token, or overlapping identifier is not an exact site. Unattested providers
emit a row-specific spanless site.

## Capability gap status

`language_capability_gaps.status` accepts exactly `open` or `exception`. There are no aliases or
version ranges.
