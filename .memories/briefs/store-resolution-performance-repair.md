---
id: store-resolution-performance-repair
title: Store Resolution Performance Repair
status: active
created: 2026-08-11T01:19:48.396Z
updated: 2026-08-11T17:10:03.674Z
tags:
  - performance
  - store
  - resolution
  - miller
---

## Goal
Make the versioned family-store resolution path usable on real repositories, including both incremental exact resolution and reliable fresh-store recovery when Miller's family store is missing.

## Current direction
Implement the approved eight-task store incremental-resolution plan: durable transition-keyed scope history, rooted predecessor overlay, bounded scoped resolution, semantic-row equivalence, and atomic base rebase. Then diagnose and fix the newly reproduced recovery bug where a missing family store causes repeated RootRebind attempts to publish partial reference-resolution input (`reference_resolution_status must be complete, found partial`) and leaves Miller unreadable.

## Acceptance
The incremental path preserves canonical semantic rows, materially improves the Miller replay, stays bounded and crash-safe, passes all plan gates, and defaults on only after evidence. Fresh missing-store recovery must converge through Miller refresh/RootRebind to complete resolution and a readable workspace, with a focused producer/integration regression.

## Boundaries
Keep extraction and store-writing behavior in julie-extractors. Do not push, publish, release, or change Miller production behavior without explicit approval; consumer contract tests are allowed where needed.
