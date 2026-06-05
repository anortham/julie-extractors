# julie-extractors

`julie-extractors` is the standalone extraction product for Julie's
Tree-sitter-based extraction work.

```text
source tree -> versioned extraction artifact
```

The primary artifact is SQLite. JSONL is the secondary export and streaming
format. The primary integration surface is the `julie-extract` CLI, so tools
written in C#, Python, Go, JavaScript, Rust, or any other language can consume
extraction results by spawning a binary and reading a durable artifact.

## Current Release

- Current release: `v2.1.1`
- Release URL: https://github.com/anortham/julie-extractors/releases/tag/v2.1.1
- Published: `2026-06-05T00:32:31Z`
- Release commit: `3455e51934c7db9e9dcb27e1ca96a6e66330a8ad`
- Release workflow: https://github.com/anortham/julie-extractors/actions/runs/26987814150
- Release evidence: [docs/release-evidence/2026-06-05-v2-1-1-release.md](docs/release-evidence/2026-06-05-v2-1-1-release.md)

| Platform | Asset | SHA-256 |
| --- | --- | --- |
| Linux x86_64 | [`julie-extract-v2.1.1-x86_64-unknown-linux-gnu.tar.gz`](https://github.com/anortham/julie-extractors/releases/download/v2.1.1/julie-extract-v2.1.1-x86_64-unknown-linux-gnu.tar.gz) | `712cac1a180986ac755ac212450c7317fccc4c721f8b1bbc457bf0f95c36653b` |
| macOS Apple Silicon | [`julie-extract-v2.1.1-aarch64-apple-darwin.tar.gz`](https://github.com/anortham/julie-extractors/releases/download/v2.1.1/julie-extract-v2.1.1-aarch64-apple-darwin.tar.gz) | `c32dbf555ed4a3e03ac2acbbf4a9f42a6cea25e7931ef185777a40833a40fdc0` |
| macOS Intel | [`julie-extract-v2.1.1-x86_64-apple-darwin.tar.gz`](https://github.com/anortham/julie-extractors/releases/download/v2.1.1/julie-extract-v2.1.1-x86_64-apple-darwin.tar.gz) | `185899e7f7648e1e8de7d9f6648864e2d9a68ef0e5a33b7d3756af12354b9078` |
| Windows x86_64 | [`julie-extract-v2.1.1-x86_64-pc-windows-msvc.zip`](https://github.com/anortham/julie-extractors/releases/download/v2.1.1/julie-extract-v2.1.1-x86_64-pc-windows-msvc.zip) | `24b9dd34f322873a59cfebeb94838603eddb823e2d1f987e1f47aac3c3e396ac` |

The v2 line starts above the old in-tree Julie extractor crate line, which had
reached v1.22.0 before this repo became the standalone product.

## Install

Download a published binary archive from the release page, extract it, and put
`julie-extract` on your `PATH`.

Linux example:

```bash
curl -L -o julie-extract-v2.1.1-x86_64-unknown-linux-gnu.tar.gz \
  https://github.com/anortham/julie-extractors/releases/download/v2.1.1/julie-extract-v2.1.1-x86_64-unknown-linux-gnu.tar.gz
tar -xzf julie-extract-v2.1.1-x86_64-unknown-linux-gnu.tar.gz
./dist/x86_64-unknown-linux-gnu/julie-extract --version
```

Build from source:

```bash
cargo build --release -p julie-extract-cli --bin julie-extract
./target/release/julie-extract --version
```

## Quickstart

Create a SQLite artifact for this repo:

```bash
mkdir -p target/example
julie-extract scan --root . --db target/example/artifact.sqlite --json
```

Inspect the artifact:

```bash
julie-extract info --db target/example/artifact.sqlite --json
```

Export the artifact to JSONL:

```bash
julie-extract export \
  --db target/example/artifact.sqlite \
  --format jsonl \
  --out target/example/artifact.jsonl \
  --json
```

List language capability metadata:

```bash
julie-extract languages --json
```

Read the SQLite artifact from Python with only the standard library:

```bash
python3 examples/python/sqlite_consumer.py target/example/artifact.sqlite
```

## CLI Surface

| Command | Purpose | Key options |
| --- | --- | --- |
| `scan` | Create or refresh an artifact for a source root. | `--root`, `--db`, `--force`, repeated `--ignore-file`, `--strict-schema`, `--json` |
| `update` | Re-extract one file in an existing artifact. | `--root`, `--db`, `--file`, repeated `--ignore-file`, `--strict-schema`, `--json` |
| `delete` | Remove one file and its child rows from an artifact. | `--root`, `--db`, `--file`, `--strict-schema`, `--json` |
| `info` | Read artifact metadata and totals without mutating the database. | `--db`, `--strict-schema`, `--json` |
| `export` | Export a SQLite artifact to JSONL v2. | `--db`, `--format jsonl`, `--out`, `--strict-schema`, `--json` |
| `languages` | Emit parser inventory and capability snapshot metadata. | `--json` |

Every command accepts `--json` for a stable machine-readable report. Human output
is intentionally not part of the contract.

## Artifact Contract

SQLite v2 is the source of truth for durable output. It stores:

- artifact metadata and schema versions;
- parser inventory and language capability snapshots;
- extraction revisions and per-file change records;
- source file metadata, hashes, and line counts;
- symbols, symbol annotations, identifiers, relationships, pending
  relationships, type facts, generic type arguments, literals, source regions,
  and parse diagnostics.

The SQLite contract requires lookup indexes for common consumer paths: files by
path/language, symbols by path/file/name-kind/parent/test-role flags,
identifiers by path/file/name-kind/containing/target, relationships by source,
target, and kind, pending relationships by terminal/file, source regions by
file span/kind/symbol, and diagnostics by path.

Source file `content_hash` values use `blake3:<hex>`. Parser inventory and
capability snapshot fingerprints use `sha256:<hex>`, and release asset digests
are also SHA-256.

Artifacts do not store complete source file contents. Consumers that need full
text should read the matching source tree directly.

## JSONL Export

JSONL v2 is derived from SQLite and is not a separate source of truth. A full
export writes deterministic `snapshot` records in this order:

1. `artifact`
2. `parser_inventory`
3. `language_capability`
4. `language_capability_fixture`
5. `language_capability_gap`
6. `revision`
7. `revision_file_change`
8. `file`
9. `symbol`
10. `symbol_annotation`
11. `identifier`
12. `relationship`
13. `pending_relationship`
14. `type_fact`
15. `type_argument_usage`
16. `type_argument`
17. `literal`
18. `source_region`
19. `parse_diagnostic`

JSON text stored in SQLite is decoded into JSON values in JSONL payloads.

## Reports And Exit Status

JSON reports use `report_schema_version: 2` and include command status, input
paths, artifact metadata, tool version, revision IDs, row counts, warnings, and
typed errors.

Stable status values are:

- `ok`
- `no_change`
- `unsupported`
- `not_found`
- `partial`
- `failed`

`partial` means the artifact is still consistent, but at least one supported file
failed extraction. Callers should treat it as an error status while preserving
usable rows from successful files.

## Supported Languages

The current `languages --json` capability snapshot reports 36 languages:

```text
bash, c, cpp, csharp, css, dart, elixir, gdscript, go, html, java,
javascript, json, jsx, kotlin, lua, markdown, php, powershell, python, qml, r,
razor, regex, ruby, rust, scala, sql, swift, toml, tsx, typescript, vbnet, vue,
yaml, zig
```

Capability rows distinguish target support from actual fixture-backed evidence.
Use `julie-extract languages --json` for the current parser and capability
snapshot instead of hard-coding this list in consumers.

Contributor-facing language contracts:

- `docs/contracts/extracted-data-v2.md`: definitive list of extracted data domains
  and support labels.
- `docs/languages/new-language-checklist.md`: checklist for adding a language or
  upgrading a language capability claim.

## Intended Users

- Miller and other non-Rust code intelligence tools that want a stable CLI and
  SQLite artifact.
- Eros and Python tools that may choose CLI-first consumption.
- Rust callers that want the in-process extractor crate.
- Maintainers adding or improving extraction support across supported languages.

## Non-Goals

This repo does not ship or own:

- Julie MCP server behavior;
- daemon or session lifecycle;
- search ranking, search indexes, or embeddings;
- watcher services, dashboards, or workspace registry behavior;
- editing, refactoring, or code modification tools.

`/Users/murphy/source/julie` remains maintenance-only while this repo owns future
extractor product development.

## Development Gates

Fast branch gates:

```bash
cargo fmt --all -- --check
cargo xtask test default
cargo xtask test contract
```

Useful focused gates:

```bash
cargo test -p xtask
cargo test -p julie-extract-artifact --test schema_contract
cargo test -p julie-extract-artifact --test jsonl_contract
cargo test -p julie-extract-cli --test cli_contract
cargo xtask dogfood repo --root . --out-dir target/dogfood/julie-extractors
cargo xtask release package-list
```

Slow gates such as parser certification and real-world release tests are kept out
of the default suite by design.

## Documentation Map

Product and architecture:

- [Product vision](docs/product/vision.md)
- [Product boundary](docs/architecture/product-boundary.md)
- [CLI architecture contract](docs/architecture/cli-contract.md)
- [Schema principles](docs/architecture/schema-principles.md)
- [Decision 0001](docs/decisions/0001-standalone-extraction-product.md)

Public contracts:

- [CLI contract](docs/contracts/cli.md)
- [Extracted data v2](docs/contracts/extracted-data-v2.md)
- [SQLite schema v2](docs/contracts/sqlite-schema-v2.md)
- [JSONL v2](docs/contracts/jsonl-v2.md)
- [JSON reports](docs/contracts/reports.md)

Language support:

- [New language checklist](docs/languages/new-language-checklist.md)

Release and testing:

- [Testing strategy](docs/testing-strategy.md)
- [Release and certification](docs/release.md)
- [v2.0.3 release notes](docs/release-notes/v2.0.3.md)
- [v2.0.3 release evidence](docs/release-evidence/2026-06-03-v2-0-3-release.md)
- [historical v2.0.2 release notes](docs/release-notes/v2.0.2.md)
- [historical v2.0.2 release evidence](docs/release-evidence/2026-06-02-v2-0-2-release.md)
- [historical v2.0.1 release notes](docs/release-notes/v2.0.1.md)
- [historical v2.0.1 release evidence](docs/release-evidence/2026-06-02-v2-0-1-release.md)
- [historical v2.0.0 release notes](docs/release-notes/v2.0.0.md)
- [historical v2.0.0 release evidence](docs/release-evidence/2026-06-01-v2-0-0-release.md)
- [historical v0.1.0 dogfood evidence](docs/release-evidence/v0.1.0-dogfood.md)
- [historical v0.1.0 release candidate audit](docs/release-evidence/2026-06-01-v0-1-0-release-candidate-audit.md)

Plans and migration history:

- [Product completion tracker](docs/plans/2026-06-01-product-completion-tracker.md)
- [Bootstrap design](docs/plans/2026-05-31-product-bootstrap-design.md)
- [Bootstrap implementation plan](docs/plans/2026-05-31-repo-bootstrap-implementation-plan.md)
- [Julie code migration implementation plan](docs/plans/2026-05-31-julie-code-migration-implementation-plan.md)
