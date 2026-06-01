# v0.1.0 Release Candidate Audit Evidence

## Run

- Commit under test: `c407cde`
- Timestamp: `2026-06-01T15:44:05Z`
- Host target: `aarch64-apple-darwin`
- Rust host: `aarch64-apple-darwin`
- Binary path: `target/release/julie-extract`
- Binary profile: Cargo `release`
- Binary version: `julie-extract 0.1.0`
- Binary SHA-256:
  `c52b86f01c369088fad94da2ca013c9ddcfc840830e787c2f758a06724cf9237`
- Binary size: `64M`

## Package Staging

- Command:
  `cargo xtask release package --version 0.1.0 --target aarch64-apple-darwin --out-dir target/release-package/v0.1.0-aarch64-apple-darwin-c407cde --binary target/release/julie-extract`
- Result: pass
- Output directory:
  `target/release-package/v0.1.0-aarch64-apple-darwin-c407cde`
- Checksum verification command:
  `cd target/release-package/v0.1.0-aarch64-apple-darwin-c407cde && shasum -a 256 -c dist/aarch64-apple-darwin/julie-extract.sha256`
- Checksum verification result:
  `dist/aarch64-apple-darwin/julie-extract: OK`
- Checksum file contents:
  `c52b86f01c369088fad94da2ca013c9ddcfc840830e787c2f758a06724cf9237  dist/aarch64-apple-darwin/julie-extract`

## Staged Files

```text
dist/aarch64-apple-darwin/julie-extract
dist/aarch64-apple-darwin/julie-extract.sha256
docs/architecture/product-boundary.md
docs/architecture/schema-principles.md
docs/contracts/cli.md
docs/contracts/jsonl-v1.md
docs/contracts/reports.md
docs/contracts/sqlite-schema-v1.md
docs/release-notes/v0.1.0.md
docs/release.md
docs/testing-strategy.md
```

Generated package, SQLite, JSONL, report, and performance artifacts stayed
under `target/`.

## Audit Finding Resolved

The audit found a contract mismatch before release:

- SQLite and JSONL contracts expose parser inventory and language capability
  rows.
- Earlier dogfood evidence had `0` rows in `parser_inventory`,
  `language_capabilities`, `language_capability_fixtures`, and
  `language_capability_gaps`.
- The release-candidate branch now persists the extractor capability snapshot
  into those tables and exports those rows through JSONL.
- No language capability claims changed.
- No parser dependency versions changed.

## Repeatable Performance Baseline

- Commit under test: `805da3b`
- Timestamp: `2026-06-01T15:43:07Z`
- Command:
  `cargo xtask performance baseline --root . --out-dir target/performance/julie-extractors-baseline-805da3b --binary target/release/julie-extract --runs 3`
- Result: pass
- Runs: `3`
- Each run used the dogfood validator.
- Each cold scan report was `status=ok`.
- Each immediate rescan report was `status=no_change`.
- Each rescan report had `created_revision_id=null`.
- Each rescan report had every `counts.rows_written` value equal to `0`.
- Each info report was `status=ok`.
- Each export report was `status=ok`.
- Generated artifacts stayed under `target/performance/julie-extractors-baseline-805da3b`.

| Run | Scan ms | Rescan ms | Info ms | Export ms | Files | Symbols | SQLite bytes | JSONL bytes | JSONL records | Rows/sec |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 1 | 7550 | 62 | 6 | 1318 | 1020 | 33099 | 138510336 | 186569602 | 216253 | 4518.597 |
| 2 | 6514 | 62 | 5 | 1321 | 1020 | 33099 | 138510336 | 186569602 | 216253 | 5237.028 |
| 3 | 6485 | 56 | 6 | 1328 | 1020 | 33099 | 138510336 | 186569602 | 216253 | 5260.889 |

| Metric | Min | Median | Max |
| --- | ---: | ---: | ---: |
| Cold scan ms | 6485 | 6514 | 7550 |
| Immediate rescan ms | 56 | 62 | 62 |
| Info ms | 5 | 6 | 6 |
| JSONL export ms | 1318 | 1321 | 1328 |
| Files | 1020 | 1020 | 1020 |
| Symbols | 33099 | 33099 | 33099 |
| SQLite bytes | 138510336 | 138510336 | 138510336 |
| JSONL bytes | 186569602 | 186569602 | 186569602 |
| JSONL records | 216253 | 216253 | 216253 |
| Rows/sec | 4518.597 | 5237.028 | 5260.889 |

## Row Totals

| Row Domain | Rows |
| --- | ---: |
| artifact_metadata | 11 |
| parser_inventory | 36 |
| language_capabilities | 36 |
| language_capability_fixtures | 76 |
| language_capability_gaps | 17 |
| extraction_revisions | 1 |
| revision_file_changes | 1020 |
| files | 1020 |
| symbols | 33099 |
| symbol_annotations | 3092 |
| identifiers | 99403 |
| relationships | 4450 |
| pending_relationships | 59459 |
| type_facts | 3841 |
| type_argument_usages | 4833 |
| type_arguments | 5752 |
| literals | 111 |
| parse_diagnostics | 6 |

## JSONL Records By Kind

| Record Kind | Records |
| --- | ---: |
| artifact | 1 |
| parser_inventory | 36 |
| language_capability | 36 |
| language_capability_fixture | 76 |
| language_capability_gap | 17 |
| revision | 1 |
| revision_file_change | 1020 |
| file | 1020 |
| symbol | 33099 |
| symbol_annotation | 3092 |
| identifier | 99403 |
| relationship | 4450 |
| pending_relationship | 59459 |
| type_fact | 3841 |
| type_argument_usage | 4833 |
| type_argument | 5752 |
| literal | 111 |
| parse_diagnostic | 6 |

## Focused Verification

| Invariant | Command | Result |
| --- | --- | --- |
| Real scan persists capability rows and export includes them | `cargo test -p julie-extract-cli --test operations_contract` | pass |
| JSONL contract still emits every v1 kind in order | `cargo test -p julie-extract-artifact --test jsonl_contract` | pass |
| Report row domains remain exhaustive | `cargo test -p julie-extract-artifact --test report_contract` | pass |
| Capability snapshot sync is idempotent | `cargo test -p julie-extract-artifact --test writer_contract capability_snapshot_sync_writes_static_rows_once` | pass |
| Release binary builds | `cargo build --release -p julie-extract-cli --bin julie-extract` | pass |
| Package staging succeeds and checksum verifies | package staging and checksum commands above | pass |
| Format check passes | `cargo fmt --all -- --check` | pass |
| xtask tests pass | `cargo test -p xtask` | pass |
| Default gate passes | `cargo xtask test default` | pass |
| Contract gate passes | `cargo xtask test contract` | pass |
| Changed-path gate covers touched extractor metadata path | `cargo xtask test changed crates/julie-extractors/Cargo.toml crates/julie-extract-artifact/src/writer.rs crates/julie-extract-cli/src/commands.rs docs/release-notes/v0.1.0.md` | pass |
| Agent guidance files remain synced | `scripts/check-agent-doc-sync.sh` | pass |
| Diff has no whitespace errors | `git diff --check` | pass |

## Judgment

The v0.1.0 release candidate is ready for PR and CI. The staged package
contains the expected binary, checksum, contracts, architecture docs,
testing/release docs, and v0.1.0 release note. The package does not include
Julie MCP/server/daemon/search/embedding, watcher, dashboard, or editing
behavior.
