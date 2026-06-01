# v2.0.0 Version And Test-Role Contract Alignment Plan

**Goal:** Align the standalone product release target with the old Julie
extractor crate lineage and make test identification queryable in the new
artifact contracts.

**Context:** The old Julie in-tree extractor crate had reached v1.22.0. This
standalone product should therefore target v2.0.0 rather than publishing below
the old crate line. Separately, extractors already emit `is_test`,
`test_container`, and `test_lifecycle` metadata, but generic JSON metadata is
not enough for a performance-first SQLite contract.

## Scope

1. Set workspace crate versions, release workflow defaults, release docs, and
   active release notes to v2.0.0.
2. Keep artifact contract versions unchanged: SQLite schema v1, JSONL v1,
   report v1, and extraction contract v1.
3. Promote current extractor test-role booleans into first-class `symbols`
   columns with required indexes.
4. Preserve the same role keys in `metadata_json` and JSONL `record.metadata`
   for metadata-oriented consumers.
5. Expose the role booleans as explicit JSONL `symbol` payload fields.

## Non-Goals

- Do not add Julie test linkage, reference scoring, or test-quality analysis.
- Do not add a speculative `test_role` enum until the standalone extractor path
  actually emits one.
- Do not back-port this schema work into `/Users/murphy/source/julie`.

## Acceptance Criteria

- [x] Package metadata and release workflow defaults target v2.0.0.
- [x] SQLite `symbols` includes `is_test`, `test_container`, and
  `test_lifecycle` integer booleans.
- [x] SQLite has required indexes for those three columns.
- [x] CLI extraction maps existing extractor metadata into the first-class
  SQLite columns.
- [x] JSONL `symbol` records include explicit role booleans and preserve
  metadata keys when present.
- [x] Contract tests cover schema columns, indexes, JSONL payload keys, and CLI
  end-to-end preservation.

## Verification

- `cargo test -p julie-extract-artifact --test schema_contract`
- `cargo test -p julie-extract-artifact --test jsonl_contract`
- `cargo test -p julie-extract-cli --test operations_contract scan_promotes_test_role_metadata_to_indexed_sqlite_columns`
- Branch gates before merge: `cargo xtask test default` and
  `cargo xtask test contract`
