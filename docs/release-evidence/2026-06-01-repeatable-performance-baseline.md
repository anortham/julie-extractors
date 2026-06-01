# Repeatable Performance Baseline Evidence

## Run

- Commit under test: `844f1bb`
- Timestamp: `2026-06-01T15:02:36Z`
- Command:
  `cargo xtask performance baseline --root . --out-dir target/performance/julie-extractors-baseline --binary target/release/julie-extract --runs 3`
- Build command:
  `cargo build --release -p julie-extract-cli --bin julie-extract`
- Binary path: `target/release/julie-extract`
- Binary profile: Cargo `release`
- Binary version: `julie-extract 0.1.0`
- Binary SHA-256:
  `62d1ba3defb30883614577280f07565cc87c54775ffad8ee592326e432e2ba5c`
- Binary size: `64M`
- Output directory: `target/performance/julie-extractors-baseline`
- Summary JSON:
  `target/performance/julie-extractors-baseline/baseline-summary.json`

## Hard Gate Result

- Result: pass
- Runs: `3`
- Each run used the existing dogfood validator.
- Each cold scan report was `status=ok`.
- Each immediate rescan report was `status=no_change`.
- Each rescan report had `created_revision_id=null`.
- Each rescan report had every `counts.rows_written` value equal to `0`.
- Each info report was `status=ok`.
- Each export report was `status=ok`.
- Python SQLite readback passed for
  `target/performance/julie-extractors-baseline/run-001/artifact.sqlite`.
- Generated SQLite, JSONL, report, and summary artifacts stayed under `target/`.

## Per-Run Metrics

| Run | Scan ms | Rescan ms | Info ms | Export ms | Files | Symbols | SQLite bytes | JSONL bytes | JSONL records | Rows/sec |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 1 | 7508 | 52 | 6 | 1330 | 1018 | 33019 | 137940992 | 185875457 | 215388 | 4533.048 |
| 2 | 6277 | 51 | 5 | 1333 | 1018 | 33019 | 137940992 | 185875457 | 215388 | 5422.198 |
| 3 | 6387 | 51 | 6 | 1330 | 1018 | 33019 | 137940992 | 185875457 | 215388 | 5329.090 |

## Aggregate Report-Only Metrics

| Metric | Min | Median | Max |
| --- | ---: | ---: | ---: |
| Cold scan ms | 6277 | 6387 | 7508 |
| Immediate rescan ms | 51 | 51 | 52 |
| Info ms | 5 | 6 | 6 |
| JSONL export ms | 1330 | 1330 | 1333 |
| Files | 1018 | 1018 | 1018 |
| Symbols | 33019 | 33019 | 33019 |
| SQLite bytes | 137940992 | 137940992 | 137940992 |
| JSONL bytes | 185875457 | 185875457 | 185875457 |
| JSONL records | 215388 | 215388 | 215388 |
| Rows/sec | 4533.048 | 5329.090 | 5422.198 |

## Row Totals

| Row Domain | Rows |
| --- | ---: |
| artifact_metadata | 11 |
| parser_inventory | 0 |
| language_capabilities | 0 |
| language_capability_fixtures | 0 |
| language_capability_gaps | 0 |
| extraction_revisions | 1 |
| revision_file_changes | 1018 |
| files | 1018 |
| symbols | 33019 |
| symbol_annotations | 3068 |
| identifiers | 99057 |
| relationships | 4419 |
| pending_relationships | 59363 |
| type_facts | 3792 |
| type_argument_usages | 4809 |
| type_arguments | 5718 |
| literals | 99 |
| parse_diagnostics | 6 |

## JSONL Records By Kind

| Record Kind | Records |
| --- | ---: |
| artifact | 1 |
| revision | 1 |
| revision_file_change | 1018 |
| file | 1018 |
| symbol | 33019 |
| symbol_annotation | 3068 |
| identifier | 99057 |
| relationship | 4419 |
| pending_relationship | 59363 |
| type_fact | 3792 |
| type_argument_usage | 4809 |
| type_argument | 5718 |
| literal | 99 |
| parse_diagnostic | 6 |

## Tradeoffs And Open Decisions

- Timings remain report-only. This slice did not add wall-clock thresholds.
- The new baseline command is repo tooling under `xtask`, not a public
  `julie-extract` CLI command.
- The baseline command rejects inconsistent row totals, JSONL record counts,
  schema versions, or roots across samples before writing aggregate evidence.
- The first cold scan was slower than the next two runs. That variance is now
  explicit in min/median/max instead of hidden in a single timing.
- Slice 5 should use this evidence for release-candidate judgment and decide
  whether any hard budget is justified.
