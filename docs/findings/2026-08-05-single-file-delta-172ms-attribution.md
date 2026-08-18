> **Superseded 2026-08-18.** The resolution write path is retired. See [2026-08-18-resolution-write-path-retirement.md](../decisions/2026-08-18-resolution-write-path-retirement.md).

# Single-file delta 81 ms → 172 ms: root-cause attribution

**Date:** 2026-08-05 · **Machine:** M-series macOS (same host as all figures below) · **Toolchain:** rustc 1.97.1, `--release` · **Harness:** `resolution_perf.rs` `single_file_delta_is_within_budget`, default seed (2000 files / 92k identifiers)

## Question

`single_file_delta_is_within_budget` is red at ~172–180 ms against the 150 ms ceiling, and the
file's "~81 ms release" note no longer reproduces on main. What regressed, and where?

## Method

Detached-worktree probes of the unmodified checked-in harness at each commit (same toolchain,
same seed, shared target dir). At the pivot commits, a local uncommitted probe re-ran
`resolution_store::resolution_report(&tx)` immediately after the timed pass and printed it
separately ("report-only"; warm cache, so it measures query cost, not I/O).

## Measurements

| Commit | Date | Delta total | Report-only | Non-report |
|---|---|---|---|---|
| `2ae8029` (documented 81 ms baseline) | 07-07 | **82 ms** | — | — |
| `6941e05~1` | 07-24 | 86 ms | 69 ms | ~17 ms |
| `6941e05` "close reference resolution for Miller takeover" | 07-24 | **146 ms** | 130 ms | ~18 ms |
| `cc50117~1` | 07-26 | 144 ms | — | — |
| `a8dc664` (main; prior measurement, repaired fixture) | 08-03 | 172 ms | — | ~41 ms (inferred) |
| `db4e8d0` (delta-resolution-soundness head) | 08-05 | 180 ms | 131 ms | ~49 ms |

The 82 ms baseline reproduces exactly at `2ae8029` on this machine, so none of the drift is
environmental.

## Root cause

**The "single-file delta" number was never mostly delta resolution.** `run_resolution` ends every
pass — full *and* delta — with `resolution_store::resolution_report(tx)`, a workspace-wide
aggregation. At the 82 ms baseline that query was already 69 ms (84%) of the measurement; true
delta work was ~17 ms.

Two independent regressions stacked on top:

1. **`6941e05` rewrote `resolution_report`** from an aggregate over the overlay tables into a
   three-branch `UNION ALL` over the *base* tables — all `relationships`, all
   `pending_relationships` (LEFT JOIN `pending_resolutions`), and all 92k `identifiers`
   (LEFT JOIN `identifier_resolutions`) — with a per-row five-column `span_present` CASE and a
   seven-column GROUP BY. Report cost 69 ms → 130 ms. Delta total 82 → 146 ms. This single
   commit is the bisected cause of the jump; the rest of `2ae8029..cc50117~1` is flat.
2. **The reference-sites era (`cc50117..a8dc664`) roughly doubled the real per-delta work**,
   ~18 ms → ~41 ms (site-carrying overlay writes, FK-indexed tables, wider rows). Attributed by
   subtraction between the two probe points, not commit-bisected — the harness could not seed in
   this span (NOT NULL `reference_sites` FK) until the branch repaired the fixture.

The branch's own soundness fixes add ~8 ms (tier-2/3 delta keys, module-candidate widening —
work the delta was previously, unsoundly, skipping).

## Production impact (worse than the gate suggests)

The single-file `update` command (`commands.rs`) runs `resolve_workspace` — and therefore the
workspace-wide report — **inside the write transaction** on every update. Miller's converge path
issues exactly this per file save. The report is O(workspace) per single-file update: ~130 ms at
92k identifiers, and the 2026-08-03 dotnet/runtime investigation puts real artifacts at 10M+
identifiers — plausibly seconds per file save, holding the write tx, before any fix. The
`finalize_resolution_metadata`/report-section consumers only need the aggregate after full scans
or on demand; the delta path recomputes it every time and mostly discards the precision.

## Recommendation

Fix, do not re-baseline. Ordered:

1. **Move `resolution_report` off the delta path** (compute on full passes and on demand;
   deltas reuse/carry the prior summary or mark it stale). Saves ~130 ms at harness scale and
   the O(workspace) scaling cliff in production. With the report out of the timed pass, delta
   lands ~50 ms — under the 100 ms design target with the current ceiling intact.
2. Optionally revisit the site-era overlay-write cost (~2× real delta work) afterwards; it is
   secondary and bounded.
3. Keep the harness printing report cost separately so the gate can never silently become a
   report benchmark again.

Not verified: the exact sub-split of the site-era +23 ms (write maintenance vs wider scans), and
report-query cost at 10M-identifier scale (extrapolated, not measured).

## Outcome (fixed on this branch, same day)

Recommendation 1 landed: `ResolutionReport::rows` is `Option<Vec<_>>`, computed only on passes
that re-derive the whole workspace (full scan, v3 backfill, crossover-promoted delta); a scoped
delta returns `None` and the JSON report section renders `totals`/`origin_totals`/`by_language`
as `null` ("not recomputed", never "zero") — contract updated in `docs/contracts/reports.md`.
Durable metadata (`status`/`version`/`last_full_revision`) is pass-derived and unchanged.

Measured after the fix (same machine/toolchain/seed): **single-file delta 51 ms** (gate green,
2.0x headroom under the 100 ms design target; ceiling untouched at 150 ms). The crossover curve
also dropped: scoped deltas now cross Full at 70–80% of the corpus (0.96x at 70%, 1.08x at 80%),
so `DELTA_SCOPE_CROSSOVER` moved 0.6 → 0.7 under the same one-sided
budgets-move-to-measurement rule that set 0.6.

Recommendation 2 (site-era overlay-write cost, ~49 ms real delta work) remains open and bounded.
