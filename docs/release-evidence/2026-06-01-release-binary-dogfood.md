# Release Binary Dogfood Evidence

## Run

- Commit under test: `a3038ee`
- Timestamp: `2026-06-01T13:49:42Z`
- Command: `cargo xtask dogfood repo --root . --out-dir target/dogfood/julie-extractors-release --binary target/release/julie-extract`
- Binary path: `target/release/julie-extract`
- Binary profile: Cargo `release`
- Binary version: `julie-extract 0.1.0`
- Binary SHA-256: `af51b3792e10eb54f6aab5d94cd04c257801b183be0fb23f08db96ba23f441ce`
- Binary size: `64M`
- Outputs:
  - `target/dogfood/julie-extractors-release/artifact.sqlite`
  - `target/dogfood/julie-extractors-release/artifact.jsonl`
  - `target/dogfood/julie-extractors-release/scan-report.json`
  - `target/dogfood/julie-extractors-release/rescan-report.json`
  - `target/dogfood/julie-extractors-release/info-report.json`
  - `target/dogfood/julie-extractors-release/export-report.json`
  - `target/dogfood/julie-extractors-release/metrics.json`

## Hard Gate Result

- Result: pass
- Cold scan report: `status=ok`, `operation=scan`, `mode=incremental`, `errors=[]`
- Immediate rescan report: `status=no_change`, `operation=scan`, `mode=incremental`, `errors=[]`
- Rescan revision: `created_revision_id=null`
- Rescan rows written: all `counts.rows_written` values are `0`
- Info report: `status=ok`, `operation=info`, `mode=read_only`, `errors=[]`
- Export report: `status=ok`, `operation=export`, `mode=jsonl`, `errors=[]`
- SQLite schema version: `1`
- Extract contract version: `1`
- JSONL schema version: `1`
- Root path: `/Users/murphy/.config/razorback/worktrees/julie-extractors/release-dogfood-evidence`
- Files scanned: `1016`
- Files written: `1012`
- Unsupported files: `4`
- Files failed: `0`
- Rescan files unchanged: `1012`
- Rescan files changed: `0`
- Rescan files deleted: `0`
- Rescan files failed: `0`
- Symbols written: `32881`
- JSONL records: `214333`
- Python SQLite consumer readback: pass

## Row Totals

| Table | Rows |
| --- | ---: |
| artifact_metadata | 11 |
| extraction_revisions | 1 |
| files | 1012 |
| identifiers | 98570 |
| language_capabilities | 0 |
| language_capability_fixtures | 0 |
| language_capability_gaps | 0 |
| literals | 99 |
| parse_diagnostics | 6 |
| parser_inventory | 0 |
| pending_relationships | 59138 |
| relationships | 4374 |
| revision_file_changes | 1012 |
| symbol_annotations | 3038 |
| symbols | 32881 |
| type_argument_usages | 4784 |
| type_arguments | 5681 |
| type_facts | 3736 |

## Repo Shape

| Top Path | Files | Symbols |
| --- | ---: | ---: |
| fixtures | 161 | 20940 |
| crates | 774 | 10929 |
| docs | 25 | 347 |
| xtask | 12 | 322 |
| .github | 3 | 142 |
| languages | 27 | 107 |
| README.md | 1 | 31 |
| examples | 2 | 20 |
| AGENTS.md | 1 | 9 |
| CLAUDE.md | 1 | 9 |
| Cargo.toml | 1 | 9 |
| .mcp.json | 1 | 6 |
| RAZORBACK.md | 1 | 6 |
| .cargo | 1 | 2 |
| scripts | 1 | 2 |

## Report-Only Metrics

| Metric | Value |
| --- | ---: |
| SQLite bytes | 137293824 |
| JSONL bytes | 184972072 |
| Cold scan duration ms | 7607 |
| Immediate no-change rescan duration ms | 52 |
| Info duration ms | 6 |
| Export duration ms | 68983 |
| Rows per second | 4455.169926944018 |

## Comparison To Debug Dogfood Evidence

| Metric | Debug Dogfood | Release-Binary Dogfood |
| --- | ---: | ---: |
| Cold scan duration ms | 18189 | 7607 |
| Immediate no-change rescan duration ms | 215 | 52 |
| Export duration ms | 76771 | 68983 |

## Tradeoffs And Open Decisions

- This run proves release-profile dogfood through the public `julie-extract`
  binary and artifact contracts. It is stronger release evidence than the
  earlier debug-profile dogfood run.
- Timings remain report-only. A hard performance budget still needs repeated
  same-machine release-profile runs.
- JSONL export remains the dominant runtime even with the release binary.
  The next slice should inspect JSONL export performance before more scan work.
- Generated SQLite, JSONL, JSON reports, and metrics stayed under `target/` and
  are not committed.
