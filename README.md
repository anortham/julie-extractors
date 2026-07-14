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

Julie Extractors currently maintains owned Tree-sitter grammar forks for C#,
SQL, and Razor. The
[grammar dependency policy and current fork inventory](docs/architecture/grammar-dependency-policy.md)
records why each fork exists and how its exact remote commit is controlled.

## Current Release

- Current release: `v2.14.0`
- Release URL: https://github.com/anortham/julie-extractors/releases/tag/v2.14.0
- Published: `2026-07-14T16:24:46Z`
- Release commit: `dc270e2f590c2b41413a9fd6e642bd8671022979`
- Release workflow: https://github.com/anortham/julie-extractors/actions/runs/29348623403
- Release evidence: [docs/release-evidence/2026-07-14-v2-14-0-release.md](docs/release-evidence/2026-07-14-v2-14-0-release.md)

| Platform | Asset | SHA-256 |
| --- | --- | --- |
| Linux x86_64 | [`julie-extract-v2.14.0-x86_64-unknown-linux-gnu.tar.gz`](https://github.com/anortham/julie-extractors/releases/download/v2.14.0/julie-extract-v2.14.0-x86_64-unknown-linux-gnu.tar.gz) | `dc94857e7c7f7bee9e4f9ea4b8948cbef11b40ecac4a35551d5653f3fa32081e` |
| macOS Apple Silicon | [`julie-extract-v2.14.0-aarch64-apple-darwin.tar.gz`](https://github.com/anortham/julie-extractors/releases/download/v2.14.0/julie-extract-v2.14.0-aarch64-apple-darwin.tar.gz) | `e0781be4d561223f57cffe544eda7d615eb05b719c20250d8f092a0b1d79162d` |
| macOS Intel | [`julie-extract-v2.14.0-x86_64-apple-darwin.tar.gz`](https://github.com/anortham/julie-extractors/releases/download/v2.14.0/julie-extract-v2.14.0-x86_64-apple-darwin.tar.gz) | `22f8717fc068a3610d127f8c641cf8b148ee332cdfade799eb24c827e4213878` |
| Windows x86_64 | [`julie-extract-v2.14.0-x86_64-pc-windows-msvc.zip`](https://github.com/anortham/julie-extractors/releases/download/v2.14.0/julie-extract-v2.14.0-x86_64-pc-windows-msvc.zip) | `6fbb7e406fb7bec396e1447c4a69568a37599aed1b80d16a7b9b49c42da3839c` |

The v2 line starts above the old in-tree Julie extractor crate line, which had
reached v1.22.0 before this repo became the standalone product.

## Install

Download a published binary archive from the release page, extract it, and put
`julie-extract` on your `PATH`.

Linux example:

```bash
curl -L -o julie-extract-v2.14.0-x86_64-unknown-linux-gnu.tar.gz \
  https://github.com/anortham/julie-extractors/releases/download/v2.14.0/julie-extract-v2.14.0-x86_64-unknown-linux-gnu.tar.gz
tar -xzf julie-extract-v2.14.0-x86_64-unknown-linux-gnu.tar.gz
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
| `export` | Export a SQLite artifact to JSONL v3. | `--db`, `--format jsonl`, `--out`, `--strict-schema`, `--json` |
| `languages` | Emit parser inventory and capability snapshot metadata. | `--json` |

Every command accepts `--json` for a stable machine-readable report. Human output
is intentionally not part of the contract.

`scan` and `update` honor `.gitignore` files automatically (root, ancestors up
to the git root, and nested subdirectories), plus `.julieignore` files with the
same semantics for extraction-specific exclusions a repo owner wants to commit,
plus caller-supplied `--ignore-file` rules, which take precedence over the
in-tree ignore files. See
[docs/contracts/cli.md](docs/contracts/cli.md) for the full layering and
precedence contract.

## Artifact Contract

SQLite schema v4 is the source of truth for durable output. It stores:

- artifact metadata and schema versions;
- parser inventory and language capability snapshots;
- extraction revisions and per-file change records;
- source file metadata, hashes, and line counts;
- symbols, symbol annotations, identifiers, relationships, pending
  relationships, type facts, generic type arguments, literals, source regions,
  structural facts, complexity metrics, and parse diagnostics.

The SQLite contract requires lookup indexes for common consumer paths: files by
path/language, symbols by path/file/name-kind/parent/test-role flags,
identifiers by path/file/name-kind/containing/target, relationships by source,
target, and kind, pending relationships by terminal/file, source regions by
file span/kind/symbol, structural facts by file span/pattern/symbol,
complexity metrics by file scope/scope-language/symbol, and diagnostics by path.

Source file `content_hash` values use `blake3:<hex>`. Parser inventory and
capability snapshot fingerprints use `sha256:<hex>`, and release asset digests
are also SHA-256.

Artifacts do not store complete source file contents. Consumers that need full
text should read the matching source tree directly.

## Data Quality

The `v2.3.0` release is a broad extraction-quality release. It raises the
supported language matrix from boolean "supported" claims to fixture-backed,
per-domain evidence. Every advertised language now has explicit coverage data
for each extraction domain, and the scorecard has no silent cells or quality-bar
debt.

Current quality scorecard:

- Languages: `36`
- Silent capability cells: `0`
- Quality-bar debts: `0`
- Strict gate: `node scripts/language-data-quality-report.mjs --strict`

| Domain | Fixture-proven native coverage | Applicable closure |
| --- | ---: | ---: |
| Symbols | 36/36 | 36/36 |
| Relationships | 36/36 | 36/36 |
| Identifiers | 33/36 | 33/33 |
| Body spans | 35/36 | 35/35 |
| Structural facts | 36/36 | 36/36 |
| Complexity metrics | 30/36 | 30/30 |
| Annotations | 23/36 | 23/23 |
| Doc comments | 35/36 | 35/35 |
| Literals | 36/36 | 36/36 |
| Source regions | 35/36 | 35/35 |
| Pending relationships | 30/36 | 30/30 |
| Types | 29/36 | 29/29 |
| Type argument usages | 20/36 | 20/20 |

Test detection is role-aware rather than a single language-level capability.
Golden fixtures prove at least one emitted test role for all 28 code languages.
Across the 108 language-role cells, 60 are supported, 6 are source-backed
`not_applicable`, and 42 remain explicit `open_gaps`; there are no unclassified
cells.

"Applicable closure" means the language either has fixture-proven native
coverage for that domain or a recorded reason why the domain is not a real
language construct. Missing extractor work cannot be hidden behind
`not_applicable`; the capability matrix enforces that every positive claim has
golden fixture evidence.

Dogfood evidence compares `v2.3.0` with the previous `v2.2.1` binary across the
three dependent projects and representative real-world repositories. The release
branch scanned `12,893` files with `0` failed files, kept parse diagnostics
unchanged, and added `451,382` structural facts plus `33,687` complexity metrics
in that corpus.

## JSONL Export

JSONL v3 is derived from SQLite and is not a separate source of truth. A full
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
19. `complexity_metric`
20. `structural_fact`
21. `parse_diagnostic`

JSON text stored in SQLite is decoded into JSON values in JSONL payloads.

## Reports And Exit Status

JSON reports use `report_schema_version: 3` and include command status, input
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

- `docs/contracts/extracted-data-v3.md`: definitive list of extracted data domains
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
- [Continuous-testing evidence boundary](docs/architecture/continuous-testing-evidence-boundary.md)
- [Decision 0001](docs/decisions/0001-standalone-extraction-product.md)

Public contracts:

- [CLI contract](docs/contracts/cli.md)
- [Extracted data v3](docs/contracts/extracted-data-v3.md)
- [SQLite schema v4](docs/contracts/sqlite-schema-v4.md)
- [JSONL v3](docs/contracts/jsonl-v3.md)
- [JSON reports](docs/contracts/reports.md)
- [Test evidence v1](docs/contracts/test-evidence-v1.md)

Language support:

- [New language checklist](docs/languages/new-language-checklist.md)
- [Language data quality plan](docs/plans/2026-06-10-language-data-quality.md)
- [Language data quality evidence](docs/release-evidence/2026-06-10-language-data-quality.md)

Release and testing:

- [Testing strategy](docs/testing-strategy.md)
- [Release and certification](docs/release.md)
- [v2.14.0 release notes](docs/release-notes/v2.14.0.md)
- [v2.14.0 release evidence](docs/release-evidence/2026-07-14-v2-14-0-release.md)
- [historical v2.13.0 release notes](docs/release-notes/v2.13.0.md)
- [historical v2.13.0 release evidence](docs/release-evidence/2026-07-12-v2-13-0-release.md)
- [historical v2.12.1 release notes](docs/release-notes/v2.12.1.md)
- [historical v2.12.0 release notes](docs/release-notes/v2.12.0.md)
- [historical v2.12.0 release evidence](docs/release-evidence/2026-07-10-v2-12-0-release.md)
- [historical v2.11.0 release notes](docs/release-notes/v2.11.0.md)
- [historical v2.11.0 release evidence](docs/release-evidence/2026-07-07-v2-11-0-release.md)
- [historical v2.10.0 release notes](docs/release-notes/v2.10.0.md)
- [historical v2.10.0 release evidence](docs/release-evidence/2026-07-07-v2-10-0-release.md)
- [historical v2.9.0 release notes](docs/release-notes/v2.9.0.md)
- [historical v2.9.0 release evidence](docs/release-evidence/2026-07-07-v2-9-0-release.md)
- [historical v2.8.1 release notes](docs/release-notes/v2.8.1.md)
- [historical v2.8.1 release evidence](docs/release-evidence/2026-07-04-v2-8-1-release.md)
- [historical v2.8.0 release notes](docs/release-notes/v2.8.0.md)
- [historical v2.8.0 release evidence](docs/release-evidence/2026-07-04-v2-8-0-release.md)
- [historical v2.7.0 release notes](docs/release-notes/v2.7.0.md)
- [historical v2.7.0 release evidence](docs/release-evidence/2026-07-02-v2-7-0-release.md)
- [historical v2.6.0 release notes](docs/release-notes/v2.6.0.md)
- [historical v2.5.10 release notes](docs/release-notes/v2.5.10.md)
- [historical v2.5.10 release evidence](docs/release-evidence/2026-07-01-v2-5-10-release.md)
- [historical v2.5.9 release notes](docs/release-notes/v2.5.9.md)
- [historical v2.5.9 release evidence](docs/release-evidence/2026-07-01-v2-5-9-release.md)
- [historical v2.5.8 release notes](docs/release-notes/v2.5.8.md)
- [historical v2.5.8 release evidence](docs/release-evidence/2026-07-01-v2-5-8-release.md)
- [historical v2.5.7 release notes](docs/release-notes/v2.5.7.md)
- [historical v2.5.7 release evidence](docs/release-evidence/2026-06-30-v2-5-7-release.md)
- [historical v2.5.6 release notes](docs/release-notes/v2.5.6.md)
- [historical v2.5.6 release evidence](docs/release-evidence/2026-06-30-v2-5-6-release.md)
- [historical v2.5.5 release notes](docs/release-notes/v2.5.5.md)
- [historical v2.5.5 release evidence](docs/release-evidence/2026-06-30-v2-5-5-release.md)
- [historical v2.5.4 release notes](docs/release-notes/v2.5.4.md)
- [historical v2.5.4 release evidence](docs/release-evidence/2026-06-30-v2-5-4-release.md)
- [historical v2.5.3 release notes](docs/release-notes/v2.5.3.md)
- [historical v2.5.3 release evidence](docs/release-evidence/2026-06-22-v2-5-3-release.md)
- [historical v2.5.2 release notes](docs/release-notes/v2.5.2.md)
- [historical v2.5.2 release evidence](docs/release-evidence/2026-06-20-v2-5-2-release.md)
- [historical v2.5.1 release notes](docs/release-notes/v2.5.1.md)
- [historical v2.5.1 release evidence](docs/release-evidence/2026-06-15-v2-5-1-release.md)
- [historical v2.5.0 release notes](docs/release-notes/v2.5.0.md)
- [historical v2.5.0 release evidence](docs/release-evidence/2026-06-14-v2-5-0-release.md)
- [historical v2.4.0 release notes](docs/release-notes/v2.4.0.md)
- [historical v2.4.0 release evidence](docs/release-evidence/2026-06-11-v2-4-0-release.md)
- [historical v2.3.0 release notes](docs/release-notes/v2.3.0.md)
- [historical v2.3.0 release evidence](docs/release-evidence/2026-06-11-v2-3-0-release.md)
- [historical v2.2.1 release notes](docs/release-notes/v2.2.1.md)
- [historical v2.2.1 release evidence](docs/release-evidence/2026-06-09-v2-2-1-release.md)
- [historical v2.2.0 release notes](docs/release-notes/v2.2.0.md)
- [historical v2.2.0 release evidence](docs/release-evidence/2026-06-09-v2-2-0-release.md)
- [historical v2.1.3 release notes](docs/release-notes/v2.1.3.md)
- [historical v2.1.3 release evidence](docs/release-evidence/2026-06-05-v2-1-3-release.md)
- [historical v2.1.2 release notes](docs/release-notes/v2.1.2.md)
- [historical v2.1.2 release evidence](docs/release-evidence/2026-06-05-v2-1-2-release.md)
- [historical v2.1.1 release notes](docs/release-notes/v2.1.1.md)
- [historical v2.1.1 release evidence](docs/release-evidence/2026-06-05-v2-1-1-release.md)
- [historical v2.1.0 release notes](docs/release-notes/v2.1.0.md)
- [historical v2.1.0 release evidence](docs/release-evidence/2026-06-03-v2-1-0-release.md)
- [historical v2.0.3 release notes](docs/release-notes/v2.0.3.md)
- [historical v2.0.3 release evidence](docs/release-evidence/2026-06-03-v2-0-3-release.md)
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
