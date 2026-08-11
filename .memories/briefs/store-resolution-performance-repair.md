---
id: store-resolution-performance-repair
title: Store Resolution Performance Repair
status: completed
created: 2026-08-11T01:19:48.396Z
updated: 2026-08-11T23:00:33.989Z
tags:
  - performance
  - store
  - resolution
  - miller
---

## Goal achieved

Store resolution is now proportional to the changed dependency closure, row-identical to forced full, bounded, crash-safe, and default-on with `JULIE_STORE_RESOLUTION_DELTA=off` as the full-path escape hatch.

## Landed result

- Durable scope journals and rooted predecessor overlays cover every manifest transition and lifecycle path.
- Scoped resolution bulk-carries unaffected rows, publishes atomic cumulative deltas, and rebases at strict >25% semantic drift or >64 MiB gap storage.
- Three deterministic faithful Miller-scale A/B runs completed scoped in 18.349–18.675s versus 32.119–32.630s full, with equal canonical/fixture digests and zero row differences.
- Miller consumer contracts prove live base rotation and same-process path reopen.
- Missing-family-store RootRebind now accepts only revision-current, identifier-total Complete/Partial legacy resolution; stale/incomplete input remains rejected. The real replaced-root regression opens an exact readable store.

## Verification

Default and contract tiers, strict Clippy/formatting, gitleaks, cargo-deny, recorded-scale replay, crash/equivalence suites, Miller fast/Scale, and focused RootRebind tests pass.

## Boundaries preserved

No push, publish, release, or Miller production behavior change was performed.
