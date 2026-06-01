# Autonomous Run: Repeatable Performance Baseline

- Status: Complete, PR open
- Branch: `codex/repeatable-performance-baseline`
- Base: `main` at `bac074a`
- Verified implementation head before report commit: `92df2e2`
- Pull request: https://github.com/anortham/julie-extractors/pull/7
- Tracker: `docs/plans/2026-06-01-product-completion-tracker.md`

## What Shipped

- Added `cargo xtask performance baseline`, a repo-tooling command that runs
  the existing dogfood validator repeatedly and writes
  `baseline-summary.json`.
- Added contract coverage for baseline argument parsing, dispatcher routing,
  min/median/max aggregation, and inconsistent row-count rejection.
- Recorded 3-run release-profile baseline evidence under
  `docs/release-evidence/2026-06-01-repeatable-performance-baseline.md`.
- Updated the product tracker and active brief to move the next active slice to
  the v0.1.0 release candidate audit.

## Verification Ledger

| Scope | Command | Result |
| --- | --- | --- |
| Red test | `cargo test -p xtask --test performance_baseline_contract` before implementation | Failed because `xtask::performance` did not exist |
| Red test | `baseline_summary_rejects_inconsistent_row_counts` before stable evidence validation | Failed until inconsistent row totals were rejected |
| Worker scope | `cargo test -p xtask` | Pass |
| Release build | `cargo build --release -p julie-extract-cli --bin julie-extract` | Pass |
| Release baseline | `cargo xtask performance baseline --root . --out-dir target/performance/julie-extractors-baseline --binary target/release/julie-extract --runs 3` | Pass |
| Non-Rust consumer | `python3 examples/python/sqlite_consumer.py target/performance/julie-extractors-baseline/run-001/artifact.sqlite` | Pass |
| Format gate | `cargo fmt --all -- --check` | Pass at `92df2e2` |
| Diff hygiene | `git diff --check main..HEAD` | Pass before PR creation |
| Branch gate | `cargo xtask test default` | Pass at `92df2e2` |
| Branch gate | `cargo xtask test contract` | Pass at `92df2e2` |

## Baseline Evidence

- Evidence doc:
  `docs/release-evidence/2026-06-01-repeatable-performance-baseline.md`
- Generated summary:
  `target/performance/julie-extractors-baseline/baseline-summary.json`
- Binary version: `julie-extract 0.1.0`
- Binary SHA-256:
  `62d1ba3defb30883614577280f07565cc87c54775ffad8ee592326e432e2ba5c`
- Binary size: `64M`
- Cold scan min/median/max: `6277ms` / `6387ms` / `7508ms`
- No-change rescan min/median/max: `51ms` / `51ms` / `52ms`
- JSONL export min/median/max: `1330ms` / `1330ms` / `1333ms`
- Stable output: `1018` files, `33019` symbols, `215388` JSONL records

## Judgment Calls

- Kept the command in `xtask` because this is release-evidence tooling, not a
  public product command.
- Reused `dogfood::run_repo` as the hard validator instead of duplicating
  artifact/report checks.
- Rejected hard timing thresholds in this slice. The evidence is now repeatable
  enough for release-candidate judgment, but budgets still need an explicit
  policy decision.
- Added a hard comparability check for row totals, JSONL record counts, schema
  versions, and root path across samples so the summary cannot aggregate
  non-equivalent runs.
- Local gates passed at `92df2e2`; PR Fast Gates should verify this report-only
  final branch head after push.

## Blockers

- None.

## Next Steps

- Watch PR #7 Fast Gates.
- After PR #7 merges, start Slice 5: v0.1.0 release candidate audit.
