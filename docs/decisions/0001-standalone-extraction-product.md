# Decision 0001: Standalone Extraction Product

## Context

Julie's extractor work is the reusable part of the system. Julie MCP/server is
expected to move into maintenance mode, while Miller, Eros, and future tools
need the tree-sitter extraction work without inheriting Julie daemon, search,
workspace, or MCP complexity.

The current Julie repo already has a `julie-extractors` crate, but Miller
consumes extraction through `julie-server extract` and SQLite. Eros currently
depends on Julie's local extractor crate through a path dependency. A crate-only
split would help Eros but leave Miller tied to Julie server releases.

## Decision

Create `/Users/murphy/source/julie-extractors` as the new canonical product for
extraction.

The new repo owns:

- Rust extractor crate.
- `julie-extract` CLI.
- SQLite primary artifact.
- JSONL export format.
- Extraction schema and report contracts.
- Fixtures, capability matrix, parser certification, and release evidence.
- Release binaries and checksums.

Julie remains intact while the new product is built. New extractor development
belongs here once the repo is bootstrapped.

## Consequences

This makes the product boundary clear and lets downstream tools consume
extraction without knowing Julie internals.

It also means the new repo must own release discipline, schema stability, test
tiering, and documentation from the start. The first migration is larger than a
crate split, but it avoids long-term compatibility tax.

## Rejected Alternatives

- **Crate-only split:** too shallow; Miller still depends on Julie server.
- **Compatibility mode first:** preserves old Julie coupling before the new
  product contract exists.
- **Move all indexing/search:** too wide; downstream tools should own those
  layers.

## Future Agents

Do not add Julie MCP/server/daemon/search behavior to this repo. If work does
not directly support source extraction, artifact writing/export, contracts,
fixtures, certification, or release packaging, it probably belongs elsewhere.
