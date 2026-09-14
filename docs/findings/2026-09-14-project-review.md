# Project Review — 2026-09-14

## Scope and evidence

This review covered extraction, artifact writing, CLI behavior, validation
tiers, performance evidence, and public documentation at baseline
`df927952` on `main`; the combined review changes are uncommitted.

It is not an exhaustive correctness audit of all 40 grammars or a full
real-world parser certification.

- `node scripts/language-data-quality-report.mjs --strict` passed for 40
  languages with `silent_cells=0` and `quality_bar_debts=0`. The scorecard also
  reports 47 declared `open_gap` roadmap items: 3 relationships, 42 structural
  facts, and 2 test-detection gaps. They are existing capability work, not 47
  newly audited defects or proof of complete coverage.
- The existing CI fast gate already runs the strict quality report, default
  tier, and contract tier. The earlier missing-quality-CI finding is closed.
- Historical v2.42.2 release evidence records a 121 second default tier and a
  327.96 second contract tier. The default tier was below its three-minute warm
  target at that release; fresh integration timing is recorded separately.

## Changes accepted during review

### Opt-in writer performance floor

`cargo xtask test perf` now exposes the existing feature-gated writer and
JSONL-export performance harness. It remains outside default and contract
tiers, and a manual `Performance Floor` workflow runs it on demand. The tier
prints its throughput measurements for review. The runner lives in
`xtask/src/test_tiers.rs`; the manual workflow is
`.github/workflows/performance.yml`.

On this machine the floor completed three tests in 3.38 seconds: a 1,500-file
write took 605 ms and JSONL export took 1,330 ms. Its 30-second write and
15-second export limits catch a collapse, not a moderate regression; an
end-to-end comparison remains necessary for that.

### Documentation accuracy

The root README and documentation index now link to the canonical current
release-notes index instead of repeating a stale release number. The testing
strategy now describes the actual default and contract contents.

### Avoided successful-file guard queries

`ensure_data_loss_guard` now returns before querying `symbols` unless a file is
`FailedPreserved`. The guard still checks that failure mode before a destructive
write, which preserves the data-loss contract. The guard is in
`crates/julie-extract-artifact/src/writer.rs` and its measurement harness is
`crates/julie-extract-artifact/tests/writer_perf.rs`.

The feature-gated `writer_forced_refresh_throughput` cargo-test-profile
workload creates 5,000 files, 20,000 symbols, and 140,000 child rows, then
force-refreshes the same artifact. After one warm-up, three warm baseline runs
were 7,333 ms, 6,079 ms, and 6,382 ms; the fixed runs were 6,587 ms, 5,776 ms,
and 5,964 ms. The median improved from 6,382 ms to 5,964 ms (about 6.5%). The ranges
overlap, so this is a modest workload-specific result, not evidence of release
CLI or project-wide speed. Fresh initial writes were unchanged within noise
(405 ms before, 414 ms after), as expected because they have no existing
symbols to query.

### Avoided scan-only snapshot-cache retention

Successful full scans no longer retain source snapshots in the process-wide
cache when no later operation can consume them. The cache remains available for
the store paths that need it. The change is in
`crates/julie-extract-cli/src/commands.rs` and `extraction.rs`.

On frozen `df927952` inputs, two separately built release binaries were warmed
once and then run as alternating pairs. Each run scanned the fixture at full
level with one fresh SQLite destination and `--json`, using a new output
directory. Baseline wall times were 17.29 s, 17.86 s, and 17.95 s; fixed times
were 17.02 s, 17.33 s, and 16.79 s. The median fell from 17.86 s to 17.02 s
(4.7%) and p95-of-three from 17.95 s to 17.33 s (3.5%). Median peak RSS fell
from 1,991,468 KiB to 1,975,756 KiB. All commands exited successfully.

The RSS reduction is not conclusive because large extractor and writer
allocations dominate the process. The deterministic change removes up to 64 MiB
of retained source snapshots, but these measurements remain evidence for this
workload rather than a general speed claim. Reproduce with two isolated `git
archive df927952` source trees, separate `CARGO_TARGET_DIR`s, one warm-up per
binary, and alternating runs of:

```bash
/usr/bin/time -f 'wall=%e maxrss_kib=%M user=%U sys=%S exit=%x' \
  <binary> scan --root <fixture> --db <fresh-output>/artifact.db \
  --level full --json >/dev/null
```

## Rejected candidate

### Bounded symbols-level complexity traversal

For one generated JavaScript file with 2,000 independent three-line functions,
a release `scan --jobs 1 --level symbols` had a 2,905 ms p95 extraction phase;
the corresponding artifact write was 33 ms.

The complexity collector evaluates each callable and walks the full root before
discarding non-overlapping children. Symbols-level complexity metrics remain
part of the tested contract.

The retained fixture is `/home/murphy/julie-extractor-review.XbLpEw`: each
`bind-N/many.js` contains N independent functions, and databases are written to
its sibling `db/` directory. Build a release binary, run one warm-up plus three
runs per N, and read `profile.languages.javascript.extract_duration_ms` from:

```bash
mkdir -p "$case_dir" "$fixture/db"
for i in $(seq 1 "$n"); do
  printf '// comment %s\nfunction f%s(){ const s="value%s"; return s; }\n' "$i" "$i" "$i"
done > "$case_dir/many.js"
```

Then run:

```bash
julie-extract scan --root "$case_dir" --db "$fixture/db/$n-$run.db" \
  --jobs 1 --json --force --level symbols
```

The original non-paired result is not evidence. A fair comparison used two
release binaries differing only in `complexity_metrics.rs`, the fixed fixture,
one warm-up per binary, then three alternating pairs at load 10.14/8.50/5.22.
Baseline JavaScript extraction was 2,243 ms, 2,228 ms, and 2,205 ms; the
bounded traversal was 2,378 ms, 2,180 ms, and 2,231 ms. The medians were flat
(2,228 ms and 2,231 ms) and p95 worsened within noise. The candidate was
reverted because the measured result did not justify its added complexity.

## Remaining opportunities

### Large single-file extraction scaling

The generated 2,000-function fixture remains a useful stress workload, but the
rejected traversal experiment did not prove the dominant cause of its cost.
Profile the release binary before changing another extractor traversal or
discarding contract-visible metrics.

### No automated end-to-end regression comparison

`cargo xtask performance baseline` gathers repeated dogfood measurements and
rejects incomparable samples, but accepts no reference baseline or regression
budget. No workflow invokes it. It cannot detect a slower scan, store import,
or export unless someone compares the generated summaries manually.

Keep the existing local release-evidence workflow for now. A future dedicated
benchmark should compare the same pinned workload and machine against an
accepted baseline, with results retained as evidence. Do not add a generic
wall-clock threshold to the default or contract tier.

### JSONL exporter allocation profile

JSONL exporter allocation behavior still needs profiling before changing its
serialization path. Its 51,002-record baseline p95 was 1,180 ms while a fresh
write took 405 ms; object construction is a candidate, not causal evidence.

## Verification

- Fresh final-source formatting, diff, and strict language-quality gates passed.
- Fresh final-source default tier: `cargo xtask test default` — passed in 85
  seconds.
- Fresh final-source contract tier: `cargo xtask test contract` — passed 381
  test results across 27 command groups, including canonical golden fixtures.
- `cargo test -p xtask` — 121 test results passed across 10 groups. These
  counts overlap feature-target coverage and are not additive.
- Windows NTFS snapshot at `ff748ebdf534314b141312ea4fed07037d7e7e64`, with
  matching source hashes: writer contract (51 tests), the new scan-cache
  regression, and three cache tests passed.
- `cargo test -p xtask --test test_tiers` — 25 passed.
- Final-source `cargo xtask test perf` — 3 passed in 3.01 seconds.
- Cache validation: CLI library suite (168 passed), store-import single-read
  contract, and scoped formatting/diff checks passed.
- Writer refresh measurement: `JULIE_PERF_FILES=5000 cargo test -q -p julie-extract-artifact --features test-perf --test writer_perf writer_forced_refresh_throughput -- --nocapture`; run one warm-up and three warm samples. The workload is `crates/julie-extract-artifact/tests/writer_perf.rs`.
- Scoped `git diff --check` passed for the review-owned edits.
