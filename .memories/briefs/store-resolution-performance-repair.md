---
id: store-resolution-performance-repair
title: Store Resolution Performance Repair
status: active
created: 2026-08-11T01:19:48.396Z
updated: 2026-08-11T01:19:48.396Z
tags:
  - performance
  - store
  - resolution
  - miller
---

## Goal
Make the versioned family-store resolution path usable on real repositories, then verify Miller's derived-sidecar behavior does not turn zero-change scans into full rebuilds.

## Current direction
Repair repeated SQLite scans and reader opens without building an unbounded whole-workspace cache. Keep candidate accumulation and memoization explicitly bounded, preserve exact resolution results, and measure with the Miller-repository store replay.

## Acceptance
The real replay preserves 37,965 gaps across 98 files, materially improves on the 6:20.55 producer baseline, stays within a bounded memory design, passes default/contract/clippy/fmt gates, and lands as an isolated commit. Miller's zero-delta sidecars must fast-forward without rewriting large databases and land separately.

## Boundaries
Do not push, publish, release, or change Miller's pinned producer without explicit approval.
