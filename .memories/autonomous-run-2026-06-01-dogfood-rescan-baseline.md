# Autonomous Run: Dogfood Incremental Rescan Baseline

- Status: Complete
- Branch: `codex/dogfood-performance-baseline`
- Base: `main` at `04f8e5d`
- Final implementation head before this report: `9987cd7`
- Plan: `docs/plans/2026-06-01-dogfood-incremental-rescan-baseline.md`

## What Shipped

- `cargo xtask dogfood repo` now runs cold scan, immediate rescan, info, and JSONL export.
- Dogfood validates `rescan-report.json` as `status=no_change`, with zero changed/deleted/failed files, positive unchanged files, `created_revision_id=null`, and zero `counts.rows_written`.
- `metrics.json` includes rescan duration and rescan file-count fields.
- Release/testing docs and `docs/release-evidence/v0.1.0-dogfood.md` record the dogfood evidence.

## Verification Ledger

| Scope | Command | Result |
| --- | --- | --- |
| Red test | `cargo test -p xtask --test dogfood_contract` before validation hardening | Failed because invalid rescan evidence passed validation |
| Focused | `cargo test -p xtask --test dogfood_contract` | Pass |
| Xtask | `cargo test -p xtask` | Pass |
| Formatting | `cargo fmt --all -- --check` | Pass |
| Diff hygiene | `git diff --check` | Pass |
| Dogfood | `cargo xtask dogfood repo --root . --out-dir target/dogfood/julie-extractors` | Pass at `17b403b` |
| SQLite consumer | `python3 examples/python/sqlite_consumer.py target/dogfood/julie-extractors/artifact.sqlite` | Pass |
| Branch gate | `cargo xtask test default` | Pass at `9987cd7` |
| Branch gate | `cargo xtask test contract` | Pass at `9987cd7` |

## Dogfood Evidence

- Cold scan: `status=ok`, `files_changed=1011`, `files_failed=0`
- Immediate rescan: `status=no_change`, `files_unchanged=1011`, `files_changed=0`, `files_deleted=0`, `files_failed=0`
- Rescan revision: `created_revision_id=null`
- Rescan rows written: all `counts.rows_written` values are `0`
- Report-only timing: cold scan `18189ms`, rescan `215ms`, export `76771ms`

## Review

- Reviewer found one Important issue: rescan validation did not enforce no created revision or zero row writes.
- Fixed with failing contract tests first, then validator hardening.
- Follow-up reviewer check found no material findings.

## Judgment Calls

- No hard timing threshold was added. One debug-profile machine run is evidence, not a reliable release budget.
- The dogfood evidence remains an `xtask` specialist gate and does not change public CLI, SQLite, JSONL, or report contracts.
- JSONL export remains the dominant runtime and is recorded as report-only evidence.

## Blockers

- None.
