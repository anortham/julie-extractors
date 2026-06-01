# julie-extractors

`julie-extractors` is the standalone extraction product for Julie's tree-sitter
work.

The product boundary is:

```text
source tree -> versioned extraction artifact
```

The primary artifact is SQLite. JSONL is a secondary export and streaming
format. The primary integration surface is the `julie-extract` CLI so projects
in any language can consume the extractor work without embedding Rust or running
Julie MCP/server/daemon code.

## Status

v2.0.0 release target. The migrated CLI, SQLite artifact writer, JSONL export,
dogfood gate, package staging, and CI workflow are in this repo. The unpublished
v0.1.0 release-candidate evidence is retained as historical audit evidence.
`/Users/murphy/source/julie` remains maintenance-only while this repo takes over
future extractor development.

## Quickstart

Build the CLI:

```bash
cargo build -p julie-extract-cli --bin julie-extract
```

Create a SQLite artifact:

```bash
cargo run -p julie-extract-cli --bin julie-extract -- \
  scan --root . --db target/example/artifact.sqlite --json
```

Inspect and export it:

```bash
cargo run -p julie-extract-cli --bin julie-extract -- \
  info --db target/example/artifact.sqlite --json
cargo run -p julie-extract-cli --bin julie-extract -- \
  export --db target/example/artifact.sqlite --format jsonl --out target/example/artifact.jsonl --json
```

Run the repo dogfood gate:

```bash
cargo xtask dogfood repo --root . --out-dir target/dogfood/julie-extractors
```

Read the dogfood SQLite artifact from Python:

```bash
python3 examples/python/sqlite_consumer.py target/dogfood/julie-extractors/artifact.sqlite
```

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

## Consumer Examples

- Python SQLite reader: [examples/python/sqlite_consumer.py](examples/python/sqlite_consumer.py)

## Non-Goals

- MCP server behavior.
- Daemon/session lifecycle.
- Search ranking, search indexes, or embeddings.
- Editing/refactoring tools.
- Julie workspace registry or watcher service.

## Current Docs

- [Product vision](docs/product/vision.md)
- [Product boundary](docs/architecture/product-boundary.md)
- [CLI contract draft](docs/architecture/cli-contract.md)
- [Schema principles](docs/architecture/schema-principles.md)
- [CLI contract](docs/contracts/cli.md)
- [SQLite schema v1](docs/contracts/sqlite-schema-v1.md)
- [JSONL v1](docs/contracts/jsonl-v1.md)
- [JSON reports](docs/contracts/reports.md)
- [Testing strategy](docs/testing-strategy.md)
- [Release and certification](docs/release.md)
- [v2.0.0 release notes](docs/release-notes/v2.0.0.md)
- [historical v0.1.0 dogfood evidence](docs/release-evidence/v0.1.0-dogfood.md)
- [historical v0.1.0 release candidate audit](docs/release-evidence/2026-06-01-v0-1-0-release-candidate-audit.md)
- [Decision 0001](docs/decisions/0001-standalone-extraction-product.md)
- [Bootstrap design](docs/plans/2026-05-31-product-bootstrap-design.md)
- [Bootstrap implementation plan](docs/plans/2026-05-31-repo-bootstrap-implementation-plan.md)
- [Julie code migration implementation plan](docs/plans/2026-05-31-julie-code-migration-implementation-plan.md)
