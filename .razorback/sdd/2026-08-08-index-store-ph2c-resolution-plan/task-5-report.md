# Task 5 Report: G1-G5 Store Resolution Mechanism Gate

## Status and state

- Status: complete with a GO verdict; execution stopped before Task 6.
- Base: `d0cc5849ff3063dd70e50c0beb35bc33bd5eede3`.
- Branch: `codex/index-store-ph2c`.
- Worktree: `/Users/murphy/source/julie-extractors/.claude/worktrees/index-store-ph2c`.
- Final commit: this report's containing commit.
- Push/release: not performed.

## Delivered behavior

- Added `cargo xtask performance store-resolution --runs 3 [--out-dir <path>]` with a three-run minimum, the two fixed Miller pairs, per-sample verdicts, stable JSON, and no averaging.
- Added a release-profile, isolated-process harness over the real Store session, exact base writer, streaming diff, persisted delta apply, reopen/integrity checks, and RSS measurement.
- Added ordered phase markers that reject missing, overlapping, or widened timing intervals.
- Added line and span locator indexes and split the locator query so SQLite can use the correct range index.
- Reused one bounded reader/prepared statement per candidate window, cached exact edge results within that window, batch-hydrated frozen phase keys, and prepared scratch flush statements once per batch.
- Validated each distinct semantic target once during exact-base completion.
- Replaced disjunctive composite-key pagination with SQLite row-value keysets across Store, base, and scratch cursors, removing the full-scale quadratic scan.

## RED/GREEN ledger

1. Parser/summary RED: the xtask route and performance module were absent. GREEN: seven contract tests cover routing, minimum runs, fixed pair/run cardinality, inclusive thresholds, release profile, vacuity, and one-failure visibility.
2. Harness RED: the first implementation measured a debug child. GREEN: the xtask invokes the release-profile integration test and records per-run JSON plus one summary.
3. Timing RED: phase boundaries could be missing, overlap, or include unrelated work. GREEN: `ResolutionDiffMarker` and session markers are ordered, non-overlapping, and contract-tested.
4. Representative performance RED: per-edge reader opens and unbatched Store/scratch work missed the gate. GREEN: bounded reader reuse, result caching, set-based phase freezing, batch hydration/flush, locator indexes, and distinct-target validation produced 54k–65k rows/sec at the 10k diagnostic scale.
5. Full hard-gate RED: all six live-scale samples passed G1/G2/G4 but failed G3a/G3b/G3c/G5 at 12.5k rows/sec, 0.867–0.895 ratio, and 62.7–63.8 seconds.
6. Root cause: 392,134 identifiers share one version; every disjunctive keyset page rescanned the growing same-version prefix. GREEN: row-value composite keysets reduced the full matrix to 66.8k–67.8k rows/sec, 0.059–0.199 ratio, and 10.5–11.4 seconds with all gates true.
7. Catalog RED: the new locator indexes changed the canonical Store catalog from the documented hash. GREEN: the schema-v1 authority now records `1897879e3cdccc86c7a90bd94e583ea71838e05982c9f218980eb41fa04d4659` and the exact catalog contract passes.
8. Clippy RED: one Store phase method bound and immediately returned a `match`. GREEN: the expression is returned directly; both strict package scopes pass.

## Hard-gate evidence

- Exact command: `RUSTUP_TOOLCHAIN=1.97.1 cargo xtask performance store-resolution --runs 3`.
- Result: exit 0; 1/1 harness test passed in 254.15 seconds.
- Per-run metrics and machine facts: `docs/findings/2026-08-08-index-store-ph2c-mechanism-gate.md`.
- G1: zero semantic differences in every sample.
- G2: zero applied differences in every sample.
- G3a: 66,806.47–67,750.86 identifier rows/sec.
- G3b: 0.05940–0.19945.
- G3c: 10,508–11,399 ms.
- G4: zero gap mismatches in every sample.
- G5: zero foreground identifier work; background 10,508–11,399 ms.

## Verification ledger

- xtask performance contract: 7/7 passed.
- artifact resolution schema/diff contract: 18/18 passed.
- Store schema contract: 13/13 passed.
- resolution session/oracle contract: 6/6 passed.
- Store mechanism contract including RSS: 12/12 passed.
- full artifact feature suite: passed.
- full CLI feature suite: passed.
- artifact and CLI all-target Clippy: passed with only the pre-existing unowned `collapsible_match` baseline allowed and all other warnings denied.
- `cargo fmt --all -- --check`: passed.
- `git diff --check`: passed.
- `RUSTUP_TOOLCHAIN=1.97.1 cargo xtask test default`: exit 0.

## Miller evidence

- Target worktree status/onboarding remained unavailable with `workspace_status_empty` and `workspace_onboarding_empty`; impact also returned `invalid_request` for the unregistered worktree.
- Per the approved fallback, source/API inspection used targeted `rg`, bounded reads, exact tests, and the working-tree diff. No Miller result was invented.

## Verdict

GO for Ph2c-a G1-G5. Task 6 was not started; the user-requested hard stop is honored.
