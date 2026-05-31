# julie-extractors

`julie-extractors` is planned as the standalone extraction product for Julie's
tree-sitter work.

The product boundary is:

```text
source tree -> versioned extraction artifact
```

The primary artifact is SQLite. JSONL is a secondary export and streaming
format. The primary integration surface is the `julie-extract` CLI so projects
in any language can consume the extractor work without embedding Rust or running
Julie MCP/server/daemon code.

## Status

Planning and bootstrap. `/Users/murphy/source/julie` remains intact while this
repo takes over future extractor development.

## Intended Users

- Miller and other non-Rust code intelligence tools that want a stable CLI and
  SQLite artifact.
- Eros and Python tools that may choose CLI-first consumption.
- Rust callers that want the in-process extractor crate.
- Maintainers adding or improving extraction support across 34+ languages.

## Product Surfaces

- `julie-extract scan --root <dir> --db <path>`
- `julie-extract update --root <dir> --db <path> --file <path>`
- `julie-extract delete --root <dir> --db <path> --file <path>`
- `julie-extract info --db <path> --json`
- `julie-extract export --db <path> --format jsonl`
- `julie-extract languages --json`
- Rust crate API for in-process extraction.

## Non-Goals

- MCP server behavior.
- Daemon/session lifecycle.
- Search ranking, search indexes, or embeddings.
- Editing/refactoring tools.
- Julie workspace registry or watcher service.

## Current Planning Docs

- [Product vision](docs/product/vision.md)
- [Product boundary](docs/architecture/product-boundary.md)
- [CLI contract draft](docs/architecture/cli-contract.md)
- [Schema principles](docs/architecture/schema-principles.md)
- [CLI contract](docs/contracts/cli.md)
- [SQLite schema v1](docs/contracts/sqlite-schema-v1.md)
- [JSONL v1](docs/contracts/jsonl-v1.md)
- [JSON reports](docs/contracts/reports.md)
- [Testing strategy](docs/testing-strategy.md)
- [Decision 0001](docs/decisions/0001-standalone-extraction-product.md)
- [Bootstrap design](docs/plans/2026-05-31-product-bootstrap-design.md)
- [Bootstrap implementation plan](docs/plans/2026-05-31-repo-bootstrap-implementation-plan.md)
- [Julie code migration implementation plan](docs/plans/2026-05-31-julie-code-migration-implementation-plan.md)
