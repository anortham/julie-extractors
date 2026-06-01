# Autonomous Run: Release-Binary Dogfood Evidence

- Status: Complete
- Branch: `codex/release-dogfood-evidence`
- Base: `main` at `a3038ee`
- Verified implementation head before report commit: `c607a16`
- Pull request: https://github.com/anortham/julie-extractors/pull/4
- Tracker: `docs/plans/2026-06-01-product-completion-tracker.md`

## What Shipped

- Added release-profile dogfood evidence for `target/release/julie-extract`.
- Recorded the public binary version, SHA-256, size, hard-gate report statuses,
  row totals, artifact sizes, repo shape, and report-only timings.
- Updated the product tracker and active brief to mark Slice 1 complete and keep
  Slice 2 focused on JSONL export performance.
- Kept generated SQLite, JSONL, JSON reports, and metrics under `target/`.

## Verification Ledger

| Scope | Command | Result |
| --- | --- | --- |
| Release build | `cargo build --release -p julie-extract-cli --bin julie-extract` | Pass |
| Release dogfood | `cargo xtask dogfood repo --root . --out-dir target/dogfood/julie-extractors-release --binary target/release/julie-extract` | Pass |
| Non-Rust consumer | `python3 examples/python/sqlite_consumer.py target/dogfood/julie-extractors-release/artifact.sqlite` | Pass |
| Branch gate | `cargo xtask test default` | Pass at `c607a16` |
| Branch gate | `cargo xtask test contract` | Pass at `c607a16` |
| Diff hygiene | `git diff --check main..HEAD` | Pass before PR creation |

## Dogfood Evidence

- Binary version: `julie-extract 0.1.0`
- Binary SHA-256:
  `af51b3792e10eb54f6aab5d94cd04c257801b183be0fb23f08db96ba23f441ce`
- Binary size: `64M`
- Cold scan: `status=ok`, `files_changed=1012`, `files_failed=0`
- Immediate rescan: `status=no_change`, `files_unchanged=1012`,
  `files_changed=0`, `files_deleted=0`, `files_failed=0`
- Rescan revision: `created_revision_id=null`
- Rescan rows written: every `counts.rows_written` value is `0`
- SQLite schema version: `1`
- Extract contract version: `1`
- JSONL schema version: `1`
- Files written: `1012`
- Symbols written: `32881`
- JSONL records: `214333`
- SQLite bytes: `137293824`
- JSONL bytes: `184972072`
- Report-only timings: cold scan `7607ms`, no-change rescan `52ms`,
  export `68983ms`

## Judgment Calls

- Timings remain report-only. One same-machine release-profile run is evidence,
  not enough data for hard performance budgets.
- This slice did not change SQLite, JSONL, report, or CLI contracts.
- This slice did not rerun dogfood after adding the report commit because the
  report is documentation-only and the branch gate already passed at the
  implementation head used to create the PR.
- JSONL export remains the dominant runtime, so the next product slice should
  plan export profiling before scan work continues.

## Blockers

- None.

## Next Steps

- Watch PR #4 Fast Gates.
- After PR #4 merges, start Slice 2: JSONL export performance plan.
