# Schema Principles

SQLite is the primary product artifact. JSONL is an export and streaming format
derived from the same canonical data.

## Goals

- Stable enough for downstream readers to pin.
- Clean enough to serve extractor users rather than Julie internals.
- Explicit about versions and capability evidence.
- Friendly to incremental update/delete operations.
- Usable from any language with SQLite support.
- Fast for both write-heavy extraction and read-heavy downstream lookup.

## Required Domains

Initial schema design should cover:

- files
- symbols
- identifiers
- relationships
- structured pending relationships
- type facts
- type arguments
- annotations
- literals
- parse diagnostics
- language capabilities
- artifact metadata
- extraction revisions

## Metadata Requirements

Every database must expose:

- `schema_version`
- `extract_contract_version`
- `binary_version`
- `root_path`
- `workspace_id` or artifact id if needed
- `hash_algorithm`
- parser inventory
- capability snapshot fingerprint
- created/updated timestamps

## Performance Requirements

Performance is a product requirement, not an implementation afterthought.

The schema and writer must support:

- full-repo scan without per-row transactions
- single-file update/delete by indexed path
- bulk replacement of one file's rows by indexed file id
- downstream lookup by file path, symbol name/kind, parent symbol, identifier
  target, relationship endpoints, pending target name, and test-role flags
- deterministic export without table scans that depend on incidental row order

The SQLite writer should use explicit transactions, prepared statements, batched
inserts, and stable deletion order. Any staging tables or deferred secondary
index creation used for force rebuild are implementation details, but the final
artifact must contain the contracted indexes.

Performance tests should start small and fast: tiny fixtures can catch missing
indexes, per-row commits, and accidental table scans before real-world corpus
gates exist.

## JSONL

JSONL records should be:

- one record per logical row/event
- schema-versioned
- stable in field names
- easy to consume without SQLite
- exportable from SQLite

JSONL should not become a second source of truth.

## Migration Position

Because Julie, Miller, and Eros will continue using their existing paths while
this repo matures, the first schema should be clean. Do not maintain old Julie
compatibility mode as a starting constraint.

When the schema is stable, Julie, Miller, and Eros migrate intentionally.
