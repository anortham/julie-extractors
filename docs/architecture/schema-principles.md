# Schema Principles

SQLite is the primary product artifact. JSONL is an export and streaming format
derived from the same canonical data.

## Goals

- Stable enough for downstream readers to pin.
- Clean enough to serve extractor users rather than Julie internals.
- Explicit about versions and capability evidence.
- Friendly to incremental update/delete operations.
- Usable from any language with SQLite support.

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
