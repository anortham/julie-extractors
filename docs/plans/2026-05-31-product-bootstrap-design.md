# Product Bootstrap Design

## Current Decision

Build `julie-extractors` as a standalone extraction product, not a support crate
inside Julie.

The product should be all that Miller, Eros, or another project needs to use
Julie's tree-sitter extraction work:

- cross-platform CLI
- SQLite primary artifact
- JSONL export
- Rust crate
- fixtures and capability evidence
- parser certification
- release binaries and checksums

## Why Now

Julie's MCP/server side is moving toward maintenance mode. Extractor work is
the part with standalone product value. Keeping it inside Julie keeps the test
suite and release process coupled to daemon, search, workspace, and MCP code
that future extractor users should not need.

## Initial Migration Stance

Do not maintain compatibility mode first.

Julie, Miller, and Eros keep using their existing integration paths while this
repo matures. The new repo should use a clean extractor-native schema and CLI
contract. When stable, downstream projects migrate intentionally.

## Product Interfaces

Primary:

- `julie-extract` CLI.
- SQLite artifact.

Secondary:

- JSONL export.
- Rust crate API.

## First Architecture Shape

```text
src tree
  -> walker / path policy
  -> language detection
  -> tree-sitter parse
  -> extractor pipeline
  -> normalized extraction model
  -> SQLite writer
  -> JSONL exporter
  -> CLI report
```

## Test Philosophy

The repo starts with a strict test hierarchy. The default suite must stay fast
by construction, with slow gates isolated from day one.

## Open Design Areas

These are not unknown placeholders; they are the next design work:

- exact SQLite table design
- JSONL record envelope
- CLI report schema
- parser inventory metadata
- per-language test command layout
- release asset matrix
- migration plan for Julie, Miller, and Eros
