# SQLite Schema v8

Schema version 8 is the current SQLite artifact contract. Extraction contract 4
is its matching producer contract.

The complete catalog authority is the normalized `sqlite_master` SHA-256 in
[`sqlite-schema-v8.catalog.sha256`](sqlite-schema-v8.catalog.sha256). The
conformance test creates the schema, orders catalog rows by `(type, name)`,
compacts SQL whitespace, hashes the complete catalog, and requires an exact
match.

v8 is a single-change delta over [v7](sqlite-schema-v7.md). Everything v7
defined is unchanged except the column below.

## `identifiers.reference_site_id` may be null

```sql
reference_site_id TEXT,
```

The column was `TEXT NOT NULL`. The foreign key to `reference_sites` stays, and
a null value satisfies it.

At the `references` level, an identifier whose site no written relationship or
pending relationship shares has a null `reference_site_id`, and the artifact has
no `reference_sites` row for it. The identifier's own columns still hold its
path, span, and containing symbol. The `full` and `facts` levels write the same
rows as v7.

A reader that joins `identifiers` to `reference_sites` must use a left join, or
read the span from `identifiers`.

A v7 artifact opens for read unless `--strict-schema` is set. Write access and
`--strict-schema` refuse it with `schema_migration_required`, including
`scan --force`. Re-extract into a new database path with
`julie-extract scan --root <project> --db <new-v8.db>`. Close readers before
replacing the old artifact with the completed v8 artifact.
