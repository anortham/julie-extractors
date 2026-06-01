# JSONL Export Buffering Evidence

## Run

- Implementation commit: `14da93e`
- Timestamp: `2026-06-01T14:30:14Z`
- Command: `target/release/julie-extract export --db /Users/murphy/source/julie-extractors/target/dogfood/julie-extractors/artifact.sqlite --format jsonl --out /dev/null --json`
- Binary path: `target/release/julie-extract`
- Binary profile: Cargo `release`
- Binary version: `julie-extract 0.1.0`
- Binary SHA-256: `62d1ba3defb30883614577280f07565cc87c54775ffad8ee592326e432e2ba5c`
- Binary size: `64M`
- Input SQLite artifact: `/Users/murphy/source/julie-extractors/target/dogfood/julie-extractors/artifact.sqlite`
- Output path: `/dev/null`

## Hard Gate Result

- Result: pass
- Export report: `status=ok`, `operation=export`, `mode=jsonl`, `errors=[]`
- SQLite schema version: `1`
- Extract contract version: `1`
- JSONL schema version: `1`
- Root path: `/Users/murphy/source/julie-extractors`
- Files: `1006`
- Symbols: `32708`
- JSONL records: `213222`
- Bounded-write regression: pass
- JSONL contract suite: pass
- CLI export focused contract: pass

## Reported Row Counts

| Row Domain | Exported Records |
| --- | ---: |
| artifact_metadata | 1 |
| parser_inventory | 0 |
| language_capabilities | 0 |
| language_capability_fixtures | 0 |
| language_capability_gaps | 0 |
| extraction_revisions | 1 |
| revision_file_changes | 1006 |
| files | 1006 |
| symbols | 32708 |
| symbol_annotations | 3019 |
| identifiers | 98068 |
| relationships | 4323 |
| pending_relationships | 58897 |
| type_facts | 3695 |
| type_argument_usages | 4760 |
| type_arguments | 5634 |
| literals | 98 |
| parse_diagnostics | 6 |

## Report-Only Metrics

| Metric | Before Buffering | After Buffering |
| --- | ---: | ---: |
| Export to `/dev/null` real seconds | 20.79 | 2.43 |
| Export to `/dev/null` user seconds | 3.83 | 1.06 |
| Export to `/dev/null` sys seconds | 15.66 | 0.21 |
| SQLite bytes | 136564736 | 136564736 |
| Existing JSONL bytes | 184001656 | 184001656 |

## TDD Evidence

- Red test: `cargo test -p julie-extract-artifact --test jsonl_contract buffered_export_uses_bounded_write_calls`
  - Failed before buffering with `2853` downstream writes for an `8558` byte fixture export.
- Green focused test: same command passed after buffering.
- Full JSONL contract: `cargo test -p julie-extract-artifact --test jsonl_contract` passed.
- CLI focused export test: `cargo test -p julie-extract-cli --test operations_contract export_jsonl_emits_valid_jsonl_records_from_scanned_artifact` passed.

## Tradeoffs And Open Decisions

- This slice buffered writes at the artifact exporter boundary, so `export_jsonl`,
  `export_jsonl_to_path`, and CLI export callers all benefit without changing
  public APIs.
- Timings remain report-only. The repeatable performance baseline slice still
  owns same-machine variance and any hard budget decision.
- The fallback per-record line buffer is not needed now because buffering
  materially reduced `real` and `sys` time against the same artifact.
- Generated JSON reports and measurement output stayed outside tracked source
  files.
