# Store Incremental Resolution Dogfood

## Result

Validated scoped resolution is ready to be the unset-env default. Three deterministic faithful
recorded-scale replay A/B runs
produced the same canonical semantic digest and zero row-level differences. Scoped exact publication
finished in 18.35–18.68 seconds, 42–43% faster than the 32.12–32.63 second forced-full control.

## Replay provenance

The read-only Miller family store at
`/home/murphy/.miller/stores/a271f2bd-7368-4da6-b5aa-24ffad69fb1f` was measured at 158,875,636 bytes,
1,635 versions, 450,520 identifiers, 71 resolution deltas, and 1,532,782 exact-gap rows. It predates
resolution-scope batches, so it cannot safely execute the scoped side of the A/B.

The deterministic faithful recorded-scale replay reconstructs the transition shape without modifying that store:
1,538 files, 392,134 identifier inputs, 89,538 pending inputs, 10,412 resolved pending inputs, 20,109
distinct identifier names, and 98 changed files. Each mode starts from an isolated store with the same
manifest and semantic rows. The canonical oracle orders and hashes base-version, identifier-resolution,
and pending-resolution rows, then runs bidirectional row-level differences.

## Recorded-scale measurements

| Run | Mode | Exact wall | Resolve | User / system CPU | Peak RSS | Scope files / names / rows | Row diff |
|---:|---|---:|---:|---:|---:|---:|---:|
| 1 | forced full | 32.630s | 27.366s | 19.66s / 2.43s | 388,132,864 B | 1,538 / 0 / 482,456 | 0 |
| 1 | scoped | 18.494s | 9.106s | 14.88s / 1.65s | 452,182,016 B | 98 / 197 / 784 | 0 |
| 2 | forced full | 32.119s | 26.882s | 19.71s / 2.36s | 388,227,072 B | 1,538 / 0 / 482,456 | 0 |
| 2 | scoped | 18.349s | 9.049s | 14.71s / 1.80s | 451,948,544 B | 98 / 197 / 784 | 0 |
| 3 | forced full | 32.129s | 26.914s | 19.70s / 2.44s | 388,190,208 B | 1,538 / 0 / 482,456 | 0 |
| 3 | scoped | 18.675s | 9.195s | 14.97s / 1.79s | 451,825,664 B | 98 / 197 / 784 | 0 |

Every artifact digest was
`3855296f8cab7f1ac8af88809c1a426360d786ee701ab498575d096b09784f04`. Scoped runs had no fallback.
The fixed 64 MiB threshold was exercised at 64 MiB + 1 byte and rebased to a 73,277,440-byte ready
base with a 22-byte empty delta and zero cumulative delta rows. Exact equality does not rebase; the
first byte over does. The 25% replacement threshold has the same strict-over boundary.

## Architecture correction

The first deterministic faithful recorded-scale replay exposed an O(corpus) row-by-row carry-forward
path: scoped took 41.557s
against 30.049s full. The final implementation attaches the validated prior base and delta read-only and
uses transaction-scoped `INSERT OR IGNORE ... SELECT` statements. Current-version filtering, delta-over-
base precedence, pending tombstones, and scratch authority remain explicit in SQL, without a corpus-sized
Rust collection. Focused equivalence, sequence, mechanism, and session contracts stayed green.

## Verification ledger

All rows apply to branch `feature/store-incremental-resolution` at dispatch commit
`3c19e21cfb6e4256a917ec71da7f7a608b0f1a8c`; the worktree remained uncommitted.

| Invariant | Command | Scope | Result | Recorded UTC |
|---|---|---|---|---|
| Recorded-scale semantic and timing gate | `cargo xtask performance store-resolution --runs 3 --out-dir target/performance/store-incremental-resolution-task8-review-final` | extractor replay | PASS, 3/3 pairs | 2026-08-11T22:18:03Z |
| Resolution semantics and planner seam | focused mechanism, session, scope-equivalence, sequence-equivalence, and performance Cargo targets | extractor contracts | PASS, 27 tests | 2026-08-11T22:28:34Z |
| Default-on and explicit-off CLI behavior | `cargo test -p julie-extract-cli --features test-store-resolution-contract --test store_resolution_contract -- --test-threads=1` | CLI contract | PASS, 24 tests | 2026-08-11T22:28:34Z |
| Performance evaluator contract | `cargo test -p xtask --test resolution_performance_contract` | xtask | PASS, 13 tests | 2026-08-11T22:28:34Z |
| Default and contract tiers | `cargo xtask test default`; `cargo xtask test contract` | workspace | PASS | 2026-08-11T22:28:34Z |
| Rust formatting and lint | `cargo fmt --all -- --check`; strict workspace all-target/all-feature Clippy | workspace | PASS | 2026-08-11T22:28:34Z |
| Dependency policy | `cargo deny --all-features check` | workspace dependencies | PASS; warnings report-only | 2026-08-11T22:28:34Z |
| Secret scan | `gitleaks detect --no-banner --redact` | repository | PASS | 2026-08-11T22:28:34Z |
| Miller fast consumer gate | `MILLER_ALLOW_MISSING_SEMANTIC=1 scripts/test.sh` | Miller worktree | PASS, 6,360 passed / 4 skipped | 2026-08-11T21:49:47Z |
| Miller Scale consumer gate | `MILLER_ALLOW_MISSING_SEMANTIC=1 scripts/test.sh scale` | Miller worktree | PASS, 99 passed / 42 skipped | 2026-08-11T21:49:47Z |

The Miller worktree was
`/home/murphy/source/miller/.worktrees/store-incremental-resolution`, branch
`feature/store-incremental-resolution-consumer`, commit
`9bf6bc2693d6cce6d70707e1fa57576fa78efa28`, with no tracked changes. The semantic-sidecar skips are
the repository's explicit source-build allowance; the combined store read-session and rebase filter
passed 27/27 against the rebuilt default-on extractor.
