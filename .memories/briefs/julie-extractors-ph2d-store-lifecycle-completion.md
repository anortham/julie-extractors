---
id: julie-extractors-ph2d-store-lifecycle-completion
title: Julie Extractors Ph2d Store Lifecycle Completion
status: active
created: 2026-08-08T23:51:08.294Z
updated: 2026-08-08T23:51:08.294Z
tags:
  - ph2d
  - index-store
  - retention
  - repair
  - release
---

## Goal
Finish the unreleased Julie family-store program through Ph2d: rooted retention and garbage collection, bounded repair, capacity preflight, immutable generation promotion, mixed-version gates, release preparation, and downstream Miller pin validation.

## Approved direction
Julie owns a public `store maintain` lifecycle surface with read-only inspection and explicit apply modes. A pure reachability planner computes protected and reclaimable objects; mutation revalidates under the writer lease. Repair and compaction build a validated new generation and atomically switch `CURRENT`, retaining rollback state. No in-place rewrite of the serving generation.

## Safety constraints
Current and historical manifests, ready-base version roots, current bindings, live pins, active requests and claims, and durable consumer cursors are roots. Existing retention defaults remain 7 days, 1.20 target, 1.25 ceiling, and 24 historical versions per path. Capacity failure is typed and occurs before mutation. Push, tag, publish, and live Miller pin changes remain explicit approval boundaries.

## Phase boundary
Ph2d is Julie Extractors work. Ph3 remains Miller production integration: registry resolution, admission/governor wiring, read sessions, sidecars, status/health/dashboard, and rollback orchestration.
