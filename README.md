# julie-extractors

`julie-extractors` turns a source tree into a versioned extraction artifact:

```text
source tree -> versioned extraction artifact
```

The primary artifact is SQLite. JSONL is the secondary export and streaming
format. The primary integration surface is the `julie-extract` CLI, so tools
written in C#, Python, Go, JavaScript, Rust, or any other language can consume
extraction results by spawning a binary and reading a durable artifact.

The project site is https://anortham.github.io/julie-extractors/, including
[why extraction is hand-written code rather than tree-sitter query files](https://anortham.github.io/julie-extractors/extractors.html).

## Install

Download a binary archive for your platform from the
[latest release](https://github.com/anortham/julie-extractors/releases/latest),
extract it, and put `julie-extract` on your `PATH`. Asset checksums are on the
release page.

Or build from source:

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

## CLI surface

| Command | Purpose | Key options |
| --- | --- | --- |
| `scan` | Create or refresh an artifact for a source root. | `--root`, `--db`, `--force`, repeated `--ignore-file`, `--strict-schema`, `--json` |
| `update` | Re-extract one file in an existing artifact. | `--root`, `--db`, `--file`, repeated `--ignore-file`, `--strict-schema`, `--json` |
| `delete` | Remove one file and its child rows from an artifact. | `--root`, `--db`, `--file`, `--strict-schema`, `--json` |
| `info` | Read artifact metadata and totals without mutating the database. | `--db`, `--strict-schema`, `--json` |
| `export` | Export a SQLite artifact to JSONL. | `--db`, `--format jsonl`, `--out`, `--strict-schema`, `--json` |
| `languages` | Emit parser inventory and capability snapshot metadata. | `--json` |
| `store` | Create and maintain a versioned family store. | `import`, `update`, `delete`, `resolve`, `export`, `maintain` |

Every command accepts `--json` for a stable machine-readable report. Human
output is intentionally not part of the contract.

`scan` and `update` honor `.gitignore` files automatically, plus `.julieignore`
files for extraction-specific exclusions a repo owner wants to commit, plus
caller-supplied `--ignore-file` rules, which take precedence over the in-tree
ignore files. See [docs/contracts/cli.md](docs/contracts/cli.md) for the full
layering and precedence contract.

## Versioned family store

The v2.31.1 release provides a separate family-store contract without changing legacy
`scan`, `update`, `delete`, `info`, or `export` artifacts. A family store keeps immutable file
versions, coherent per-view manifests, durable queued requests, exact resolution bases/deltas, and
retained store generations behind an atomic `CURRENT` pointer.

```bash
julie-extract store import --store target/family --family <uuid> \
  --root . --view main --level full --json
julie-extract store resolve --store target/family --view main --json
julie-extract store maintain inspect --store target/family --json
```

Mutating maintenance commands require `--apply`. `gc` performs bounded retention/demotion and
reclamation; `repair` validates and checkpoint-recovers; `promote` builds and atomically publishes a
validated new generation. See the [store CLI contract](docs/contracts/cli.md),
[store contract](docs/contracts/store-v1.md), and
[architecture](docs/architecture/versioned-index-store.md). Miller Ph3 consumer wiring targets this
contract; Miller keeps store mode explicit until its own release and scale-default decision.

## Artifact contract

SQLite schema v6 is the source of truth for legacy durable output. It stores artifact
metadata, parser inventory and capability snapshots, extraction revisions,
per-file change records, and the extracted data itself: symbols, annotations,
identifiers, relationships, type facts, literals, source regions, structural
facts, complexity metrics, and parse diagnostics. The full table and index
contract is in
[docs/contracts/sqlite-schema-v6.md](docs/contracts/sqlite-schema-v6.md).

Artifacts do not store complete source file contents. Consumers that need full
text should read the matching source tree directly.

JSONL v4 is derived from SQLite and is not a separate source of truth. A full
export writes deterministic `snapshot` records in a fixed order, with JSON text
from SQLite decoded into JSON values. See
[docs/contracts/jsonl-v4.md](docs/contracts/jsonl-v4.md).

## Reports and exit status

JSON reports use `report_schema_version: 3` and include command status, input
paths, artifact metadata, tool version, revision IDs, row counts, warnings, and
typed errors. Stable status values are `ok`, `no_change`, `unsupported`,
`not_found`, `partial`, and `failed`.

`partial` means the artifact is still consistent, but at least one supported
file failed extraction. Callers should treat it as an error status while
preserving usable rows from successful files.

## Supported languages

The current `languages --json` capability snapshot reports 38 languages:

```text
bash, c, cpp, csharp, css, dart, elixir, erlang, gdscript, go, html, java,
javascript, json, jsx, kotlin, lua, markdown, php, powershell, python, qml, r,
razor, regex, ruby, rust, scala, sql, swift, toml, tsx, typescript, vbnet, vue,
xml, yaml, zig
```

Erlang is extracted at the full capability tier — symbols, relationships,
pending relationships, identifiers, and types. XML is extracted at the data tier
— symbols and identifiers — plus document, XSD, and WSDL structural facts.

Use `julie-extract languages --json` for the current parser and capability
snapshot instead of hard-coding this list in consumers.

Capability claims are backed by golden fixture evidence, per language and per
extraction domain. A positive claim means the extractor emits useful rows for
that domain, and missing extractor work is tracked as an explicit gap rather
than hidden behind `not_applicable`. The strict quality gate is
`node scripts/language-data-quality-report.mjs --strict`, which requires zero
silent capability cells and zero quality-bar debts.

The project maintains owned Tree-sitter grammar forks for C#, SQL, and Razor.
The [grammar dependency policy](docs/architecture/grammar-dependency-policy.md)
records why each fork exists and how its exact remote commit is controlled.

## Intended users

- Miller and other non-Rust code intelligence tools that want a stable CLI and
  SQLite artifact.
- Eros and Python tools that may choose CLI-first consumption.
- Rust callers that want the in-process extractor crate.
- Maintainers adding or improving extraction support across supported
  languages.

## Non-goals

This repo does not ship or own MCP server behavior, daemon or session
lifecycle, search ranking or embeddings, watcher services, dashboards, or
editing tools.

## Development

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

Slow gates such as parser certification and real-world release tests are kept
out of the default suite by design.

## Documentation

Product and architecture:

- [Product vision](docs/product/vision.md)
- [Product boundary](docs/architecture/product-boundary.md)
- [CLI architecture contract](docs/architecture/cli-contract.md)
- [Schema principles](docs/architecture/schema-principles.md)

Public contracts:

- [CLI contract](docs/contracts/cli.md)
- [Extracted data v4](docs/contracts/extracted-data-v4.md)
- [SQLite schema v6](docs/contracts/sqlite-schema-v6.md)
- [SQLite schema v5](docs/contracts/sqlite-schema-v5.md) (superseded)
- [JSONL v4](docs/contracts/jsonl-v4.md)
- [JSON reports](docs/contracts/reports.md)
- [Progress file v1](docs/contracts/progress-file-v1.md)

Language support:

- [New language checklist](docs/languages/new-language-checklist.md)

Release and testing:

- [Testing strategy](docs/testing-strategy.md)
- [Release and certification](docs/release.md)
- Release notes and evidence for every version live in
  [docs/release-notes/](docs/release-notes/) and
  [docs/release-evidence/](docs/release-evidence/).
